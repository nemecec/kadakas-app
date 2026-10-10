//! Landscape walls: a path with a thickness, a top that is fixed, follows the ground or slopes,
//! and a bottom that follows the existing or the proposed ground.
//!
//! The wall is a polyline object with settings under `Object::data["landscape_wall"]`. A wall
//! style is a resource of type `wall_style` (components and coping). The wall solid is made
//! from stations along the path. A station has a top and a bottom height. Lengths are mm.

use std::fmt::Write;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use cadektor_core::curve;
use cadektor_core::mesh::SceneSink;
use cadektor_core::model::{Document, MAX_COORD, Object, ObjectId, Shape};
use cadektor_core::resource::{ResourceBody, ResourceId};

use super::{Follow, Ground, esc, finite, meters, read};
use crate::modifier::Modifier;

/// The key in `Object::data` that holds the settings.
pub const KEY: &str = "landscape_wall";
/// Thickness of a wall without a style.
pub const DEFAULT_THICKNESS: f64 = 300.0;
/// Largest distance between two stations.
pub const STATION_STEP: f64 = 500.0;
/// Most stations of one wall.
pub const MAX_STATIONS: usize = 4000;
/// Most pads that one wall makes when it acts as a site modifier.
pub const MAX_PADS: usize = 120;
const COLOR: [u8; 4] = [168, 160, 150, 255];

/// How the top of the wall is set.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Top {
    /// One height for the whole wall.
    Fixed { elevation: f64 },
    /// The ground plus an offset.
    Terrain { offset: f64 },
    /// A straight slope from the height at the start to the height at the end.
    Slope { start: f64, end: f64 },
}

impl Default for Top {
    fn default() -> Self {
        Top::Terrain { offset: 900.0 }
    }
}

/// The settings of a wall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WallData {
    /// Thickness. A style with a thickness overrides it.
    pub thickness: f64,
    pub top: Top,
    /// The ground that the bottom (and a terrain-following top) follows.
    pub bottom: Follow,
    /// How far the wall goes below the ground (footing).
    pub embed: f64,
    pub style: Option<ResourceId>,
    /// The site model to follow. `None`: the first one on the layer.
    pub site: Option<ObjectId>,
    /// The wall makes a vertical break in the proposed surface. It needs a fixed or sloped top,
    /// and then it follows the existing ground.
    pub site_modifier: bool,
}

impl Default for WallData {
    fn default() -> Self {
        WallData {
            thickness: DEFAULT_THICKNESS,
            top: Top::default(),
            bottom: Follow::Existing,
            embed: 0.0,
            style: None,
            site: None,
            site_modifier: false,
        }
    }
}

impl WallData {
    /// Checks the numbers and the rules between them.
    pub fn validate(&self) -> Result<(), String> {
        finite("thickness", self.thickness)?;
        finite("embed", self.embed)?;
        if !(self.thickness > 0.0 && self.thickness <= 10_000.0) {
            return Err("thickness must be more than 0 and at most 10 000 mm".into());
        }
        if !(0.0..=MAX_COORD).contains(&self.embed) {
            return Err("embed must be 0 or more".into());
        }
        match self.top {
            Top::Fixed { elevation } => {
                finite("top elevation", elevation)?;
            }
            Top::Terrain { offset } => {
                finite("top offset", offset)?;
            }
            Top::Slope { start, end } => {
                finite("top start", start)?;
                finite("top end", end)?;
            }
        }
        let bound = |v: f64| v.abs() <= MAX_COORD;
        let ok = match self.top {
            Top::Fixed { elevation } => bound(elevation),
            Top::Terrain { offset } => bound(offset),
            Top::Slope { start, end } => bound(start) && bound(end),
        };
        if !ok {
            return Err("a top height is too large".into());
        }
        if self.site_modifier && matches!(self.top, Top::Terrain { .. }) {
            return Err("a wall that acts as a site modifier needs a fixed or sloped top (mode fixed or slope)".into());
        }
        Ok(())
    }

    /// The ground that the wall follows. A site modifier wall follows the existing ground, because
    /// the proposed ground depends on the wall itself.
    pub fn follow(&self) -> Follow {
        if self.site_modifier {
            Follow::Existing
        } else {
            self.bottom
        }
    }
}

/// One layer of a wall, across its thickness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub material: String,
    pub thickness: f64,
}

/// The cap on top of a wall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coping {
    pub height: f64,
    /// How far the coping reaches past each face of the wall.
    #[serde(default)]
    pub overhang: f64,
    #[serde(default)]
    pub material: String,
}

/// A wall style: the build-up of a wall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WallStyle {
    /// Thickness of the wall. With components it is their total.
    pub thickness: f64,
    pub material: String,
    pub components: Vec<Component>,
    pub coping: Option<Coping>,
    /// Fill color of the wall in 2D and 3D, `[r, g, b, a]`.
    pub color: Option<[u8; 4]>,
}

impl Default for WallStyle {
    fn default() -> Self {
        WallStyle {
            thickness: DEFAULT_THICKNESS,
            material: String::new(),
            components: Vec::new(),
            coping: None,
            color: None,
        }
    }
}

