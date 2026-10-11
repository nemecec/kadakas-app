//! Model > Extrude and Model > Sweep (3D-03): turns 2D shapes into 3D solids in one batch.
//!
//! The batch is one `Command::Batch`, so it is one undo step. The solid keeps the profile and the
//! parameters, so it stays editable. A source shape is removed by default (as in Vectorworks, the
//! 2D shape becomes the solid); `delete_source: false` keeps it.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::curve::{self, Vertex, VertexKind};
use crate::landscape;
use crate::model::{Document, Object, ObjectId, Shape};
use crate::solid3d::{self, Extrude, Sweep};

/// What a batch does.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    /// The batch to run. `None` if no shape could be converted.
    pub command: Option<Command>,
    /// (source object, new solid).
    pub created: Vec<(ObjectId, ObjectId)>,
    /// (source object, reason).
    pub skipped: Vec<(ObjectId, String)>,
}

impl Made {
    fn new() -> Made {
        Made {
            command: None,
            created: Vec::new(),
            skipped: Vec::new(),
        }
    }
}

/// A profile: the outline and the holes.
pub type Profile = (Vec<Vertex>, Vec<Vec<Vertex>>);

fn circle_vertices(center: DVec2, radius: f64) -> Vec<Vertex> {
    [DVec2::X, DVec2::Y, -DVec2::X, -DVec2::Y]
        .iter()
        .map(|d| Vertex::new(center + *d * radius, VertexKind::PointOnArc))
        .collect()
}

/// The closed profile of a shape that can be extruded, or why it cannot. A landscape area brings
/// its holes.
pub fn closed_profile_of(o: &Object) -> Result<Profile, String> {
    let (outline, holes) = match &o.shape {
        Shape::Polyline {
            points,
            closed: true,
        } => (points.clone(), Vec::new()),
        Shape::Polyline { .. } => return Err("the polyline is not closed".into()),
        Shape::LandscapeArea { boundary, .. } => (
            boundary.clone(),
            landscape::data(o).map(|d| d.holes).unwrap_or_default(),
        ),
        Shape::Hardscape { boundary, .. } => (boundary.clone(), Vec::new()),
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            (
                curve::corners(&[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]),
                Vec::new(),
            )
        }
        Shape::Circle { center, radius } if *radius > 0.0 => {
            (circle_vertices(*center, *radius), Vec::new())
        }
        Shape::Polyline3d {
            points,
            closed: true,
        } => (
            curve::corners(&points.iter().map(|p| p.truncate()).collect::<Vec<_>>()),
            Vec::new(),
        ),
        Shape::Extrude(_) | Shape::Sweep(_) => return Err("already a 3D solid".into()),
        other => return Err(format!("a {} is not a closed shape", other.kind())),
    };
    if outline.len() < 3 {
        return Err("the outline needs at least 3 vertices".into());
    }
    Ok((outline, holes))
}

fn default_height() -> f64 {
    1000.0
}
fn default_true() -> bool {
    true
}
fn default_sweep() -> f64 {
    solid3d::DEFAULT_SWEEP_ANGLE
}
fn default_segment() -> f64 {
    solid3d::DEFAULT_SEGMENT_ANGLE
}

/// Options of Model > Extrude.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtrudeOptions {
    /// Height in mm. A negative height extrudes downward.
    pub height: f64,
    /// Height of the profile above the layer, in mm.
    pub base: f64,
    /// Remove the source shapes (default). Without it the shapes stay.
    pub delete_source: bool,
}

impl Default for ExtrudeOptions {
    fn default() -> Self {
        ExtrudeOptions {
            height: default_height(),
            base: 0.0,
            delete_source: default_true(),
        }
    }
}

/// Options of Model > Sweep.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SweepOptions {
    /// Sweep angle in degrees. 360 is a full turn.
    pub sweep_angle_deg: f64,
    /// Angle of one segment in degrees.
    pub segment_angle_deg: f64,
    /// Rise along the axis for each full turn, in mm.
    pub pitch: f64,
    /// A line object that is the axis (a line, or a polyline with two vertices). It stays in the
    /// drawing. The profile turns around the line in the plan plane; the axis is horizontal.
    pub axis_id: Option<ObjectId>,
    /// Without an axis line: a vertical axis at this point. The profile is then read like an
    /// elevation: its x is the distance from the axis, its y the height above the point.
    /// Default: the left edge and the bottom edge of the profile.
    pub axis_at: Option<DVec2>,
    /// Remove the source shapes (default). Without it the shapes stay.
    pub delete_source: bool,
}

impl Default for SweepOptions {
    fn default() -> Self {
        SweepOptions {
            sweep_angle_deg: default_sweep(),
            segment_angle_deg: default_segment(),
            pitch: 0.0,
            axis_id: None,
            axis_at: None,
            delete_source: default_true(),
        }
    }
}

