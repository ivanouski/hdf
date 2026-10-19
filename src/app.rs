//! The finder running in its pane: the keys and how it was left

use std::io;
use std::ops::ControlFlow::{self, Break, Continue};
use std::sync::atomic::Ordering;

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::text::Line;

use crate::config::Theme;
use crate::context::Context;
use crate::finder::Finder;
use crate::history::History;
use crate::pane::Pane;
use crate::ui::Status;
use crate::{tty, ui};

/// How the finder was left
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Enter on a command, to run it
    Picked(String),
    /// Tab on a command, to edit it first
    Edited(String),
    /// Esc, or Enter and Tab without matches
    Quit,
    /// Ctrl-C
    Abort,
    /// SIGINT, SIGTERM or SIGHUP, with its number
    Signaled(usize),
}

/// Runs the finder in a pane under the cursor's line, returns how it was left
pub fn pick(
    finder: &mut Finder,
    history: &mut History,
    theme: &Theme,
    prompt: Option<&Line>,
) -> io::Result<Outcome> {
    let mut pane = Pane::open()?;
    let mut offset = 0;
    let mut status = None;
    let mut page = 1;
    let signal = tty::watch_signals()?;

    let outcome = loop {
        pane.draw(|frame| {
            page = ui::draw(frame, finder, theme, prompt, status.as_ref(), &mut offset);
        })?;

        match tty::next_event(&signal)? {
            None => break Outcome::Signaled(signal.load(Ordering::Relaxed)),
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                match handle_key(key, finder, history, page) {
                    Break(outcome) => break outcome,
                    Continue(next_status) => status = next_status,
                }
            }
            // Other events, like a resize, only redraw
            Some(_) => {}
        }
    };

    // The shell prompt stays for bash to draw over
    let width = pane.width();
    let closed = pane.close(ui::shell_prompt(prompt, width).map_or(0, Line::width));

    // After SIGHUP the terminal may be gone, the signal still decides the exit status
    match outcome {
        Outcome::Signaled(_) => Ok(outcome),
        _ => closed.map(|()| outcome),
    }
}

/// Leaves the finder with an outcome, or stays with the status to show until the next key. PgUp and
/// PgDn move the selection by the page, the rows of the list.
fn handle_key(
    key: KeyEvent,
    finder: &mut Finder,
    history: &mut History,
    page: usize,
) -> ControlFlow<Outcome, Option<Status>> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match (key.code, control) {
        (KeyCode::Esc, _) => return Break(Outcome::Quit),
        (KeyCode::Char('c'), true) => return Break(Outcome::Abort),
        (KeyCode::Enter | KeyCode::Tab, _) => {
            let pick = match key.code {
                KeyCode::Enter => Outcome::Picked,
                _ => Outcome::Edited,
            };

            return Break(match finder.selection() {
                Some(command) => pick(command.original.clone()),
                None => Outcome::Quit,
            });
        }
        (KeyCode::Char('d'), true) => return Continue(delete(finder, history)),
        (KeyCode::Up, _) | (KeyCode::Char('p'), true) => finder.up(),
        (KeyCode::Down, _) | (KeyCode::Char('n'), true) => finder.down(),
        (KeyCode::PageUp, _) => finder.page_up(page),
        (KeyCode::PageDown, _) => finder.page_down(page),
        // Option-Left and Right on a Mac send Alt-B and F or Alt-Left and Right, Linux terminals
        // Ctrl-Left and Right
        (KeyCode::Left, _) if control || alt => finder.word_left(),
        (KeyCode::Right, _) if control || alt => finder.word_right(),
        (KeyCode::Char('b'), false) if alt => finder.word_left(),
        (KeyCode::Char('f'), false) if alt => finder.word_right(),
        (KeyCode::Left, _) => finder.left(),
        (KeyCode::Right, _) => finder.right(),
        (KeyCode::Home, _) | (KeyCode::Char('a'), true) => finder.home(),
        (KeyCode::End, _) | (KeyCode::Char('e'), true) => finder.end(),
        (KeyCode::Backspace, _) if alt => finder.delete_word_back(),
        (KeyCode::Backspace, _) => finder.delete_back(),
        (KeyCode::Delete, _) => finder.delete_forward(),
        (KeyCode::Char('u'), true) => finder.delete_to_start(),
        (KeyCode::Char('w'), true) => finder.delete_big_word_back(),
        // Alt and a letter is a shortcut elsewhere, not text for the query
        (KeyCode::Char(c), false) if !alt => finder.push(c),
        _ => {}
    }

    Continue(None)
}

