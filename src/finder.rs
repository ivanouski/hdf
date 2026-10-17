use std::collections::HashMap;
use std::mem;
use std::ops::Range;

use chrono::{DateTime, Local};

use crate::history::Entry;

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M";
// The width of every time, years have four digits
pub const TIME_WIDTH: usize = "2023-11-14 22:13".len();

// A distinct command and every history entry holding it, most recent first
pub struct Command {
    // As shown in the finder, without trailing whitespace
    pub text: String,
    // As stored in the most recent entry, for putting on the prompt line
    pub original: String,
    // Local time of the most recent entry with a timestamp
    pub time: Option<String>,
    pub entry_indices: Vec<usize>,
}

pub struct Match {
    pub command_index: usize,
    // Bytes of the command matched by the query, for highlighting
    pub range: Range<usize>,
}

pub struct Finder {
    pub commands: Vec<Command>,
    pub query: String,
    pub matches: Vec<Match>,
    pub selected: usize,
}

impl Finder {
    pub fn new(entries: &[Entry], query: String) -> Self {
        let mut commands: Vec<Command> = Vec::new();
        let mut positions: HashMap<&str, usize> = HashMap::new();

        // Bash keeps the trailing space of a completed word, so `ls ` and `ls` count as one command
        for (index, entry) in entries.iter().enumerate().rev() {
            let text = entry.text.trim_end();

            match positions.get(text) {
                Some(&position) => {
                    let command = &mut commands[position];
                    command.entry_indices.push(index);
                    command.time = command.time.take().or_else(|| local_time(entry.time?));
                }
                None => {
                    positions.insert(text, commands.len());
                    commands.push(Command {
                        text: text.to_string(),
                        original: entry.original.clone(),
                        time: entry.time.and_then(local_time),
                        entry_indices: vec![index],
                    });
                }
            }
        }

        let mut finder = Self {
            commands,
            query,
            matches: Vec::new(),
            selected: 0,
        };
        finder.refilter();
        finder
    }

    // Rebuilds the commands from reloaded entries, keeping the query and the selection
    pub fn reload(&mut self, entries: &[Entry]) {
        let selected = self.selected;
        *self = Self::new(entries, mem::take(&mut self.query));
        self.select(selected);
    }

    pub fn push(&mut self, char: char) {
        self.query.push(char);
        self.refilter();
    }

    pub fn pop(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn down(&mut self) {
        self.select(self.selected + 1);
    }

    pub fn selection(&self) -> Option<&Command> {
        self.matches
            .get(self.selected)
            .map(|found| &self.commands[found.command_index])
    }

    // Moves the selection, staying within the matches
    fn select(&mut self, index: usize) {
        self.selected = index.min(self.matches.len().saturating_sub(1));
    }

    fn refilter(&mut self) {
        self.matches = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(command_index, Command { text, .. })| {
                let range = find(text, &self.query)?;

                Some(Match {
                    command_index,
                    range,
                })
            })
            .collect();
        self.selected = 0;
    }
}

fn local_time(seconds: i64) -> Option<String> {
    let time = DateTime::from_timestamp(seconds, 0)?.with_timezone(&Local);

    Some(time.format(TIME_FORMAT).to_string())
}

// Case-insensitive substring search
fn find(haystack: &str, needle: &str) -> Option<Range<usize>> {
    let mut starts = haystack
        .char_indices()
        .map(|(start, _)| start)
        .chain([haystack.len()]);

    starts.find_map(|start| Some(start..start + prefix_len(&haystack[start..], needle)?))
}

// Byte length of needle at the start of haystack, ignoring case
fn prefix_len(haystack: &str, needle: &str) -> Option<usize> {
    let mut chars = haystack.chars();
    let mut len = 0;

    for expected in needle.chars() {
        let char = chars.next().filter(|&char| fold(char) == fold(expected))?;
        len += char.len_utf8();
    }

    Some(len)
}

fn fold(char: char) -> char {
    char.to_lowercase().next().unwrap_or(char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times_as_wide_as_the_column() {
        assert_eq!(local_time(0).unwrap().len(), TIME_WIDTH);
        assert_eq!(local_time(1_700_000_000).unwrap().len(), TIME_WIDTH);
    }

    #[test]
    fn finds_substrings_ignoring_case() {
        assert_eq!(find("sbx run Claude", "clau"), Some(8..12));
        assert_eq!(find("CLAUDE", "clau"), Some(0..4));
        assert_eq!(find("cl-au", "clau"), None);
    }

    #[test]
    fn finds_the_first_occurrence() {
        assert_eq!(find("ab ab", "ab"), Some(0..2));
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(find("ls", ""), Some(0..0));
        assert_eq!(find("", ""), Some(0..0));
    }

    #[test]
    fn treats_special_characters_literally() {
        assert_eq!(find("ls *.rb", "*.rb"), Some(3..7));
        assert_eq!(find("claude", "*aude"), None);
    }

    #[test]
    fn handles_multibyte_characters() {
        assert_eq!(find("echo Привет", "привет"), Some(5..17));
        assert_eq!(find("ünï", "ï"), Some(3..5));
    }
}