impl WallStyle {
    /// The thickness that the wall gets: the total of the components if there are any.
    pub fn total_thickness(&self) -> f64 {
        if self.components.is_empty() {
            self.thickness
        } else {
            self.components.iter().map(|c| c.thickness).sum()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && v > 0.0 && v <= 10_000.0;
        if !ok(self.thickness) {
            return Err("thickness must be more than 0 and at most 10 000 mm".into());
        }
        if let Some(c) = self.components.iter().find(|c| !ok(c.thickness)) {
            return Err(format!(
                "component '{}' needs a thickness of more than 0 and at most 10 000 mm",
                c.name
            ));
        }
        if !ok(self.total_thickness()) {
            return Err("the components are too thick".into());
        }
        let bad_coping = |c: &Coping| {
            !(c.height.is_finite()
                && c.height > 0.0
                && c.height <= 5000.0
                && c.overhang.is_finite()
                && (0.0..=2000.0).contains(&c.overhang))
        };
        if self.coping.as_ref().is_some_and(bad_coping) {
            return Err("coping needs a height of more than 0 (at most 5000) and an overhang from 0 to 2000 mm".into());
        }
        Ok(())
    }

    /// The resource body of the style.
    pub fn to_body(&self) -> ResourceBody {
        let mut v = serde_json::to_value(self).expect("style is JSON");
        v["type"] = "wall_style".into();
        ResourceBody::Unknown(v)
    }
}

/// The style with this resource ID, if it exists and is a wall style.
pub fn style_of(doc: &Document, id: ResourceId) -> Option<WallStyle> {
    match &doc.resources.get(id)?.body {
        ResourceBody::Unknown(v) if v.get("type").and_then(Value::as_str) == Some("wall_style") => {
            serde_json::from_value(v.clone()).ok()
        }
        _ => None,
    }
}

/// The settings of a wall object. `None` for other objects.
pub fn data(obj: &Object) -> Option<WallData> {
    if !matches!(obj.shape, Shape::Polyline { .. }) {
        return None;
    }
    read(obj, KEY)
}

/// The center line of a wall, curves flattened. A closed wall repeats its first point at the end.
pub fn centerline(obj: &Object) -> Option<Vec<DVec2>> {
    let Shape::Polyline { points, closed } = &obj.shape else {
        return None;
    };
    let mut pts = curve::flatten(points, *closed, 2.0);
    pts.dedup_by(|a, b| a.distance_squared(*b) < 1e-12);
    if *closed && pts.len() > 2 {
        pts.push(pts[0]);
    }
    (pts.len() >= 2).then_some(pts)
}

/// Points along the path with the distance from its start. Every corner is a point, and no two
/// points are more than `step` apart (more if that would make too many).
pub fn stations(path: &[DVec2], step: f64) -> Vec<(f64, DVec2)> {
    let total: f64 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    let step = step.max(total / MAX_STATIONS as f64).max(1e-6);
    let mut out = Vec::new();
    let mut s = 0.0;
    for (i, w) in path.windows(2).enumerate() {
        let len = w[0].distance(w[1]);
        let parts = ((len / step).ceil() as usize).max(1);
        for k in 0..parts {
            out.push((
                s + len * k as f64 / parts as f64,
                w[0].lerp(w[1], k as f64 / parts as f64),
            ));
        }
        s += len;
        if i + 2 == path.len() {
            out.push((s, w[1]));
        }
    }
    out
}

/// A place along the wall with its heights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    /// Distance from the start of the wall.
    pub s: f64,
    pub p: DVec2,
    pub top: f64,
    pub bottom: f64,
    /// False when the site model does not cover the place. The ground counts as 0 there.
    pub on_ground: bool,
}

impl Station {
    pub fn height(&self) -> f64 {
        (self.top - self.bottom).max(0.0)
    }
}

/// The heights of the wall along its path. `ground` is the site model (or nothing).
pub fn profile(d: &WallData, path: &[DVec2], step: f64, ground: Ground<'_>) -> Vec<Station> {
    let st = stations(path, step);
    let total = st.last().map_or(0.0, |s| s.0);
    let follow = d.follow();
    st.into_iter()
        .map(|(s, p)| {
            let g = ground(p, follow);
            let base = g.unwrap_or(0.0);
            let top = match d.top {
                Top::Fixed { elevation } => elevation,
                Top::Terrain { offset } => base + offset,
                Top::Slope { start, end } => {
                    if total > 0.0 {
                        start + (end - start) * s / total
                    } else {
                        start
                    }
                }
            };
            Station {
                s,
                p,
                top,
                bottom: base - d.embed,
                on_ground: g.is_some(),
            }
        })
        .collect()
}

