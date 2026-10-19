//! The command line arguments and the help text

use std::ffi::OsString;

use crate::config;

const USAGE: &str = "\
Usage: hdf [OPTIONS] [QUERY]

Search bash history in $HISTFILE and print the chosen command. The query matches
case-insensitively.

Options:
  --print-query      on Esc, print the query and exit with 3
  --prompt TEXT      show TEXT, the shell prompt as bash prints it, instead of >
  --delete-last [N]  delete the last N commands from history, 1 by default
  --init-config      write the default config, unless it exists
  -h, --help         print this help
  -V, --version      print the version

Keys:
  Up, Down, Ctrl-P, Ctrl-N  move the selection
  PgUp, PgDn                move the selection by a page
  Enter                     print the selected command
  Tab                       print the selected command, exit with 4
  Ctrl-D                    delete the selected command from history
  Left, Right, Home, End    move the cursor in the query
  Ctrl-A, Ctrl-E            move the cursor to the start, the end
  Alt-B, Alt-F              move the cursor a word back, forward
  Alt/Ctrl-Left, -Right     move the cursor a word back, forward
  Backspace, Delete         delete the character before, at the cursor
  Alt-Backspace             delete the word before the cursor
  Ctrl-W                    delete back to whitespace
  Ctrl-U                    delete the query before the cursor
  Esc                       quit, exit with 1
  Ctrl-C                    quit, exit with 130

Without matches, Enter and Tab quit like Esc.

Config: $XDG_CONFIG_HOME/hdf/config.toml, defaults:

{defaults}

Errors exit with 2, signals with 128 + the signal.
";

const DELETE_LAST_ALONE: &str = "--delete-last takes no other arguments";
const PROMPT_TEXT: &str = "--prompt takes the prompt text";

#[derive(Debug, PartialEq)]
pub enum Args {
    Search {
        query: String,
        print_query: bool,
        prompt: Option<String>,
    },
    DeleteLast(usize),
    InitConfig,
    Help,
    Version,
}

/// The arguments without the program name
pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut words = Vec::new();
    let mut options = true;
    let mut print_query = false;
    let mut prompt = None;

    let args: Vec<OsString> = args.into_iter().collect();

    if args.first().is_some_and(|arg| arg == "--delete-last") {
        return match &args[1..] {
            [] => Ok(Args::DeleteLast(1)),
            // Asked for as anywhere else, not taken for a count
            [help] if help == "-h" || help == "--help" => Ok(Args::Help),
            [version] if version == "-V" || version == "--version" => Ok(Args::Version),
            [count] => parse_count(&count.to_string_lossy()).map(Args::DeleteLast),
            _ => Err(usage_error(DELETE_LAST_ALONE)),
        };
    }

    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        // A query with its invalid bytes replaced would change the line bash gets back
        let Some(text) = arg.to_str() else {
            return Err(format!("not valid UTF-8: {}", arg.to_string_lossy()));
        };

        match text {
            "--" if options => options = false,
            "-h" | "--help" if options => return Ok(Args::Help),
            "-V" | "--version" if options => return Ok(Args::Version),
            "--init-config" if options => return Ok(Args::InitConfig),
            "--print-query" if options => print_query = true,
            // Taken as is, a prompt may start with a dash. It is only shown, so invalid bytes
            // become replacement characters.
            "--prompt" if options => match args.next() {
                Some(text) => prompt = Some(text.to_string_lossy().into_owned()),
                None => return Err(usage_error(PROMPT_TEXT)),
            },
            "--delete-last" if options => {
                return Err(usage_error(DELETE_LAST_ALONE));
            }
            option if options && option.starts_with('-') && option.len() > 1 => {
                return Err(usage_error(&format!("invalid option: {option}")));
            }
            _ => words.push(text.to_string()),
        }
    }

    Ok(Args::Search {
        query: words.join(" "),
        print_query,
        prompt,
    })
}

/// Digits only, so a sign or an option in its place is rejected
fn parse_count(count: &str) -> Result<usize, String> {
    let digits = !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit());

    match count.parse() {
        Ok(number) if digits && number > 0 => Ok(number),
        _ => Err(usage_error(&format!("invalid count: {count}"))),
    }
}

fn usage_error(message: &str) -> String {
    format!("{message}\nTry 'hdf --help' for usage.")
}

/// The usage with the default config, indented
pub fn help() -> String {
    let defaults: String = config::DEFAULT
        .lines()
        .flat_map(|line| ["    ", line, "\n"])
        .collect();

    USAGE.replace("{defaults}\n", &defaults)
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStringExt;

    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(OsString::from))
    }

    fn invalid_utf8(text: &[u8]) -> OsString {
        OsString::from_vec(text.to_vec())
    }

    fn search(query: &str, print_query: bool) -> Args {
        Args::Search {
            query: query.to_string(),
            print_query,
            prompt: None,
        }
    }

    #[test]
    fn joins_the_words_into_the_query() {
        assert_eq!(parse(&[]), Ok(search("", false)));
        assert_eq!(parse(&["git", "log"]), Ok(search("git log", false)));
    }

    #[test]
    fn reads_options_between_the_words() {
        assert_eq!(
            parse(&["git", "--print-query", "log"]),
            Ok(search("git log", true))
        );
    }

    #[test]
    fn takes_the_next_argument_for_the_prompt() {
        assert_eq!(
            parse(&["--prompt", "-$ ", "--print-query", "--", "git"]),
            Ok(Args::Search {
                query: "git".to_string(),
                print_query: true,
                prompt: Some("-$ ".to_string()),
            })
        );
    }

    #[test]
    fn replaces_invalid_bytes_in_the_prompt() {
        let args = [OsString::from("--prompt"), invalid_utf8(b"\xff$ ")];

        assert_eq!(
            parse_args(args),
            Ok(Args::Search {
                query: String::new(),
                print_query: false,
                prompt: Some("\u{fffd}$ ".to_string()),
            })
        );
    }

    #[test]
    fn rejects_invalid_bytes_in_the_query() {
        assert_eq!(
            parse_args([invalid_utf8(b"ls \xff")]),
            Err("not valid UTF-8: ls \u{fffd}".to_string())
        );
    }

    #[test]
    fn rejects_a_prompt_option_without_the_text() {
        assert!(parse(&["--prompt"]).unwrap_err().starts_with(PROMPT_TEXT));
    }

    #[test]
    fn takes_everything_after_a_double_dash_for_the_query() {
        assert_eq!(
            parse(&["--", "--help", "-x", "--delete-last"]),
            Ok(search("--help -x --delete-last", false))
        );
    }

    #[test]
    fn takes_a_lone_dash_for_the_query() {
        assert_eq!(parse(&["cd", "-"]), Ok(search("cd -", false)));
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
    fn shows_help_and_version_after_delete_last() {
        for help in ["-h", "--help"] {
            assert_eq!(parse(&["--delete-last", help]), Ok(Args::Help));
        }
        for version in ["-V", "--version"] {
            assert_eq!(parse(&["--delete-last", version]), Ok(Args::Version));
        }
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
        for count in ["0", "-1", "+1", "1x", "", "--print-query"] {
            let error = parse(&["--delete-last", count]).unwrap_err();

            assert!(error.starts_with(&format!("invalid count: {count}\n")));
        }
    }
}
