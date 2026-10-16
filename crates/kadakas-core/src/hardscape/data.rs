//! The settings of a hardscape object, kept in `Object::data["hardscape"]`.
//!
//! - A hardscape without this key is a plain slab: it keeps its own material, thickness and
//!   elevation in its shape. Files from before hardscape styles load as these.
//! - A hardscape with the key can have a style, a path outline, a slope and 3D options. The
//!   shape still holds the outline (`boundary`), and a copy of the material and the total
//!   thickness, so bounds, picking and drawing need no access to the style. Commands keep
//!   the copy up to date.
//! - For a path, the center line is the source and `boundary` is derived from it.

use crate::uid::AsUid;
use glam::DVec2;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::offset::{self, Align, Join};
use crate::curve::{self, Vertex};
use crate::model::{Document, MAX_COORD, Object, Shape};
use crate::resource::ResourceId;

/// The key in `Object::data` that holds the settings.
pub const KEY: &str = "hardscape";

/// Largest cross slope in percent.
pub const MAX_SLOPE_PERCENT: f64 = 1000.0;
/// Flattening tolerance of curves, in mm.
pub const TOLERANCE: f64 = 1.0;
/// Most vertices of a boundary, a center line or a hole that an object may have.
pub const MAX_VERTICES: usize = 50_000;

/// A closed area, or a line with a width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Slab,
    Path,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Slab => "slab",
            Mode::Path => "path",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        [Mode::Slab, Mode::Path]
            .into_iter()
            .find(|m| m.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// How the top surface is inclined. All slopes make a plane.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Slope {
    /// Flat at the elevation.
    #[default]
    None,
    /// Heights above the layer. A slab gives one height per boundary corner, and a plane through
    /// them is fitted. A path gives two: at the start and at the end of the center line.
    Edges { elevations: Vec<f64> },
    /// A fall of `percent` toward `angle_deg` (counterclockwise from +X). The elevation applies in
    /// the middle. A path falls to the right of its direction from start to end, and ignores the angle.
    Cross {
        percent: f64,
        #[serde(default)]
        angle_deg: f64,
    },
    /// A fall by an angle in degrees (0 to 45), toward `angle_deg` like `Cross`.
    Angle {
        degrees: f64,
        #[serde(default)]
        angle_deg: f64,
    },
    /// A fall of `rise` over `run` (for example 1 over 20), toward `angle_deg` like `Cross`.
    RiseRun {
        rise: f64,
        run: f64,
        #[serde(default)]
        angle_deg: f64,
    },
}

impl Slope {
    pub fn id(&self) -> &'static str {
        match self {
            Slope::None => "none",
            Slope::Edges { .. } => "edges",
            Slope::Cross { .. } => "cross",
            Slope::Angle { .. } => "angle",
            Slope::RiseRun { .. } => "rise_run",
        }
    }

    /// The fall in percent and the direction of the fall in degrees, for the slopes that fall
    /// toward a direction. `None` for no slope and for edge elevations.
    pub fn fall(&self) -> Option<(f64, f64)> {
        match self {
            Slope::Cross { percent, angle_deg } => Some((*percent, *angle_deg)),
            Slope::Angle { degrees, angle_deg } => {
                Some((100.0 * degrees.to_radians().tan(), *angle_deg))
            }
            Slope::RiseRun {
                rise,
                run,
                angle_deg,
            } => Some((100.0 * rise / run, *angle_deg)),
            Slope::None | Slope::Edges { .. } => None,
        }
    }
}

fn default_width() -> f64 {
    1200.0
}

/// The settings of a hardscape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HardscapeData {
    pub style: Option<ResourceId>,
    pub mode: Mode,
    /// The center line of a path.
    pub center: Vec<Vertex>,
    /// The width of a path.
    pub width: f64,
    pub join: Join,
    /// A closed path has no ends. Its center line is a ring.
    pub closed: bool,
    /// Which part of the path the center line is.
    pub align: Align,
    /// Moves the path sideways from the line, to the left of the direction of the line (mm).
    pub offset: f64,
    pub slope: Slope,
    /// Follow the site model on the layer. The elevation is then the height above the ground.
    pub drape: bool,
    /// Act as a pad on the site model.
    pub site_modifier: bool,
    /// Parts of the surface that stay open, for example a planting island or a manhole.
    pub holes: Vec<Vec<Vertex>>,
    /// Where the joint pattern starts, in drawing coordinates. `None`: the middle of the box of
    /// the outline, or what the style says.
    pub origin: Option<DVec2>,
    /// Angle of the joint pattern in degrees. It replaces the angle of the style.
    pub angle: Option<f64>,
}

