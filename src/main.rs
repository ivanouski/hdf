mod cli;
mod config;
mod context;
mod finder;
mod history;
mod prompt;
#[cfg(test)]
mod test_util;
mod tty;
mod ui;

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::ops::ControlFlow::{self, Break, Continue};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::Ordering;

use ratatui::backend::{Backend, ClearType, CrosstermBackend};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{cursor, terminal};
use ratatui::layout::Position;
use ratatui::text::Line;
use ratatui::widgets::ListState;
use ratatui::{Terminal, TerminalOptions, Viewport};

use cli::Args;
use config::{Config, Theme};
use context::Context;
use finder::Finder;
use history::History;
use tty::{RawMode, StdoutToTty};
use ui::Status;

const PANE_HEIGHT: u16 = 12;

// How the finder was left
#[derive(Debug, PartialEq)]
enum Outcome {
    // Enter or Tab on a command
    Picked(String),
    // Esc, or Enter and Tab without matches
    Quit,
    // Ctrl-C
    Abort,
    // SIGINT, SIGTERM or SIGHUP, with its number
    Signaled(usize),
}

// .hdfrc tells these apart: a pick or the query goes on the prompt line, an error or Ctrl-C
// leaves the line as it was. Printing the query has a status of its own, so .hdfrc does not take
// a failure of an hdf without --print-query, an older one, for an empty query.
const QUIT_STATUS: u8 = 1;
const ERROR_STATUS: u8 = 2;
const QUERY_STATUS: u8 = 3;
const ABORT_STATUS: u8 = 130;

