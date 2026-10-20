//! The distinct commands, the query and its matches, apart from drawing them

use std::collections::HashMap;
use std::mem;
use std::ops::Range;

use chrono::{DateTime, Local};

use crate::history::Entry;

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M";
/// The width of every time, years have four digits
pub const TIME_WIDTH: usize = "2023-11-14 22:13".len();

/// A distinct command and every history entry holding it, most recent first
#[derive(Debug)]
pub struct Command {
    /// As shown in the finder, without trailing whitespace
    pub text: String,
    /// As stored in the most recent entry, for putting on the prompt line
    pub original: String,
    /// Local time of the most recent entry with a timestamp
    pub time: Option<String>,
    pub entry_indices: Vec<usize>,
}

#[derive(Debug)]
pub struct Match {
    pub command_index: usize,
    /// Bytes of the command matched by the words of the query, for highlighting: sorted, apart
    /// from each other, none for a query without words
    pub ranges: Vec<Range<usize>>,
}

#[derive(Debug)]
pub struct Finder {
    commands: Vec<Command>,
    /// Whether any command has a time, so the finder shows a column for them
    has_times: bool,
    query: String,
    /// Bytes into the query, where typing goes in, always on a character boundary
    cursor: usize,
    /// The commands the query matches, kept in step with it
    matches: Vec<Match>,
    /// Index into the matches, 0 without any
    selected: usize,
}

impl Finder {
    pub fn new(entries: &[Entry], query: String) -> Self {
        let commands = commands(entries);
        let mut finder = Self {
            has_times: commands.iter().any(|command| command.time.is_some()),
            commands,
            cursor: query.len(),
            query,
            matches: Vec::new(),
            selected: 0,
        };
        finder.refilter();
        finder
    }

    /// Rebuilds the commands from reloaded entries, keeping the query, its cursor and the selection
    pub fn reload(&mut self, entries: &[Entry]) {
        let (selected, cursor) = (self.selected, self.cursor);
        *self = Self::new(entries, mem::take(&mut self.query));
        self.cursor = cursor;
        self.select(selected);
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    pub fn has_times(&self) -> bool {
        self.has_times
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn push(&mut self, c: char) {
        self.query.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.refilter();
    }

    pub fn delete_back(&mut self) {
        if let Some(start) = self.previous_boundary() {
            self.delete_back_to(start);
        }
    }

    pub fn delete_forward(&mut self) {
        if self.cursor < self.query.len() {
            self.query.remove(self.cursor);
            self.refilter();
        }
    }

    /// Like Ctrl-U in bash, keeps what is after the cursor
    pub fn delete_to_start(&mut self) {
        self.delete_back_to(0);
    }

    /// Letters and digits, like Alt-Backspace in bash
    pub fn delete_word_back(&mut self) {
        self.delete_back_to(self.word_start(char::is_alphanumeric));
    }

    /// Everything up to whitespace, like Ctrl-W in bash
    pub fn delete_big_word_back(&mut self) {
        self.delete_back_to(self.word_start(|c| !c.is_whitespace()));
    }

    pub fn left(&mut self) {
        if let Some(start) = self.previous_boundary() {
            self.cursor = start;
        }
    }

    pub fn right(&mut self) {
        if let Some(end) = self.next_boundary() {
            self.cursor = end;
        }
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_start(char::is_alphanumeric);
    }

    pub fn word_right(&mut self) {
        let after = &self.query[self.cursor..];
        let rest = after
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .trim_start_matches(char::is_alphanumeric);

        self.cursor = self.query.len() - rest.len();
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.query.len();
    }

    pub fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn down(&mut self) {
        self.select(self.selected + 1);
    }

    pub fn page_up(&mut self, rows: usize) {
        self.selected = self.selected.saturating_sub(rows.max(1));
    }

    pub fn page_down(&mut self, rows: usize) {
        self.select(self.selected + rows.max(1));
    }

    /// Moves the selection, staying within the matches
    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.matches.len().saturating_sub(1));
    }

    pub fn selection(&self) -> Option<&Command> {
        self.matches
            .get(self.selected)
            .map(|found| &self.commands[found.command_index])
    }

    fn delete_back_to(&mut self, start: usize) {
        if start < self.cursor {
            self.query.drain(start..self.cursor);
            self.cursor = start;
            self.refilter();
        }
    }

