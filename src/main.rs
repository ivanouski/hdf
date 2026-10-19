//! hdf, a finder for bash history: prints the command picked from $HISTFILE

mod app;
mod cli;
mod config;
mod context;
mod finder;
mod history;
mod pane;
mod prompt;
#[cfg(test)]
mod test_util;
mod tty;
mod ui;

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use app::Outcome;
use cli::Args;
use config::Config;
use context::Context;
use finder::Finder;
use history::History;

// .hdfrc tells these apart: a pick runs, an edit or the query goes on the prompt line, an error or
// Ctrl-C leaves the line as it was. Printing the query has a status of its own, so .hdfrc does not
// take a failure of an hdf without --print-query, an older one, for an empty query.
const QUIT_STATUS: u8 = 1;
const ERROR_STATUS: u8 = 2;
const QUERY_STATUS: u8 = 3;
const EDIT_STATUS: u8 = 4;
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
            print_line(&Config::init()?.display().to_string())?;
            return Ok(ExitCode::SUCCESS);
        }
        Args::Help => {
            print(&cli::help())?;
            return Ok(ExitCode::SUCCESS);
        }
        Args::Version => {
            print_line(&format!("hdf {}", env!("CARGO_PKG_VERSION")))?;
            return Ok(ExitCode::SUCCESS);
        }
    };

    let config = Config::load()?;
    let mut history = load_history()?;
    let mut finder = Finder::new(history.entries(), query);
    let prompt = prompt.map(|text| prompt::parse(&text));

    let outcome = app::pick(&mut finder, &mut history, &config.colors, prompt.as_ref())
        .context("terminal error")?;

    match outcome {
        Outcome::Picked(command) => {
            print_line(&command)?;
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Edited(command) => {
            print_line(&command)?;
            Ok(ExitCode::from(EDIT_STATUS))
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

/// Prints the deleted commands, oldest first. Fewer commands than asked for deletes them all.
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
    print(&format!("{text}\n"))
}

fn print(text: &str) -> Result<(), String> {
    write!(io::stdout(), "{text}").context("cannot print")
}