impl Default for HardscapeData {
    fn default() -> Self {
        HardscapeData {
            style: None,
            mode: Mode::Slab,
            center: Vec::new(),
            width: default_width(),
            join: Join::Miter,
            closed: false,
            align: Align::Center,
            offset: 0.0,
            slope: Slope::None,
            drape: false,
            site_modifier: false,
            holes: Vec::new(),
            origin: None,
            angle: None,
        }
    }
}

/// Most holes in one hardscape.
pub const MAX_HOLES: usize = 1000;

impl HardscapeData {
    /// Checks the numbers and the rules between the fields.
    pub fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite() || self.width <= 0.0 || self.width > MAX_COORD {
            return Err("the width must be a positive, finite number".into());
        }
        if self.center.iter().any(|v| !v.is_finite()) {
            return Err("numbers must be finite".into());
        }
        if self.mode == Mode::Path && self.center.len() < if self.closed { 3 } else { 2 } {
            return Err(format!(
                "a path needs a center line of {} or more points",
                if self.closed { 3 } else { 2 }
            ));
        }
        if !self.offset.is_finite() || self.offset.abs() > MAX_COORD {
            return Err("the offset must be a finite number".into());
        }
        if self.holes.len() > MAX_HOLES {
            return Err(format!("a hardscape has at most {MAX_HOLES} holes"));
        }
        for (i, h) in self.holes.iter().enumerate() {
            if h.len() < 3 {
                return Err(format!("hole {} needs 3 or more points", i + 1));
            }
            if h.len() > MAX_VERTICES {
                return Err(format!("a hole has at most {MAX_VERTICES} points"));
            }
            if h.iter()
                .any(|v| !v.is_finite() || v.p.abs().max_element() > MAX_COORD)
            {
                return Err("numbers must be finite".into());
            }
        }
        if self
            .origin
            .is_some_and(|o| !o.is_finite() || o.abs().max_element() > MAX_COORD)
        {
            return Err("the origin must be a finite point".into());
        }
        if self.angle.is_some_and(|a| !a.is_finite() || a.abs() > 1e6) {
            return Err("the angle must be a finite number".into());
        }
        if self.center.len() > MAX_VERTICES {
            return Err(format!("a path has at most {MAX_VERTICES} points"));
        }
        match &self.slope {
            Slope::None => {}
            Slope::Edges { elevations } => {
                if elevations
                    .iter()
                    .any(|e| !e.is_finite() || e.abs() > MAX_COORD)
                {
                    return Err("edge elevations must be finite numbers".into());
                }
                let need = if self.mode == Mode::Path { 2 } else { 3 };
                if elevations.len() < need {
                    return Err(format!(
                        "a slope by edge elevations needs {need} or more elevations"
                    ));
                }
            }
            Slope::Cross { .. } | Slope::Angle { .. } | Slope::RiseRun { .. } => {
                if let Slope::Angle { degrees, .. } = &self.slope
                    && (degrees.is_nan() || degrees.abs() > 45.0 + 1e-9)
                {
                    return Err("the slope angle must be between -45 and 45 degrees".into());
                }
                if let Slope::RiseRun { run, .. } = &self.slope
                    && !(run.is_finite() && *run > 0.0)
                {
                    return Err("the run of a slope must be a positive number".into());
                }
                let (percent, angle) = self.slope.fall().expect("falls");
                if !percent.is_finite() || percent.abs() > MAX_SLOPE_PERCENT || !angle.is_finite() {
                    return Err(format!(
                        "the slope must be a finite number, at most {MAX_SLOPE_PERCENT} percent"
                    ));
                }
            }
        }
        Ok(())
    }

    /// The center line as points.
    pub fn center_points(&self) -> Vec<DVec2> {
        let mut pts = offset::dedupe(&curve::flatten(&self.center, self.closed, TOLERANCE));
        if self.closed && pts.len() > 1 {
            // A closed ring: repeat the first point so that the last leg is part of the line.
            pts.push(pts[0]);
        }
        pts
    }

    /// The line the path is built around: the center line moved by the alignment and the offset.
    pub fn axis(&self) -> Vec<DVec2> {
        let shift = self.offset
            + match self.align {
                Align::Center => 0.0,
                // The line is the left edge: the path lies to its right.
                Align::Left => -self.width / 2.0,
                Align::Right => self.width / 2.0,
            };
        offset::shifted(&self.center_points(), shift)
    }

    /// The outline of a path, as corner vertices. Empty for a slab or a path without length.
    pub fn outline(&self) -> Vec<Vertex> {
        if self.mode != Mode::Path {
            return Vec::new();
        }
        curve::corners(&offset::path_outline(
            &self.axis(),
            self.width,
            self.join,
            self.closed,
        ))
    }
}