    /// Where the word before the cursor starts, skipping the characters between. `in_word` tells
    /// which characters words are made of.
    fn word_start(&self, in_word: impl Fn(char) -> bool) -> usize {
        self.query[..self.cursor]
            .trim_end_matches(|c| !in_word(c))
            .trim_end_matches(in_word)
            .len()
    }

    /// Where the character before the cursor starts
    fn previous_boundary(&self) -> Option<usize> {
        let (start, _) = self.query[..self.cursor].char_indices().next_back()?;

        Some(start)
    }

    /// Where the character after the cursor ends
    fn next_boundary(&self) -> Option<usize> {
        let c = self.query[self.cursor..].chars().next()?;

        Some(self.cursor + c.len_utf8())
    }

    fn refilter(&mut self) {
        self.matches = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(command_index, Command { text, .. })| {
                let ranges = find_words(text, &self.query)?;

                Some(Match {
                    command_index,
                    ranges,
                })
            })
            .collect();
        self.selected = 0;
    }
}

/// One command per distinct text, most recent first, with the entries it ran as. Bash keeps the
/// trailing space of a completed word, so `ls ` and `ls` count as one command.
fn commands(entries: &[Entry]) -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::new();
    let mut positions: HashMap<&str, usize> = HashMap::new();

    for (index, entry) in entries.iter().enumerate().rev() {
        let text = entry.text.trim_end();

        if let Some(&position) = positions.get(text) {
            let command = &mut commands[position];
            command.entry_indices.push(index);
            command.time = command.time.take().or_else(|| local_time(entry.time?));
        } else {
            positions.insert(text, commands.len());
            commands.push(Command {
                text: text.to_string(),
                original: entry.original.clone(),
                time: entry.time.and_then(local_time),
                entry_indices: vec![index],
            });
        }
    }

    commands
}

fn local_time(seconds: i64) -> Option<String> {
    let time = DateTime::from_timestamp(seconds, 0)?.with_timezone(&Local);

    Some(time.format(TIME_FORMAT).to_string())
}

/// Where each word of the query is in haystack, in any order, ignoring case. A word matches at
/// its first occurrence, and ranges that overlap or touch are merged. None when a word is missing.
fn find_words(haystack: &str, query: &str) -> Option<Vec<Range<usize>>> {
    let mut found = query
        .split_whitespace()
        .map(|word| find(haystack, word))
        .collect::<Option<Vec<_>>>()?;
    found.sort_by_key(|range| range.start);

    let mut ranges: Vec<Range<usize>> = Vec::with_capacity(found.len());

    for range in found {
        match ranges.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => ranges.push(range),
        }
    }

    Some(ranges)
}

/// Case-insensitive substring search
fn find(haystack: &str, needle: &str) -> Option<Range<usize>> {
    let mut starts = haystack
        .char_indices()
        .map(|(start, _)| start)
        .chain([haystack.len()]);

    starts.find_map(|start| Some(start..start + prefix_len(&haystack[start..], needle)?))
}

