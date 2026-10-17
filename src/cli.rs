use std::env;

use crate::config;

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

const DELETE_LAST_ALONE: &str = "--delete-last takes no other arguments";

pub enum Args {
    Search { query: String, print_query: bool },
    DeleteLast(usize),
    InitConfig,
    Help,
    Version,
}

pub fn parse_args() -> Result<Args, String> {
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

// The usage with the default config, indented
pub fn help() -> String {
    let defaults: String = config::DEFAULT
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect();

    USAGE.replace("{defaults}\n", &defaults)
}
