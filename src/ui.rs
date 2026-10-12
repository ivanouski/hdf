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

    // A time column only when the history has timestamps, blank for commands from before
    let timed = finder.commands.iter().any(|command| command.time.is_some());
    let items = finder.matches.iter().map(|found| {
        let command = &finder.commands[found.command];
        let mut spans = vec![Span::raw("  ")];

        if timed {
            let time = command.time.as_deref().unwrap_or_default();
            spans.push(format!("{time:<16}  ").dark_gray());
        }

        spans.extend(highlight(&command.text, &found.range));

        if command.entries.len() > 1 {
            spans.push(format!(" ({})", command.entries.len()).dark_gray());
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

fn highlight<'a>(text: &'a str, range: &Range<usize>) -> [Span<'a>; 3] {
    let matched = Style::new().fg(Color::Yellow).bold();

    [
        Span::raw(&text[..range.start]),
        Span::styled(&text[range.clone()], matched),
        Span::raw(&text[range.end..]),
    ]
}