/// Byte length of needle at the start of haystack, ignoring case
fn prefix_len(haystack: &str, needle: &str) -> Option<usize> {
    let mut chars = haystack.chars();
    let mut len = 0;

    for expected in needle.chars() {
        let c = chars.next().filter(|&c| fold(c) == fold(expected))?;
        len += c.len_utf8();
    }

    Some(len)
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
// A command matched in one place has one range of bytes, not a range to collect
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;
    use crate::history;

    /// Two commands match p
    const HISTORY: &[u8] = b"ls\npwd\nps\n";

    fn finder(query: &str) -> Finder {
        Finder::new(&history::parse(HISTORY), query.to_string())
    }

    #[test]
    fn starts_with_the_cursor_after_the_query() {
        assert_eq!(finder("ls").cursor, 2);
    }

    #[test]
    fn types_and_deletes_at_the_cursor() {
        let mut finder = finder("sbx rn");

        finder.left();
        finder.push('u');
        assert_eq!((finder.query.as_str(), finder.cursor), ("sbx run", 6));

        finder.delete_back();
        assert_eq!((finder.query.as_str(), finder.cursor), ("sbx rn", 5));

        finder.delete_forward();
        assert_eq!((finder.query.as_str(), finder.cursor), ("sbx r", 5));
    }

    #[test]
    fn moves_the_cursor_by_characters_within_the_query() {
        let mut finder = finder("äö");

        finder.left();
        assert_eq!(finder.cursor, 2);
        finder.left();
        finder.left();
        assert_eq!(finder.cursor, 0);
        finder.right();
        assert_eq!(finder.cursor, 2);
        finder.end();
        finder.right();
        assert_eq!(finder.cursor, 4);
        finder.home();
        assert_eq!(finder.cursor, 0);
    }

    #[test]
    fn deletes_before_the_cursor_like_bash() {
        let mut finder = finder("git log");

        for _ in 0..3 {
            finder.left();
        }
        finder.delete_to_start();

        assert_eq!((finder.query.as_str(), finder.cursor), ("log", 0));
    }

    #[test]
    fn deletes_nothing_past_the_ends() {
        let mut finder = finder("p");
        finder.down();

        finder.delete_forward();
        finder.home();
        finder.delete_back();
        finder.delete_to_start();

        // The selection stays, as the matches did not change
        assert_eq!((finder.query.as_str(), finder.selected), ("p", 1));
    }

    #[test]
    fn moves_the_selection_by_pages_and_stops_at_the_ends() {
        let mut finder = finder("");

        finder.page_down(2);
        assert_eq!(finder.selected, 2);

        finder.page_down(2);
        assert_eq!(finder.selected, 2);

        finder.page_up(1);
        assert_eq!(finder.selected, 1);

        finder.page_up(2);
        assert_eq!(finder.selected, 0);

        // A pane too short for any rows still moves
        finder.page_down(0);
        assert_eq!(finder.selected, 1);
    }

    #[test]
    fn moves_the_cursor_by_words_of_letters_and_digits() {
        let mut finder = finder("git log --all");

        for cursor in [10, 4, 0, 0] {
            finder.word_left();
            assert_eq!(finder.cursor, cursor);
        }

        for cursor in [3, 7, 13, 13] {
            finder.word_right();
            assert_eq!(finder.cursor, cursor);
        }
    }

    #[test]
    fn deletes_words_before_the_cursor() {
        let mut finder = finder("cd ~/src/utils ");

        finder.delete_word_back();
        assert_eq!((finder.query.as_str(), finder.cursor), ("cd ~/src/", 9));

        finder.delete_big_word_back();
        assert_eq!((finder.query.as_str(), finder.cursor), ("cd ", 3));

        finder.home();
        finder.delete_big_word_back();
        assert_eq!(finder.query, "cd ");
    }

    #[test]
    fn keeps_the_cursor_on_reload() {
        let mut finder = finder("ls");

        finder.left();
        finder.reload(&history::parse(HISTORY));

        assert_eq!(finder.cursor, 1);
    }

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
    fn finds_every_word_in_any_order() {
        assert_eq!(
            find_words("git -C api push", "push git").unwrap(),
            [0..3, 11..15]
        );
        assert_eq!(find_words("git -C api push", "git pull"), None);
    }

    #[test]
    fn merges_words_that_overlap_or_touch() {
        assert_eq!(find_words("git push", "gi git").unwrap(), [0..3]);
        assert_eq!(find_words("ls ls", "ls ls").unwrap(), [0..2]);
        assert_eq!(find_words("abc", "a bc").unwrap(), [0..3]);
        assert_eq!(find_words("git push", "ls ls"), None);
    }

    #[test]
    fn ignores_the_spaces_between_words() {
        assert_eq!(find_words("ls -la", "  ls\t -la  ").unwrap(), [0..2, 3..6]);
        assert_eq!(find_words("lsof", "ls ").unwrap(), [0..2]);
    }

    #[test]
    fn query_without_words_matches_everything() {
        for (haystack, query) in [("ls", ""), ("ls", "   "), ("", "")] {
            assert_eq!(find_words(haystack, query).unwrap(), [], "{query:?}");
        }
    }

    #[test]
    fn finds_words_ignoring_case_in_any_script() {
        assert_eq!(
            find_words("echo Привет Мир", "мир ПРИВЕТ").unwrap(),
            [5..17, 18..24]
        );
    }

    #[test]
    fn handles_multibyte_characters() {
        assert_eq!(find("echo Привет", "привет"), Some(5..17));
        assert_eq!(find("ünï", "ï"), Some(3..5));
    }
}
