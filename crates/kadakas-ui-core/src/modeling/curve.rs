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
    /// The centre and the radius in the drawing. The radius and the length are true only when
    /// `uniform` is set.
    pub center: [f64; 3],
    pub radius: f64,
    /// False if the placement of the model bends the plane of the circle: its two axes do not
    /// keep one length and a right angle (a plan remap with a height scale bends a vertical
    /// circle and leaves a horizontal one). The circle is an ellipse in the drawing then, so the
    /// radius, the sweep and the length are not shown.
    pub uniform: bool,
    /// False for a kind of circle that a newer program wrote. Only the kind is shown then.
    pub known: bool,
    /// The number of edges of the chain.
    pub segments: usize,
    /// For an arc, the angle it sweeps, in degrees.
    pub sweep_deg: Option<f64>,
    /// The circumference of a circle, else the length of the chain (an arc, the true perimeter of
    /// a polygon).
    pub length: f64,
}

/// The shape of a curve of a model, if it has an exact circle.
pub fn curve_shape(doc: &Document, model: ObjectId, curve: CurveId) -> Option<CurveShape> {
    let m = model_of(doc, model)?;
    let a = m.curve_geometry(curve)?;
    let fr = frame_of(doc, model);
    // The placement is not an edit of the model, so the core does not see it. It keeps the circle
    // true when it maps the two axes of the circle's own plane to vectors of one length at a
    // right angle (any scale of the axis that is not in the plane does not matter).
    let axis = a.axis();
    let side = a.normal().normalize_or_zero().cross(axis);
    let (ma, ms) = (fr.matrix3 * axis, fr.matrix3 * side);
    let scale = ma.length();
    let close = |p: f64, q: f64| (p - q).abs() <= 1e-9 * scale.max(1e-12);
    let uniform = close(ma.length(), ms.length()) && close(ma.dot(ms) / scale.max(1e-12), 0.0);
    let center = fr.transform_point3(a.center()).to_array();
    Some(CurveShape {
        model,
        curve: curve.0,
        kind: a.kind.clone(),
        center,
        radius: a.radius * scale,
        uniform,
        known: kadakas_core::surface::analytic::is_known_kind(&a.kind),
        segments: m.curve_segments(curve)?,
        sweep_deg: a.sweep().map(f64::to_degrees),
        length: m.curve_length(curve)? * scale,
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
