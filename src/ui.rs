use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::config::Theme;
use crate::finder::Finder;

// The status, the outcome of the last Ctrl-D, shows in place of the count until the next key
pub fn draw(
    frame: &mut Frame,
    finder: &Finder,
    theme: &Theme,
    status: Option<&Result<String, String>>,
    list: &mut ListState,
) {
    let [prompt_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(frame.area());

    let count = match status {
        Some(Ok(message)) => Span::styled(message, theme.deleted),
        Some(Err(message)) => Span::styled(message, theme.error),
        None => Span::styled(
            format!("{}/{}", finder.matches.len(), finder.commands.len()),
            theme.count,
        ),
    };
    let prompt = Line::from(vec![
        Span::styled("> ", theme.prompt),
        Span::raw(&finder.query),
    ]);

    // The prompt goes over the count, so a long query stays readable
    frame.set_cursor_position((prompt_area.x + prompt.width() as u16, prompt_area.y));
    frame.render_widget(Paragraph::new(count).right_aligned(), prompt_area);
    frame.render_widget(Paragraph::new(prompt), prompt_area);

    // A time column only when the history has timestamps, as wide as the widest time and blank
    // for commands from before
    let time_width = finder
        .commands
        .iter()
        .filter_map(|command| command.time.as_deref())
        .map(|time| Span::raw(time).width())
        .max();
    let items = finder.matches.iter().enumerate().map(|(index, found)| {
        let command = &finder.commands[found.command];
        // The selected row has a style of its own across its width, the match drawn over it. Its
        // time and number of runs are not dimmed, dim colors are hard to read on its background.
        let selected = index == finder.selected;
        let dim = if selected { Style::new() } else { theme.dim };
        let mut spans = vec![Span::raw("  ")];

        if let Some(width) = time_width {
            let time = command.time.as_deref().unwrap_or_default();
            spans.push(Span::styled(format!("{time:<width$}  "), dim));
        }

        spans.extend(highlight(&command.text, &found.range, theme.matched));

        if command.entries.len() > 1 {
            spans.push(Span::styled(format!(" ({})", command.entries.len()), dim));
        }

        // The item style goes on the row first and the spans patch it, while a highlight style
        // of the list would go over the spans and could hide the match
        let item = ListItem::new(Line::from(spans));

        if selected {
            item.style(theme.selected)
        } else {
            item
        }
    });

    list.select((!finder.matches.is_empty()).then_some(finder.selected));
    frame.render_stateful_widget(List::new(items), list_area, list);
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
    use std::fs;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::history::History;

    fn finder(name: &str, content: &str) -> Finder {
        let path = std::env::temp_dir().join(format!("hdf-ui-test-{name}-{}", std::process::id()));
        fs::write(&path, content).unwrap();
        let history = History::load(&path).unwrap();
        fs::remove_file(&path).unwrap();

        Finder::new(history.entries(), "ls".to_string())
    }

    fn rows(finder: &Finder, theme: &Theme) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(40, 4)).unwrap();
        terminal
            .draw(|frame| draw(frame, finder, theme, None, &mut ListState::default()))
            .unwrap();

        terminal.backend().buffer().clone()
    }

    // The match is at 20 and 21, after the indent, the time and two spaces
    const HISTORY: &str = "#1700000000\nls -la\n#1700000000\nls\n";

    #[test]
    fn shows_the_selected_row_on_its_background_with_the_match_over_it() {
        let buffer = rows(&finder("selected", HISTORY), &Theme::default());
        let cell = |x| &buffer[(x, 1)];

        assert!((0..40).all(|x| cell(x).bg == Color::Indexed(238)));
        assert!((20..22).all(|x| cell(x).fg == Color::Yellow));
        assert!(cell(20).modifier.contains(Modifier::BOLD));
        // Neither the time nor the rest of the command dimmed
        assert!((0..20).chain(22..40).all(|x| cell(x).fg == Color::Reset));
    }

    #[test]
    fn dims_the_times_of_other_rows() {
        let buffer = rows(&finder("other", HISTORY), &Theme::default());
        let cell = |x| &buffer[(x, 2)];

        assert!((0..40).all(|x| cell(x).bg == Color::Reset));
        assert!((2..18).all(|x| cell(x).fg == Color::DarkGray));
        assert!((20..22).all(|x| cell(x).fg == Color::Yellow));
        assert!(cell(23).fg == Color::Reset);
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
        let buffer = rows(&finder("theme", HISTORY), &theme);

        assert_eq!(buffer[(0, 0)].fg, Color::Green);
        assert_eq!(buffer[(39, 0)].fg, Color::Magenta);
        assert_eq!(buffer[(0, 1)].bg, Color::Blue);
        assert_eq!(buffer[(20, 1)].fg, Color::Red);
        assert_eq!(buffer[(20, 1)].bg, Color::Blue);
        assert_eq!(buffer[(2, 2)].fg, Color::Gray);
        assert_eq!(buffer[(20, 2)].fg, Color::Red);
    }

    #[test]
    fn pads_the_time_column_to_the_widest_time() {
        let buffer = rows(
            &finder("width", "ls\n#1700000000\nls -la\n"),
            &Theme::default(),
        );
        let row = |y| -> String { (0..40).map(|x| buffer[(x, y)].symbol()).collect() };

        // The command after the time and two spaces, also where the time is blank
        assert_eq!(row(2).find("ls").unwrap(), row(1).find("ls -la").unwrap());
        assert_eq!(
            row(1).find("ls -la").unwrap(),
            2 + "2023-11-14 22:13".len() + 2
        );
    }
}
