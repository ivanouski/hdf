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

#[derive(Debug, PartialEq)]
pub enum Args {
    Search { query: String, print_query: bool },
    DeleteLast(usize),
    InitConfig,
    Help,
    Version,
}

// The arguments without the program name
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut words = Vec::new();
    let mut options = true;
    let mut print_query = false;

    let args: Vec<String> = args.into_iter().collect();

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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    fn search(query: &str, print_query: bool) -> Result<Args, String> {
        Ok(Args::Search {
            query: query.to_string(),
            print_query,
        })
    }

    #[test]
    fn joins_the_words_into_the_query() {
        assert_eq!(parse(&[]), search("", false));
        assert_eq!(parse(&["git", "log"]), search("git log", false));
    }

    #[test]
    fn reads_options_between_the_words() {
        assert_eq!(
            parse(&["git", "--print-query", "log"]),
            search("git log", true)
        );
    }

    #[test]
    fn takes_everything_after_a_double_dash_for_the_query() {
        assert_eq!(
            parse(&["--", "--help", "-x", "--delete-last"]),
            search("--help -x --delete-last", false)
        );
    }

    #[test]
    fn takes_a_lone_dash_for_the_query() {
        assert_eq!(parse(&["cd", "-"]), search("cd -", false));
    }

    #[test]
    fn reads_help_version_and_init_config_anywhere() {
        assert_eq!(parse(&["ls", "--help"]), Ok(Args::Help));
        assert_eq!(parse(&["-h"]), Ok(Args::Help));
        assert_eq!(parse(&["ls", "-V"]), Ok(Args::Version));
        assert_eq!(parse(&["--version"]), Ok(Args::Version));
        assert_eq!(parse(&["ls", "--init-config"]), Ok(Args::InitConfig));
    }

    #[test]
    fn rejects_unknown_options() {
        let error = parse(&["-x"]).unwrap_err();

        assert!(error.starts_with("invalid option: -x\n"));
        assert!(error.ends_with("Try 'hdf --help' for usage."));
    }

    #[test]
    fn deletes_one_command_by_default() {
        assert_eq!(parse(&["--delete-last"]), Ok(Args::DeleteLast(1)));
        assert_eq!(parse(&["--delete-last", "10"]), Ok(Args::DeleteLast(10)));
    }

    #[test]
    fn rejects_delete_last_with_other_arguments() {
        for args in [
            &["--delete-last", "1", "2"][..],
            &["ls", "--delete-last"],
            &["--print-query", "--delete-last"],
        ] {
            assert!(parse(args).unwrap_err().starts_with(DELETE_LAST_ALONE));
        }
    }

    #[test]
    fn rejects_counts_other_than_positive_numbers() {
        for count in ["0", "-1", "+1", "1x", "", "--help"] {
            let error = parse(&["--delete-last", count]).unwrap_err();

            assert!(error.starts_with(&format!("invalid count: {count}\n")));
        }
    }
}