/// The settings of a hardscape object. `None` for other objects, for a hardscape without
/// settings, and for settings that cannot be read.
pub fn data(obj: &Object) -> Option<HardscapeData> {
    if !matches!(obj.shape, Shape::Hardscape { .. }) {
        return None;
    }
    serde_json::from_value(obj.data.get(KEY)?.clone()).ok()
}

/// Writes the settings of a hardscape object.
pub fn set_data(obj: &mut Object, d: &HardscapeData) {
    obj.data.insert(
        KEY.into(),
        serde_json::to_value(d).expect("settings are JSON"),
    );
}

/// True if the object has settings that cannot be read. Such objects are left alone.
pub fn has_unreadable_data(obj: &Object) -> bool {
    obj.data.get(KEY).is_some_and(|v| !matches!(v, Value::Null))
        && data(obj).is_none()
        && matches!(obj.shape, Shape::Hardscape { .. })
}

/// The hardscape style an object refers to, if the style exists.
pub fn style_of(
    doc: &Document,
    obj: &Object,
) -> Option<(ResourceId, super::style::HardscapeStyle)> {
    let id = data(obj)?.style?;
    Some((id, doc.resources.hardscape_style(id)?.clone()))
}

/// The ID of the hardscape style that an object refers to, without checking that it exists.
pub fn style_id(obj: &Object) -> Option<ResourceId> {
    obj.data.get(KEY)?.get("style")?.as_uid().map(ResourceId)
}

/// Copies the material and the total thickness of the style into the shape.
pub fn sync_shape(doc: &Document, obj: &mut Object) {
    let Some((id, style)) = style_of(doc, obj) else {
        return;
    };
    let name = doc
        .resources
        .get(id)
        .map(|r| r.name.clone())
        .unwrap_or_default();
    if let Shape::Hardscape {
        material,
        thickness,
        ..
    } = &mut obj.shape
    {
        *material = style.label(&name);
        *thickness = Some(style.total_thickness());
    }
}

/// A change of hardscape settings. Missing fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HardscapePatch {
    pub style: Option<ResourceId>,
    /// Remove the style. The hardscape keeps the material and thickness of the style.
    pub unstyle: bool,
    pub mode: Option<Mode>,
    pub center: Option<Vec<Vertex>>,
    pub width: Option<f64>,
    pub join: Option<Join>,
    pub closed: Option<bool>,
    pub align: Option<Align>,
    pub offset: Option<f64>,
    pub slope: Option<Slope>,
    pub drape: Option<bool>,
    pub site_modifier: Option<bool>,
    /// New holes. They replace the old ones. Each hole must lie inside the outline.
    pub holes: Option<Vec<Vec<Vertex>>>,
    /// Where the joint pattern starts.
    pub origin: Option<DVec2>,
    /// Remove the origin of the object.
    pub clear_origin: bool,
    /// Angle of the joint pattern, in degrees.
    pub angle: Option<f64>,
    /// Remove the angle of the object: the style gives it again.
    pub clear_angle: bool,
    /// Start the pattern at vertex `n` of the outline and run it along the edge to vertex `n + 1`
    /// (sets the origin and the angle).
    pub align_to_edge: Option<usize>,
    /// Material of a hardscape without style.
    pub material: Option<String>,
    /// Thickness of a hardscape without style.
    pub thickness: Option<f64>,
    /// Height of the top surface above the layer (or above the ground, when draped).
    pub elevation: Option<f64>,
}

impl HardscapePatch {
    /// True if the patch changes nothing.
    pub fn is_empty(&self) -> bool {
        *self == HardscapePatch::default()
    }
}

