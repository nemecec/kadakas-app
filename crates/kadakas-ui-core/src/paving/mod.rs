//! The logic of the paving UI (PAT-06 to PAT-12), without egui: regions from loose curves, zones,
//! courses, the pattern check and the counts of units in Object Info.
//!
//! Every action goes through the same API methods that an agent calls (`region.from_curves`,
//! `hardscape.split`, `hardscape.course`, `unit_pattern.check`, `report.unit_pattern`), so the
//! drawing, the report and the export see the same data. A UI program supplies a [`Backend`]; the
//! modules here build the parameters, read the replies into plain structs and keep the state of a
//! dialog. A preview is a call on a copy of the document ([`Backend::rehearse`]): it runs the very
//! same code and changes nothing. Only the part that the API has no call for (the settings of a
//! zone) is a core command on the plan data of the object, the one that `hardscape.split` stores.
//!
//! - [`curves`]: Create Objects from Shapes (PAT-06).
//! - [`split`]: zones, and the settings of a zone (PAT-07).
//! - [`course`]: courses along an edge (PAT-08).
//! - [`check`]: the pattern check and its findings (PAT-10).
//! - [`counts`]: the counts of whole and cut units of an object (PAT-04).
//! - [`pick`]: picking an edge of an outline.

use kadakas_core::model::{Document, ObjectId};
use serde_json::Value;

pub mod check;
pub mod counts;
pub mod course;
pub mod curves;
pub mod pick;
pub mod split;

/// What the dialogs need from the program: the document, the selection and the API.
pub trait Backend {
    fn document(&self) -> &Document;
    fn selection(&self) -> &[ObjectId];
    /// The revision of the document; it changes with every edit.
    fn revision(&self) -> u64;
    /// The most units that a pattern may have (`SessionLimits::pattern_units`).
    fn pattern_units(&self) -> usize;
    /// Runs an API method on the open document. A method that changes the document is one undo
    /// step. The error is the text of the API error.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// Runs an API method on a copy of the document and gives the copy back with the reply. The
    /// open document does not change. This is the preview.
    fn rehearse(&self, method: &str, params: Value) -> Result<(Value, Document), String>;
    /// Runs a core command on the open document (one undo step).
    fn execute(&mut self, cmd: &kadakas_core::command::Command) -> Result<(), String>;
}

/// A point `[x, y]` of a reply.
pub(crate) fn xy(v: &Value) -> Option<glam::DVec2> {
    let a = v.as_array()?;
    Some(glam::DVec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

/// The same for a field of an object.
pub(crate) fn field_xy(v: &Value, key: &str) -> Option<glam::DVec2> {
    xy(v.get(key)?)
}

/// A fake backend for the tests of this module: it answers from a list of replies.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use kadakas_core::command::Command;
    use std::collections::VecDeque;

    pub struct Fake {
        pub doc: Document,
        pub selection: Vec<ObjectId>,
        pub revision: u64,
        pub replies: VecDeque<Result<Value, String>>,
        /// The answers of previews, and the methods that were previewed.
        pub previews: std::cell::RefCell<VecDeque<Result<Value, String>>>,
        pub previewed: std::cell::RefCell<Vec<(String, Value)>>,
        pub calls: Vec<(String, Value)>,
        pub commands: Vec<Command>,
    }

    impl Fake {
        pub fn new() -> Fake {
            Fake {
                doc: Document::default(),
                selection: Vec::new(),
                revision: 1,
                replies: VecDeque::new(),
                previews: Default::default(),
                previewed: Default::default(),
                calls: Vec::new(),
                commands: Vec::new(),
            }
        }
        pub fn reply(mut self, v: Value) -> Fake {
            self.replies.push_back(Ok(v));
            self
        }
        pub fn preview(self, v: Value) -> Fake {
            self.previews.borrow_mut().push_back(Ok(v));
            self
        }
    }

    impl Backend for Fake {
        fn document(&self) -> &Document {
            &self.doc
        }
        fn selection(&self) -> &[ObjectId] {
            &self.selection
        }
        fn revision(&self) -> u64 {
            self.revision
        }
        fn pattern_units(&self) -> usize {
            100_000
        }
        fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
            self.calls.push((method.to_string(), params));
            self.replies.pop_front().unwrap_or(Err("no reply".into()))
        }
        fn rehearse(&self, method: &str, params: Value) -> Result<(Value, Document), String> {
            self.previewed.borrow_mut().push((method.to_string(), params));
            let reply = self
                .previews
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err("no rehearsal".into()))?;
            Ok((reply, self.doc.clone()))
        }
        fn execute(&mut self, cmd: &Command) -> Result<(), String> {
            self.commands.push(cmd.clone());
            Ok(())
        }
    }
}

/// A lot with a stack bond of 200 x 100 mm units for the tests of this module.
#[cfg(test)]
pub(crate) mod lot {
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::curve::corners;
    use kadakas_core::hardscape::style::{JointKind, JointPattern};
    use kadakas_core::hardscape::{HardscapePatch, HardscapeStyle};
    use kadakas_core::model::ObjectId;
    use kadakas_core::resource::{ResourceBody, ResourceId};

    pub fn style(editor: &mut Editor) -> ResourceId {
        let joints = JointPattern {
            kind: JointKind::Stack,
            unit_length: 200.0,
            unit_width: 100.0,
            spacing: 5.0,
            ..Default::default()
        };
        ResourceId(
            editor
                .execute(&Command::AddResource {
                    name: "Pavers".into(),
                    body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                        joints: Some(joints),
                        ..Default::default()
                    })),
                })
                .unwrap()
                .created[0],
        )
    }

    /// A 4 m by 3 m hardscape with the style.
    pub fn new() -> (Editor, ObjectId) {
        let mut editor = Editor::default();
        let style = style(&mut editor);
        let id = ObjectId(
            editor
                .execute(&Command::AddHardscape {
                    boundary: Some(corners(&[
                        DVec2::new(0.0, 0.0),
                        DVec2::new(4000.0, 0.0),
                        DVec2::new(4000.0, 3000.0),
                        DVec2::new(0.0, 3000.0),
                    ])),
                    patch: Box::new(HardscapePatch {
                        style: Some(style),
                        ..Default::default()
                    }),
                    layer: None,
                    class: None,
                    attrs: Default::default(),
                })
                .unwrap()
                .created[0],
        );
        (editor, id)
    }
}
