use std::iter;
use std::str::{Chars, Split};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

// SGR codes, from ECMA-48 and the extensions terminals share
const RESET: u16 = 0;
const BOLD: u16 = 1;
const DIM: u16 = 2;
const ITALIC: u16 = 3;
// With a style after a colon, as in 4:3 for curly, where 4:0 turns it off
const UNDERLINED: u16 = 4;
const NOT_UNDERLINED_STYLE: u16 = 0;
const SLOW_BLINK: u16 = 5;
const RAPID_BLINK: u16 = 6;
const REVERSED: u16 = 7;
const HIDDEN: u16 = 8;
const CROSSED_OUT: u16 = 9;
const DOUBLE_UNDERLINED: u16 = 21;
// Neither bold nor dim
const NORMAL: u16 = 22;
const NOT_ITALIC: u16 = 23;
const NOT_UNDERLINED: u16 = 24;
const NOT_BLINKING: u16 = 25;
const NOT_REVERSED: u16 = 27;
const NOT_HIDDEN: u16 = 28;
const NOT_CROSSED_OUT: u16 = 29;
const FG_BLACK: u16 = 30;
const FG_WHITE: u16 = 37;
// Followed by INDEXED or RGB and their numbers, after semicolons or colons
const FG_EXTENDED: u16 = 38;
const FG_DEFAULT: u16 = 39;
const BG_BLACK: u16 = 40;
const BG_WHITE: u16 = 47;
const BG_EXTENDED: u16 = 48;
const BG_DEFAULT: u16 = 49;
const FG_BRIGHT_BLACK: u16 = 90;
const FG_BRIGHT_WHITE: u16 = 97;
const BG_BRIGHT_BLACK: u16 = 100;
const BG_BRIGHT_WHITE: u16 = 107;
const INDEXED: u16 = 5;
const RGB: u16 = 2;