/// The height of a wall at distance `s` from its start, between stations.
pub fn at(stations: &[Station], s: f64) -> Option<Station> {
    let first = stations.first()?;
    let last = stations.last()?;
    if s <= first.s {
        return Some(*first);
    }
    if s >= last.s {
        return Some(*last);
    }
    let i = stations
        .partition_point(|x| x.s <= s)
        .clamp(1, stations.len() - 1);
    let (a, b) = (&stations[i - 1], &stations[i]);
    let t = if b.s > a.s {
        (s - a.s) / (b.s - a.s)
    } else {
        0.0
    };
    Some(Station {
        s,
        p: a.p.lerp(b.p, t),
        top: a.top + (b.top - a.top) * t,
        bottom: a.bottom + (b.bottom - a.bottom) * t,
        on_ground: a.on_ground && b.on_ground,
    })
}

/// Quantities of a wall. Areas are mm2 and volumes mm3.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quantities {
    pub length: f64,
    /// One face: length times height.
    pub face_area: f64,
    pub volume: f64,
    pub coping_volume: f64,
    pub min_height: f64,
    pub max_height: f64,
    pub avg_height: f64,
}

/// Quantities from the stations. The height between two stations is linear, so a wall on flat
/// ground gives the exact length times height times thickness.
pub fn quantities(st: &[Station], thickness: f64, coping: Option<&Coping>) -> Quantities {
    let mut q = Quantities {
        min_height: f64::INFINITY,
        ..Quantities::default()
    };
    for w in st.windows(2) {
        let ds = w[1].s - w[0].s;
        q.length += ds;
        q.face_area += ds * (w[0].height() + w[1].height()) / 2.0;
    }
    for s in st {
        q.min_height = q.min_height.min(s.height());
        q.max_height = q.max_height.max(s.height());
    }
    if st.is_empty() {
        q.min_height = 0.0;
    }
    q.volume = q.face_area * thickness;
    q.avg_height = if q.length > 0.0 {
        q.face_area / q.length
    } else {
        0.0
    };
    q.coping_volume = coping.map_or(0.0, |c| {
        q.length * (thickness + 2.0 * c.overhang) * c.height
    });
    q
}

/// A wall with everything computed.
#[derive(Clone, Debug)]
pub struct Wall {
    pub id: ObjectId,
    pub data: WallData,
    pub thickness: f64,
    pub coping: Option<Coping>,
    pub style_name: Option<String>,
    pub color: [u8; 4],
    pub closed: bool,
    pub stations: Vec<Station>,
    pub quantities: Quantities,
}

impl Wall {
    /// Stations that lie outside the site model.
    pub fn outside(&self) -> usize {
        self.stations.iter().filter(|s| !s.on_ground).count()
    }
}

/// The thickness, coping and color that a wall gets from its style.
fn look(doc: &Document, d: &WallData) -> (f64, Option<Coping>, Option<String>, [u8; 4]) {
    match d.style.and_then(|id| style_of(doc, id).map(|s| (id, s))) {
        Some((id, s)) => {
            let name = doc.resources.get(id).map(|r| r.name.clone());
            (
                s.total_thickness(),
                s.coping.clone(),
                name,
                s.color.unwrap_or(COLOR),
            )
        }
        None => (d.thickness, None, None, COLOR),
    }
}

/// Computes a wall object. `ground` is the site model, or `None` for flat ground at 0.
pub fn evaluate(doc: &Document, obj: &Object, ground: Option<Ground<'_>>) -> Option<Wall> {
    let d = data(obj)?;
    let path = centerline(obj)?;
    let none = |_: DVec2, _: Follow| None;
    let stations = profile(&d, &path, STATION_STEP, ground.unwrap_or(&none));
    let (thickness, coping, style_name, color) = look(doc, &d);
    let quantities = quantities(&stations, thickness, coping.as_ref());
    let closed = matches!(obj.shape, Shape::Polyline { closed: true, .. });
    Some(Wall {
        id: obj.id,
        data: d,
        thickness,
        coping,
        style_name,
        color,
        closed,
        stations,
        quantities,
    })
}

fn perp(v: DVec2) -> DVec2 {
    DVec2::new(-v.y, v.x)
}

/// The left and right edge points of a ribbon around the path. Corners are mitered.
/// A closed path repeats its first point at the end.
pub fn offsets(pts: &[DVec2], half: f64) -> (Vec<DVec2>, Vec<DVec2>) {
    let n = pts.len();
    let closed = n > 3 && pts[0].distance_squared(pts[n - 1]) < 1e-9;
    let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let prev = if i > 0 {
            Some(pts[i - 1])
        } else if closed {
            Some(pts[n - 2])
        } else {
            None
        };
        let next = if i + 1 < n {
            Some(pts[i + 1])
        } else if closed {
            Some(pts[1])
        } else {
            None
        };
        let d1 = prev.map(|q| (pts[i] - q).normalize_or_zero());
        let d2 = next.map(|q| (q - pts[i]).normalize_or_zero());
        let nrm = match (d1, d2) {
            (Some(a), Some(b)) => {
                let m = perp(a + b).normalize_or_zero();
                if m == DVec2::ZERO {
                    perp(a)
                } else {
                    m / m.dot(perp(a)).max(0.35)
                }
            }
            (Some(a), None) => perp(a),
            (None, Some(b)) => perp(b),
            (None, None) => DVec2::ZERO,
        };
        left.push(pts[i] + nrm * half);
        right.push(pts[i] - nrm * half);
    }
    (left, right)
}