/// Applies a patch to a hardscape object: updates the settings, the outline of a path and the
/// copies in the shape. The style must exist (the caller checks). Nothing changes on error.
pub fn apply_patch(doc: &Document, obj: &mut Object, patch: &HardscapePatch) -> Result<(), String> {
    if !matches!(obj.shape, Shape::Hardscape { .. }) {
        return Err(format!("object {} is not a hardscape", obj.id));
    }
    if has_unreadable_data(obj) {
        return Err(format!(
            "object {} has hardscape data that cannot be read",
            obj.id
        ));
    }
    if patch.unstyle && patch.style.is_some() {
        return Err("give a style or unstyle, not both".into());
    }
    let mut d = data(obj).unwrap_or_default();
    let mut shape = obj.shape.clone();
    if let Some(m) = patch.mode {
        d.mode = m;
    }
    if let Some(c) = &patch.center {
        d.center = c.clone();
        if patch.mode.is_none() {
            d.mode = Mode::Path;
        }
    }
    if let Some(w) = patch.width {
        d.width = w;
    }
    if let Some(j) = patch.join {
        d.join = j;
    }
    if let Some(v) = patch.closed {
        d.closed = v;
    }
    if let Some(v) = patch.align {
        d.align = v;
    }
    if let Some(v) = patch.offset {
        d.offset = v;
    }
    if let Some(s) = &patch.slope {
        d.slope = s.clone();
    }
    if let Some(v) = patch.drape {
        d.drape = v;
    }
    if let Some(v) = patch.site_modifier {
        d.site_modifier = v;
    }
    if let Some(h) = &patch.holes {
        d.holes = h.clone();
    }
    if patch.clear_origin {
        d.origin = None;
    }
    if patch.clear_angle {
        d.angle = None;
    }
    if let Some(i) = patch.align_to_edge {
        let Shape::Hardscape { boundary, .. } = &obj.shape else {
            unreachable!("checked above")
        };
        // The edge of the outline after this patch: a new center line moves the outline.
        let outline = if d.mode == Mode::Path {
            d.outline()
        } else {
            boundary.clone()
        };
        // Inside a kerb border the pattern starts on the inner border line.
        let style = if patch.unstyle {
            None
        } else {
            patch.style.or(d.style)
        };
        let border = style
            .and_then(|id| doc.resources.hardscape_style(id))
            .and_then(|st| st.border.as_ref())
            .map(|b| (b.width, b.sides));
        let p = align_start(&outline, &d, border, i)?;
        d.origin = p.origin;
        d.angle = Some(p.angle);
    }
    if let Some(o) = patch.origin {
        d.origin = Some(o);
    }
    if let Some(a) = patch.angle {
        d.angle = Some(a);
    }
    if patch.unstyle {
        // Keep what the style gave.
        d.style = None;
    }
    if let Some(s) = patch.style {
        d.style = Some(s);
    }
    // The outline changed after the slope was set (or the file is old): keep the surface.
    if let (Slope::Edges { elevations }, Shape::Hardscape { boundary, .. }) = (&d.slope, &shape)
        && d.mode == Mode::Slab
        && patch.slope.is_none()
        && elevations.len() != boundary.len()
    {
        let plane = super::surface::surface_of(obj, data(obj).as_ref(), None).plane;
        d.slope = Slope::Edges {
            elevations: curve::positions(boundary)
                .iter()
                .map(|p| plane.z(*p))
                .collect(),
        };
    }
    d.validate()?;
    let Shape::Hardscape {
        boundary,
        material,
        thickness,
        elevation,
    } = &mut shape
    else {
        unreachable!("checked above")
    };
    if d.mode == Mode::Path {
        let outline = d.outline();
        if outline.len() < 3 {
            return Err(
                "the path has no length. Give a center line with two different points".into(),
            );
        }
        *boundary = outline;
    } else if patch.mode == Some(Mode::Slab) {
        d.center.clear();
    }
    if let Some(m) = &patch.material {
        *material = m.clone();
    }
    if let Some(t) = patch.thickness {
        if !t.is_finite() || t <= 0.0 || t > MAX_COORD {
            return Err("thickness must be a positive, finite number".into());
        }
        *thickness = Some(t);
    }
    if let Some(e) = patch.elevation {
        if !e.is_finite() || e.abs() > MAX_COORD {
            return Err("elevation must be a finite number".into());
        }
        *elevation = Some(e);
    }
    if let Slope::Edges { elevations } = &d.slope
        && d.mode == Mode::Slab
        && elevations.len() != boundary.len()
    {
        return Err(format!(
            "give one elevation per boundary corner ({} corners)",
            boundary.len()
        ));
    }
    if !d.holes.is_empty() {
        let Shape::Hardscape { boundary, .. } = &shape else {
            unreachable!("checked above")
        };
        let ring = curve::flatten(boundary, true, TOLERANCE);
        for (i, h) in d.holes.iter().enumerate() {
            if !crate::layout::ring_within(&curve::flatten(h, true, TOLERANCE), &ring) {
                return Err(format!("hole {} is not inside the outline", i + 1));
            }
        }
    }
    obj.shape = shape;
    set_data(obj, &d);
    sync_shape(doc, obj);
    Ok(())
}

