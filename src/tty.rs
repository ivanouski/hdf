use std::io::{self, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::terminal;

pub struct RawMode;

impl RawMode {
    pub fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

// Points stdout at the terminal until dropped. crossterm asks the terminal for the cursor position
// on stdout, which is a pipe when the command is captured, as in the bash setup.
pub struct StdoutToTty {
    saved: OwnedFd,
}

impl StdoutToTty {
    pub fn new(tty: &impl AsRawFd) -> io::Result<Self> {
        let saved = io::stdout().as_fd().try_clone_to_owned()?;
        dup2(tty.as_raw_fd(), libc::STDOUT_FILENO)?;
        Ok(Self { saved })
    }
}

impl Drop for StdoutToTty {
    fn drop(&mut self) {
        let _ = io::stdout().flush();
        let _ = dup2(self.saved.as_raw_fd(), libc::STDOUT_FILENO);
    }
}

fn dup2(from: i32, to: i32) -> io::Result<()> {
    // SAFETY: dup2 only takes file descriptors; both are open for the duration of the call
    match unsafe { libc::dup2(from, to) } {
        -1 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

// Raw mode keeps Ctrl-C from sending SIGINT, but other processes can still send signals: they are
// taken as a way out, so the pane is cleared and the terminal restored like on Esc. Returns the
// number of the signal once one arrived, 0 until then.
pub fn watch_signals() -> io::Result<Arc<AtomicUsize>> {
    let signal = Arc::new(AtomicUsize::new(0));

    for number in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        signal_hook::flag::register_usize(number, Arc::clone(&signal), number as usize)?;
    }

    exit_when_stuck(Arc::clone(&signal));

    Ok(signal)
}

// crossterm keeps reading a closed terminal and never returns, so the event loop never sees the
// signal that came with closing it. Exits for the signal once the loop has had time to clean up.
fn exit_when_stuck(signal: Arc<AtomicUsize>) {
    // Compared with the parent at the start, not with 1: a shell that is PID 1, as in a container,
    // runs hdf as its child
    let parent = std::os::unix::process::parent_id();

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));

            // Orphaned: the shell is gone, maybe without a signal reaching hdf. Taken for SIGHUP,
            // so a loop that is not stuck closes the pane and restores the terminal.
            if std::os::unix::process::parent_id() != parent {
                let hangup = libc::SIGHUP as usize;
                let _ = signal.compare_exchange(0, hangup, Ordering::Relaxed, Ordering::Relaxed);
            }

            let number = signal.load(Ordering::Relaxed);

            if number != 0 {
                thread::sleep(Duration::from_millis(500));
                process::exit(signal_status(number).into());
            }
        }
    });
}

// The exit status for a signal, as shells report it
pub fn signal_status(signal: usize) -> u8 {
    128 + signal as u8
}

// The next terminal event, or None once a signal arrived
pub fn next_event(signal: &AtomicUsize) -> io::Result<Option<Event>> {
    loop {
        if signal.load(Ordering::Relaxed) != 0 {
            return Ok(None);
        }

        match event::poll(Duration::from_millis(100)) {
            Ok(true) => return event::read().map(Some),
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}
