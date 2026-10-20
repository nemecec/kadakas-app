//! A host that has a person at the screen (the app) must not let an agent replace a drawing
//! that the person has not saved. With the guard on, `file.open`, `document.load` and
//! `collab.connect` (also with `replace: true`) refuse with the reason `unsaved_changes`, and the
//! session remembers what was asked, so that the host can show the person a notice
//! (Save and open / Don't save / Refuse). The guard is off by default: a headless session
//! (`kadakas serve`, the library, the tests) has no person to ask, and its document is the
//! agent's own scratch drawing. `version.restore` is not guarded: it is one undoable step that
//! first keeps the drawing as a version.

use crate::{RpcError, Session};
use std::path::PathBuf;

/// What a refused call wanted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// `file.open` of this file.
    Open(PathBuf),
    /// `document.load` of a document that came in the call.
    Load,
    /// `collab.connect`: the shared document would replace the drawing.
    Connect,
}

impl Session {
    /// Turns the guard on or off (see the module text). The app turns it on.
    pub fn set_guard_unsaved(&mut self, on: bool) {
        self.guard_unsaved = on;
    }

    /// True when the drawing has changes that nobody saved: since the last save or load, or, in
    /// a session that never saved or loaded, since the start.
    pub fn has_unsaved_changes(&self) -> bool {
        match self.clean_rev() {
            Some(r) => self.editor.revision() != r,
            None => self.editor.undo_depth() > 0,
        }
    }

    /// Calls the drawing unsaved (a recovered copy was never saved).
    pub fn mark_unsaved(&mut self) {
        self.set_clean_rev(Some(u64::MAX));
    }

    /// What the last refused call wanted. The host takes it once, to tell the person.
    pub fn take_blocked(&mut self) -> Option<Blocked> {
        self.blocked.take()
    }

    /// Refuses a call that would replace the drawing, when the guard is on and the drawing has
    /// unsaved changes.
    pub(crate) fn refuse_replace(&mut self, what: Blocked, method: &str) -> Result<(), RpcError> {
        if !self.guard_unsaved || !self.has_unsaved_changes() {
            return Ok(());
        }
        self.blocked = Some(what);
        Err(RpcError::conflict(format!(
            "{method} was refused: the open drawing has changes that the person has not saved, and nothing was replaced. Ask the person to save the drawing (the app shows them a notice with Save and open, Don't save and Refuse), then call {method} again"
        ))
        .with_reason(crate::errors::REASON_UNSAVED_CHANGES))
    }
}
