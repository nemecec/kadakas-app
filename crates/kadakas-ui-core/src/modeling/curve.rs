//! Circles and arcs in the modeler (MOD-15): what Entity Info shows for a curve with an exact
//! circle, and the change of its number of segments.
//!
//! The data is in the drawing (world) coordinates and in mm. The core keeps the exact circle
//! next to the chain of edges (`kadakas_core::surface::Analytic`).

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{CurveId, Edit};

use super::scenes::{frame_of, model_of};

/// Fewest and most segments of a circle or an arc that the field takes.
pub const SEGMENTS: std::ops::RangeInclusive<u32> = 3..=1000;

/// The exact circle of a curve, for Entity Info.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveShape {
    /// The model object that the curve is in.
    pub model: ObjectId,
    pub curve: u32,
    /// `circle`, `arc` or `polygon`.
    pub kind: String,
    /// The centre and the radius in the drawing.
    pub center: [f64; 3],
    pub radius: f64,
    /// The number of edges of the chain.
    pub segments: usize,
    /// For an arc, the start and the end angle in degrees about the normal, from the axis of the
    /// circle.
    pub arc_deg: Option<[f64; 2]>,
    /// The length of the arc, or the circumference of a circle.
    pub length: f64,
}

/// The shape of a curve of a model, if it has an exact circle.
pub fn curve_shape(doc: &Document, model: ObjectId, curve: CurveId) -> Option<CurveShape> {
    let m = model_of(doc, model)?;
    let a = m.curve_geometry(curve)?;
    let fr = frame_of(doc, model);
    // The models are placed rigidly or scaled the same in all directions (a circle that is
    // scaled differently along its axes loses its description in the core).
    let scale = fr.matrix3.determinant().abs().cbrt();
    let center = fr.transform_point3(a.center()).to_array();
    Some(CurveShape {
        model,
        curve: curve.0,
        kind: a.kind.clone(),
        center,
        radius: a.radius * scale,
        segments: m.curve_segments(curve)?,
        arc_deg: a
            .start
            .zip(a.end)
            .map(|(s, e)| [s.to_degrees(), e.to_degrees()]),
        length: a.length() * scale,
    })
}

/// Sets the number of segments of a curve (one undo step). Returns the notes of the edit in the
/// language of the window, or the reason of a refusal.
pub fn set_segments(
    editor: &mut Editor,
    model: ObjectId,
    curve: u32,
    segments: u32,
) -> Result<Vec<String>, String> {
    let cmd = Command::EditSurface {
        id: model,
        edit: Edit::SetCurveSegments {
            curve: CurveId(curve),
            segments,
        },
    };
    match editor.execute(&cmd) {
        Ok(out) => Ok(out
            .notes
            .iter()
            .map(|n| super::messages::localize(n))
            .collect()),
        Err(e) => Err(match &e {
            kadakas_core::command::CommandError::Surface(s) => super::messages::localize_error(s),
            other => super::messages::localize(&other.to_string()),
        }),
    }
}