/// After the outline of a slab was edited: a slope by edge elevations has one height per corner.
/// When the number of corners changed, the heights are read again from the surface that the
/// object had before, so the slope stays as it was. `before` is the object before the edit.
pub fn fix_edge_elevations(before: &Object, after: &mut Object) {
    let Some(mut d) = data(after) else { return };
    let Slope::Edges { elevations } = &d.slope else {
        return;
    };
    let Shape::Hardscape { boundary, .. } = &after.shape else {
        return;
    };
    if d.mode != Mode::Slab || elevations.len() == boundary.len() {
        return;
    }
    let plane = super::surface::surface_of(before, data(before).as_ref(), None).plane;
    d.slope = Slope::Edges {
        elevations: curve::positions(boundary)
            .iter()
            .map(|p| plane.z(*p))
            .collect(),
    };
    set_data(after, &d);
}

/// The holes of a hardscape as flat rings: merged where they overlap and cut to the outline. A
/// hole that touches or crosses the outline is part of the outline then, and not in the list.
/// Each ring runs clockwise, so a consumer that wants outer rings counterclockwise and holes
/// clockwise can use them as they are. `None` if there are no holes.
pub fn merged_holes(
    boundary: &[Vertex],
    holes: &[Vec<Vertex>],
    tolerance: f64,
) -> Option<Vec<Vec<DVec2>>> {
    if holes.is_empty() {
        return None;
    }
    let region = crate::layout::Region::new(boundary, holes, tolerance).ok()?;
    Some(
        region
            .shapes
            .iter()
            .flat_map(|s| s.iter().skip(1))
            .cloned()
            .collect(),
    )
}

/// Cuts the holes of a hardscape to its outline, when a merge of two edits (or a file) put them
/// partly outside. Returns a note when something changed.
pub fn repair_holes(obj: &mut Object) -> Option<String> {
    let mut d = data(obj)?;
    let Shape::Hardscape { boundary, .. } = &obj.shape else {
        return None;
    };
    if d.holes.is_empty() {
        return None;
    }
    let ring = curve::flatten(boundary, true, TOLERANCE);
    let mut kept: Vec<Vec<Vertex>> = Vec::new();
    let mut changed = false;
    for h in &d.holes {
        let flat = curve::flatten(h, true, TOLERANCE);
        if crate::layout::ring_within(&flat, &ring) {
            kept.push(h.clone());
            continue;
        }
        changed = true;
        for piece in crate::layout::clip_ring_to(&flat, &ring) {
            if piece.len() >= 3 {
                kept.push(curve::corners(&piece));
            }
        }
    }
    if !changed {
        return None;
    }
    d.holes = kept;
    set_data(obj, &d);
    Some(format!(
        "object {}: holes that lay outside the hardscape outline were cut to it",
        obj.id
    ))
}

/// The direction of travel at the start of a segment.
fn start_direction(seg: &curve::Seg) -> DVec2 {
    match *seg {
        curve::Seg::Line { from, to } => to - from,
        curve::Seg::Arc { start, sweep, .. } => {
            DVec2::new(-start.sin(), start.cos()) * sweep.signum()
        }
        curve::Seg::Quad { from, c, to } => {
            if c != from {
                c - from
            } else {
                to - from
            }
        }
        curve::Seg::Cubic { from, c1, c2, to } => [c1 - from, c2 - from, to - from]
            .into_iter()
            .find(|d| d.length_squared() > 0.0)
            .unwrap_or(DVec2::ZERO),
    }
}

/// The directions of the outline at corner `index`: arriving and leaving. At the end of an arc
/// this is the tangent of the arc, not the chord. Corners that a fillet changed fall back to the
/// chords.
fn corner_directions(outline: &[Vertex], index: usize) -> (DVec2, DVec2) {
    let pts = curve::positions(outline);
    let n = pts.len();
    let p = pts[index];
    let chord_in = (p - pts[(index + n - 1) % n]).normalize_or_zero();
    let chord_out = (pts[(index + 1) % n] - p).normalize_or_zero();
    let path = curve::path(outline, true);
    let arriving = path
        .segs
        .iter()
        .find(|s| s.to().distance(p) < 1e-6)
        .map(|s| s.end_direction().normalize_or_zero())
        .filter(|d| *d != DVec2::ZERO);
    let leaving = path
        .segs
        .iter()
        .find(|s| s.from().distance(p) < 1e-6)
        .map(|s| start_direction(s).normalize_or_zero())
        .filter(|d| *d != DVec2::ZERO);
    (arriving.unwrap_or(chord_in), leaving.unwrap_or(chord_out))
}

