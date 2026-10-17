use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{env, fs, io};

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

// The defaults as a config file, for --help and --init-config. A test keeps them the same as
// Theme::default.
pub const DEFAULT: &str = r#"[colors]
prompt   = { fg = "blue", bold = true }
count    = "dark-gray"     # matches/commands
deleted  = "green"         # the status after Ctrl-D
error    = "red"           # Ctrl-D failed
match    = { fg = "yellow", bold = true }
dim      = "dark-gray"     # times and numbers of runs
selected = { bg = "238" }  # the selected row, under the match
"#;

// The config file, every key optional
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub colors: Theme,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    #[serde(deserialize_with = "style")]
    pub prompt: Style,
    // The match count right of the prompt
    #[serde(deserialize_with = "style")]
    pub count: Style,
    // The status after Ctrl-D deleted a command, or failed to
    #[serde(deserialize_with = "style")]
    pub deleted: Style,
    #[serde(deserialize_with = "style")]
    pub error: Style,
    #[serde(rename = "match", deserialize_with = "style")]
    pub matched: Style,
    // The time column and the number of runs, on rows other than the selected one
    #[serde(deserialize_with = "style")]
    pub dim: Style,
    // The whole selected row, with the match drawn over it
    #[serde(deserialize_with = "style")]
    pub selected: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            prompt: Style::new().fg(Color::Blue).bold(),
            count: Style::new().fg(Color::DarkGray),
            deleted: Style::new().fg(Color::Green),
            error: Style::new().fg(Color::Red),
            matched: Style::new().fg(Color::Yellow).bold(),
            dim: Style::new().fg(Color::DarkGray),
            // A gray a step above the usual dark backgrounds: 236 is close to them, and dark gray
            // is the color of the dim text on the other rows
            selected: Style::new().bg(Color::Indexed(238)),
        }
    }
}

impl Config {
    // Reads the config file, the defaults when there is none
    pub fn load() -> Result<Self, String> {
        let Some(path) = path() else {
            return Ok(Self::default());
        };

        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("cannot read {}: {error}", path.display())),
        }
    }

    // Writes the defaults to the config file, never over one already there, returns its path
    pub fn init() -> Result<PathBuf, String> {
        let path = path().ok_or("XDG_CONFIG_HOME and HOME are not set")?;
        let error = |error: io::Error| format!("cannot create {}: {error}", path.display());

        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(error)?;
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|failure| match failure.kind() {
                io::ErrorKind::AlreadyExists => format!("{} already exists", path.display()),
                _ => error(failure),
            })?;
        // A half-written file would be taken for a config the next time, by hdf and by itself
        if let Err(failure) = file.write_all(DEFAULT.as_bytes()) {
            let _ = fs::remove_file(&path);
            return Err(error(failure));
        }

        Ok(path)
    }

    fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

// A relative XDG_CONFIG_HOME is invalid, the XDG spec says to ignore it
fn path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| Path::new(dir).is_absolute())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".config")))
        .map(|dir| dir.join("hdf").join("config.toml"))
}

// A style is a table of colors and modifiers, or just a color for the foreground
fn style<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Style, D::Error> {
    struct StyleVisitor;

    impl<'de> Visitor<'de> for StyleVisitor {
        type Value = Style;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a color or a table of fg, bg, bold, italic, underlined, reversed")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Style, E> {
            parse_color(value).map(|color| Style::new().fg(color))
        }

        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Style, M::Error> {
            StyleTable::deserialize(de::value::MapAccessDeserializer::new(map)).map(Style::from)
        }
    }

    deserializer.deserialize_any(StyleVisitor)
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StyleTable {
    #[serde(deserialize_with = "color")]
    fg: Option<Color>,
    #[serde(deserialize_with = "color")]
    bg: Option<Color>,
    bold: bool,
    italic: bool,
    underlined: bool,
    reversed: bool,
}

impl From<StyleTable> for Style {
    fn from(table: StyleTable) -> Self {
        let mut style = Style::new();
        style.fg = table.fg;
        style.bg = table.bg;

        for (on, modifier) in [
            (table.bold, Modifier::BOLD),
            (table.italic, Modifier::ITALIC),
            (table.underlined, Modifier::UNDERLINED),
            (table.reversed, Modifier::REVERSED),
        ] {
            if on {
                style = style.add_modifier(modifier);
            }
        }

        style
    }
}

fn color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Color>, D::Error> {
    let name = String::deserialize(deserializer)?;
    parse_color(&name).map(Some)
}

// Names like yellow and light-blue, 0 to 255, or #rrggbb
fn parse_color<E: de::Error>(name: &str) -> Result<Color, E> {
    Color::from_str(name).map_err(|_| E::custom(format!("unknown color \"{name}\"")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(text: &str) -> Theme {
        Config::parse(text).unwrap().colors
    }

    fn error(text: &str) -> String {
        Config::parse(text).err().unwrap().to_string()
    }

    #[test]
    fn defaults_without_a_config() {
        assert_eq!(theme(""), Theme::default());
        assert_eq!(theme("[colors]\n"), Theme::default());
    }

    #[test]
    fn writes_the_defaults_as_a_config() {
        assert_eq!(theme(DEFAULT), Theme::default());
    }

    #[test]
    fn keeps_the_defaults_of_keys_left_out() {
        let theme = theme("[colors]\nprompt = { fg = \"green\" }\n");

        assert_eq!(theme.prompt, Style::new().fg(Color::Green));
        assert_eq!(theme.matched, Theme::default().matched);
    }

    #[test]
    fn reads_a_table_of_colors_and_modifiers() {
        let theme = theme(
            "[colors]\nselected = { fg = \"black\", bg = \"white\", bold = true, italic = true, \
             underlined = true, reversed = true }\n",
        );

        assert_eq!(
            theme.selected,
            Style::new()
                .fg(Color::Black)
                .bg(Color::White)
                .bold()
                .italic()
                .underlined()
                .reversed()
        );
    }

    #[test]
    fn reads_a_color_alone_as_the_foreground() {
        assert_eq!(
            theme("[colors]\nmatch = \"cyan\"\n").matched,
            Style::new().fg(Color::Cyan)
        );
    }

    #[test]
    fn reads_names_indexes_and_hex_colors() {
        let theme = theme(
            "[colors]\nprompt = \"light-blue\"\ncount = \"dark-gray\"\nmatch = \"208\"\n\
             dim = \"#ff8800\"\n",
        );

        assert_eq!(theme.prompt.fg, Some(Color::LightBlue));
        assert_eq!(theme.count.fg, Some(Color::DarkGray));
        assert_eq!(theme.matched.fg, Some(Color::Indexed(208)));
        assert_eq!(theme.dim.fg, Some(Color::Rgb(255, 136, 0)));
    }

    #[test]
    fn rejects_unknown_colors() {
        assert!(error("[colors]\nmatch = \"yelow\"\n").contains("unknown color \"yelow\""));
        assert!(
            error("[colors]\nmatch = { bg = \"#ff88\" }\n").contains("unknown color \"#ff88\"")
        );
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(error("[colours]\n").contains("unknown field `colours`"));
        assert!(error("[colors]\nmatched = \"red\"\n").contains("unknown field `matched`"));
        assert!(error("[colors]\nmatch = { blink = true }\n").contains("unknown field `blink`"));
    }

    #[test]
    fn rejects_other_values() {
        assert!(error("[colors]\nmatch = 3\n").contains("expected a color or a table"));
        assert!(error("[colors]\nmatch = { bold = \"yes\" }\n").contains("expected a boolean"));
    }
}
