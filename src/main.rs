mod finder;
mod history;
mod ui;

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::mem;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ratatui::backend::{Backend, ClearType, CrosstermBackend};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{cursor, terminal};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::ListState;
use ratatui::{Terminal, TerminalOptions, Viewport};

use finder::Finder;
use history::History;

const HEIGHT: u16 = 12;

const USAGE: &str = "\
Usage: hdf [QUERY]

Searches bash history in $HISTFILE and prints the chosen command. Commands can be deleted
from the history on the way, every occurrence of them.

The query matches commands containing it, ignoring case.

~/.hdfrc, sourced from .bashrc, opens hdf on Page Up and Page Down and puts the chosen command on the
prompt line, or the query as edited when leaving with Esc.

Options:
  --print-query  when quitting with Esc, or Enter and Tab without matches, print the query
                 as last edited and exit with 3
  -h, --help     print this help
  -V, --version  print the version
  --             end of options, the rest is the query

Keys:
  Up, Down, Ctrl-P, Ctrl-N  move the selection
  Enter, Tab                print the selected command; without matches, quit like Esc
  Ctrl-D                    delete the selected command, every occurrence of it
  Backspace, Ctrl-U         delete a character, clear the query
  Esc                       quit, exits with 1 (3 with --print-query)
  Ctrl-C                    quit, exits with 130

Errors exit with 2. SIGINT, SIGTERM and SIGHUP close the pane and exit with 128 + the signal.
";

enum Args {
    Search { query: String, print_query: bool },
    Help,
    Version,
}

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
    let (query, print_query) = match parse_args()? {
        Args::Search { query, print_query } => (query, print_query),
        Args::Help => {
            print!("{USAGE}");
            return Ok(ExitCode::SUCCESS);
        }
        Args::Version => {
            println!("hdf {}", env!("CARGO_PKG_VERSION"));
            return Ok(ExitCode::SUCCESS);
        }
    };

    let path = history_path()?;
    let mut history =
        History::load(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut finder = Finder::new(history.entries(), query);

    let outcome =
        pick(&mut finder, &mut history).map_err(|error| format!("terminal error: {error}"))?;

    match outcome {
        Outcome::Picked(command) => {
            println!("{command}");
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Quit if print_query => {
            println!("{}", finder.query);
            Ok(ExitCode::from(QUERY_STATUS))
        }
        Outcome::Quit => Ok(ExitCode::from(QUIT_STATUS)),
        Outcome::Abort => Ok(ExitCode::from(ABORT_STATUS)),
        Outcome::Signaled(signal) => Ok(ExitCode::from(128 + signal as u8)),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut words = Vec::new();
    let mut options = true;
    let mut print_query = false;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--" if options => options = false,
            "-h" | "--help" if options => return Ok(Args::Help),
            "-V" | "--version" if options => return Ok(Args::Version),
            "--print-query" if options => print_query = true,
            option if options && option.starts_with('-') && option.len() > 1 => {
                return Err(format!(
                    "invalid option: {option}\nTry 'hdf --help' for usage."
                ));
            }
            _ => words.push(arg),
        }
    }

    Ok(Args::Search {
        query: words.join(" "),
        print_query,
    })
}

fn history_path() -> Result<PathBuf, String> {
    match env::var_os("HISTFILE").filter(|path| !path.is_empty()) {
        Some(path) => Ok(path.into()),
        None => env::home_dir()
            .map(|home| home.join(".bash_history"))
            .ok_or_else(|| "HISTFILE and HOME are not set".to_string()),
    }
}

struct RawMode;

