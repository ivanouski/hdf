mod cli;
mod config;
mod context;
mod finder;
mod history;
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
use ratatui::layout::{Position, Rect};
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
    let (query, print_query) = match cli::parse_args()? {
        Args::Search { query, print_query } => (query, print_query),
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

    let outcome = pick(&mut finder, &mut history, &config.colors).context("terminal error")?;

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
fn pick(finder: &mut Finder, history: &mut History, theme: &Theme) -> io::Result<Outcome> {
    // Draw on the terminal itself so the finder works with stdout captured
    let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    let _stdout = StdoutToTty::new(&tty)?;
    let _raw_mode = RawMode::enable()?;

    // Bash clears the prompt line before running a bind -x command, so the pane opens there.
    // When the cursor is still on some text, it opens on the next line to keep that text.
    let (column, _) = cursor::position()?;

    if column > 0 {
        tty.write_all(b"\r\n")?;
    }

    let height = terminal::size()?.1.min(PANE_HEIGHT);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(tty),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;
    let mut list = ListState::default();
    let mut area = Rect::default();
    let mut status = None;
    let signal = tty::watch_signals()?;

    let outcome = loop {
        terminal.draw(|frame| {
            area = frame.area();
            ui::draw(frame, finder, theme, status.as_ref(), &mut list);
        })?;

        let Some(event) = tty::next_event(&signal)? else {
            break Outcome::Signaled(signal.load(Ordering::Relaxed));
        };
        let Event::Key(key) = event else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        match handle_key(key, finder, history) {
            Break(outcome) => break outcome,
            Continue(next_status) => status = next_status,
        }
    };

    let cleared = clear_pane(&mut terminal, area, column);

    // After SIGHUP the terminal may be gone, the signal still decides the exit status
    match outcome {
        Outcome::Signaled(_) => Ok(outcome),
        _ => cleared.map(|()| outcome),
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
        (KeyCode::Up, _) | (KeyCode::Char('p'), true) => finder.up(),
        (KeyCode::Down, _) | (KeyCode::Char('n'), true) => finder.down(),
        (KeyCode::Backspace, _) => finder.pop(),
        (KeyCode::Char('u'), true) => finder.clear(),
        // Alt and a letter is a shortcut elsewhere, not text for the query
        (KeyCode::Char(char), false) if !alt => finder.push(char),
        _ => {}
    }

    Continue(None)
}

// Leaves no trace of the pane and puts the cursor back where it was
fn clear_pane(
    terminal: &mut Terminal<CrosstermBackend<File>>,
    area: Rect,
    column: u16,
) -> io::Result<()> {
    let backend = terminal.backend_mut();
    backend.set_cursor_position(area.as_position())?;
    backend.clear_region(ClearType::AfterCursor)?;

    if column > 0 {
        backend.set_cursor_position(Position::new(column, area.y.saturating_sub(1)))?;
    }

    terminal.show_cursor()
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