/// The corner `index` of a closed outline moved inward: the point where the two edges at the
/// corner, the edge before it moved inward by `d_before` and the edge after it by `d_after`, meet.
/// An edge next to an arc counts as the tangent of the arc at the corner. Used for the start of a
/// pattern inside a kerb border.
pub fn inset_corner(outline: &[Vertex], index: usize, d_before: f64, d_after: f64) -> DVec2 {
    let pts = curve::positions(outline);
    let p = pts[index];
    if d_before == 0.0 && d_after == 0.0 {
        return p;
    }
    let ring = curve::flatten(outline, true, 1.0);
    let ccw = crate::geom::signed_area(&ring) >= 0.0;
    let inward = |dir: DVec2| {
        let left = DVec2::new(-dir.y, dir.x);
        if ccw { left } else { -left }
    };
    let (u1, u2) = corner_directions(outline, index);
    let (n1, n2) = (inward(u1), inward(u2));
    let cross = u1.perp_dot(u2);
    if cross.abs() < 1e-9 {
        return if d_after != 0.0 {
            p + n2 * d_after
        } else {
            p + n1 * d_before
        };
    }
    // Solve p + n1 d1 + s u1 = p + n2 d2 + t u2 for the meeting point.
    let r = n2 * d_after - n1 * d_before;
    let s = r.perp_dot(u2) / cross;
    p + n1 * d_before + u1 * s
}

/// How far the pattern starts inside the edge `edge` of the outline of `d`: the width of the kerb
/// border on that edge, or 0. A slab has the border on all edges. A path has it on its long
/// sides, as the style says, and not on its ends.
fn border_on_edge(
    outline: &[Vertex],
    d: &HardscapeData,
    border: (f64, super::style::Sides),
    edge: usize,
) -> f64 {
    let (width, sides) = border;
    if d.mode != Mode::Path {
        return width;
    }
    let pts = curve::positions(outline);
    let n = pts.len();
    let mid = (pts[edge % n] + pts[(edge + 1) % n]) / 2.0;
    let axis = d.axis();
    let mut best: Option<(f64, f64)> = None; // (distance, side)
    for w in axis.windows(2) {
        let (a, b) = (w[0], w[1]);
        let Some(u) = (b - a).try_normalize() else {
            continue;
        };
        let t = (mid - a).dot(u).clamp(0.0, a.distance(b));
        let q = a + u * t;
        let dist = q.distance(mid);
        if best.is_none_or(|(bd, _)| dist < bd) {
            best = Some((dist, u.perp_dot(mid - q)));
        }
    }
    let Some((dist, side)) = best else { return 0.0 };
    // An edge across the end of a path is on the center line: no border there.
    if dist < d.width * 0.25 {
        return 0.0;
    }
    use super::style::Sides;
    let on = match sides {
        Sides::Both => true,
        Sides::Left => side > 0.0,
        Sides::Right => side < 0.0,
    };
    if on { width } else { 0.0 }
}

/// A placement that starts at vertex `index` of `outline` and runs along the edge to the next
/// vertex. With a kerb border (`border`: the width and the sides), the start is on the inner
/// border line: the corner of the paved part, where the border of the two edges at the corner
/// meet (an edge without border does not move it). A curved edge counts as its chord for the
/// angle.
pub fn align_start(
    outline: &[Vertex],
    d: &HardscapeData,
    border: Option<(f64, super::style::Sides)>,
    index: usize,
) -> Result<crate::layout::Placement, String> {
    let mut p = crate::layout::Placement::along_edge(outline, index)?;
    if let Some(b) = border.filter(|b| b.0 > 0.0) {
        let n = outline.len();
        let before = border_on_edge(outline, d, b, (index + n - 1) % n);
        let after = border_on_edge(outline, d, b, index);
        p.origin = Some(inset_corner(outline, index, before, after));
    }
    Ok(p)
}

