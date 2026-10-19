use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};

use ratatui::backend::{Backend, ClearType, CrosstermBackend};
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::crossterm::{cursor, execute, queue, terminal};
use ratatui::layout::Position;
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

use crate::tty::{RawMode, StdoutToTty};

const PANE_HEIGHT: u16 = 12;

// Holds a whole frame, so it reaches the terminal at once rather than cell by cell
const FRAME_BUFFER: usize = 1 << 16;

// The finder's pane, drawn on the terminal itself so the finder works with stdout captured
pub struct Pane {
    terminal: Terminal<CrosstermBackend<BufWriter<File>>>,
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
            CrosstermBackend::new(BufWriter::with_capacity(FRAME_BUFFER, tty)),
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

    // Terminals that support synchronized updates show the frame only once all of it arrived. The
    // update is ended even when drawing fails, or the terminal keeps waiting for it.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
        queue!(self.terminal.backend_mut(), BeginSynchronizedUpdate)?;
        let drawn = self.terminal.draw(render).map(|_| ());
        let ended = execute!(self.terminal.backend_mut(), EndSynchronizedUpdate);
        drawn.and(ended)
    }

    pub fn width(&mut self) -> u16 {
        self.terminal.get_frame().area().width
    }

    // Clears the pane but the shell prompt, the first kept columns of its first row, and puts the
    // cursor back where it was. Bash draws the same prompt there after hdf, so the line doesn't
    // flash empty in between. It takes the rest of the line for empty, so the query goes.
    pub fn close(mut self, kept: usize) -> io::Result<()> {
        let area = self.terminal.get_frame().area();
        // Opened under some text, the cursor goes back to it and the pane has no prompt to keep
        let (kept, cursor) = if self.column > 0 {
            (0, Position::new(self.column, area.y.saturating_sub(1)))
        } else {
            (kept.min(usize::from(area.width)) as u16, area.as_position())
        };
        let backend = self.terminal.backend_mut();
        backend.set_cursor_position(Position::new(kept, area.y))?;
        backend.clear_region(ClearType::AfterCursor)?;
        backend.set_cursor_position(cursor)?;

        self.terminal.show_cursor()
    }
}
