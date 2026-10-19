//! Reading the bash history file and deleting entries from it

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::ops::Range;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::str;

#[derive(Debug)]
pub struct Entry {
    /// The command as stored, without the newline that ends it
    pub original: String,
    /// The command as shown in the finder: control characters like newlines and tabs become spaces
    pub text: String,
    /// Seconds since the epoch, from the `#<epoch>` line bash writes when HISTTIMEFORMAT is set
    pub time: Option<i64>,
    /// Bytes of the entry in the file, including its timestamp line and trailing newline
    bytes: Range<usize>,
}

pub struct History {
    path: PathBuf,
    content: Vec<u8>,
    entries: Vec<Entry>,
}

impl History {
    pub fn load(path: &Path) -> io::Result<Self> {
        // Resolve symlinks so the rewrite replaces the real file, not the link
        let path = fs::canonicalize(path)?;
        let content = fs::read(&path)?;
        let entries = parse(&content);

        Ok(Self {
            path,
            content,
            entries,
        })
    }

    /// Reads the file again, after a delete or changes from other shells
    pub fn reload(&mut self) -> io::Result<()> {
        *self = Self::load(&self.path)?;
        Ok(())
    }

    /// Oldest first, like the file
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Removes the entries from the file, keeping lines other shells appended since it was loaded
    pub fn delete(&self, indices: &[usize]) -> io::Result<()> {
        let current = fs::read(&self.path)?;
        // A line appended after a last line without a newline joins it, so the loaded entries no
        // longer match the file
        let joined = current.len() > self.content.len()
            && !self.content.is_empty()
            && !self.content.ends_with(b"\n");

        if joined || !current.starts_with(&self.content) {
            return Err(io::Error::other(
                "history file was changed by another process, try again",
            ));
        }

        write_atomically(
            &self.path,
            &remove_entries(&current, &self.entries, indices),
        )
    }
}

/// The lines of an entry in the file
struct EntryLines {
    timestamp: Option<Range<usize>>,
    command: Range<usize>,
}

impl EntryLines {
    /// None for a blank command
    fn entry(self, content: &[u8]) -> Option<Entry> {
        let time = self
            .timestamp
            .as_ref()
            .and_then(|line| seconds(&content[line.clone()]));
        let original = String::from_utf8_lossy(trim_newline(&content[self.command.clone()]));
        let text = printable(&original);
        let start = self.timestamp.map_or(self.command.start, |line| line.start);

        (!text.trim().is_empty()).then(|| Entry {
            original: original.into_owned(),
            text,
            time,
            bytes: start..self.command.end,
        })
    }
}

/// With HISTTIMEFORMAT set bash writes a `#<epoch>` line before each entry, which can start
/// anywhere in the file when the setting was turned on later. Like bash, a timestamp belongs to the
/// line after it. When the file starts with a timestamp, an entry also spans every line up to the
/// next timestamp (multi-line commands), otherwise every line is an entry.
pub fn parse(content: &[u8]) -> Vec<Entry> {
    group(content, line_ranges(content))
        .into_iter()
        .filter_map(|lines| lines.entry(content))
        .collect()
}

/// Each line of content, with its newline
fn line_ranges(content: &[u8]) -> Vec<Range<usize>> {
    let mut start = 0;

    content
        .split_inclusive(|&byte| byte == b'\n')
        .map(|line| {
            let range = start..start + line.len();
            start = range.end;
            range
        })
        .collect()
}

fn group(content: &[u8], lines: Vec<Range<usize>>) -> Vec<EntryLines> {
    let multiline = lines
        .first()
        .is_some_and(|line| is_timestamp(&content[line.clone()]));
    let mut entries: Vec<EntryLines> = Vec::new();
    let mut timestamp: Option<Range<usize>> = None;

    for line in lines {
        // A timestamp followed by another one has no command and is left out, like one at the end
        if is_timestamp(&content[line.clone()]) {
            timestamp = Some(line);
            continue;
        }

        match (timestamp.take(), entries.last_mut()) {
            (None, Some(last)) if multiline => last.command.end = line.end,
            (timestamp, _) => entries.push(EntryLines {
                timestamp,
                command: line,
            }),
        }
    }

    entries
}

fn is_timestamp(line: &[u8]) -> bool {
    let line = trim_newline(line);

    line.len() > 1 && line[0] == b'#' && line[1..].iter().all(u8::is_ascii_digit)
}

/// The seconds of a timestamp line, None when they overflow
fn seconds(timestamp: &[u8]) -> Option<i64> {
    let digits = trim_newline(timestamp).strip_prefix(b"#")?;

    str::from_utf8(digits).ok()?.parse().ok()
}

