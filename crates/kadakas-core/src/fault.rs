//! A test hook (feature `fault-injection`): a command of this thread panics after it has changed
//! the document. The API tests use it to check that a panic leaves no trace. It is a feature that
//! no program turns on; `scripts/check-test-hook.sh` checks that.

use std::cell::Cell;

thread_local! {
    static COUNTDOWN: Cell<u32> = const { Cell::new(0) };
}

/// The next check on this thread panics.
pub fn arm() {
    arm_after(1);
}

/// The `n`-th check from now panics (1 is the next one), so a test can choose a later step.
pub fn arm_after(n: u32) {
    COUNTDOWN.with(|c| c.set(n));
}

/// True once, when the countdown reaches its end.
pub fn take() -> bool {
    COUNTDOWN.with(|c| match c.get() {
        0 => false,
        n => {
            c.set(n - 1);
            n == 1
        }
    })
}