impl ExtrudeOptions {
    pub fn check(&self) -> Result<(), String> {
        if !(self.height.is_finite() && self.base.is_finite()) || self.height == 0.0 {
            return Err("height must be a number that is not zero, and base a number".into());
        }
        Ok(())
    }
}

impl SweepOptions {
    pub fn check(&self) -> Result<(), String> {
        if !(self.pitch.is_finite()
            && self.axis_at.is_none_or(|a| a.is_finite())
            && self.sweep_angle_deg.is_finite()
            && self.segment_angle_deg.is_finite())
        {
            return Err("the numbers must be finite".into());
        }
        if !(self.sweep_angle_deg > 0.0 && self.sweep_angle_deg <= solid3d::MAX_SWEEP_ANGLE) {
            return Err(format!(
                "sweep_angle must be between 0 and {} degrees",
                solid3d::MAX_SWEEP_ANGLE
            ));
        }
        if !(self.segment_angle_deg >= solid3d::MIN_SEGMENT_ANGLE
            && self.segment_angle_deg <= 180.0)
        {
            return Err(format!(
                "segment_angle must be between {} and 180 degrees",
                solid3d::MIN_SEGMENT_ANGLE
            ));
        }
        if self.axis_id.is_some() && self.axis_at.is_some() {
            return Err("give axis_id or axis_at, not both".into());
        }
        Ok(())
    }
}

