//! A test hook (feature `fault-injection`): the next `add_object` of this thread panics after it
//! has changed the document. The API tests use it to check that a panic leaves no trace.

use std::cell::Cell;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

/// The next command on this thread that checks the hook panics.
pub fn arm() {
    ARMED.with(|a| a.set(true));
}

/// True once after `arm` (and disarms).
pub fn take() -> bool {
    ARMED.with(|a| a.replace(false))
}
