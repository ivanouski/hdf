use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::Theme;
use crate::finder::{Command, Finder, Match, TIME_WIDTH};

// Before the commands, or their times, when they do not line up under the query
const INDENT: usize = 2;
// In place of the text cut from a command
const ELLIPSIS: &str = "…";
// Most lines of the preview, the last one ending in … when the command needs more
const PREVIEW_LINES: usize = 3;
// Kept for the list when the preview takes rows from it
const MIN_LIST_ROWS: usize = 3;

// The outcome of the last Ctrl-D, shown in place of the count until the next key
#[derive(Debug, PartialEq)]
pub enum Status {
    Deleted(String),
    Failed(String),
}

// Returns the rows of the list, a page for PgUp and PgDn
pub fn draw(
    frame: &mut Frame,
    finder: &Finder,
    theme: &Theme,
    prompt: Option<&Line>,
    status: Option<&Status>,
    list: &mut ListState,
) -> usize {
    let area = frame.area();
    // The shell prompt when it shows anything, > otherwise
    let shell_prompt = prompt.filter(|prompt| prompt.width() > 0);
    let layout = RowLayout::new(finder, shell_prompt.map(Line::width), area.width);
    let preview = preview(finder, &layout, area, theme).unwrap_or_default();
    let [prompt_area, list_area, preview_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(preview.len() as u16),
    ])
    .areas(area);

    draw_prompt_line(frame, prompt_area, shell_prompt, finder, theme, status);

    let rows = finder
        .matches
        .iter()
        .enumerate()
        .map(|(index, found)| row(finder, found, index == finder.selected, &layout, theme));

    list.select((!finder.matches.is_empty()).then_some(finder.selected));
    frame.render_stateful_widget(List::new(rows), list_area, list);
    frame.render_widget(Paragraph::new(preview), preview_area);

    usize::from(list_area.height)
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
    let cursor = line.width() + Span::raw(&finder.query[..finder.cursor]).width();
    line.push_span(Span::raw(&finder.query));

    // The prompt goes over the count, so a long query stays readable
    frame.set_cursor_position((area.x + cursor as u16, area.y));
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
    // Left for the command and its number of runs
    command_width: usize,
}

impl RowLayout {
    fn new(finder: &Finder, prompt_width: Option<usize>, pane_width: u16) -> Self {
        let has_times = finder.commands.iter().any(|command| command.time.is_some());
        let time_width = if has_times { TIME_WIDTH + 2 } else { 0 };
        let column = command_column(prompt_width, time_width, pane_width);

        Self {
            has_times,
            indent: " ".repeat(column - time_width),
            command_width: usize::from(pane_width).saturating_sub(column),
        }
    }

    // The part of a command its row shows, next to its number of runs
    fn fit(&self, command: &Command, range: &Range<usize>) -> Fitted {
        let runs_width = runs(command).map_or(0, |runs| runs.width());

        fit(
            &command.text,
            range,
            self.command_width.saturating_sub(runs_width),
        )
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
    let fitted = layout.fit(command, &found.range);
    let mut spans = vec![Span::raw(layout.indent.as_str())];

    if layout.has_times {
        let time = command.time.as_deref().unwrap_or_default();
        spans.push(Span::styled(format!("{time:<TIME_WIDTH$}  "), dim));
    }

    if fitted.cut_start {
        spans.push(Span::styled(ELLIPSIS, dim));
    }

    spans.extend(highlight(
        &command.text,
        &found.range,
        fitted.shown,
        theme.matched,
    ));

    if fitted.cut_end {
        spans.push(Span::styled(ELLIPSIS, dim));
    }

    if let Some(runs) = runs(command) {
        spans.push(Span::styled(runs, dim));
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

// After a command run more than once
fn runs(command: &Command) -> Option<String> {
    let runs = command.entry_indices.len();

    (runs > 1).then(|| format!(" ({runs})"))
}

// The selected command in full under the list, after a divider, when its row is cut and the pane
// has room for it
fn preview<'a>(
    finder: &'a Finder,
    layout: &RowLayout,
    area: Rect,
    theme: &Theme,
) -> Option<Vec<Line<'a>>> {
    let found = finder.matches.get(finder.selected)?;
    let command = &finder.commands[found.command_index];

    if !layout.fit(command, &found.range).is_cut() {
        return None;
    }

    let width = usize::from(area.width);
    let mut lines = wrap(&command.text, width);
    let cut = lines.len() > PREVIEW_LINES;
    lines.truncate(PREVIEW_LINES);

    if usize::from(area.height) < 1 + MIN_LIST_ROWS + 1 + lines.len() {
        return None;
    }

    if cut && let Some(last) = lines.last_mut() {
        last.end = prefix_end(&command.text, last.start, width.saturating_sub(1));
    }

    let divider = Line::styled("─".repeat(width), theme.dim);
    let mut preview: Vec<Line> = lines
        .into_iter()
        .map(|shown| Line::from_iter(highlight(&command.text, &found.range, shown, theme.matched)))
        .collect();

    if cut && let Some(last) = preview.last_mut() {
        last.push_span(Span::styled(ELLIPSIS, theme.dim));
    }

    preview.insert(0, divider);
    Some(preview)
}

// The shown part of text, with what it holds of the match highlighted
fn highlight<'a>(
    text: &'a str,
    range: &Range<usize>,
    shown: Range<usize>,
    matched: Style,
) -> [Span<'a>; 3] {
    let clamp = |index: usize| index.clamp(shown.start, shown.end);
    let (start, end) = (clamp(range.start), clamp(range.end));

    [
        Span::raw(&text[shown.start..start]),
        Span::styled(&text[start..end], matched),
        Span::raw(&text[end..shown.end]),
    ]
}

