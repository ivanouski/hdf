use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::config::Theme;
use crate::finder::{Finder, Match, TIME_WIDTH};

// Before the commands, or their times, when they do not line up under the query
const INDENT: usize = 2;

// The outcome of the last Ctrl-D, shown in place of the count until the next key
#[derive(Debug, PartialEq)]
pub enum Status {
    Deleted(String),
    Failed(String),
}

pub fn draw(
    frame: &mut Frame,
    finder: &Finder,
    theme: &Theme,
    prompt: Option<&Line>,
    status: Option<&Status>,
    list: &mut ListState,
) {
    let [prompt_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(frame.area());
    // The shell prompt when it shows anything, > otherwise
    let shell_prompt = prompt.filter(|prompt| prompt.width() > 0);

    draw_prompt_line(frame, prompt_area, shell_prompt, finder, theme, status);

    let layout = RowLayout::new(finder, shell_prompt.map(Line::width), list_area.width);
    let rows = finder
        .matches
        .iter()
        .enumerate()
        .map(|(index, found)| row(finder, found, index == finder.selected, &layout, theme));

    list.select((!finder.matches.is_empty()).then_some(finder.selected));
    frame.render_stateful_widget(List::new(rows), list_area, list);
}

fn draw_prompt_line(
    frame: &mut Frame,
    area: Rect,
    shell_prompt: Option<&Line>,
    finder: &Finder,
    theme: &Theme,
    status: Option<&Status>,
) {
    let mut line = shell_prompt
        .cloned()
        .unwrap_or_else(|| Line::from(Span::styled("> ", theme.prompt)));
    line.push_span(Span::raw(&finder.query));

    // The prompt goes over the count, so a long query stays readable
    frame.set_cursor_position((area.x + line.width() as u16, area.y));
    frame.render_widget(
        Paragraph::new(count(finder, theme, status)).right_aligned(),
        area,
    );
    frame.render_widget(Paragraph::new(line), area);
}

// Matches and commands, or the status in their place
fn count<'a>(finder: &Finder, theme: &Theme, status: Option<&'a Status>) -> Span<'a> {
    match status {
        Some(Status::Deleted(message)) => Span::styled(message, theme.deleted),
        Some(Status::Failed(message)) => Span::styled(message, theme.error),
        None => Span::styled(
            format!("{}/{}", finder.matches.len(), finder.commands.len()),
            theme.count,
        ),
    }
}

// What every row starts with
struct RowLayout {
    // A time column only when the history has timestamps, blank for commands from before
    has_times: bool,
    // Before the time, or before the command without times
    indent: String,
}

impl RowLayout {
    fn new(finder: &Finder, prompt_width: Option<usize>, pane_width: u16) -> Self {
        let has_times = finder.commands.iter().any(|command| command.time.is_some());
        let time_width = if has_times { TIME_WIDTH + 2 } else { 0 };
        let column = command_column(prompt_width, time_width, pane_width);

        Self {
            has_times,
            indent: " ".repeat(column - time_width),
        }
    }
}

// Under a shell prompt the commands start where the query does, the times right before them.
// Without one, or with one too narrow for the times or wider than 60% of the pane, they keep the
// indent.
fn command_column(prompt_width: Option<usize>, time_width: usize, pane_width: u16) -> usize {
    prompt_width
        .filter(|&width| width >= time_width && width * 5 <= usize::from(pane_width) * 3)
        .unwrap_or(INDENT + time_width)
}

fn row<'a>(
    finder: &'a Finder,
    found: &Match,
    selected: bool,
    layout: &'a RowLayout,
    theme: &Theme,
) -> ListItem<'a> {
    let command = &finder.commands[found.command_index];
    // The selected row has a style of its own across its width, the match drawn over it. Its
    // time and number of runs are not dimmed, dim colors are hard to read on its background.
    let dim = if selected { Style::new() } else { theme.dim };
    let mut spans = vec![Span::raw(layout.indent.as_str())];

    if layout.has_times {
        let time = command.time.as_deref().unwrap_or_default();
        spans.push(Span::styled(format!("{time:<TIME_WIDTH$}  "), dim));
    }

    spans.extend(highlight(&command.text, &found.range, theme.matched));

    let runs = command.entry_indices.len();

    if runs > 1 {
        spans.push(Span::styled(format!(" ({runs})"), dim));
    }

    // The item style goes on the row first and the spans patch it, while a highlight style
    // of the list would go over the spans and could hide the match
    let item = ListItem::new(Line::from(spans));

    if selected {
        item.style(theme.selected)
    } else {
        item
    }
}