/// The outline of the wall in plan: one ring for an open wall, two rings (the two edges) for a closed one.
pub fn footprint(path: &[DVec2], thickness: f64) -> Vec<Vec<DVec2>> {
    let (l, r) = offsets(path, thickness / 2.0);
    let closed = path.len() > 3 && path[0].distance_squared(path[path.len() - 1]) < 1e-9;
    if closed {
        let (mut a, mut b) = (l, r);
        a.pop();
        b.pop();
        vec![a, b]
    } else {
        let mut ring = l;
        ring.extend(r.into_iter().rev());
        vec![ring]
    }
}

/// Pads that make the wall a site modifier: one flat pad per piece of the wall, at the height of the
/// top. The ground beside the wall keeps its height, so the wall is a vertical break. Empty unless
/// the wall asks for it.
pub fn pads_of(doc: &Document, obj: &Object) -> Vec<Modifier> {
    let Some(d) = data(obj).filter(|d| d.site_modifier && d.validate().is_ok()) else {
        return Vec::new();
    };
    let Some(path) = centerline(obj) else {
        return Vec::new();
    };
    let (thickness, ..) = look(doc, &d);
    let total: f64 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    let ground = |_: DVec2, _: Follow| Some(0.0);
    let st = profile(
        &d,
        &path,
        (total / MAX_PADS as f64).max(STATION_STEP),
        &ground,
    );
    let (l, r) = offsets(&st.iter().map(|s| s.p).collect::<Vec<_>>(), thickness / 2.0);
    (0..st.len().saturating_sub(1))
        .map(|i| Modifier::Pad { boundary: vec![l[i], l[i + 1], r[i + 1], r[i]], elevation: (st[i].top + st[i + 1].top) / 2.0, slope: None })
        .filter(|m| matches!(m, Modifier::Pad { boundary, .. } if boundary.windows(2).all(|w| w[0].distance(w[1]) > 1e-6)))
        .collect()
}

struct Mesh {
    pos: Vec<DVec3>,
    idx: Vec<u32>,
}

impl Mesh {
    fn quad(&mut self, q: [DVec3; 4]) {
        let b = self.pos.len() as u32;
        self.pos.extend(q);
        self.idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
    }
}

/// A solid along the path between `lo` and `hi` heights, `half` wide on each side.
fn ribbon(pts: &[DVec2], half: f64, lo: &[f64], hi: &[f64]) -> Mesh {
    let (l, r) = offsets(pts, half);
    let closed = pts.len() > 3 && pts[0].distance_squared(pts[pts.len() - 1]) < 1e-9;
    let mut m = Mesh {
        pos: Vec::new(),
        idx: Vec::new(),
    };
    let v = |p: DVec2, z: f64| p.extend(z);
    for i in 0..pts.len() - 1 {
        let j = i + 1;
        m.quad([
            v(l[i], lo[i]),
            v(l[i], hi[i]),
            v(l[j], hi[j]),
            v(l[j], lo[j]),
        ]);
        m.quad([
            v(r[i], lo[i]),
            v(r[j], lo[j]),
            v(r[j], hi[j]),
            v(r[i], hi[i]),
        ]);
        m.quad([
            v(l[i], hi[i]),
            v(l[j], hi[j]),
            v(r[j], hi[j]),
            v(r[i], hi[i]),
        ]);
    }
    if !closed {
        let e = pts.len() - 1;
        m.quad([
            v(l[0], lo[0]),
            v(r[0], lo[0]),
            v(r[0], hi[0]),
            v(l[0], hi[0]),
        ]);
        m.quad([
            v(r[e], lo[e]),
            v(l[e], lo[e]),
            v(l[e], hi[e]),
            v(r[e], hi[e]),
        ]);
    }
    m
}

/// Adds the solid of the wall (and its coping) to a 3D scene.
pub fn emit(wall: &Wall, sink: &mut SceneSink<'_>) {
    if wall.stations.len() < 2 {
        return;
    }
    let pts: Vec<DVec2> = wall.stations.iter().map(|s| s.p).collect();
    let bottom: Vec<f64> = wall.stations.iter().map(|s| s.bottom).collect();
    let top: Vec<f64> = wall.stations.iter().map(|s| s.top.max(s.bottom)).collect();
    let m = ribbon(&pts, wall.thickness / 2.0, &bottom, &top);
    sink.mesh(&m.pos, &m.idx, wall.color);
    if let Some(c) = &wall.coping {
        let up: Vec<f64> = top.iter().map(|t| t + c.height).collect();
        let m = ribbon(&pts, wall.thickness / 2.0 + c.overhang, &top, &up);
        let lighter = [
            wall.color[0].saturating_add(30),
            wall.color[1].saturating_add(30),
            wall.color[2].saturating_add(30),
            255,
        ];
        sink.mesh(&m.pos, &m.idx, lighter);
    }
}