// The part of a command shown in a row, and whether an ellipsis goes before or after it
#[derive(Debug, PartialEq)]
struct Fitted {
    shown: Range<usize>,
    cut_start: bool,
    cut_end: bool,
}

impl Fitted {
    fn is_cut(&self) -> bool {
        self.cut_start || self.cut_end
    }
}

// Fits text into width columns, ellipses included, keeping the match in view: the start of the
// text when the match is in it, else the end of the text when the match is in that, else the
// match a third of the way in
fn fit(text: &str, range: &Range<usize>, width: usize) -> Fitted {
    let fitted = |shown, cut_start, cut_end| Fitted {
        shown,
        cut_start,
        cut_end,
    };

    if text.width() <= width {
        return fitted(0..text.len(), false, false);
    }

    let room = width.saturating_sub(1);

    if text[..range.end].width() <= room {
        return fitted(0..prefix_end(text, 0, room), false, true);
    }

    let end_start = suffix_start(text, room);

    if end_start <= range.start {
        return fitted(end_start..text.len(), true, false);
    }

    let room = width.saturating_sub(2);
    let start = suffix_start(&text[..range.start], room / 3);

    fitted(start..prefix_end(text, start, room), true, true)
}

// Byte ranges of the lines text wraps into at width columns, broken after spaces where possible.
// A space at the end of a full line stays on it, past the width.
fn wrap(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = prefix_end(text, start, width);
        let end = if end == text.len() {
            end
        } else if text[end..].starts_with(' ') {
            end + 1
        } else if let Some(space) = text[start..end].rfind(' ') {
            start + space + 1
        } else if end == start {
            // A character wider than the line
            start + text[start..].chars().next().map_or(1, char::len_utf8)
        } else {
            end
        };

        lines.push(start..end);
        start = end;
    }

    lines
}

// Where text from start ends when it may take width columns
fn prefix_end(text: &str, start: usize, width: usize) -> usize {
    let mut used = 0;

    for (index, char) in text[start..].char_indices() {
        used += char.width().unwrap_or(0);

        if used > width {
            return start + index;
        }
    }

    text.len()
}