/// Deletes every occurrence of the selected command and reloads the history
fn delete(finder: &mut Finder, history: &mut History) -> Option<Status> {
    let command = finder.selection()?;
    let message = format!("deleted {}", command.text);
    let deleted = history
        .delete(&command.entry_indices)
        .context("cannot delete");
    let reloaded = history.reload().context("cannot read history");

    // Keep the selection in place, on the command after the deleted one
    finder.reload(history.entries());

    Some(match deleted.and(reloaded) {
        Ok(()) => Status::Deleted(message),
        Err(error) => Status::Failed(error),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::TempFile;

    /// The finder lists the commands most recent first: echo, pwd, ls
    const HISTORY: &str = "ls\npwd\necho\n";
    /// Rows of the list, for PgUp and PgDn
    const PAGE: usize = 2;

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const CONTROL: KeyModifiers = KeyModifiers::CONTROL;
    const ALT: KeyModifiers = KeyModifiers::ALT;

    /// The finder on a history file of its own
    struct Session {
        file: TempFile,
        history: History,
        finder: Finder,
    }

    impl Session {
        fn open(name: &str) -> Self {
            let file = TempFile::new(&format!("keys-{name}"), HISTORY);
            let history = History::load(&file.path).unwrap();
            let finder = Finder::new(history.entries(), String::new());

            Self {
                file,
                history,
                finder,
            }
        }

        fn press(
            &mut self,
            code: KeyCode,
            modifiers: KeyModifiers,
        ) -> ControlFlow<Outcome, Option<Status>> {
            handle_key(
                KeyEvent::new(code, modifiers),
                &mut self.finder,
                &mut self.history,
                PAGE,
            )
        }

        /// Presses a key that keeps the finder open without a status
        fn edit(&mut self, code: KeyCode, modifiers: KeyModifiers) {
            assert_eq!(self.press(code, modifiers), Continue(None));
        }
    }

    #[test]
    fn moves_the_selection_with_arrows_and_ctrl_p_n() {
        let mut session = Session::open("move");

        for (code, modifiers, selected) in [
            (KeyCode::Up, NONE, 0),
            (KeyCode::Down, NONE, 1),
            (KeyCode::Char('n'), CONTROL, 2),
            (KeyCode::Down, NONE, 2),
            (KeyCode::Up, NONE, 1),
            (KeyCode::Char('p'), CONTROL, 0),
        ] {
            session.edit(code, modifiers);
            assert_eq!(session.finder.selected(), selected);
        }
    }

    #[test]
    fn moves_the_selection_by_a_page_with_page_keys() {
        let mut session = Session::open("page");

        for (code, selected) in [
            (KeyCode::PageDown, 2),
            (KeyCode::PageDown, 2),
            (KeyCode::Up, 1),
            (KeyCode::PageUp, 0),
        ] {
            session.edit(code, NONE);
            assert_eq!(session.finder.selected(), selected);
        }
    }

    #[test]
    fn edits_the_query() {
        let mut session = Session::open("edit");

        session.edit(KeyCode::Char('e'), NONE);
        session.edit(KeyCode::Char('C'), KeyModifiers::SHIFT);
        assert_eq!(session.finder.query(), "eC");

        session.edit(KeyCode::Backspace, NONE);
        assert_eq!(session.finder.query(), "e");

        session.edit(KeyCode::Char('u'), CONTROL);
        assert_eq!(session.finder.query(), "");
    }

    #[test]
    fn moves_the_cursor_and_edits_at_it() {
        let mut session = Session::open("cursor");

        for c in "gt lg".chars() {
            session.edit(KeyCode::Char(c), NONE);
        }

        for (code, modifiers, query, cursor) in [
            (KeyCode::Left, NONE, "gt lg", 4),
            (KeyCode::Char('o'), NONE, "gt log", 5),
            (KeyCode::Home, NONE, "gt log", 0),
            (KeyCode::Right, NONE, "gt log", 1),
            (KeyCode::Char('i'), NONE, "git log", 2),
            (KeyCode::End, NONE, "git log", 7),
            (KeyCode::Char('a'), CONTROL, "git log", 0),
            (KeyCode::Delete, NONE, "it log", 0),
            (KeyCode::Char('e'), CONTROL, "it log", 6),
            (KeyCode::Left, NONE, "it log", 5),
            (KeyCode::Left, NONE, "it log", 4),
            (KeyCode::Char('u'), CONTROL, "og", 0),
        ] {
            session.edit(code, modifiers);
            assert_eq!(
                (session.finder.query(), session.finder.cursor()),
                (query, cursor)
            );
        }
    }

    #[test]
    fn moves_the_cursor_and_deletes_by_words() {
        let mut session = Session::open("words");

        for c in "cd ~/src/utils".chars() {
            session.edit(KeyCode::Char(c), NONE);
        }

        for (code, modifiers, query, cursor) in [
            (KeyCode::Char('b'), ALT, "cd ~/src/utils", 9),
            (KeyCode::Left, ALT, "cd ~/src/utils", 5),
            (KeyCode::Left, CONTROL, "cd ~/src/utils", 0),
            (KeyCode::Char('f'), ALT, "cd ~/src/utils", 2),
            (KeyCode::Right, ALT, "cd ~/src/utils", 8),
            (KeyCode::Right, CONTROL, "cd ~/src/utils", 14),
            (KeyCode::Backspace, ALT, "cd ~/src/", 9),
            (KeyCode::Char('w'), CONTROL, "cd ", 3),
        ] {
            session.edit(code, modifiers);
            assert_eq!(
                (session.finder.query(), session.finder.cursor()),
                (query, cursor)
            );
        }
    }

    #[test]
    fn leaves_alt_and_control_letters_out_of_the_query() {
        let mut session = Session::open("alt");

        session.edit(KeyCode::Char('x'), ALT);
        session.edit(KeyCode::Char('x'), CONTROL);

        assert_eq!(session.finder.query(), "");
    }

    #[test]
    fn picks_the_selection_with_enter_to_run_and_tab_to_edit() {
        let mut session = Session::open("pick");

        assert_eq!(
            session.press(KeyCode::Enter, NONE),
            Break(Outcome::Picked("echo".to_string()))
        );

        session.edit(KeyCode::Down, NONE);

        assert_eq!(
            session.press(KeyCode::Tab, NONE),
            Break(Outcome::Edited("pwd".to_string()))
        );
    }

    #[test]
    fn quits_on_enter_and_tab_without_matches() {
        let mut session = Session::open("no-matches");
        session.edit(KeyCode::Char('z'), NONE);

        assert_eq!(session.press(KeyCode::Enter, NONE), Break(Outcome::Quit));
        assert_eq!(session.press(KeyCode::Tab, NONE), Break(Outcome::Quit));
    }

    #[test]
    fn quits_on_esc_and_aborts_on_ctrl_c() {
        let mut session = Session::open("quit");

        assert_eq!(session.press(KeyCode::Esc, NONE), Break(Outcome::Quit));
        assert_eq!(
            session.press(KeyCode::Char('c'), CONTROL),
            Break(Outcome::Abort)
        );
    }

    #[test]
    fn deletes_the_selection_and_selects_the_next_command() {
        let mut session = Session::open("delete");
        session.edit(KeyCode::Down, NONE);

        assert_eq!(
            session.press(KeyCode::Char('d'), CONTROL),
            Continue(Some(Status::Deleted("deleted pwd".to_string())))
        );
        assert_eq!(session.file.read(), "ls\necho\n");
        assert_eq!(session.finder.selection().unwrap().text, "ls");
    }

    #[test]
    fn shows_the_error_when_another_shell_changed_the_history() {
        let mut session = Session::open("delete-changed");
        fs::write(&session.file.path, "date\n").unwrap();

        assert_eq!(
            session.press(KeyCode::Char('d'), CONTROL),
            Continue(Some(Status::Failed(
                "cannot delete: history file was changed by another process, try again".to_string()
            )))
        );
        assert_eq!(session.file.read(), "date\n");
        // Reloaded all the same, so the finder shows what the file holds
        assert_eq!(session.finder.selection().unwrap().text, "date");
    }

    #[test]
    fn deletes_nothing_without_matches() {
        let mut session = Session::open("delete-none");
        session.edit(KeyCode::Char('z'), NONE);
        session.edit(KeyCode::Char('d'), CONTROL);

        assert_eq!(session.file.read(), HISTORY);
    }
}
