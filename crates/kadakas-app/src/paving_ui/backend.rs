//! The session as the backend of the paving dialogs (see `kadakas_ui_core::paving`). A call goes
//! through `Session::handle`, the same entry that an agent uses; a preview runs it on a copy of
//! the document.

use kadakas_api::Session;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Document, ObjectId};
use kadakas_ui_core::paving::Backend;
use serde_json::Value;

pub struct SessionBackend<'a>(pub &'a mut Session);

impl Backend for SessionBackend<'_> {
    fn document(&self) -> &Document {
        self.0.editor.doc()
    }

    fn selection(&self) -> &[ObjectId] {
        &self.0.selection
    }

    fn revision(&self) -> u64 {
        self.0.editor.revision()
    }

    fn pattern_units(&self) -> usize {
        self.0.limits().pattern_units
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.0.handle(method, params).map_err(|e| e.message)
    }

    fn rehearse(&self, method: &str, params: Value) -> Result<(Value, Document), String> {
        // A session of its own on a copy of the document: nothing it does reaches the open one.
        let mut copy = Session::default();
        copy.set_limits(self.0.limits().clone());
        copy.editor = Editor::new(self.0.editor.doc().clone());
        copy.selection = self.0.selection.clone();
        let reply = copy.handle(method, params).map_err(|e| e.message)?;
        Ok((reply, copy.editor.doc().clone()))
    }

    fn execute(&mut self, cmd: &Command) -> Result<(), String> {
        self.0
            .editor
            .execute(cmd)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
