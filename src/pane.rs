use std::fs::{File, OpenOptions};
use std::io::{self, Write};

use ratatui::backend::{Backend, ClearType, CrosstermBackend};
use ratatui::crossterm::{cursor, terminal};
use ratatui::layout::Position;
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

use crate::tty::{RawMode, StdoutToTty};

const PANE_HEIGHT: u16 = 12;

// The finder's pane, drawn on the terminal itself so the finder works with stdout captured
pub struct Pane {
    terminal: Terminal<CrosstermBackend<File>>,
    // Of the cursor when opened, on text to go back to unless 0
    column: u16,
    // Fields drop in order, so these are restored after the terminal is gone
    _raw_mode: RawMode,
    _stdout: StdoutToTty,
}

impl Pane {
    pub fn open() -> io::Result<Self> {
        let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let stdout = StdoutToTty::new(&tty)?;
        let raw_mode = RawMode::enable()?;

        // Bash clears the prompt line before running a bind -x command, so the pane opens there.
        // When the cursor is still on some text, it opens on the next line to keep that text.
        let (column, _) = cursor::position()?;

        if column > 0 {
            tty.write_all(b"\r\n")?;
        }

        let height = terminal::size()?.1.min(PANE_HEIGHT);
        let terminal = Terminal::with_options(
            CrosstermBackend::new(tty),
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?;

        Ok(Self {
            terminal,
            column,
            _raw_mode: raw_mode,
            _stdout: stdout,
        })
    }

    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
        self.terminal.draw(render).map(|_| ())
    }

    // Leaves no trace of the pane and puts the cursor back where it was
    pub fn close(mut self) -> io::Result<()> {
        let area = self.terminal.get_frame().area();
        let backend = self.terminal.backend_mut();
        backend.set_cursor_position(area.as_position())?;
        backend.clear_region(ClearType::AfterCursor)?;

        if self.column > 0 {
            backend.set_cursor_position(Position::new(self.column, area.y.saturating_sub(1)))?;
        }

        self.terminal.show_cursor()
    }
}
