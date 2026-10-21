//! Which way the edits that add edges find their candidates: with the spatial index (`near`) or
//! with the scans of all vertices, edges and faces (the code as it was before the index). Both give
//! the same result to the bit. The edits use the index; tests can choose the scans, so that the
//! index is compared with them (see `tests_index`). Measured on a model of 80,000 edges, the index
//! is as fast as the scans already for an edit of one edge (its build costs about as much as the
//! scans of one edge), so there is no switch by size.

#[cfg(test)]
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::surface) enum Mode {
    Index,
    #[cfg_attr(not(test), allow(dead_code))]
    Scan,
}

#[cfg(test)]
thread_local! {
    static MODE: Cell<Mode> = const { Cell::new(Mode::Index) };
}

/// The way chosen for the edits on this thread.
pub(in crate::surface) fn mode() -> Mode {
    #[cfg(test)]
    {
        MODE.with(Cell::get)
    }
    #[cfg(not(test))]
    {
        Mode::Index
    }
}

/// Runs `f` with the way `mode` (on this thread).
#[cfg(test)]
pub(in crate::surface) fn with<T>(mode: Mode, f: impl FnOnce() -> T) -> T {
    struct Back(Mode);
    impl Drop for Back {
        fn drop(&mut self) {
            MODE.with(|c| c.set(self.0));
        }
    }
    let _back = Back(MODE.with(|c| c.replace(mode)));
    f()
}