fn highlight<'a>(text: &'a str, range: &Range<usize>, matched: Style) -> [Span<'a>; 3] {
    [
        Span::raw(&text[..range.start]),
        Span::styled(&text[range.clone()], matched),
        Span::raw(&text[range.end..]),
    ]
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::{history, prompt};

    fn finder(content: &str) -> Finder {
        Finder::new(&history::parse(content.as_bytes()), "ls".to_string())
    }

    fn render(finder: &Finder, theme: &Theme) -> Buffer {
        render_with_prompt(finder, theme, None, 40)
    }

    fn render_with_prompt(
        finder: &Finder,
        theme: &Theme,
        prompt: Option<&str>,
        width: u16,
    ) -> Buffer {
        let prompt = prompt.map(prompt::parse);
        let mut terminal = Terminal::new(TestBackend::new(width, 4)).unwrap();
        terminal
            .draw(|frame| {
                let list = &mut ListState::default();
                draw(frame, finder, theme, prompt.as_ref(), None, list);
            })
            .unwrap();

        terminal.backend().buffer().clone()
    }

    fn text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    // The column of the first command, on the selected row
    fn first_command_column(buffer: &Buffer) -> usize {
        text(buffer, 1).find("ls").unwrap()
    }

    // The match is at 20 and 21, after the indent, the time and two spaces
    const HISTORY: &str = "#1700000000\nls -la\n#1700000000\nls\n";

    #[test]
    fn shows_the_selected_row_on_its_background_with_the_match_over_it() {
        let buffer = render(&finder(HISTORY), &Theme::default());
        let cell = |x| &buffer[(x, 1)];

        assert!((0..40).all(|x| cell(x).bg == Color::Indexed(238)));
        assert!((20..22).all(|x| cell(x).fg == Color::Yellow));
        assert!(cell(20).modifier.contains(Modifier::BOLD));
        // Neither the time nor the rest of the command dimmed
        assert!((0..20).chain(22..40).all(|x| cell(x).fg == Color::Reset));
    }

    #[test]
    fn dims_the_times_of_other_rows() {
        let buffer = render(&finder(HISTORY), &Theme::default());
        let cell = |x| &buffer[(x, 2)];

        assert!((0..40).all(|x| cell(x).bg == Color::Reset));
        assert!((2..18).all(|x| cell(x).fg == Color::DarkGray));
        assert!((20..22).all(|x| cell(x).fg == Color::Yellow));
        assert_eq!(cell(23).fg, Color::Reset);
    }

    #[test]
    fn draws_with_the_theme() {
        let theme = Theme {
            prompt: Style::new().fg(Color::Green),
            count: Style::new().fg(Color::Magenta),
            matched: Style::new().fg(Color::Red),
            dim: Style::new().fg(Color::Gray),
            selected: Style::new().bg(Color::Blue),
            ..Theme::default()
        };
        let buffer = render(&finder(HISTORY), &theme);

        assert_eq!(buffer[(0, 0)].fg, Color::Green);
        assert_eq!(buffer[(39, 0)].fg, Color::Magenta);
        assert_eq!(buffer[(0, 1)].bg, Color::Blue);
        assert_eq!(buffer[(20, 1)].fg, Color::Red);
        assert_eq!(buffer[(20, 1)].bg, Color::Blue);
        assert_eq!(buffer[(2, 2)].fg, Color::Gray);
        assert_eq!(buffer[(20, 2)].fg, Color::Red);
    }

    #[test]
    fn pads_the_time_column() {
        let buffer = render(&finder("ls\n#1700000000\nls -la\n"), &Theme::default());

        // The command after the time and two spaces, also where the time is blank
        assert_eq!(
            text(&buffer, 2).find("ls").unwrap(),
            first_command_column(&buffer)
        );
        assert_eq!(first_command_column(&buffer), 2 + TIME_WIDTH + 2);
    }

    #[test]
    fn shows_the_shell_prompt_in_its_colors_before_the_query() {
        let prompt = "\x01\x1b[35m\x02dir\x01\x1b[0m\x02$ ";
        let buffer = render_with_prompt(&finder("ls\n"), &Theme::default(), Some(prompt), 40);

        assert!(text(&buffer, 0).starts_with("dir$ ls "));
        assert_eq!(buffer[(0, 0)].fg, Color::Magenta);
        assert_eq!(buffer[(3, 0)].fg, Color::Reset);
        assert!(text(&buffer, 0).ends_with("1/1"));
    }

    #[test]
    fn shows_the_arrow_for_a_prompt_with_nothing_to_show() {
        let prompt = "\x01\x1b]0;title\x07\x02";
        let buffer = render_with_prompt(&finder("ls\n"), &Theme::default(), Some(prompt), 40);

        assert!(text(&buffer, 0).starts_with("> ls "));
        assert_eq!(first_command_column(&buffer), 2);
    }

    #[test]
    fn lines_the_commands_up_under_the_query_with_the_times_before_them() {
        let prompt = "[mon@Aer ~/src/utils (main)]$ ";
        let buffer = render_with_prompt(&finder(HISTORY), &Theme::default(), Some(prompt), 120);

        assert_eq!(first_command_column(&buffer), prompt.len());
        assert_eq!(
            text(&buffer, 1).find("20").unwrap(),
            prompt.len() - 2 - TIME_WIDTH
        );
        assert_eq!(text(&buffer, 2).find("ls").unwrap(), prompt.len());
        assert!((0..120).all(|x| buffer[(x, 1)].bg == Color::Indexed(238)));
    }

    #[test]
    fn starts_the_commands_under_the_query_after_a_shell_prompt() {
        assert_eq!(command_column(Some(30), TIME_WIDTH + 2, 80), 30);
        assert_eq!(command_column(Some(18), TIME_WIDTH + 2, 80), 18);
        // Without times the gutter is blank
        assert_eq!(command_column(Some(13), 0, 40), 13);
    }

    #[test]
    fn keeps_the_indent_without_a_shell_prompt() {
        assert_eq!(
            command_column(None, TIME_WIDTH + 2, 80),
            INDENT + TIME_WIDTH + 2
        );
        assert_eq!(command_column(None, 0, 80), INDENT);
    }

    #[test]
    fn keeps_the_indent_for_a_prompt_too_narrow_for_the_times() {
        assert_eq!(
            command_column(Some(17), TIME_WIDTH + 2, 80),
            INDENT + TIME_WIDTH + 2
        );
    }

    #[test]
    fn keeps_the_indent_for_a_prompt_wider_than_60_percent_of_the_pane() {
        assert_eq!(command_column(Some(24), 0, 40), 24);
        assert_eq!(command_column(Some(25), 0, 40), INDENT);
    }
}
