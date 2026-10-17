mod config;
mod finder;
mod history;
mod ui;

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::mem;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::{self, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use ratatui::backend::{Backend, ClearType, CrosstermBackend};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{cursor, terminal};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::ListState;
use ratatui::{Terminal, TerminalOptions, Viewport};

use config::{Config, Theme};
use finder::Finder;
use history::History;

const HEIGHT: u16 = 12;

const USAGE: &str = "\
Usage: hdf [OPTIONS] [QUERY]

Search bash history in $HISTFILE and print the chosen command. The query matches
case-insensitively.

Options:
  --print-query      on Esc, print the query and exit with 3
  --delete-last [N]  delete the last N commands from history, 1 by default
  --init-config      write the default config, unless it exists
  -h, --help         print this help
  -V, --version      print the version

Keys:
  Up, Down, Ctrl-P, Ctrl-N  move the selection
  Enter, Tab                print the selected command
  Ctrl-D                    delete the selected command from history
  Backspace, Ctrl-U         delete a character, clear the query
  Esc                       quit, exit with 1
  Ctrl-C                    quit, exit with 130

Config: $XDG_CONFIG_HOME/hdf/config.toml, defaults:

{defaults}

Errors exit with 2, signals with 128 + the signal.
";

enum Args {
    Search { query: String, print_query: bool },
    DeleteLast(usize),
    InitConfig,
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

const DELETE_LAST_ALONE: &str = "--delete-last takes no other arguments";

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
        Args::DeleteLast(count) => {
            delete_last(count)?;
            return Ok(ExitCode::SUCCESS);
        }
        Args::InitConfig => {
            println!("{}", Config::init()?.display());
            return Ok(ExitCode::SUCCESS);
        }
        Args::Help => {
            let defaults: String = config::DEFAULT
                .lines()
                .map(|line| format!("    {line}\n"))
                .collect();
            print!("{}", USAGE.replace("{defaults}\n", &defaults));
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

    let outcome = pick(&mut finder, &mut history, &config.colors)
        .map_err(|error| format!("terminal error: {error}"))?;

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
        Outcome::Signaled(signal) => Ok(ExitCode::from(128 + signal as u8)),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut words = Vec::new();
    let mut options = true;
    let mut print_query = false;

    let args: Vec<String> = env::args().skip(1).collect();

    if args.first().is_some_and(|arg| arg == "--delete-last") {
        return match &args[1..] {
            [] => Ok(Args::DeleteLast(1)),
            [count] => parse_count(count).map(Args::DeleteLast),
            _ => Err(usage_error(DELETE_LAST_ALONE)),
        };
    }

    for arg in args {
        match arg.as_str() {
            "--" if options => options = false,
            "-h" | "--help" if options => return Ok(Args::Help),
            "-V" | "--version" if options => return Ok(Args::Version),
            "--init-config" if options => return Ok(Args::InitConfig),
            "--print-query" if options => print_query = true,
            "--delete-last" if options => {
                return Err(usage_error(DELETE_LAST_ALONE));
            }
            option if options && option.starts_with('-') && option.len() > 1 => {
                return Err(usage_error(&format!("invalid option: {option}")));
            }
            _ => words.push(arg),
        }
    }

    Ok(Args::Search {
        query: words.join(" "),
        print_query,
    })
}

// Digits only, so a sign or an option in its place is rejected
fn parse_count(count: &str) -> Result<usize, String> {
    Some(count)
        .filter(|count| !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|count| count.parse().ok())
        .filter(|&count| count > 0)
        .ok_or_else(|| usage_error(&format!("invalid count: {count}")))
}

fn usage_error(message: &str) -> String {
    format!("{message}\nTry 'hdf --help' for usage.")
}

// Prints the deleted commands, oldest first. Fewer commands than asked for deletes them all.
fn delete_last(count: usize) -> Result<(), String> {
    let history = load_history()?;
    let entries = history.entries();
    let indices: Vec<usize> = (entries.len().saturating_sub(count)..entries.len()).collect();

    if indices.is_empty() {
        return Ok(());
    }

    history
        .delete(&indices)
        .map_err(|error| format!("cannot delete: {error}"))?;

    // The commands are already deleted, so a closed stdout is no reason to fail
    let mut stdout = io::stdout().lock();

    for &index in &indices {
        if writeln!(stdout, "{}", entries[index].original).is_err() {
            break;
        }
    }

    Ok(())
}

fn print_line(text: &str) -> Result<(), String> {
    writeln!(io::stdout(), "{text}").map_err(|error| format!("cannot print: {error}"))
}

fn load_history() -> Result<History, String> {
    let path = history_path()?;

    History::load(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))
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

    exit_when_stuck(Arc::clone(&signal));

    let outcome = loop {
        terminal.draw(|frame| {
            area = frame.area();
            ui::draw(frame, finder, theme, status.as_ref(), &mut list);
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

// crossterm keeps reading a closed terminal and never returns, so the loop in pick never sees the
// signal that came with closing it. Exits for the signal once the loop has had time to clean up.
fn exit_when_stuck(signal: Arc<AtomicUsize>) {
    // Compared with the parent at the start, not with 1: a shell that is PID 1, as in a container,
    // runs hdf as its child
    let parent = std::os::unix::process::parent_id();

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));

            // Orphaned: the shell is gone, maybe without a signal reaching hdf. Taken for SIGHUP,
            // so a loop that is not stuck closes the pane and restores the terminal.
            if std::os::unix::process::parent_id() != parent {
                let hangup = libc::SIGHUP as usize;
                let _ = signal.compare_exchange(0, hangup, Ordering::Relaxed, Ordering::Relaxed);
            }

            let number = signal.load(Ordering::Relaxed);

            if number != 0 {
                thread::sleep(Duration::from_millis(500));
                process::exit(128 + number as i32);
            }
        }
    });
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
