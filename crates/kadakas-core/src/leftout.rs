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
use crate::surface::{EdgeId, Edit, FaceId, SurfaceError, SurfaceModel};
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
    /// Hidden faces that could not be taken out of the copy and stay in the export. Only
    /// [`prepare`] fills this.
    pub hidden_faces_kept: usize,
    /// Hidden edges that border a face that is shown. They stay in the geometry. The exports do not
    /// draw them as lines. Only [`prepare`] fills this.
    pub hidden_edges_kept: usize,
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
        if self.hidden_faces_kept > 0 {
            v.push(format!(
                "Hidden faces that could not be taken out stay in the export: {}",
                self.hidden_faces_kept
            ));
        }
        if self.hidden_edges_kept > 0 {
            v.push(format!(
                "Hidden edges that border shown faces stay in the geometry and are not drawn as lines: {}",
                self.hidden_edges_kept
            ));
        }
        v
    }
}

/// The document that an export writes, and what it really left out.
#[derive(Clone, Debug)]
pub struct Prepared {
    /// A copy without the guides, the hidden objects and the hidden faces (and the hidden edges
    /// that no face needs).
    pub doc: Document,
    /// What was really taken out, and what stayed although it is hidden.
    pub left: LeftOut,
}

/// Makes the document for an export. Cloning the whole document costs time and memory in
/// proportion to its size, so a caller does this only when [`LeftOut::of`] is not empty.
pub fn prepare(doc: &Document) -> Prepared {
    prepare_with(doc, &|m, faces| m.apply(&Edit::EraseFaces { faces }))
}

type EraseFaces<'a> = &'a dyn Fn(&mut SurfaceModel, Vec<FaceId>) -> Result<(), SurfaceError>;

/// Like [`prepare`], with the way to erase faces given (a test makes it fail).
pub fn prepare_with(doc: &Document, erase: EraseFaces<'_>) -> Prepared {
    let mut left = LeftOut::of(doc);
    left.hidden_faces = 0;
    left.hidden_edges = 0;
    let mut out = doc.clone();
    out.objects
        .retain(|_, o| !matches!(o.shape, Shape::Guide(_)) && !is_hidden(o));
    for o in out.objects.values_mut() {
        let Shape::SurfaceModel(m) = &mut o.shape else {
            continue;
        };
        let faces: Vec<FaceId> = m.hidden_faces();
        if !faces.is_empty() {
            let n = faces.len();
            match erase(m, faces) {
                Ok(()) => left.hidden_faces += n,
                Err(_) => left.hidden_faces_kept += n,
            }
        }
        // A hidden edge that no face uses is taken out too. One that borders a face stays.
        let hidden: Vec<EdgeId> = m.hidden_edges();
        let (loose, border): (Vec<EdgeId>, Vec<EdgeId>) = hidden
            .into_iter()
            .partition(|e| m.faces_of_edge(*e).is_empty());
        if !loose.is_empty() {
            let n = loose.len();
            match m.apply(&Edit::EraseEdges { edges: loose }) {
                Ok(()) => left.hidden_edges += n,
                Err(_) => left.hidden_edges_kept += n,
            }
        }
        left.hidden_edges_kept += border.len();
    }
    Prepared { doc: out, left }
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
            hidden_faces_kept: 3,
            hidden_edges_kept: 1,
        };
        assert_eq!(
            n.notes(),
            vec![
                "1 guide left out: construction geometry",
                "2 hidden objects left out",
                "1 hidden face and 0 hidden edges left out",
                "Hidden faces that could not be taken out stay in the export: 3",
                "Hidden edges that border shown faces stay in the geometry and are not drawn as lines: 1"
            ]
        );
        assert!(!n.is_empty());
    }
}