fn trim_newline(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn remove_entries(content: &[u8], entries: &[Entry], indices: &[usize]) -> Vec<u8> {
    let mut removed: Vec<&Range<usize>> =
        indices.iter().map(|&index| &entries[index].bytes).collect();
    removed.sort_by_key(|bytes| bytes.start);

    let mut kept = Vec::with_capacity(content.len());
    let mut position = 0;

    for bytes in removed {
        kept.extend_from_slice(&content[position..bytes.start]);
        position = bytes.end;
    }

    kept.extend_from_slice(&content[position..]);
    kept
}

fn write_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let temp_path = path.with_file_name(format!(".{file_name}.hdf-{}", process::id()));
    let result = replace_through(path, &temp_path, content);

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

/// Writes the content to the temporary file, then moves it over the path with the path's
/// permissions
fn replace_through(path: &Path, temp_path: &Path, content: &[u8]) -> io::Result<()> {
    let mut file = create_private(temp_path)?;
    file.write_all(content)?;
    file.sync_all()?;
    fs::set_permissions(temp_path, fs::metadata(path)?.permissions())?;
    fs::rename(temp_path, path)
}

/// A new file only its owner can read, as history holds whatever was typed on the command line.
/// A file left at the path by an earlier, killed run is replaced, as it may be readable by others.
fn create_private(path: &Path) -> io::Result<File> {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error);
    }

    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::test_util::TempFile;

    fn commands(content: &str) -> Vec<String> {
        parse(content.as_bytes())
            .into_iter()
            .map(|entry| entry.text)
            .collect()
    }

    fn removing(content: &str, indices: &[usize]) -> String {
        String::from_utf8(remove_entries(
            content.as_bytes(),
            &parse(content.as_bytes()),
            indices,
        ))
        .unwrap()
    }

    #[test]
    fn parses_one_entry_per_line() {
        assert_eq!(commands("ls\ngit status\n"), ["ls", "git status"]);
    }

    #[test]
    fn parses_last_line_without_newline() {
        assert_eq!(commands("ls\ngit status"), ["ls", "git status"]);
    }

    #[test]
    fn skips_blank_lines() {
        assert_eq!(commands("ls\n\n  \npwd\n"), ["ls", "pwd"]);
    }

    #[test]
    fn replaces_control_characters_for_display() {
        assert_eq!(commands("echo\ta\r\n"), ["echo a "]);
    }

    #[test]
    fn groups_lines_under_timestamps() {
        assert_eq!(
            commands("#1700000000\nls\n#1700000001\nfor x in y\ndo echo\ndone\n"),
            ["ls", "for x in y do echo done"]
        );
    }

    #[test]
    fn attaches_timestamps_that_start_mid_file() {
        assert_eq!(
            commands("ls\npwd\n#1700000000\ngit status\n#1700000001\ntc\n"),
            ["ls", "pwd", "git status", "tc"]
        );
    }

    #[test]
    fn keeps_one_entry_per_line_when_timestamps_start_mid_file() {
        assert_eq!(commands("ls\n#1\na\nb\n"), ["ls", "a", "b"]);
    }

    #[test]
    fn skips_timestamps_without_a_command() {
        assert_eq!(commands("#1\n#2\nls\n#3\n"), ["ls"]);
    }

    #[test]
    fn keeps_the_original_command() {
        let originals: Vec<_> = parse(b"printf '\\ta\tb'\nls my\\ \n#1\nfor x\ndo\n")
            .into_iter()
            .map(|entry| entry.original)
            .collect();

        assert_eq!(originals, ["printf '\\ta\tb'", "ls my\\ ", "for x", "do"]);
    }

    #[test]
    fn reads_timestamps() {
        let times: Vec<_> = parse(b"ls\n#1700000000\npwd\n#1700000001\nfor x\ndo\n")
            .into_iter()
            .map(|entry| entry.time)
            .collect();

        assert_eq!(
            times,
            [None, Some(1_700_000_000), Some(1_700_000_001), None]
        );
    }

    #[test]
    fn treats_other_hash_lines_as_commands() {
        assert_eq!(commands("# note\n#12a\n#\n"), ["# note", "#12a", "#"]);
    }

    #[test]
    fn entry_bytes_cover_the_line() {
        assert_eq!(removing("a\nb\nc\n", &[1]), "a\nc\n");
        assert_eq!(removing("a\nb", &[1]), "a\n");
    }

    #[test]
    fn entry_bytes_cover_the_timestamp() {
        assert_eq!(
            removing("#1\na\n#2\nb\nb2\n#3\nc\n", &[1]),
            "#1\na\n#3\nc\n"
        );
    }

    #[test]
    fn entry_bytes_cover_a_timestamp_that_starts_mid_file() {
        assert_eq!(removing("ls\n#1\na\n#2\nb\n", &[1]), "ls\n#2\nb\n");
    }

    #[test]
    fn removes_several_entries_in_any_order() {
        assert_eq!(removing("a\nb\nc\nd\n", &[3, 0, 2]), "b\n");
    }

    #[test]
    fn creates_files_only_the_owner_can_read() {
        let file = TempFile::new("private", "left by a killed run");
        fs::set_permissions(&file.path, fs::Permissions::from_mode(0o644)).unwrap();

        create_private(&file.path).unwrap();
        let metadata = fs::metadata(&file.path).unwrap();

        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn refuses_to_delete_when_a_line_joined_the_last_one() {
        let file = TempFile::new("joined", "a\nb");
        let history = History::load(&file.path).unwrap();
        fs::write(&file.path, "a\nbc\n").unwrap();

        assert!(history.delete(&[1]).is_err());
        assert_eq!(file.read(), "a\nbc\n");
    }

    #[test]
    fn deletes_from_a_file_without_a_final_newline_left_alone() {
        let file = TempFile::new("no-newline", "a\nb");
        let history = History::load(&file.path).unwrap();

        history.delete(&[1]).unwrap();

        assert_eq!(file.read(), "a\n");
    }
}
