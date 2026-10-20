//! The last log lines and the names of the last commands, kept in memory for the crash file.
//!
//! One process-wide ring. Writers are the session (`kadakas-api`) and the app; the reader is the
//! panic hook. Both use `try_lock`: a panic that fires while a writer holds the lock does not
//! wait for it, and the hook then simply has less to write. A command is a name only (`edit.move`):
//! the parameters never come here.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::file::{MAX_COMMANDS, MAX_LINES};

/// The longest log line or command name that is kept.
const MAX_ITEM: usize = 300;

struct Ring {
    log: VecDeque<String>,
    commands: VecDeque<String>,
}

static RING: Mutex<Ring> = Mutex::new(Ring {
    log: VecDeque::new(),
    commands: VecDeque::new(),
});

fn cut(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_ITEM)
        .collect()
}

fn push(q: &mut VecDeque<String>, max: usize, item: String) {
    if q.len() >= max {
        q.pop_front();
    }
    q.push_back(item);
}

/// Adds a log line. Callers pass text that is already free of document content (the session
/// scrubs its lines); the crash report scrubs again when it is assembled.
pub fn note_log(line: &str) {
    if let Ok(mut r) = RING.try_lock() {
        push(&mut r.log, MAX_LINES, cut(line));
    }
}

/// Adds the name of a command or API method. Only a plain name is kept: anything with a
/// character other than letters, digits, `_`, `.`, `-` and `:` is dropped, so a parameter or a
/// piece of text cannot come in by mistake.
pub fn note_command(name: &str) {
    if name.is_empty()
        || name.len() > 80
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ':'))
    {
        return;
    }
    if let Ok(mut r) = RING.try_lock() {
        push(&mut r.commands, MAX_COMMANDS, name.to_string());
    }
}

thread_local! {
    static CATCHING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Marks a place where a panic is caught on purpose (`catch_unwind`). While a guard lives, the
/// panic hook of this crate writes no crash file for a panic on this thread: the program goes
/// on, so it is no crash. Hold the guard around the `catch_unwind` call.
///
/// `std::panic::resume_unwind` does not call the panic hook at all, so a panic that is caught
/// and resumed inside a guard writes no file. A new panic in the handler of a caught panic is a
/// panic of its own and is written unless another guard is held.
pub struct CatchGuard(());

impl CatchGuard {
    pub fn new() -> CatchGuard {
        CATCHING.with(|c| c.set(c.get().saturating_add(1)));
        CatchGuard(())
    }
}

impl Default for CatchGuard {
    fn default() -> Self {
        CatchGuard::new()
    }
}

impl Drop for CatchGuard {
    fn drop(&mut self) {
        CATCHING.with(|c| c.set(c.get().saturating_sub(1)));
    }
}

/// True while a [`CatchGuard`] lives on this thread.
pub fn catching() -> bool {
    CATCHING.try_with(|c| c.get() > 0).unwrap_or(false)
}

/// The lines and the command names, oldest first. Empty if the ring is busy.
pub fn snapshot() -> (Vec<String>, Vec<String>) {
    match RING.try_lock() {
        Ok(r) => (
            r.log.iter().cloned().collect(),
            r.commands.iter().cloned().collect(),
        ),
        Err(_) => (Vec::new(), Vec::new()),
    }
}

/// Empties the ring (tests).
pub fn clear() {
    if let Ok(mut r) = RING.lock() {
        r.log.clear();
        r.commands.clear();
    }
}

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_last_items_only() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        for i in 0..(MAX_LINES + 20) {
            note_log(&format!("line {i}"));
        }
        for i in 0..(MAX_COMMANDS + 20) {
            note_command(&format!("edit.move{i}"));
        }
        let (log, cmds) = snapshot();
        assert_eq!(log.len(), MAX_LINES);
        assert_eq!(log.last().unwrap(), &format!("line {}", MAX_LINES + 19));
        assert_eq!(log[0], "line 20");
        assert_eq!(cmds.len(), MAX_COMMANDS);
        assert_eq!(cmds[0], "edit.move20");
        clear();
        assert_eq!(snapshot(), (vec![], vec![]));
    }

    #[test]
    fn only_plain_names_are_commands() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        for bad in [
            "",
            "move 3,4",
            "open /Users/ann/x.kad",
            "name=Oak",
            "{\"a\":1}",
            "emoji😀",
            &"x".repeat(81),
        ] {
            note_command(bad);
        }
        note_command("edit.move");
        note_command("view:zoom-extents");
        let (_, cmds) = snapshot();
        assert_eq!(cmds, vec!["edit.move", "view:zoom-extents"]);
        clear();
    }

    #[test]
    fn log_lines_are_cut_and_cleaned() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        note_log(&format!("a\u{0}b{}", "z".repeat(1000)));
        let (log, _) = snapshot();
        assert_eq!(log[0].chars().count(), MAX_ITEM);
        assert!(log[0].starts_with("ab"));
        clear();
    }

    #[test]
    fn guards_nest_and_end() {
        assert!(!catching());
        {
            let _a = CatchGuard::new();
            assert!(catching());
            {
                let _b = CatchGuard::default();
                assert!(catching());
            }
            assert!(catching(), "the outer guard still lives");
        }
        assert!(!catching());
        // A guard is per thread.
        let _g = CatchGuard::new();
        assert!(!std::thread::spawn(catching).join().unwrap());
    }

    #[test]
    fn a_busy_ring_does_not_block() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        let held = RING.lock().unwrap();
        note_log("lost");
        note_command("lost");
        assert_eq!(snapshot(), (vec![], vec![]));
        drop(held);
        assert_eq!(snapshot(), (vec![], vec![]));
    }
}