impl RawMode {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

// Points stdout at the terminal until dropped. crossterm asks the terminal for the cursor position
// on stdout, which is a pipe when the command is captured, as in the bash setup.
struct StdoutToTty {
    saved: OwnedFd,
}

impl StdoutToTty {
    fn new(tty: &impl AsRawFd) -> io::Result<Self> {
        let saved = io::stdout().as_fd().try_clone_to_owned()?;
        dup2(tty.as_raw_fd(), libc::STDOUT_FILENO)?;
        Ok(Self { saved })
    }
}

impl Drop for StdoutToTty {
    fn drop(&mut self) {
        let _ = io::stdout().flush();
        let _ = dup2(self.saved.as_raw_fd(), libc::STDOUT_FILENO);
    }
}

fn dup2(from: i32, to: i32) -> io::Result<()> {
    // SAFETY: dup2 only takes file descriptors; both are open for the duration of the call
    match unsafe { libc::dup2(from, to) } {
        -1 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

// Runs the finder in a pane under the cursor's line, returns how it was left
fn pick(finder: &mut Finder, history: &mut History) -> io::Result<Outcome> {
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

    let height = terminal::size()?.1.min(HEIGHT);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(tty),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;
    let mut list = ListState::default();
    let mut area = Rect::default();
    let mut status: Option<Result<String, String>> = None;

    // Raw mode keeps Ctrl-C from sending SIGINT, but other processes can still send signals:
    // take them as a way out, so the pane is cleared and the terminal restored like on Esc
    let signal = Arc::new(AtomicUsize::new(0));

    for number in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        signal_hook::flag::register_usize(number, Arc::clone(&signal), number as usize)?;
    }

    let outcome = loop {
        terminal.draw(|frame| {
            area = frame.area();
            ui::draw(frame, finder, status.as_ref(), &mut list);
        })?;

        let Some(event) = next_event(&signal)? else {
            break Outcome::Signaled(signal.load(Ordering::Relaxed));
        };
        let Event::Key(key) = event else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        status = None;

        match key.code {
            KeyCode::Esc => break Outcome::Quit,
            KeyCode::Char('c') if control => break Outcome::Abort,
            KeyCode::Enter | KeyCode::Tab => match finder.selection() {
                Some(command) => break Outcome::Picked(command.original.clone()),
                None => break Outcome::Quit,
            },
            KeyCode::Char('d') if control => status = delete(finder, history),
            KeyCode::Up => finder.up(),
            KeyCode::Char('p') if control => finder.up(),
            KeyCode::Down => finder.down(),
            KeyCode::Char('n') if control => finder.down(),
            KeyCode::Backspace => finder.pop(),
            KeyCode::Char('u') if control => finder.clear(),
            // Alt and a letter is a shortcut elsewhere, not text for the query
            KeyCode::Char(char) if !control && !alt => finder.push(char),
            _ => {}
        }
    };

    // Leave no trace of the pane and put the cursor back where it was
    let cleared = (|| {
        let backend = terminal.backend_mut();
        backend.set_cursor_position(area.as_position())?;
        backend.clear_region(ClearType::AfterCursor)?;

        if column > 0 {
            backend.set_cursor_position(Position::new(column, area.y.saturating_sub(1)))?;
        }

        terminal.show_cursor()
    })();

    // After SIGHUP the terminal may be gone, the signal still decides the exit status
    match outcome {
        Outcome::Signaled(_) => Ok(outcome),
        _ => cleared.map(|()| outcome),
    }
}

// The next terminal event, or None once a signal arrived
fn next_event(signal: &AtomicUsize) -> io::Result<Option<Event>> {
    loop {
        if signal.load(Ordering::Relaxed) != 0 {
            return Ok(None);
        }

        match event::poll(Duration::from_millis(100)) {
            Ok(true) => return event::read().map(Some),
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

// Deletes every occurrence of the selected command and reloads the history,
// returns the message for the status line
fn delete(finder: &mut Finder, history: &mut History) -> Option<Result<String, String>> {
    let command = finder.selection()?;
    let message = format!("deleted {}", command.text);
    let deleted = history
        .delete(&command.entries)
        .map_err(|error| format!("cannot delete: {error}"));
    let reloaded = history
        .reload()
        .map_err(|error| format!("cannot read history: {error}"));

    // Keep the selection in place, on the command after the deleted one
    let selected = finder.selected;
    *finder = Finder::new(history.entries(), mem::take(&mut finder.query));
    finder.select(selected);

    Some(deleted.and(reloaded).map(|()| message))
}
