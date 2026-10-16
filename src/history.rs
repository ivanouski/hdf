use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::ops::Range;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub struct Entry {
    // The command as stored, without the newline that ends it
    pub original: String,
    // The command as shown in the finder: newlines, tabs and other control characters become spaces
    pub command: String,
    // Seconds since the epoch, from the #<epoch> line bash writes when HISTTIMEFORMAT is set
    pub time: Option<i64>,
    // Bytes of the entry in the file, including its timestamp line and trailing newline
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

    // Reads the file again, after a delete or changes from other shells
    pub fn reload(&mut self) -> io::Result<()> {
        *self = Self::load(&self.path)?;
        Ok(())
    }

    // Oldest first, like the file
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    // Removes the entries from the file, keeping lines other shells appended since it was loaded
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

        write_atomically(&self.path, &without(&current, &self.entries, indices))
    }
}

// With HISTTIMEFORMAT set bash writes a #<epoch> line before each entry, which can start anywhere
// in the file when the setting was turned on later. Like bash, a timestamp belongs to the line after
// it. When the file starts with a timestamp, an entry also spans every line up to the next timestamp
// (multi-line commands), otherwise every line is an entry.
fn parse(content: &[u8]) -> Vec<Entry> {
    let mut lines = Vec::new();
    let mut start = 0;

    for line in content.split_inclusive(|&byte| byte == b'\n') {
        lines.push(start..start + line.len());
        start += line.len();
    }

    let multiline = lines
        .first()
        .is_some_and(|line| is_timestamp(&content[line.clone()]));
    // Bytes of each entry, and of its command without the timestamp
    let mut entries: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    let mut timestamp: Option<Range<usize>> = None;

    for line in lines {
        if is_timestamp(&content[line.clone()]) {
            // A timestamp followed by another one has no command
            if let Some(orphan) = timestamp.replace(line) {
                entries.push((orphan.clone(), orphan.end..orphan.end));
            }

            continue;
        }

        match (timestamp.take(), entries.last_mut()) {
            (Some(timestamp), _) => entries.push((timestamp.start..line.end, line)),
            (None, Some((bytes, command))) if multiline => {
                bytes.end = line.end;
                command.end = line.end;
            }
            (None, _) => entries.push((line.clone(), line)),
        }
    }

    if let Some(orphan) = timestamp {
        entries.push((orphan.clone(), orphan.end..orphan.end));
    }

    entries
        .into_iter()
        .filter_map(|(bytes, command)| {
            // Whatever comes before the command is its timestamp line, # and digits
            let time = content
                .get(bytes.start + 1..command.start)
                .and_then(|seconds| std::str::from_utf8(seconds).ok())
                .and_then(|seconds| seconds.trim_end().parse().ok());
            let text = &content[command];
            let original = String::from_utf8_lossy(text.strip_suffix(b"\n").unwrap_or(text));
            let command = display(&original);

            (!command.trim().is_empty()).then(|| Entry {
                original: original.into_owned(),
                command,
                time,
                bytes,
            })
        })
        .collect()
}

fn without(content: &[u8], entries: &[Entry], indices: &[usize]) -> Vec<u8> {
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

fn is_timestamp(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\n").unwrap_or(line);

    line.len() > 1 && line[0] == b'#' && line[1..].iter().all(u8::is_ascii_digit)
}

fn display(text: &str) -> String {
    text.chars()
        .map(|char| if char.is_control() { ' ' } else { char })
        .collect()
}

fn write_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let temp_path = path.with_file_name(format!(".{file_name}.hdf-{}", std::process::id()));

    let result = (|| {
        let mut file = create_private(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::set_permissions(&temp_path, fs::metadata(path)?.permissions())?;
        fs::rename(&temp_path, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

// A new file only its owner can read, as history holds whatever was typed on the command line.
// A file left at the path by an earlier, killed run is replaced, as it may be readable by others.
fn create_private(path: &Path) -> io::Result<File> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }

    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn commands(content: &str) -> Vec<String> {
        parse(content.as_bytes())
            .into_iter()
            .map(|entry| entry.command)
            .collect()
    }

    fn removing(content: &str, indices: &[usize]) -> String {
        String::from_utf8(without(
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
    fn creates_files_only_the_owner_can_read() {
        let path = env::temp_dir().join(format!("hdf-test-private-{}", std::process::id()));
        fs::write(&path, "left by a killed run").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        create_private(&path).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn refuses_to_delete_when_a_line_joined_the_last_one() {
        let path = env::temp_dir().join(format!("hdf-test-joined-{}", std::process::id()));
        fs::write(&path, "a\nb").unwrap();
        let history = History::load(&path).unwrap();
        fs::write(&path, "a\nbc\n").unwrap();

        let result = history.delete(&[1]);
        let content = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(result.is_err());
        assert_eq!(content, "a\nbc\n");
    }

    #[test]
    fn deletes_from_a_file_without_a_final_newline_left_alone() {
        let path = env::temp_dir().join(format!("hdf-test-no-newline-{}", std::process::id()));
        fs::write(&path, "a\nb").unwrap();
        let history = History::load(&path).unwrap();

        history.delete(&[1]).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(content, "a\n");
    }

    #[test]
    fn reads_timestamps() {
        let times: Vec<_> = parse(b"ls\n#1700000000\npwd\n#1700000001\nfor x\ndo\n")
            .into_iter()
            .map(|entry| entry.time)
            .collect();

        assert_eq!(times, [None, Some(1700000000), Some(1700000001), None]);
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
}