/// The free data of an object: not the keys that styles and records own (the commands refuse to
/// set those).
fn free_data(doc: &Document, o: &Object) -> std::collections::BTreeMap<String, serde_json::Value> {
    use crate::resource::ResourceBody;
    o.data
        .iter()
        .filter(|(k, _)| {
            !(crate::plants::link::RESERVED_DATA_KEYS.contains(&k.as_str())
                || crate::symbols::RESERVED_KEYS.contains(&k.as_str())
                || k.as_str() == crate::worksheet_table::KEY
                || doc.resources.iter().any(|r| {
                    matches!(r.body, ResourceBody::RecordFormat(_))
                        && r.name.eq_ignore_ascii_case(k.trim())
                }))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The add command of a new solid that takes the place of `o`: same layer, class, attributes and
/// group. The free data of the source goes along.
fn place(doc: &Document, o: &Object, shape: Shape, new_id: ObjectId, commands: &mut Vec<Command>) {
    commands.push(match o.parent {
        Some(parent) => Command::AddObjectIn {
            parent,
            shape,
            class: Some(o.class),
            attrs: o.attrs.clone(),
            id: None,
        },
        None => Command::AddObject {
            shape,
            layer: Some(o.layer),
            class: Some(o.class),
            attrs: o.attrs.clone(),
        },
    });
    let data = free_data(doc, o);
    if !data.is_empty() {
        commands.push(Command::SetData {
            id: new_id,
            set: data,
            remove: Vec::new(),
        });
    }
}

fn finish(made: &mut Made, mut commands: Vec<Command>, delete_source: bool) {
    if delete_source && !made.created.is_empty() {
        commands.push(Command::DeleteObjects {
            ids: made.created.iter().map(|c| c.0).collect(),
        });
    }
    if !commands.is_empty() {
        made.command = Some(Command::Batch { commands });
    }
}

/// Plans Model > Extrude for the given shapes. Objects that cannot be extruded are skipped with a reason.
pub fn extrude(doc: &Document, ids: &[ObjectId], opts: &ExtrudeOptions) -> Made {
    let mut made = Made::new();
    let mut commands = Vec::new();
    let mut next = doc.id_plan();
    let mut seen = std::collections::HashSet::new();
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(o) = doc.object(id) else {
            made.skipped.push((id, "no such object".into()));
            continue;
        };
        let (profile, holes) = match closed_profile_of(o) {
            Ok(p) => p,
            Err(why) => {
                made.skipped.push((id, why));
                continue;
            }
        };
        let mut e = Extrude::new(profile, opts.height);
        e.holes = holes;
        e.base = opts.base;
        if let Some(why) = e.problem() {
            made.skipped.push((id, why));
            continue;
        }
        let new_id = ObjectId(next.take());
        place(doc, o, Shape::Extrude(Box::new(e)), new_id, &mut commands);
        made.created.push((id, new_id));
    }
    finish(&mut made, commands, opts.delete_source);
    made
}

/// The profile of a sweep in the plane of the axis: (distance from the axis, position along it).
struct Frame {
    origin: DVec2,
    /// Unit direction of the axis in the plan, or `None` for a vertical axis.
    along: Option<DVec2>,
}

fn axis_line(doc: &Document, id: ObjectId) -> Result<(DVec2, DVec2), String> {
    let o = doc.object(id).ok_or("the axis object does not exist")?;
    let (a, b) = match &o.shape {
        Shape::Line { a, b } => (*a, *b),
        Shape::Polyline { points, .. } if points.len() == 2 => (points[0].p, points[1].p),
        _ => return Err("the axis must be a line".into()),
    };
    if a.distance(b) < 1e-6 {
        return Err("the axis line has no length".into());
    }
    Ok((a, b))
}

/// Plans Model > Sweep for the given shapes. A closed profile gives a solid, an open polyline or a
/// line gives a surface of revolution.
pub fn sweep(doc: &Document, ids: &[ObjectId], opts: &SweepOptions) -> Made {
    let mut made = Made::new();
    let mut commands = Vec::new();
    let mut next = doc.id_plan();
    let mut seen = std::collections::HashSet::new();
    let axis = match opts.axis_id.map(|a| axis_line(doc, a)).transpose() {
        Ok(a) => a,
        Err(why) => {
            for &id in ids {
                made.skipped.push((id, why.clone()));
            }
            return made;
        }
    };
    for &id in ids {
        if !seen.insert(id) || Some(id) == opts.axis_id {
            continue;
        }
        let Some(o) = doc.object(id) else {
            made.skipped.push((id, "no such object".into()));
            continue;
        };
        // A closed shape, or an open polyline or line for a surface.
        let (profile, closed) = match closed_profile_of(o) {
            Ok((p, _)) => (p, true),
            Err(why) => match &o.shape {
                Shape::Line { a, b } => (curve::corners(&[*a, *b]), false),
                Shape::Polyline {
                    points,
                    closed: false,
                } if points.len() >= 2 => (points.clone(), false),
                _ => {
                    made.skipped.push((id, why));
                    continue;
                }
            },
        };
        let positions = curve::positions(&profile);
        let frame = match axis {
            Some((a, b)) => Frame {
                origin: a,
                along: Some((b - a).normalize()),
            },
            None => {
                let (lo, _) = curve::bounds(&profile, closed);
                Frame {
                    origin: opts.axis_at.unwrap_or(lo),
                    along: None,
                }
            }
        };
        // Profile points as (u, v): the distance from the axis and the position along it.
        let (uv, rotation) = match frame.along {
            None => (
                profile
                    .iter()
                    .map(|v| Vertex::new(v.p - frame.origin, v.kind))
                    .collect::<Vec<_>>(),
                0.0,
            ),
            Some(w) => {
                let side = |p: DVec2| (p - frame.origin).perp_dot(w);
                let neg = positions.iter().any(|p| side(*p) < -1e-6);
                let pos = positions.iter().any(|p| side(*p) > 1e-6);
                if neg && pos {
                    made.skipped
                        .push((id, "the profile crosses the axis".into()));
                    continue;
                }
                let sign = if neg { -1.0 } else { 1.0 };
                let mapped = profile
                    .iter()
                    .map(|v| {
                        let d = v.p - frame.origin;
                        Vertex::new(DVec2::new(sign * d.perp_dot(w), d.dot(w)), v.kind)
                    })
                    .collect::<Vec<_>>();
                (mapped, 0.0)
            }
        };
        let mut s = Sweep::new(uv);
        s.closed = closed;
        s.sweep_angle_deg = opts.sweep_angle_deg;
        s.segment_angle_deg = opts.segment_angle_deg;
        s.pitch = opts.pitch;
        s.origin = frame.origin.extend(0.0);
        if let Some(w) = frame.along {
            s.axis = w.extend(0.0);
            // The profile starts where the shape is: in the plan, on the side of the axis that
            // the drawn profile lies on.
            let (_, e1, _) = s.frame();
            let side = positions
                .iter()
                .map(|p| (*p - frame.origin).dot(e1.truncate()))
                .fold(0.0_f64, |m, x| if x.abs() > m.abs() { x } else { m });
            s.rotation_deg = if side < 0.0 { 180.0 } else { rotation };
        }
        if let Some(why) = s.problem() {
            made.skipped.push((id, why));
            continue;
        }
        let new_id = ObjectId(next.take());
        place(doc, o, Shape::Sweep(Box::new(s)), new_id, &mut commands);
        made.created.push((id, new_id));
    }
    finish(&mut made, commands, opts.delete_source);
    made
}

/// The 3D box of the solids made by a batch, for the camera: the union of their boxes.
pub fn union_bounds(doc: &Document, ids: &[ObjectId]) -> Option<(DVec3, DVec3)> {
    ids.iter()
        .filter_map(|id| doc.object(*id)?.shape.solid_bounds3())
        .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
}

#[cfg(test)]
mod tests;