fn main() -> ExitCode {
    match run() {
        Ok(status) => status,
        Err(message) => {
            eprintln!("hdf: {message}");
            ExitCode::from(ERROR_STATUS)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let (query, print_query, prompt) = match cli::parse_args(env::args_os().skip(1))? {
        Args::Search {
            query,
            print_query,
            prompt,
        } => (query, print_query, prompt),
        Args::DeleteLast(count) => {
            delete_last(count)?;
            return Ok(ExitCode::SUCCESS);
        }
        Args::InitConfig => {
            println!("{}", Config::init()?.display());
            return Ok(ExitCode::SUCCESS);
        }
        Args::Help => {
            print!("{}", cli::help());
            return Ok(ExitCode::SUCCESS);
        }
        Args::Version => {
            println!("hdf {}", env!("CARGO_PKG_VERSION"));
            return Ok(ExitCode::SUCCESS);
        }
    };

    let config = Config::load()?;
    let mut history = load_history()?;
    let mut finder = Finder::new(history.entries(), query);
    let prompt = prompt.map(|text| prompt::parse(&text));

    let outcome = pick(&mut finder, &mut history, &config.colors, prompt.as_ref())
        .context("terminal error")?;

    match outcome {
        Outcome::Picked(command) => {
            print_line(&command)?;
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Quit if print_query => {
            print_line(&finder.query)?;
            Ok(ExitCode::from(QUERY_STATUS))
        }
        Outcome::Quit => Ok(ExitCode::from(QUIT_STATUS)),
        Outcome::Abort => Ok(ExitCode::from(ABORT_STATUS)),
        Outcome::Signaled(signal) => Ok(ExitCode::from(tty::signal_status(signal))),
    }
}

// Runs the finder in a pane under the cursor's line, returns how it was left
fn pick(
    finder: &mut Finder,
    history: &mut History,
    theme: &Theme,
    prompt: Option<&Line>,
) -> io::Result<Outcome> {
    let mut pane = Pane::open()?;
    let mut list = ListState::default();
    let mut status = None;
    let signal = tty::watch_signals()?;

    let outcome = loop {
        pane.terminal.draw(|frame| {
            ui::draw(frame, finder, theme, prompt, status.as_ref(), &mut list);
        })?;

        match tty::next_event(&signal)? {
            None => break Outcome::Signaled(signal.load(Ordering::Relaxed)),
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                match handle_key(key, finder, history) {
                    Break(outcome) => break outcome,
                    Continue(next_status) => status = next_status,
                }
            }
            // Other events, like a resize, only redraw
            Some(_) => {}
        }
    };

    let closed = pane.close();

    // After SIGHUP the terminal may be gone, the signal still decides the exit status
    match outcome {
        Outcome::Signaled(_) => Ok(outcome),
        _ => closed.map(|()| outcome),
    }
}

// The finder's pane, drawn on the terminal itself so the finder works with stdout captured
struct Pane {
    terminal: Terminal<CrosstermBackend<File>>,
    // Of the cursor when opened, on text to go back to unless 0
    column: u16,
    // Fields drop in order, so these are restored after the terminal is gone
    _raw_mode: RawMode,
    _stdout: StdoutToTty,
}

impl Pane {
    fn open() -> io::Result<Self> {
        let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let stdout = StdoutToTty::new(&tty)?;
        let raw_mode = RawMode::enable()?;

        // Bash clears the prompt line before running a bind -x command, so the pane opens there.
        // When the cursor is still on some text, it opens on the next line to keep that text.
        let (column, _) = cursor::position()?;

        if column > 0 {
            tty.write_all(b"\r\n")?;
        }

        let height = terminal::size()?.1.min(PANE_HEIGHT);
        let terminal = Terminal::with_options(
            CrosstermBackend::new(tty),
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?;

        Ok(Self {
            terminal,
            column,
            _raw_mode: raw_mode,
            _stdout: stdout,
        })
    }

    // Leaves no trace of the pane and puts the cursor back where it was
    fn close(mut self) -> io::Result<()> {
        let area = self.terminal.get_frame().area();
        let backend = self.terminal.backend_mut();
        backend.set_cursor_position(area.as_position())?;
        backend.clear_region(ClearType::AfterCursor)?;

        if self.column > 0 {
            backend.set_cursor_position(Position::new(self.column, area.y.saturating_sub(1)))?;
        }

        self.terminal.show_cursor()
    }
}

// Leaves the finder with an outcome, or stays with the status to show until the next key
fn handle_key(
    key: KeyEvent,
    finder: &mut Finder,
    history: &mut History,
) -> ControlFlow<Outcome, Option<Status>> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match (key.code, control) {
        (KeyCode::Esc, _) => return Break(Outcome::Quit),
        (KeyCode::Char('c'), true) => return Break(Outcome::Abort),
        (KeyCode::Enter | KeyCode::Tab, _) => {
            return Break(match finder.selection() {
                Some(command) => Outcome::Picked(command.original.clone()),
                None => Outcome::Quit,
            });
        }
        (KeyCode::Char('d'), true) => return Continue(delete(finder, history)),
        (KeyCode::Up | KeyCode::PageUp, _) | (KeyCode::Char('p'), true) => finder.up(),
        (KeyCode::Down | KeyCode::PageDown, _) | (KeyCode::Char('n'), true) => finder.down(),
        // Option-Left and Right on a Mac send Alt-B and F or Alt-Left and Right, Linux terminals
        // Ctrl-Left and Right
        (KeyCode::Left, _) if control || alt => finder.word_left(),
        (KeyCode::Right, _) if control || alt => finder.word_right(),
        (KeyCode::Char('b'), false) if alt => finder.word_left(),
        (KeyCode::Char('f'), false) if alt => finder.word_right(),
        (KeyCode::Left, _) => finder.left(),
        (KeyCode::Right, _) => finder.right(),
        (KeyCode::Home, _) | (KeyCode::Char('a'), true) => finder.home(),
        (KeyCode::End, _) | (KeyCode::Char('e'), true) => finder.end(),
        (KeyCode::Backspace, _) if alt => finder.delete_word_back(),
        (KeyCode::Backspace, _) => finder.delete_back(),
        (KeyCode::Delete, _) => finder.delete_forward(),
        (KeyCode::Char('u'), true) => finder.delete_to_start(),
        (KeyCode::Char('w'), true) => finder.delete_big_word_back(),
        // Alt and a letter is a shortcut elsewhere, not text for the query
        (KeyCode::Char(char), false) if !alt => finder.push(char),
        _ => {}
    }

    Continue(None)
}

// Deletes every occurrence of the selected command and reloads the history
fn delete(finder: &mut Finder, history: &mut History) -> Option<Status> {
    let command = finder.selection()?;
    let message = format!("deleted {}", command.text);
    let deleted = history
        .delete(&command.entry_indices)
        .context("cannot delete");
    let reloaded = history.reload().context("cannot read history");

    // Keep the selection in place, on the command after the deleted one
    finder.reload(history.entries());

    Some(match deleted.and(reloaded) {
        Ok(()) => Status::Deleted(message),
        Err(error) => Status::Failed(error),
    })
}

// Prints the deleted commands, oldest first. Fewer commands than asked for deletes them all.
fn delete_last(count: usize) -> Result<(), String> {
    let history = load_history()?;
    let entries = history.entries();
    let indices: Vec<usize> = (entries.len().saturating_sub(count)..entries.len()).collect();

    if indices.is_empty() {
        return Ok(());
    }

    history.delete(&indices).context("cannot delete")?;

    // The commands are already deleted, so a closed stdout is no reason to fail
    let mut stdout = io::stdout().lock();

    for &index in &indices {
        if writeln!(stdout, "{}", entries[index].original).is_err() {
            break;
        }
    }

    Ok(())
}

fn load_history() -> Result<History, String> {
    let path = history_path()?;

    History::load(&path).context(format!("cannot read {}", path.display()))
}

fn history_path() -> Result<PathBuf, String> {
    match env::var_os("HISTFILE").filter(|path| !path.is_empty()) {
        Some(path) => Ok(path.into()),
        None => env::home_dir()
            .map(|home| home.join(".bash_history"))
            .ok_or_else(|| "HISTFILE and HOME are not set".to_string()),
    }
}

fn print_line(text: &str) -> Result<(), String> {
    writeln!(io::stdout(), "{text}").context("cannot print")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::TempFile;

    // The finder lists the commands most recent first: echo, pwd, ls
    const HISTORY: &str = "ls\npwd\necho\n";

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const CONTROL: KeyModifiers = KeyModifiers::CONTROL;

    // The finder on a history file of its own
    struct Session {
        file: TempFile,
        history: History,
        finder: Finder,
    }

    impl Session {
        fn open(name: &str) -> Self {
            let file = TempFile::new(&format!("keys-{name}"), HISTORY);
            let history = History::load(&file.path).unwrap();
            let finder = Finder::new(history.entries(), String::new());

            Self {
                file,
                history,
                finder,
            }
        }

        fn press(
            &mut self,
            code: KeyCode,
            modifiers: KeyModifiers,
        ) -> ControlFlow<Outcome, Option<Status>> {
            handle_key(
                KeyEvent::new(code, modifiers),
                &mut self.finder,
                &mut self.history,
            )
        }

        // Presses a key that keeps the finder open without a status
        fn edit(&mut self, code: KeyCode, modifiers: KeyModifiers) {
            assert_eq!(self.press(code, modifiers), Continue(None));
        }
    }

    #[test]
    fn moves_the_selection_with_arrows_page_keys_and_ctrl_p_n() {
        let mut session = Session::open("move");

        for (code, modifiers, selected) in [
            (KeyCode::Up, NONE, 0),
            (KeyCode::Down, NONE, 1),
            (KeyCode::Char('n'), CONTROL, 2),
            (KeyCode::Down, NONE, 2),
            (KeyCode::Up, NONE, 1),
            (KeyCode::Char('p'), CONTROL, 0),
            (KeyCode::PageDown, NONE, 1),
            (KeyCode::PageUp, NONE, 0),
        ] {
            session.edit(code, modifiers);
            assert_eq!(session.finder.selected, selected);
        }
    }

    #[test]
    fn edits_the_query() {
        let mut session = Session::open("edit");

        session.edit(KeyCode::Char('e'), NONE);
        session.edit(KeyCode::Char('C'), KeyModifiers::SHIFT);
        assert_eq!(session.finder.query, "eC");

        session.edit(KeyCode::Backspace, NONE);
        assert_eq!(session.finder.query, "e");

        session.edit(KeyCode::Char('u'), CONTROL);
        assert_eq!(session.finder.query, "");
    }

    #[test]
    fn moves_the_cursor_and_edits_at_it() {
        let mut session = Session::open("cursor");

        for char in "gt lg".chars() {
            session.edit(KeyCode::Char(char), NONE);
        }

        for (code, modifiers, query, cursor) in [
            (KeyCode::Left, NONE, "gt lg", 4),
            (KeyCode::Char('o'), NONE, "gt log", 5),
            (KeyCode::Home, NONE, "gt log", 0),
            (KeyCode::Right, NONE, "gt log", 1),
            (KeyCode::Char('i'), NONE, "git log", 2),
            (KeyCode::End, NONE, "git log", 7),
            (KeyCode::Char('a'), CONTROL, "git log", 0),
            (KeyCode::Delete, NONE, "it log", 0),
            (KeyCode::Char('e'), CONTROL, "it log", 6),
            (KeyCode::Left, NONE, "it log", 5),
            (KeyCode::Left, NONE, "it log", 4),
            (KeyCode::Char('u'), CONTROL, "og", 0),
        ] {
            session.edit(code, modifiers);
            assert_eq!(
                (session.finder.query.as_str(), session.finder.cursor),
                (query, cursor)
            );
        }
    }

    #[test]
    fn moves_the_cursor_and_deletes_by_words() {
        let mut session = Session::open("words");
        const ALT: KeyModifiers = KeyModifiers::ALT;

        for char in "cd ~/src/utils".chars() {
            session.edit(KeyCode::Char(char), NONE);
        }

        for (code, modifiers, query, cursor) in [
            (KeyCode::Char('b'), ALT, "cd ~/src/utils", 9),
            (KeyCode::Left, ALT, "cd ~/src/utils", 5),
            (KeyCode::Left, CONTROL, "cd ~/src/utils", 0),
            (KeyCode::Char('f'), ALT, "cd ~/src/utils", 2),
            (KeyCode::Right, ALT, "cd ~/src/utils", 8),
            (KeyCode::Right, CONTROL, "cd ~/src/utils", 14),
            (KeyCode::Backspace, ALT, "cd ~/src/", 9),
            (KeyCode::Char('w'), CONTROL, "cd ", 3),
        ] {
            session.edit(code, modifiers);
            assert_eq!(
                (session.finder.query.as_str(), session.finder.cursor),
                (query, cursor)
            );
        }
    }

    #[test]
    fn leaves_alt_and_control_letters_out_of_the_query() {
        let mut session = Session::open("alt");

        session.edit(KeyCode::Char('x'), KeyModifiers::ALT);
        session.edit(KeyCode::Char('x'), CONTROL);

        assert_eq!(session.finder.query, "");
    }

    #[test]
    fn picks_the_selection_with_enter_and_tab() {
        let mut session = Session::open("pick");

        assert_eq!(
            session.press(KeyCode::Enter, NONE),
            Break(Outcome::Picked("echo".to_string()))
        );

        session.edit(KeyCode::Down, NONE);

        assert_eq!(
            session.press(KeyCode::Tab, NONE),
            Break(Outcome::Picked("pwd".to_string()))
        );
    }

    #[test]
    fn quits_on_enter_without_matches() {
        let mut session = Session::open("no-matches");
        session.edit(KeyCode::Char('z'), NONE);

        assert_eq!(session.press(KeyCode::Enter, NONE), Break(Outcome::Quit));
    }

    #[test]
    fn quits_on_esc_and_aborts_on_ctrl_c() {
        let mut session = Session::open("quit");

        assert_eq!(session.press(KeyCode::Esc, NONE), Break(Outcome::Quit));
        assert_eq!(
            session.press(KeyCode::Char('c'), CONTROL),
            Break(Outcome::Abort)
        );
    }

    #[test]
    fn deletes_the_selection_and_selects_the_next_command() {
        let mut session = Session::open("delete");
        session.edit(KeyCode::Down, NONE);

        assert_eq!(
            session.press(KeyCode::Char('d'), CONTROL),
            Continue(Some(Status::Deleted("deleted pwd".to_string())))
        );
        assert_eq!(session.file.read(), "ls\necho\n");
        assert_eq!(session.finder.selection().unwrap().text, "ls");
    }

    #[test]
    fn shows_the_error_when_another_shell_changed_the_history() {
        let mut session = Session::open("delete-changed");
        fs::write(&session.file.path, "date\n").unwrap();

        assert_eq!(
            session.press(KeyCode::Char('d'), CONTROL),
            Continue(Some(Status::Failed(
                "cannot delete: history file was changed by another process, try again".to_string()
            )))
        );
        assert_eq!(session.file.read(), "date\n");
        // Reloaded all the same, so the finder shows what the file holds
        assert_eq!(session.finder.selection().unwrap().text, "date");
    }

    #[test]
    fn deletes_nothing_without_matches() {
        let mut session = Session::open("delete-none");
        session.edit(KeyCode::Char('z'), NONE);
        session.edit(KeyCode::Char('d'), CONTROL);

        assert_eq!(session.file.read(), HISTORY);
    }
}
