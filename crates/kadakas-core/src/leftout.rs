//! What an export leaves out (MOD-05): guides (construction geometry), objects that the person
//! hid with Hide, and hidden faces and edges of surface models. An export never drops these
//! silently. [`LeftOut::of`] counts them and [`LeftOut::notes`] gives the lines for the report
//! ("3 guides left out: construction geometry"). [`without_hidden`] makes the document that the
//! export writes.
//!
//! Objects on a layer or in a class that is not visible are not counted here. The exports leave
//! them out by the rules of layers and classes, as before.

use serde::Serialize;

use crate::model::{Document, Shape};
use crate::surface::{Edit, FaceId};
use crate::visibility::is_hidden;

/// The numbers of items that an export leaves out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LeftOut {
    /// Guide lines and guide points.
    pub guides: usize,
    /// Objects hidden with Hide (groups are not counted, their members are).
    pub hidden_objects: usize,
    /// Hidden faces of surface models that are not hidden themselves.
    pub hidden_faces: usize,
    /// Hidden edges of those surface models.
    pub hidden_edges: usize,
}

impl LeftOut {
    /// Counts what an export would leave out of the document.
    pub fn of(doc: &Document) -> LeftOut {
        let mut n = LeftOut::default();
        for o in doc.objects.values() {
            match &o.shape {
                Shape::Guide(_) => n.guides += 1,
                Shape::Group { .. } if is_hidden(o) => {}
                _ if is_hidden(o) => {
                    if doc.is_visible_with_hidden(o) {
                        n.hidden_objects += 1;
                    }
                }
                Shape::SurfaceModel(m) if doc.is_visible(o) => {
                    n.hidden_faces += m.hidden_faces().len();
                    n.hidden_edges += m.hidden_edges().len();
                }
                _ => {}
            }
        }
        n
    }

    pub fn is_empty(&self) -> bool {
        *self == LeftOut::default()
    }

    /// The lines for the export report, in English like the other messages of the API.
    pub fn notes(&self) -> Vec<String> {
        let mut v = Vec::new();
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        if self.guides > 0 {
            v.push(format!(
                "{} left out: construction geometry",
                plural(self.guides, "guide", "guides")
            ));
        }
        if self.hidden_objects > 0 {
            v.push(format!(
                "{} left out",
                plural(self.hidden_objects, "hidden object", "hidden objects")
            ));
        }
        if self.hidden_faces > 0 || self.hidden_edges > 0 {
            v.push(format!(
                "{} and {} left out",
                plural(self.hidden_faces, "hidden face", "hidden faces"),
                plural(self.hidden_edges, "hidden edge", "hidden edges")
            ));
        }
        v
    }
}

/// A copy of the document without the guides, without the hidden objects and without the hidden
/// faces of surface models. The export writes this copy.
pub fn without_hidden(doc: &Document) -> Document {
    let mut out = doc.clone();
    out.objects
        .retain(|_, o| !matches!(o.shape, Shape::Guide(_)) && !is_hidden(o));
    for o in out.objects.values_mut() {
        if let Shape::SurfaceModel(m) = &mut o.shape {
            let faces: Vec<FaceId> = m.hidden_faces();
            if !faces.is_empty() {
                // A model that refuses the edit keeps its hidden faces (they stay in the export).
                let _ = m.apply(&Edit::EraseFaces { faces });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_counted_and_plain() {
        assert!(LeftOut::default().notes().is_empty());
        let n = LeftOut {
            guides: 1,
            hidden_objects: 2,
            hidden_faces: 1,
            hidden_edges: 0,
        };
        assert_eq!(
            n.notes(),
            vec![
                "1 guide left out: construction geometry",
                "2 hidden objects left out",
                "1 hidden face and 0 hidden edges left out"
            ]
        );
        assert!(!n.is_empty());
    }
}