/// SVG of the wall in plan: the footprint and the tags of the top and bottom at both ends.
/// `scale` is the drawing scale of the layer (paper mm to model mm).
pub fn svg(wall: &Wall, scale: f64) -> String {
    let mut out = String::new();
    let Some(first) = wall.stations.first() else {
        return out;
    };
    let pts: Vec<DVec2> = wall.stations.iter().map(|s| s.p).collect();
    let rings = footprint(&pts, wall.thickness);
    let d: String = rings
        .iter()
        .map(|r| {
            let body: Vec<String> = r.iter().map(|p| format!("{} {}", p.x, -p.y)).collect();
            format!("M {} Z", body.join(" L "))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let fill = format!(
        "rgba({},{},{},0.55)",
        wall.color[0], wall.color[1], wall.color[2]
    );
    let _ = writeln!(
        out,
        r#"<path d="{d}" fill="{fill}" fill-rule="evenodd" stroke="none" data-kind="landscape_wall" data-wall="{}"/>"#,
        wall.id
    );
    let height = 2.5 * scale;
    let mut tag = |s: &Station| {
        let text = format!("TW {} BW {}", meters(s.top, 2), meters(s.bottom, 2));
        let _ = writeln!(
            out,
            r#"<text x="{}" y="{}" font-size="{height}" font-family="sans-serif" fill="black" data-kind="wall-tag" data-wall="{}">{}</text>"#,
            s.p.x,
            -s.p.y - wall.thickness / 2.0 - height * 0.4,
            wall.id,
            esc(&text)
        );
    };
    tag(first);
    if let Some(last) = wall.stations.last().filter(|_| !wall.closed) {
        tag(last);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grading::testutil::*;
    use cadektor_core::command::Editor;
    use serde_json::json;

    fn flat(_: DVec2, _: Follow) -> Option<f64> {
        Some(0.0)
    }

    #[test]
    fn validation_rules() {
        assert!(WallData::default().validate().is_ok());
        let bad = |f: &dyn Fn(&mut WallData)| {
            let mut d = WallData::default();
            f(&mut d);
            d.validate().is_err()
        };
        assert!(bad(&|d| d.thickness = 0.0));
        assert!(bad(&|d| d.thickness = f64::NAN));
        assert!(bad(&|d| d.thickness = 20_000.0));
        assert!(bad(&|d| d.embed = -1.0));
        assert!(bad(&|d| d.embed = f64::INFINITY));
        assert!(bad(&|d| d.top = Top::Fixed {
            elevation: f64::NAN
        }));
        assert!(bad(&|d| d.top = Top::Fixed { elevation: 1e12 }));
        assert!(bad(&|d| d.top = Top::Terrain { offset: f64::NAN }));
        assert!(bad(&|d| d.top = Top::Terrain { offset: 1e12 }));
        assert!(bad(&|d| d.top = Top::Slope {
            start: f64::NAN,
            end: 0.0
        }));
        assert!(bad(&|d| d.top = Top::Slope {
            start: 0.0,
            end: f64::NAN
        }));
        assert!(bad(&|d| d.top = Top::Slope {
            start: 0.0,
            end: 1e12
        }));
        assert!(bad(&|d| d.site_modifier = true));
        let mut ok = WallData {
            site_modifier: true,
            top: Top::Fixed { elevation: 500.0 },
            bottom: Follow::Proposed,
            ..WallData::default()
        };
        assert!(ok.validate().is_ok());
        assert_eq!(ok.follow(), Follow::Existing);
        ok.site_modifier = false;
        assert_eq!(ok.follow(), Follow::Proposed);
    }

    #[test]
    fn wall_on_flat_ground_has_exact_quantities() {
        let d = WallData {
            thickness: 250.0,
            top: Top::Fixed { elevation: 1200.0 },
            ..WallData::default()
        };
        let path = [
            DVec2::new(0.0, 0.0),
            DVec2::new(4000.0, 0.0),
            DVec2::new(4000.0, 3000.0),
        ];
        let st = profile(&d, &path, STATION_STEP, &flat);
        let q = quantities(&st, 250.0, None);
        assert!((q.length - 7000.0).abs() < 1e-9);
        assert!((q.face_area - 7000.0 * 1200.0).abs() < 1e-6);
        assert!((q.volume - 7000.0 * 1200.0 * 250.0).abs() < 1e-3);
        assert_eq!(
            (q.min_height, q.max_height, q.avg_height),
            (1200.0, 1200.0, 1200.0)
        );
        assert!(st.iter().all(|s| s.on_ground));
        let c = Coping {
            height: 50.0,
            overhang: 25.0,
            material: "stone".into(),
        };
        assert!(
            (quantities(&st, 250.0, Some(&c)).coping_volume - 7000.0 * 300.0 * 50.0).abs() < 1e-3
        );
        assert_eq!(quantities(&[], 250.0, None).min_height, 0.0);
    }

    #[test]
    fn top_modes() {
        let path = [DVec2::new(0.0, 0.0), DVec2::new(1000.0, 0.0)];
        let ramp = |p: DVec2, _: Follow| Some(p.x / 10.0);
        let terrain = WallData {
            top: Top::Terrain { offset: 600.0 },
            embed: 50.0,
            ..WallData::default()
        };
        let st = profile(&terrain, &path, 500.0, &ramp);
        assert_eq!(st.len(), 3);
        assert!(st.iter().all(|s| (s.height() - 650.0).abs() < 1e-9));
        let slope = WallData {
            top: Top::Slope {
                start: 100.0,
                end: 300.0,
            },
            ..WallData::default()
        };
        let st = profile(&slope, &path, 500.0, &flat);
        assert_eq!((st[0].top, st[1].top, st[2].top), (100.0, 200.0, 300.0));
        // A top below the bottom has no height.
        let low = WallData {
            top: Top::Fixed { elevation: -5.0 },
            ..WallData::default()
        };
        assert_eq!(profile(&low, &path, 500.0, &flat)[0].height(), 0.0);
        // Outside the site model the ground counts as 0.
        let none = |_: DVec2, _: Follow| None;
        let st = profile(&terrain, &path, 500.0, &none);
        assert!(
            st.iter()
                .all(|s| !s.on_ground && (s.top - 600.0).abs() < 1e-9)
        );
        // A zero length path.
        let one = profile(&slope, &[DVec2::ZERO, DVec2::ZERO], 500.0, &flat);
        assert_eq!(one[0].top, 100.0);
    }

    #[test]
    fn interpolation_along_the_wall() {
        let d = WallData {
            top: Top::Slope {
                start: 0.0,
                end: 1000.0,
            },
            ..WallData::default()
        };
        let st = profile(&d, &[DVec2::ZERO, DVec2::new(1000.0, 0.0)], 500.0, &flat);
        assert_eq!(at(&st, -5.0).unwrap().top, 0.0);
        assert_eq!(at(&st, 5000.0).unwrap().top, 1000.0);
        let m = at(&st, 250.0).unwrap();
        assert!((m.top - 250.0).abs() < 1e-9 && (m.p.x - 250.0).abs() < 1e-9);
        assert!(at(&[], 1.0).is_none());
        let twin = [st[0], st[0]];
        assert_eq!(at(&twin, 0.0).unwrap().top, 0.0);
        let same = [
            Station { s: 1.0, ..st[0] },
            Station { s: 1.0, ..st[1] },
            Station { s: 2.0, ..st[1] },
        ];
        assert!(at(&same, 1.5).is_some());
    }

    #[test]
    fn stations_cap_and_corners() {
        let path = [
            DVec2::ZERO,
            DVec2::new(100.0, 0.0),
            DVec2::new(100.0, 100.0),
        ];
        let st = stations(&path, 1000.0);
        assert_eq!(st.len(), 3);
        assert_eq!(st[2].0, 200.0);
        let long = [DVec2::ZERO, DVec2::new(1e7, 0.0)];
        assert!(stations(&long, 1.0).len() <= MAX_STATIONS + 1);
    }

    #[test]
    fn offsets_and_footprints() {
        let line = [DVec2::ZERO, DVec2::new(1000.0, 0.0)];
        let (l, r) = offsets(&line, 100.0);
        assert_eq!(
            (l[0], r[0]),
            (DVec2::new(0.0, 100.0), DVec2::new(0.0, -100.0))
        );
        let fp = footprint(&line, 200.0);
        assert_eq!(fp.len(), 1);
        assert_eq!(fp[0].len(), 4);
        // A right angle corner is mitered: the outer corner is 100 mm from both faces.
        let corner = [
            DVec2::ZERO,
            DVec2::new(1000.0, 0.0),
            DVec2::new(1000.0, 1000.0),
        ];
        let (l, r) = offsets(&corner, 100.0);
        assert!((l[1] - DVec2::new(900.0, 100.0)).length() < 1e-9);
        assert!((r[1] - DVec2::new(1100.0, -100.0)).length() < 1e-9);
        // A reversal has no miter: it falls back to the side normal.
        let back = [DVec2::ZERO, DVec2::new(1000.0, 0.0), DVec2::ZERO];
        assert!(offsets(&back, 100.0).0[1].is_finite());
        let ring = [
            DVec2::ZERO,
            DVec2::new(1000.0, 0.0),
            DVec2::new(1000.0, 1000.0),
            DVec2::new(0.0, 1000.0),
            DVec2::ZERO,
        ];
        let fp = footprint(&ring, 200.0);
        assert_eq!(fp.len(), 2);
        let area = |r: &[DVec2]| cadektor_core::geom::polygon_area(r).abs();
        // The ring runs counterclockwise: the left edge is the inner one.
        assert!(
            (area(&fp[0]) - 800.0 * 800.0).abs() < 1.0,
            "{}",
            area(&fp[0])
        );
        assert!((area(&fp[1]) - 1200.0 * 1200.0).abs() < 1.0);
        assert_eq!(offsets(&[DVec2::ZERO], 10.0).0[0], DVec2::ZERO);
    }

    #[test]
    fn styles_are_resources() {
        let mut s = WallStyle {
            thickness: 200.0,
            ..WallStyle::default()
        };
        assert!(s.validate().is_ok());
        s.components = vec![
            Component {
                name: "Block".into(),
                material: "Concrete".into(),
                thickness: 150.0,
            },
            Component {
                name: "Render".into(),
                material: "Lime".into(),
                thickness: 20.0,
            },
        ];
        assert_eq!(s.total_thickness(), 170.0);
        s.coping = Some(Coping {
            height: 60.0,
            overhang: 20.0,
            material: "Stone".into(),
        });
        assert!(s.validate().is_ok());
        let body = s.to_body();
        assert!(matches!(&body, ResourceBody::Unknown(v) if v["type"] == "wall_style"));
        let mut ed = Editor::default();
        ed.execute(&cadektor_core::command::Command::AddResource {
            name: "Block wall".into(),
            body,
        })
        .unwrap();
        let id = ed.doc().resources.iter().next().unwrap().id;
        assert_eq!(style_of(ed.doc(), id), Some(s.clone()));
        assert!(style_of(ed.doc(), ResourceId(999)).is_none());
        ed.execute(&cadektor_core::command::Command::AddResource {
            name: "Other".into(),
            body: ResourceBody::Unknown(json!({"type": "x"})),
        })
        .unwrap();
        let other = ed.doc().resources.iter().nth(1).unwrap().id;
        assert!(style_of(ed.doc(), other).is_none());
        for bad in [
            WallStyle {
                thickness: 0.0,
                ..WallStyle::default()
            },
            WallStyle {
                components: vec![Component {
                    name: "x".into(),
                    material: "".into(),
                    thickness: -1.0,
                }],
                ..WallStyle::default()
            },
            WallStyle {
                components: vec![
                    Component {
                        name: "x".into(),
                        material: "".into(),
                        thickness: 9000.0,
                    },
                    Component {
                        name: "y".into(),
                        material: "".into(),
                        thickness: 9000.0,
                    },
                ],
                ..WallStyle::default()
            },
            WallStyle {
                coping: Some(Coping {
                    height: 0.0,
                    overhang: 0.0,
                    material: "".into(),
                }),
                ..WallStyle::default()
            },
            WallStyle {
                coping: Some(Coping {
                    height: 10.0,
                    overhang: -1.0,
                    material: "".into(),
                }),
                ..WallStyle::default()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn evaluate_reads_object_and_style() {
        let mut ed = Editor::default();
        let style = WallStyle {
            thickness: 400.0,
            color: Some([1, 2, 3, 255]),
            coping: Some(Coping {
                height: 50.0,
                overhang: 0.0,
                material: "".into(),
            }),
            ..WallStyle::default()
        };
        ed.execute(&cadektor_core::command::Command::AddResource {
            name: "W".into(),
            body: style.to_body(),
        })
        .unwrap();
        let sid = ed.doc().resources.iter().next().unwrap().id;
        let d = WallData {
            top: Top::Fixed { elevation: 1000.0 },
            style: Some(sid),
            ..WallData::default()
        };
        let id = add_with(
            &mut ed,
            path(&[(0.0, 0.0), (2000.0, 0.0)]),
            KEY,
            serde_json::to_value(&d).unwrap(),
        );
        let w = evaluate(ed.doc(), ed.doc().object(id).unwrap(), Some(&flat)).unwrap();
        assert_eq!(
            (w.thickness, w.color, w.style_name.as_deref()),
            (400.0, [1, 2, 3, 255], Some("W"))
        );
        assert!((w.quantities.volume - 2000.0 * 1000.0 * 400.0).abs() < 1e-3);
        assert_eq!(w.outside(), 0);
        let w = evaluate(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        assert_eq!(w.outside(), w.stations.len());
        // Not a wall.
        let plain = add(&mut ed, path(&[(0.0, 0.0), (1.0, 0.0)]));
        assert!(evaluate(ed.doc(), ed.doc().object(plain).unwrap(), None).is_none());
        let circle = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        assert!(data(ed.doc().object(circle).unwrap()).is_none());
        assert!(centerline(ed.doc().object(circle).unwrap()).is_none());
        // A wall with a missing style gets the thickness of its own settings.
        let d2 = WallData {
            style: Some(ResourceId(77)),
            thickness: 123.0,
            ..d
        };
        let id2 = add_with(
            &mut ed,
            path(&[(0.0, 0.0), (2000.0, 0.0)]),
            KEY,
            serde_json::to_value(&d2).unwrap(),
        );
        assert_eq!(
            evaluate(ed.doc(), ed.doc().object(id2).unwrap(), None)
                .unwrap()
                .thickness,
            123.0
        );
    }

    #[test]
    fn pads_make_a_vertical_break() {
        let mut ed = Editor::default();
        let d = WallData {
            top: Top::Slope {
                start: 500.0,
                end: 900.0,
            },
            site_modifier: true,
            thickness: 300.0,
            ..WallData::default()
        };
        let id = add_with(
            &mut ed,
            path(&[(1000.0, 5000.0), (9000.0, 5000.0)]),
            KEY,
            serde_json::to_value(&d).unwrap(),
        );
        let pads = pads_of(ed.doc(), ed.doc().object(id).unwrap());
        assert!(pads.len() >= 8);
        let Modifier::Pad { elevation, .. } = &pads[0] else {
            panic!()
        };
        assert!(*elevation > 500.0 && *elevation < 560.0);
        // Without the flag, with a terrain top, or on a wall without data there is nothing.
        let off = WallData {
            site_modifier: false,
            ..d.clone()
        };
        let id2 = add_with(
            &mut ed,
            path(&[(0.0, 0.0), (1000.0, 0.0)]),
            KEY,
            serde_json::to_value(&off).unwrap(),
        );
        assert!(pads_of(ed.doc(), ed.doc().object(id2).unwrap()).is_empty());
        let terrain = serde_json::to_value(WallData {
            site_modifier: true,
            ..WallData::default()
        })
        .unwrap();
        let id3 = add_with(&mut ed, path(&[(0.0, 0.0), (1000.0, 0.0)]), KEY, terrain);
        assert!(pads_of(ed.doc(), ed.doc().object(id3).unwrap()).is_empty());
        let circle = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        assert!(pads_of(ed.doc(), ed.doc().object(circle).unwrap()).is_empty());
        // A wall data on a shape without a path.
        let bad = add_with(
            &mut ed,
            Shape::Polyline {
                points: curve::corners(&[DVec2::ZERO, DVec2::ZERO]),
                closed: false,
            },
            KEY,
            serde_json::to_value(&d).unwrap(),
        );
        assert!(pads_of(ed.doc(), ed.doc().object(bad).unwrap()).is_empty());
    }

    #[test]
    fn drawing_the_wall() {
        let mut ed = Editor::default();
        let d = WallData {
            top: Top::Fixed { elevation: 1000.0 },
            style: None,
            ..WallData::default()
        };
        let id = add_with(
            &mut ed,
            path(&[(0.0, 0.0), (2000.0, 0.0), (2000.0, 2000.0)]),
            KEY,
            serde_json::to_value(&d).unwrap(),
        );
        let w = evaluate(ed.doc(), ed.doc().object(id).unwrap(), Some(&flat)).unwrap();
        let out = svg(&w, 100.0);
        assert!(
            out.contains(r#"data-kind="landscape_wall""#)
                && out.matches("wall-tag").count() == 2
                && out.contains("TW 1.00 BW 0.00")
        );
        // A closed wall has one tag and two rings.
        let ring = Shape::Polyline {
            points: curve::corners(&[
                DVec2::ZERO,
                DVec2::new(2000.0, 0.0),
                DVec2::new(2000.0, 2000.0),
                DVec2::new(0.0, 2000.0),
            ]),
            closed: true,
        };
        let rid = add_with(&mut ed, ring, KEY, serde_json::to_value(&d).unwrap());
        let w = evaluate(ed.doc(), ed.doc().object(rid).unwrap(), Some(&flat)).unwrap();
        assert!(w.closed);
        let out = svg(&w, 100.0);
        assert_eq!(out.matches("wall-tag").count(), 1);
        assert_eq!(out.matches('M').count(), 2);
        let empty = Wall {
            stations: vec![],
            ..w.clone()
        };
        assert!(svg(&empty, 100.0).is_empty());
    }

    #[test]
    fn solid_has_faces_and_coping() {
        use cadektor_core::mesh;
        let mut ed = Editor::default();
        let style = WallStyle {
            thickness: 300.0,
            coping: Some(Coping {
                height: 50.0,
                overhang: 20.0,
                material: "".into(),
            }),
            ..WallStyle::default()
        };
        ed.execute(&cadektor_core::command::Command::AddResource {
            name: "W".into(),
            body: style.to_body(),
        })
        .unwrap();
        let sid = ed.doc().resources.iter().next().unwrap().id;
        let d = WallData {
            top: Top::Fixed { elevation: 1000.0 },
            style: Some(sid),
            ..WallData::default()
        };
        let id = add_with(
            &mut ed,
            path(&[(0.0, 0.0), (2000.0, 0.0)]),
            KEY,
            serde_json::to_value(&d).unwrap(),
        );
        let cache = crate::SiteCache::default();
        let ext = crate::draw::scene_ext(&cache, ed.revision());
        let scene = mesh::build_with(ed.doc(), DVec3::ZERO, Some(&ext));
        let (lo, hi) = scene.object_bounds(id).unwrap();
        let (lo, hi) = (scene.to_world(lo), scene.to_world(hi));
        assert!(
            (hi.z - 1050.0).abs() < 1e-3 && lo.z.abs() < 1e-3,
            "{lo:?} {hi:?}"
        );
        assert!(
            (hi.y - lo.y - 340.0).abs() < 1e-3,
            "coping is 20 mm wider on each side"
        );
    }
}