// From black to white, in the order of their codes
const COLORS: [Color; 8] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
];
const BRIGHT_COLORS: [Color; 8] = [
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

// The last line of a prompt as bash prints it, from ${PS1@P}, styled by its SGR sequences. Other
// escape sequences, like a window title, and control characters, like the \[ \] markers bash
// turns into \001 and \002, are left out.
pub fn parse(text: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::new();
    let mut chars = text.chars();

    while let Some(char) = chars.next() {
        match char {
            '\x1b' => {
                if let Some(params) = escape(&mut chars) {
                    style = sgr(style, &params);
                }
            }
            // A style set on an earlier line still holds on the last one. A carriage return starts
            // the line over, as prompts with a part on the right do after printing it.
            '\n' | '\r' => spans.clear(),
            char if char.is_control() => {}
            char => match spans.last_mut() {
                Some(span) if span.style == style => span.content.to_mut().push(char),
                _ => spans.push(Span::styled(char.to_string(), style)),
            },
        }
    }

    Line::from(spans)
}

// Skips the sequence after an escape, returns its parameters when it sets a style
fn escape(chars: &mut Chars) -> Option<String> {
    match chars.next()? {
        '[' => {
            let mut params = String::new();

            // Parameter and intermediate bytes up to the final one. Private sequences, like
            // \e[>4;2m for keyboard modes, end with m too, but have more than digits, ; and :.
            for char in chars.by_ref() {
                if ('@'..='~').contains(&char) {
                    let codes = params
                        .chars()
                        .all(|char| matches!(char, '0'..='9' | ';' | ':'));

                    return (char == 'm' && codes).then_some(params);
                }

                params.push(char);
            }

            None
        }
        // An OSC, like a window title, ends with BEL or ESC \
        ']' => {
            while let Some(char) = chars.next() {
                match char {
                    '\x07' => break,
                    '\x1b' => {
                        chars.next();
                        break;
                    }
                    _ => {}
                }
            }

            None
        }
        // Intermediate bytes, then the final one, like ESC ( B from tput sgr0
        mut char => {
            while (' '..='/').contains(&char) {
                char = chars.next()?;
            }

            None
        }
    }
}

fn sgr(mut style: Style, params: &str) -> Style {
    // Semicolons separate the codes, colons the parts of one, as in 4:3 or 38:2::255:0:0
    let mut params = params.split(';');

    while let Some(param) = params.next() {
        let (code, parts): (u16, Vec<u16>) = match param.split_once(':') {
            Some((code, parts)) => (number(code), parts.split(':').map(number).collect()),
            None => (number(param), Vec::new()),
        };

        style = match code {
            RESET => Style::new(),
            BOLD => style.add_modifier(Modifier::BOLD),
            DIM => style.add_modifier(Modifier::DIM),
            ITALIC => style.add_modifier(Modifier::ITALIC),
            UNDERLINED if parts.first() == Some(&NOT_UNDERLINED_STYLE) => {
                style.remove_modifier(Modifier::UNDERLINED)
            }
            UNDERLINED | DOUBLE_UNDERLINED => style.add_modifier(Modifier::UNDERLINED),
            SLOW_BLINK => style.add_modifier(Modifier::SLOW_BLINK),
            RAPID_BLINK => style.add_modifier(Modifier::RAPID_BLINK),
            REVERSED => style.add_modifier(Modifier::REVERSED),
            HIDDEN => style.add_modifier(Modifier::HIDDEN),
            CROSSED_OUT => style.add_modifier(Modifier::CROSSED_OUT),
            NORMAL => style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            NOT_ITALIC => style.remove_modifier(Modifier::ITALIC),
            NOT_UNDERLINED => style.remove_modifier(Modifier::UNDERLINED),
            NOT_BLINKING => style.remove_modifier(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK),
            NOT_REVERSED => style.remove_modifier(Modifier::REVERSED),
            NOT_HIDDEN => style.remove_modifier(Modifier::HIDDEN),
            NOT_CROSSED_OUT => style.remove_modifier(Modifier::CROSSED_OUT),
            FG_BLACK..=FG_WHITE => style.fg(COLORS[usize::from(code - FG_BLACK)]),
            FG_EXTENDED => {
                extended_color(&parts, &mut params).map_or(style, |color| style.fg(color))
            }
            FG_DEFAULT => style.fg(Color::Reset),
            BG_BLACK..=BG_WHITE => style.bg(COLORS[usize::from(code - BG_BLACK)]),
            BG_EXTENDED => {
                extended_color(&parts, &mut params).map_or(style, |color| style.bg(color))
            }
            BG_DEFAULT => style.bg(Color::Reset),
            FG_BRIGHT_BLACK..=FG_BRIGHT_WHITE => {
                style.fg(BRIGHT_COLORS[usize::from(code - FG_BRIGHT_BLACK)])
            }
            BG_BRIGHT_BLACK..=BG_BRIGHT_WHITE => {
                style.bg(BRIGHT_COLORS[usize::from(code - BG_BRIGHT_BLACK)])
            }
            _ => style,
        };
    }

    style
}

// Empty codes count as 0
fn number(code: &str) -> u16 {
    code.parse().unwrap_or(0)
}

// The color after FG_EXTENDED or BG_EXTENDED, in its parts after colons or in the codes after it:
// an index of 256 colors, or red, green and blue. After colons a color space may come first.
fn extended_color(parts: &[u16], params: &mut Split<char>) -> Option<Color> {
    let parts = if parts.is_empty() {
        let kind = number(params.next()?);
        let count = match kind {
            INDEXED => 1,
            RGB => 3,
            _ => 0,
        };

        iter::once(kind)
            .chain(params.take(count).map(number))
            .collect()
    } else {
        parts.to_vec()
    };
    let byte = |code: &u16| u8::try_from(*code).ok();

    match parts.as_slice() {
        [INDEXED, index] => Some(Color::Indexed(byte(index)?)),
        [RGB, red, green, blue] | [RGB, _, red, green, blue] => {
            Some(Color::Rgb(byte(red)?, byte(green)?, byte(blue)?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<Span<'static>> {
        parse(text).spans
    }

    #[test]
    fn styles_the_text_by_its_sgr_sequences() {
        assert_eq!(
            spans("\x1b[1;34mdir\x1b[0m$ "),
            [
                Span::styled("dir", Style::new().fg(Color::Blue).bold()),
                Span::raw("$ "),
            ]
        );
    }

    #[test]
    fn leaves_out_the_invisible_parts() {
        assert_eq!(
            spans("\x01\x1b]0;title\x07\x02\x01\x1b]2;title\x1b\\\x02$\t "),
            [Span::raw("$ ")]
        );
    }

    #[test]
    fn keeps_the_last_line_with_the_style_from_before() {
        assert_eq!(
            spans("\x1b[32mone\ntwo$ "),
            [Span::styled("two$ ", Style::new().fg(Color::Green))]
        );
    }

    #[test]
    fn starts_the_line_over_after_a_carriage_return() {
        assert_eq!(
            spans("\x1b[2m       12:00\r\x1b[0muser@host$ "),
            [Span::raw("user@host$ ")]
        );
    }

    #[test]
    fn reads_256_colors_and_true_colors() {
        assert_eq!(
            spans("\x1b[38;5;208ma\x1b[48:2:1:2:3mb"),
            [
                Span::styled("a", Style::new().fg(Color::Indexed(208))),
                Span::styled(
                    "b",
                    Style::new().fg(Color::Indexed(208)).bg(Color::Rgb(1, 2, 3))
                ),
            ]
        );
    }

    #[test]
    fn reads_bright_colors_and_backgrounds() {
        assert_eq!(
            spans("\x1b[95;41ma\x1b[102mb"),
            [
                Span::styled("a", Style::new().fg(Color::LightMagenta).bg(Color::Red)),
                Span::styled(
                    "b",
                    Style::new().fg(Color::LightMagenta).bg(Color::LightGreen)
                ),
            ]
        );
    }

    #[test]
    fn turns_attributes_and_colors_off() {
        let on = Style::new()
            .fg(Color::Red)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED)
            .add_modifier(Modifier::REVERSED);
        let off = on
            .remove_modifier(Modifier::BOLD | Modifier::DIM)
            .remove_modifier(Modifier::ITALIC | Modifier::UNDERLINED | Modifier::REVERSED)
            .fg(Color::Reset)
            .bg(Color::Reset);

        assert_eq!(
            spans("\x1b[31;44;1;3;4;7ma\x1b[22;23;24;27;39;49mb"),
            [Span::styled("a", on), Span::styled("b", off)]
        );
    }

    #[test]
    fn reads_true_colors_with_a_color_space_after_colons() {
        assert_eq!(
            spans("\x1b[38:2::255:0:0;1ma"),
            [Span::styled(
                "a",
                Style::new().fg(Color::Rgb(255, 0, 0)).bold()
            )]
        );
    }

    #[test]
    fn reads_underline_styles_after_colons() {
        assert_eq!(
            spans("\x1b[4:3ma\x1b[4:0mb"),
            [
                Span::styled("a", Style::new().underlined()),
                Span::styled("b", Style::new().not_underlined()),
            ]
        );
    }

    #[test]
    fn reads_the_other_attributes() {
        let on = Modifier::SLOW_BLINK
            | Modifier::RAPID_BLINK
            | Modifier::HIDDEN
            | Modifier::CROSSED_OUT
            | Modifier::UNDERLINED;
        let off = Modifier::SLOW_BLINK | Modifier::RAPID_BLINK | Modifier::HIDDEN;

        assert_eq!(
            spans("\x1b[5;6;8;9;21ma\x1b[25;28;29mb"),
            [
                Span::styled("a", Style::new().add_modifier(on)),
                Span::styled(
                    "b",
                    Style::new()
                        .add_modifier(Modifier::UNDERLINED)
                        .remove_modifier(off | Modifier::CROSSED_OUT)
                ),
            ]
        );
    }

    #[test]
    fn ignores_private_sequences_ending_with_m() {
        assert_eq!(
            spans("\x1b[31m\x1b[>4;2ma"),
            [Span::styled("a", Style::new().fg(Color::Red))]
        );
    }

    #[test]
    fn resets_with_tput_sgr0() {
        assert_eq!(
            spans("\x1b[31ma\x1b(B\x1b[mb"),
            [
                Span::styled("a", Style::new().fg(Color::Red)),
                Span::raw("b"),
            ]
        );
    }

    #[test]
    fn skips_other_sequences() {
        assert_eq!(spans("\x1b[Ka\x1b7b\x1b[38;9mc"), [Span::raw("abc")]);
    }
}
