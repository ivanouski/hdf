use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListState, Paragraph};

use crate::finder::Finder;

// The status, the outcome of the last Ctrl-D, shows in place of the count until the next key
pub fn draw(
    frame: &mut Frame,
    finder: &Finder,
    status: Option<&Result<String, String>>,
    list: &mut ListState,
) {
    let [prompt_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(frame.area());

    let count = match status {
        Some(Ok(message)) => message.as_str().green(),
        Some(Err(message)) => message.as_str().red(),
        None => format!("{}/{}", finder.matches.len(), finder.commands.len()).dark_gray(),
    };
    let prompt = Line::from(vec!["> ".blue().bold(), Span::raw(&finder.query)]);

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
        // The selected row is shown reversed: colors of its own would reverse into a patchwork of
        // backgrounds, so it keeps only the bold match
        let selected = index == finder.selected;
        let dim = if selected {
            Style::new()
        } else {
            Style::new().dark_gray()
        };
        let mut spans = vec![Span::raw("  ")];

        if let Some(width) = time_width {
            let time = command.time.as_deref().unwrap_or_default();
            spans.push(Span::styled(format!("{time:<width$}  "), dim));
        }

        spans.extend(highlight(&command.text, &found.range, selected));

        if command.entries.len() > 1 {
            spans.push(Span::styled(format!(" ({})", command.entries.len()), dim));
        }

        Line::from(spans)
    });

    list.select((!finder.matches.is_empty()).then_some(finder.selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Modifier::REVERSED),
        list_area,
        list,
    );
}

fn highlight<'a>(text: &'a str, range: &Range<usize>, selected: bool) -> [Span<'a>; 3] {
    let matched = if selected {
        Style::new().bold()
    } else {
        Style::new().fg(Color::Yellow).bold()
    };

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

    use super::*;
    use crate::history::History;

    fn finder(name: &str, content: &str) -> Finder {
        let path = std::env::temp_dir().join(format!("hdf-ui-test-{name}-{}", std::process::id()));
        fs::write(&path, content).unwrap();
        let history = History::load(&path).unwrap();
        fs::remove_file(&path).unwrap();

        Finder::new(history.entries(), "ls".to_string())
    }

    fn rows(finder: &Finder) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(40, 4)).unwrap();
        terminal
            .draw(|frame| draw(frame, finder, None, &mut ListState::default()))
            .unwrap();

        terminal.backend().buffer().clone()
    }

    #[test]
    fn shows_the_selected_row_in_one_style() {
        let buffer = rows(&finder("selected", "#1700000000\nls\n#1700000000\nls\n"));
        let styles: Vec<Style> = (0..40)
            .map(|x| buffer[(x, 1)].style())
            .map(|style| Style {
                add_modifier: style.add_modifier - Modifier::BOLD,
                ..style
            })
            .collect();

        assert!(
            styles.windows(2).all(|pair| pair[0] == pair[1]),
            "{styles:?}"
        );
    }

    #[test]
    fn pads_the_time_column_to_the_widest_time() {
        let buffer = rows(&finder("width", "ls\n#1700000000\nls -la\n"));
        let row = |y| -> String { (0..40).map(|x| buffer[(x, y)].symbol()).collect() };

        // The command after the time and two spaces, also where the time is blank
        assert_eq!(row(2).find("ls").unwrap(), row(1).find("ls -la").unwrap());
        assert_eq!(
            row(1).find("ls -la").unwrap(),
            2 + "2023-11-14 22:13".len() + 2
        );
    }
}