// Where the end of text starts when it may take width columns
fn suffix_start(text: &str, width: usize) -> usize {
    let mut used = 0;

    for (index, char) in text.char_indices().rev() {
        used += char.width().unwrap_or(0);

        if used > width {
            return index + char.len_utf8();
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Position;
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
        render_sized(finder, theme, prompt, width, 4)
    }

    fn render_sized(
        finder: &Finder,
        theme: &Theme,
        prompt: Option<&str>,
        width: u16,
        height: u16,
    ) -> Buffer {
        render_paged(finder, theme, prompt, width, height).0
    }

    // The rendered pane and the rows draw gives for PgUp and PgDn
    fn render_paged(
        finder: &Finder,
        theme: &Theme,
        prompt: Option<&str>,
        width: u16,
        height: u16,
    ) -> (Buffer, usize) {
        let prompt = prompt.map(prompt::parse);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut page = 0;
        terminal
            .draw(|frame| {
                let list = &mut ListState::default();
                page = draw(frame, finder, theme, prompt.as_ref(), None, list);
            })
            .unwrap();

        (terminal.backend().buffer().clone(), page)
    }

    fn page(finder: &Finder, height: u16) -> usize {
        render_paged(finder, &Theme::default(), None, 40, height).1
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
    fn puts_the_cursor_where_it_is_in_the_query() {
        let mut finder = finder("ls\n");
        finder.left();
        let mut terminal = Terminal::new(TestBackend::new(40, 4)).unwrap();
        terminal
            .draw(|frame| {
                let list = &mut ListState::default();
                draw(frame, &finder, &Theme::default(), None, None, list);
            })
            .unwrap();

        assert_eq!(terminal.get_cursor_position().unwrap(), Position::new(3, 0));
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

    // Most recent first, a command of 54 columns run twice, matched by ls at its end
    const LONG: &str = "ls -la\n\
        echo one two three four five six seven eight nine; ls\n\
        echo one two three four five six seven eight nine; ls\n";

    fn shown(text: &str, fitted: &Fitted) -> String {
        let start = if fitted.cut_start { ELLIPSIS } else { "" };
        let end = if fitted.cut_end { ELLIPSIS } else { "" };

        format!("{start}{}{end}", &text[fitted.shown.clone()])
    }

    #[test]
    fn fits_text_that_fits_as_it_is() {
        let fitted = fit("0123456789", &(3..4), 10);

        assert_eq!(shown("0123456789", &fitted), "0123456789");
        assert!(!fitted.is_cut());
    }

    #[test]
    fn cuts_the_end_when_the_match_is_early() {
        assert_eq!(
            shown("0123456789", &fit("0123456789", &(1..2), 6)),
            "01234…"
        );
    }

    #[test]
    fn shows_the_end_when_the_match_is_in_it() {
        assert_eq!(
            shown("0123456789", &fit("0123456789", &(8..9), 6)),
            "…56789"
        );
    }

    #[test]
    fn puts_a_match_in_the_middle_a_third_of_the_way_in() {
        let text = "0123456789abcdefghij";

        assert_eq!(shown(text, &fit(text, &(10..11), 8)), "…89abcd…");
    }

    #[test]
    fn fits_wide_characters_without_splitting_them() {
        let text = "日本語のテキスト";

        for width in 3..16 {
            for range in [0..3, 12..15, 21..24] {
                let fitted = fit(text, &range, width);

                assert!(shown(text, &fitted).width() <= width, "{width} {range:?}");
                assert!(fitted.shown.start <= range.start, "{width} {range:?}");
            }
        }
    }

    #[test]
    fn wraps_after_spaces() {
        assert_eq!(wrap("aa bb cc", 5), [0..6, 6..8]);
        assert_eq!(wrap("aa bbb cc", 5), [0..3, 3..7, 7..9]);
    }

    #[test]
    fn wraps_words_longer_than_the_line_where_they_fill_it() {
        assert_eq!(wrap("abcdefgh", 3), [0..3, 3..6, 6..8]);
        assert_eq!(wrap("日本", 1), [0..3, 3..6]);
    }

    #[test]
    fn shows_the_match_of_a_long_row_with_the_number_of_runs() {
        let buffer = render_sized(&finder(LONG), &Theme::default(), None, 40, 4);
        let row = text(&buffer, 1);

        assert_eq!(row, "  …our five six seven eight nine; ls (2)");
        assert_eq!(
            buffer[(row.find("ls").unwrap() as u16 - 2, 1)].fg,
            Color::Yellow
        );
        assert!((0..40).all(|x| buffer[(x, 1)].bg == Color::Indexed(238)));
    }

    #[test]
    fn previews_the_selected_command_when_its_row_is_cut() {
        let buffer = render_sized(&finder(LONG), &Theme::default(), None, 40, 8);

        assert_eq!(text(&buffer, 5), "─".repeat(40));
        assert_eq!(
            text(&buffer, 6).trim_end(),
            "echo one two three four five six seven"
        );
        assert_eq!(text(&buffer, 7).trim_end(), "eight nine; ls");
        assert_eq!(buffer[(12, 7)].fg, Color::Yellow);
    }

    #[test]
    fn previews_nothing_for_a_row_that_fits() {
        let mut finder = finder(LONG);
        finder.down();
        let buffer = render_sized(&finder, &Theme::default(), None, 40, 8);

        assert!(!(0..8).any(|y| text(&buffer, y).starts_with('─')));
    }

    #[test]
    fn previews_nothing_without_room_for_the_list() {
        let buffer = render_sized(&finder(LONG), &Theme::default(), None, 40, 6);

        assert!(!(0..6).any(|y| text(&buffer, y).starts_with('─')));
    }

    #[test]
    fn gives_the_list_rows_as_the_page_without_the_preview() {
        let mut finder = finder(LONG);

        // The prompt line, the divider and two lines of preview leave 4 rows
        assert_eq!(page(&finder, 8), 4);

        finder.down();
        assert_eq!(page(&finder, 8), 7);
    }
}