/// Moves the data that holds positions with the object: the center line of a path, the holes and
/// the origin of the pattern.
pub fn translate_data(obj: &mut Object, delta: DVec2) {
    let Some(mut d) = data(obj) else { return };
    if d.center.is_empty() && d.holes.is_empty() && d.origin.is_none() {
        return;
    }
    for v in d.center.iter_mut().chain(d.holes.iter_mut().flatten()) {
        v.p += delta;
    }
    if let Some(o) = &mut d.origin {
        *o += delta;
    }
    // The outline comes from the center line again, so the two always agree to the last bit.
    if d.mode == Mode::Path
        && let Shape::Hardscape { boundary, .. } = &mut obj.shape
    {
        let outline = d.outline();
        if outline.len() >= 3 {
            *boundary = outline;
        }
    }
    set_data(obj, &d);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClassId, LayerId, ObjectId};
    use crate::resource::{Resource, ResourceBody};

    fn slab() -> Object {
        let b = curve::corners(&[
            DVec2::ZERO,
            DVec2::new(3000.0, 0.0),
            DVec2::new(3000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ]);
        Object::new(
            ObjectId::from_num(5),
            LayerId::from_num(1),
            ClassId::from_num(0),
            Shape::Hardscape {
                boundary: b,
                material: "Gravel".into(),
                thickness: None,
                elevation: None,
            },
        )
    }

    fn doc_with_style() -> (Document, ResourceId) {
        let mut doc = Document::default();
        let id = ResourceId(doc.alloc_id());
        let style = super::super::style::HardscapeStyle {
            material: "Setts".into(),
            thickness: 60.0,
            components: vec![super::super::style::Component {
                thickness: 140.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        doc.resources.push(Resource {
            id,
            name: "Setts".into(),
            body: ResourceBody::HardscapeStyle(Box::new(style)),
        });
        (doc, id)
    }

    #[test]
    fn plain_hardscape_has_no_data() {
        let o = slab();
        assert!(data(&o).is_none());
        assert!(!has_unreadable_data(&o));
        let mut text = Object::new(
            ObjectId::from_num(6),
            LayerId::from_num(1),
            ClassId::from_num(0),
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        text.data
            .insert(KEY.into(), serde_json::json!({"mode": "path"}));
        assert!(data(&text).is_none(), "only hardscapes have settings");
    }

    #[test]
    fn patch_sets_style_and_copies_into_the_shape() {
        let (doc, sid) = doc_with_style();
        let mut o = slab();
        apply_patch(
            &doc,
            &mut o,
            &HardscapePatch {
                style: Some(sid),
                elevation: Some(30.0),
                ..Default::default()
            },
        )
        .unwrap();
        let d = data(&o).unwrap();
        assert_eq!(d.style, Some(sid));
        assert_eq!(style_id(&o), Some(sid));
        let Shape::Hardscape {
            material,
            thickness,
            elevation,
            ..
        } = &o.shape
        else {
            panic!()
        };
        assert_eq!(
            (material.as_str(), *thickness, *elevation),
            ("Setts", Some(200.0), Some(30.0))
        );
        assert_eq!(style_of(&doc, &o).unwrap().0, sid);
        // Unstyle keeps the copied fields.
        apply_patch(
            &doc,
            &mut o,
            &HardscapePatch {
                unstyle: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(data(&o).unwrap().style.is_none());
        assert!(style_of(&doc, &o).is_none());
        let Shape::Hardscape { material, .. } = &o.shape else {
            panic!()
        };
        assert_eq!(material, "Setts");
        // A style and unstyle together are refused.
        assert!(
            apply_patch(
                &doc,
                &mut o,
                &HardscapePatch {
                    style: Some(sid),
                    unstyle: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn patch_makes_a_path_outline() {
        let (doc, _) = doc_with_style();
        let mut o = slab();
        let before = o.clone();
        let p = HardscapePatch {
            center: Some(curve::corners(&[DVec2::ZERO, DVec2::new(4000.0, 0.0)])),
            width: Some(1000.0),
            ..Default::default()
        };
        apply_patch(&doc, &mut o, &p).unwrap();
        let d = data(&o).unwrap();
        assert_eq!(d.mode, Mode::Path);
        assert!((o.shape.area().unwrap() - 4.0e6).abs() < 1e-6);
        // A new width changes the outline.
        apply_patch(
            &doc,
            &mut o,
            &HardscapePatch {
                width: Some(2000.0),
                ..Default::default()
            },
        )
        .unwrap();
        assert!((o.shape.area().unwrap() - 8.0e6).abs() < 1e-6);
        // Moving the object moves the center line.
        translate_data(&mut o, DVec2::new(10.0, 20.0));
        assert_eq!(data(&o).unwrap().center[0].p, DVec2::new(10.0, 20.0));
        // Back to a slab keeps the outline.
        apply_patch(
            &doc,
            &mut o,
            &HardscapePatch {
                mode: Some(Mode::Slab),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(data(&o).unwrap().center.is_empty());
        // Errors leave the object alone.
        let mut o2 = before.clone();
        let bad = HardscapePatch {
            center: Some(curve::corners(&[DVec2::ZERO, DVec2::ZERO])),
            ..Default::default()
        };
        assert!(
            apply_patch(&doc, &mut o2, &bad)
                .unwrap_err()
                .contains("no length")
        );
        assert_eq!(o2, before);
        let bad = HardscapePatch {
            mode: Some(Mode::Path),
            ..Default::default()
        };
        assert!(
            apply_patch(&doc, &mut o2, &bad)
                .unwrap_err()
                .contains("center line")
        );
        let bad = HardscapePatch {
            width: Some(-1.0),
            ..Default::default()
        };
        assert!(apply_patch(&doc, &mut o2, &bad).is_err());
    }

    #[test]
    fn patch_checks_numbers_and_slopes() {
        let (doc, _) = doc_with_style();
        let mut o = slab();
        let try_patch = |o: &mut Object, p: HardscapePatch| apply_patch(&doc, o, &p);
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    thickness: Some(0.0),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    elevation: Some(f64::NAN),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    slope: Some(Slope::Cross {
                        percent: f64::NAN,
                        angle_deg: 0.0
                    }),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    slope: Some(Slope::Cross {
                        percent: 5000.0,
                        angle_deg: 0.0
                    }),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    slope: Some(Slope::Edges {
                        elevations: vec![0.0, 1.0]
                    }),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    slope: Some(Slope::Edges {
                        elevations: vec![0.0, 1.0, f64::INFINITY, 2.0]
                    }),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    slope: Some(Slope::Edges {
                        elevations: vec![0.0, 1.0, 2.0]
                    }),
                    ..Default::default()
                }
            )
            .is_err(),
            "four corners need four elevations"
        );
        assert!(
            try_patch(
                &mut o,
                HardscapePatch {
                    slope: Some(Slope::Edges {
                        elevations: vec![0.0, 1.0, 2.0, 3.0]
                    }),
                    material: Some("Slate".into()),
                    thickness: Some(80.0),
                    ..Default::default()
                }
            )
            .is_ok()
        );
        let Shape::Hardscape {
            material,
            thickness,
            ..
        } = &o.shape
        else {
            panic!()
        };
        assert_eq!((material.as_str(), *thickness), ("Slate", Some(80.0)));
        assert_eq!(data(&o).unwrap().slope.id(), "edges");
        assert!(HardscapePatch::default().is_empty());
        assert!(
            !HardscapePatch {
                drape: Some(true),
                ..Default::default()
            }
            .is_empty()
        );
        // Not a hardscape.
        let mut line = Object::new(
            ObjectId::from_num(9),
            LayerId::from_num(1),
            ClassId::from_num(0),
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        assert!(
            try_patch(
                &mut line,
                HardscapePatch {
                    drape: Some(true),
                    ..Default::default()
                }
            )
            .is_err()
        );
        // Unreadable data.
        let mut weird = slab();
        weird
            .data
            .insert(KEY.into(), serde_json::json!({"mode": 5}));
        assert!(has_unreadable_data(&weird));
        assert!(
            try_patch(
                &mut weird,
                HardscapePatch {
                    drape: Some(true),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn slope_and_mode_names() {
        assert_eq!(Slope::None.id(), "none");
        assert_eq!(
            Slope::Cross {
                percent: 1.0,
                angle_deg: 0.0
            }
            .id(),
            "cross"
        );
        assert_eq!(Mode::from_name(" Path "), Some(Mode::Path));
        assert_eq!(Mode::from_name("x"), None);
        assert_eq!(Mode::Slab.id(), "slab");
        let d: HardscapeData =
            serde_json::from_value(serde_json::json!({"slope": {"type": "cross", "percent": 2}}))
                .unwrap();
        assert_eq!(
            d.slope,
            Slope::Cross {
                percent: 2.0,
                angle_deg: 0.0
            }
        );
        assert_eq!(d.width, 1200.0);
    }
}
