//! Site sections: a section line (`Shape::Line` with settings under `Object::data["site_section"]`)
//! gives a 2D profile of the ground (existing and proposed), the walls, the hardscapes and the
//! plants that the line cuts or passes. `drawing` turns the profile into plain shapes with a
//! vertical exaggeration. The API places them on a layer.

use std::f64::consts::TAU;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use cadektor_core::curve;
use cadektor_core::geom::point_in_polygon;
use cadektor_core::hardscape;
use cadektor_core::mesh::DEFAULT_SLAB_THICKNESS;
use cadektor_core::model::{Document, Object, ObjectId, Shape};

use super::{Follow, Ground, finite, ground_of, meters, read, wall};
use crate::drape::drape;
use crate::model::SiteResult;
use crate::tin::Tin;

/// The key in `Object::data` that holds the settings.
pub const KEY: &str = "site_section";
/// The key on the shapes that the API placed for a section.
pub const PROFILE_KEY: &str = "site_section_profile";
/// Most samples along a section.
pub const MAX_SAMPLES: usize = 20_000;

/// The settings of a section line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SectionData {
    /// The site model. `None`: the first one on the layer.
    pub site: Option<ObjectId>,
    pub name: String,
    /// Vertical exaggeration of the profile drawing.
    pub exaggeration: f64,
    /// Distance between samples when the line leaves the site model, mm.
    pub step: f64,
    /// Plants within this distance of the line (each side) show as silhouettes, mm.
    pub band: f64,
    pub walls: bool,
    pub hardscapes: bool,
    pub plants: bool,
}

impl Default for SectionData {
    fn default() -> Self {
        SectionData { site: None, name: "A".into(), exaggeration: 2.0, step: 1000.0, band: 2000.0, walls: true, hardscapes: true, plants: true }
    }
}

impl SectionData {
    pub fn validate(&self) -> Result<(), String> {
        finite("exaggeration", self.exaggeration)?;
        finite("step", self.step)?;
        finite("band", self.band)?;
        if !(self.exaggeration > 0.0 && self.exaggeration <= 50.0) {
            return Err("exaggeration must be more than 0 and at most 50".into());
        }
        if !(10.0..=100_000.0).contains(&self.step) {
            return Err("step must be from 10 to 100 000 mm".into());
        }
        if !(0.0..=1.0e6).contains(&self.band) {
            return Err("band must be from 0 to 1 000 000 mm".into());
        }
        Ok(())
    }
}

/// A wall that the section line crosses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallCut {
    pub id: ObjectId,
    /// Distance along the section to the middle of the cut.
    pub s: f64,
    /// Width of the cut along the section: thickness over the sine of the crossing angle.
    pub width: f64,
    pub top: f64,
    pub bottom: f64,
}

/// A hardscape that the section line cuts: from `s0` to `s1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlabCut {
    pub id: ObjectId,
    pub s0: f64,
    pub s1: f64,
    pub top0: f64,
    pub top1: f64,
    pub thickness: f64,
}

/// A plant near the section line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlantCut {
    pub id: ObjectId,
    pub s: f64,
    /// Distance from the section line.
    pub offset: f64,
    pub spread: f64,
    pub height: f64,
    /// The proposed ground under the plant.
    pub base: f64,
}

/// The profile of a section.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub length: f64,
    /// Runs of (distance, elevation). A line that leaves the site model has several runs.
    pub existing: Vec<Vec<(f64, f64)>>,
    pub proposed: Vec<Vec<(f64, f64)>>,
    /// True if the proposed surface is not the existing one.
    pub modified: bool,
    pub walls: Vec<WallCut>,
    pub slabs: Vec<SlabCut>,
    pub plants: Vec<PlantCut>,
    pub z_min: f64,
    pub z_max: f64,
}

/// The runs of (distance, elevation) of a surface along the line a to b. When the whole line lies
/// on the surface, the runs have a point at every triangle edge, so the profile is exact. Else the
/// line is sampled every `step` and the points off the surface are left out.
pub fn sample(tin: &Tin, a: DVec2, b: DVec2, step: f64) -> Vec<Vec<(f64, f64)>> {
    if let Ok(pts) = drape(tin, &[a, b], false) {
        return vec![pts.iter().map(|p| (a.distance(p.truncate()), p.z)).collect()];
    }
    let len = a.distance(b);
    let n = ((len / step).ceil() as usize).clamp(1, MAX_SAMPLES);
    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut open = false;
    for i in 0..=n {
        let s = len * i as f64 / n as f64;
        match tin.elevation_at(a.lerp(b, i as f64 / n as f64)) {
            Some(z) => {
                if !open {
                    runs.push(Vec::new());
                    open = true;
                }
                runs.last_mut().expect("a run is open").push((s, z));
            }
            None => open = false,
        }
    }
    runs.retain(|r| r.len() > 1);
    runs
}

/// Where the segment a-b crosses the segment c-d: the parameters (t on a-b, u on c-d).
fn cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<(f64, f64)> {
    let (r, s) = (b - a, d - c);
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = (c - a).perp_dot(s) / denom;
    let u = (c - a).perp_dot(r) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// The parts of the segment a-b inside a polygon, as parameter ranges (0 to 1).
pub fn clip(ring: &[DVec2], a: DVec2, b: DVec2) -> Vec<(f64, f64)> {
    let n = ring.len();
    if n < 3 {
        return Vec::new();
    }
    let mut ts = vec![0.0, 1.0];
    for i in 0..n {
        if let Some((t, _)) = cross(a, b, ring[i], ring[(i + 1) % n]) {
            ts.push(t);
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in ts.windows(2) {
        if point_in_polygon(a.lerp(b, (w[0] + w[1]) / 2.0), ring) {
            match out.last_mut() {
                Some(last) if (last.1 - w[0]).abs() < 1e-12 => last.1 = w[1],
                _ => out.push((w[0], w[1])),
            }
        }
    }
    out
}

fn wall_cuts(doc: &Document, section: &Object, a: DVec2, b: DVec2, ground: Ground<'_>) -> Vec<WallCut> {
    let len = a.distance(b);
    let dir = (b - a) / len;
    let mut out = Vec::new();
    for o in doc.objects.values().filter(|o| o.layer == section.layer && doc.is_visible(o)) {
        let Some(w) = wall::evaluate(doc, o, Some(ground)) else { continue };
        let Some(path) = wall::centerline(o) else { continue };
        let mut along = 0.0;
        for seg in path.windows(2) {
            let seg_len = seg[0].distance(seg[1]);
            if let Some((t, u)) = cross(a, b, seg[0], seg[1]) {
                let wdir = (seg[1] - seg[0]) / seg_len;
                let sin = dir.perp_dot(wdir).abs().max(0.1);
                if let Some(st) = wall::at(&w.stations, along + u * seg_len) {
                    let top = st.top.max(st.bottom) + w.coping.as_ref().map_or(0.0, |c| c.height);
                    out.push(WallCut { id: o.id, s: t * len, width: w.thickness / sin, top, bottom: st.bottom });
                }
            }
            along += seg_len;
        }
    }
    out
}

fn slab_cuts(doc: &Document, section: &Object, a: DVec2, b: DVec2, ground: Ground<'_>) -> Vec<SlabCut> {
    let len = a.distance(b);
    let mut out = Vec::new();
    for o in doc.objects.values().filter(|o| o.layer == section.layer && doc.is_visible(o)) {
        let Shape::Hardscape { boundary, thickness, elevation, .. } = &o.shape else { continue };
        let drapes = hardscape::data(o).is_some_and(|d| d.drape);
        let depth = match hardscape::data::style_of(doc, o) {
            Some((_, s)) => s.total_thickness(),
            None => thickness.filter(|t| t.is_finite() && *t > 0.0).unwrap_or(DEFAULT_SLAB_THICKNESS),
        };
        let raise = elevation.filter(|e| e.is_finite()).unwrap_or(0.0);
        let top_at = |t: f64| raise + if drapes { ground(a.lerp(b, t), Follow::Proposed).unwrap_or(0.0) } else { 0.0 };
        for (t0, t1) in clip(&curve::flatten(boundary, true, 2.0), a, b) {
            out.push(SlabCut { id: o.id, s0: t0 * len, s1: t1 * len, top0: top_at(t0), top1: top_at(t1), thickness: depth });
        }
    }
    out
}

fn plant_cuts(doc: &Document, section: &Object, a: DVec2, b: DVec2, band: f64, ground: Ground<'_>) -> Vec<PlantCut> {
    let len = a.distance(b);
    let dir = (b - a) / len;
    doc.objects
        .values()
        .filter(|o| o.layer == section.layer && doc.is_visible(o))
        .filter_map(|o| {
            let (at, spread, height) = match &o.shape {
                Shape::Plant { at, spread, height, .. } | Shape::ExistingTree { at, spread, height, .. } => (*at, *spread, *height),
                _ => return None,
            };
            let s = (at - a).dot(dir);
            let offset = (at - a).perp_dot(dir).abs();
            ((0.0..=len).contains(&s) && offset <= band).then(|| PlantCut {
                id: o.id,
                s,
                offset,
                spread,
                height,
                base: ground(at, Follow::Proposed).unwrap_or(0.0),
            })
        })
        .collect()
}

/// The settings of a section object. `None` for other objects.
pub fn data(obj: &Object) -> Option<SectionData> {
    if !matches!(obj.shape, Shape::Line { .. }) {
        return None;
    }
    read(obj, KEY)
}

/// The profile of a section object over a site model. `None` if the object is not a section or
/// the line has no length.
pub fn profile(doc: &Document, obj: &Object, site: &SiteResult) -> Option<Profile> {
    let Shape::Line { a, b } = &obj.shape else { return None };
    let d = data(obj)?;
    let length = a.distance(*b);
    if !(length > 0.0 && length.is_finite()) {
        return None;
    }
    let existing = sample(site.surface(crate::Surface::Existing), *a, *b, d.step);
    let modified = site.proposed.is_some();
    let proposed = if modified { sample(site.surface(crate::Surface::Proposed), *a, *b, d.step) } else { existing.clone() };
    let ground = ground_of(site);
    let walls = if d.walls { wall_cuts(doc, obj, *a, *b, &ground) } else { Vec::new() };
    let slabs = if d.hardscapes { slab_cuts(doc, obj, *a, *b, &ground) } else { Vec::new() };
    let plants = if d.plants { plant_cuts(doc, obj, *a, *b, d.band, &ground) } else { Vec::new() };
    let mut zs: Vec<f64> = existing.iter().chain(&proposed).flatten().map(|p| p.1).collect();
    zs.extend(walls.iter().flat_map(|w| [w.top, w.bottom]));
    zs.extend(slabs.iter().flat_map(|s| [s.top0, s.top1, s.top0 - s.thickness]));
    zs.extend(plants.iter().flat_map(|p| [p.base, p.base + p.height]));
    let z_min = zs.iter().copied().fold(f64::INFINITY, f64::min);
    let z_max = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (z_min, z_max) = if zs.is_empty() { (0.0, 0.0) } else { (z_min, z_max) };
    Some(Profile { length, existing, proposed, modified, walls, slabs, plants, z_min, z_max })
}

/// One shape of the drawing of a profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub shape: Shape,
    /// What the shape is: existing, proposed, wall, hardscape, plant, axis or label.
    pub role: &'static str,
    pub pen: [u8; 4],
    pub fill: Option<[u8; 4]>,
    /// Line weight on paper, mm.
    pub weight: f64,
}

impl Item {
    /// The item with all lengths multiplied by `k`.
    pub fn scaled(&self, k: f64) -> Item {
        let shape = match &self.shape {
            Shape::Line { a, b } => Shape::Line { a: *a * k, b: *b * k },
            Shape::Polyline { points, closed } => Shape::Polyline { points: points.iter().map(|v| curve::Vertex::corner(v.p * k)).collect(), closed: *closed },
            Shape::Text { at, text, height } => Shape::Text { at: *at * k, text: text.clone(), height: *height * k },
            other => other.clone(),
        };
        Item { shape, ..self.clone() }
    }
}

fn poly(points: &[DVec2], closed: bool) -> Shape {
    Shape::Polyline { points: curve::corners(points), closed }
}

/// The shapes of a profile drawing. The start of the section is at `at`, distances go to the right,
/// and heights above the baseline go up, multiplied by the exaggeration. `text` is the text height
/// in model units. The baseline is the lowest height rounded down to a whole meter, minus 1 m.
pub fn drawing(p: &Profile, d: &SectionData, at: DVec2, text: f64) -> Vec<Item> {
    let base = (p.z_min / 1000.0).floor() * 1000.0 - 1000.0;
    let x = |s: f64| at.x + s;
    let y = |z: f64| at.y + (z - base) * d.exaggeration;
    let pt = |s: f64, z: f64| DVec2::new(x(s), y(z));
    let mut out = Vec::new();
    let item = |shape: Shape, role: &'static str, pen: [u8; 4], fill: Option<[u8; 4]>, weight: f64| Item { shape, role, pen, fill, weight };
    let ink = [0, 0, 0, 255];
    out.push(item(Shape::Line { a: at, b: DVec2::new(x(p.length), at.y) }, "axis", ink, None, 0.35));
    for (z, s) in [(p.z_min, 0.0), (p.z_max, 0.0)] {
        out.push(item(Shape::Text { at: DVec2::new(x(s) - text * 8.0, y(z)), text: format!("{} m", meters(z, 2)), height: text }, "label", ink, None, 0.25));
    }
    out.push(item(
        Shape::Text { at: DVec2::new(at.x, at.y - text * 3.0), text: format!("Section {}  (vertical x{}, baseline {} m)", d.name, d.exaggeration, meters(base, 2)), height: text },
        "label",
        ink,
        None,
        0.25,
    ));
    for run in &p.existing {
        let pts: Vec<DVec2> = run.iter().map(|(s, z)| pt(*s, *z)).collect();
        out.push(item(poly(&pts, false), "existing", [150, 104, 60, 255], None, 0.25));
    }
    if p.modified {
        for run in &p.proposed {
            let pts: Vec<DVec2> = run.iter().map(|(s, z)| pt(*s, *z)).collect();
            out.push(item(poly(&pts, false), "proposed", [30, 120, 50, 255], None, 0.5));
        }
    }
    for w in &p.walls {
        let r = [pt(w.s - w.width / 2.0, w.bottom), pt(w.s + w.width / 2.0, w.bottom), pt(w.s + w.width / 2.0, w.top), pt(w.s - w.width / 2.0, w.top)];
        out.push(item(poly(&r, true), "wall", ink, Some([168, 160, 150, 255]), 0.35));
    }
    for s in &p.slabs {
        let r = [pt(s.s0, s.top0 - s.thickness), pt(s.s1, s.top1 - s.thickness), pt(s.s1, s.top1), pt(s.s0, s.top0)];
        out.push(item(poly(&r, true), "hardscape", ink, Some([200, 200, 205, 255]), 0.25));
    }
    for pl in &p.plants {
        let trunk = 0.35 * pl.height;
        out.push(item(Shape::Line { a: pt(pl.s, pl.base), b: pt(pl.s, pl.base + trunk) }, "plant", [90, 60, 30, 255], None, 0.25));
        let (cz, rz) = (pl.base + trunk + (pl.height - trunk) / 2.0, (pl.height - trunk) / 2.0);
        let ring: Vec<DVec2> = (0..24).map(|k| pt(pl.s + pl.spread / 2.0 * (TAU * k as f64 / 24.0).cos(), cz + rz * (TAU * k as f64 / 24.0).sin())).collect();
        out.push(item(poly(&ring, true), "plant", [40, 120, 50, 255], Some([120, 190, 110, 160]), 0.25));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grading::testutil::*;
    use crate::model::tests::grid_site;
    use cadektor_core::command::Editor;
    use cadektor_core::model::ModifierKind;
    use proptest::prelude::*;
    use serde_json::json;

    fn section(ed: &mut Editor, a: (f64, f64), b: (f64, f64), extra: serde_json::Value) -> ObjectId {
        add_with(ed, Shape::Line { a: DVec2::new(a.0, a.1), b: DVec2::new(b.0, b.1) }, KEY, extra)
    }

    #[test]
    fn settings_are_checked() {
        assert!(SectionData::default().validate().is_ok());
        for bad in [
            SectionData { exaggeration: 0.0, ..SectionData::default() },
            SectionData { exaggeration: 100.0, ..SectionData::default() },
            SectionData { exaggeration: f64::NAN, ..SectionData::default() },
            SectionData { step: 1.0, ..SectionData::default() },
            SectionData { step: f64::INFINITY, ..SectionData::default() },
            SectionData { band: -1.0, ..SectionData::default() },
            SectionData { band: f64::NAN, ..SectionData::default() },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn clipping_a_segment_by_a_polygon() {
        let sq = [DVec2::new(2.0, -1.0), DVec2::new(4.0, -1.0), DVec2::new(4.0, 1.0), DVec2::new(2.0, 1.0)];
        let r = clip(&sq, DVec2::ZERO, DVec2::new(10.0, 0.0));
        assert_eq!(r.len(), 1);
        assert!((r[0].0 - 0.2).abs() < 1e-12 && (r[0].1 - 0.4).abs() < 1e-12);
        assert!(clip(&sq, DVec2::new(0.0, 5.0), DVec2::new(10.0, 5.0)).is_empty());
        assert!(clip(&sq[..2], DVec2::ZERO, DVec2::X).is_empty());
        // Starting inside, and a concave ring that cuts the segment twice.
        assert_eq!(clip(&sq, DVec2::new(3.0, 0.0), DVec2::new(10.0, 0.0)).len(), 1);
        let u = [DVec2::new(0.0, 0.0), DVec2::new(6.0, 0.0), DVec2::new(6.0, 4.0), DVec2::new(4.0, 4.0), DVec2::new(4.0, 1.0), DVec2::new(2.0, 1.0), DVec2::new(2.0, 4.0), DVec2::new(0.0, 4.0)];
        assert_eq!(clip(&u, DVec2::new(-1.0, 3.0), DVec2::new(7.0, 3.0)).len(), 2);
        assert!(cross(DVec2::ZERO, DVec2::X, DVec2::new(0.0, 1.0), DVec2::new(1.0, 1.0)).is_none());
    }

    #[test]
    fn profile_equals_sampling_the_surface() {
        let mut ed = Editor::default();
        let sid = add(&mut ed, grid_site(11, 1000.0, |x, y| (x / 7.0).sin() * 300.0 + y / 9.0));
        let sec = section(&mut ed, (500.0, 800.0), (9300.0, 8200.0), json!({}));
        let site = SiteResult::compute(ed.doc(), sid).unwrap();
        let p = profile(ed.doc(), ed.doc().object(sec).unwrap(), &site).unwrap();
        let (a, b) = (DVec2::new(500.0, 800.0), DVec2::new(9300.0, 8200.0));
        assert!((p.length - a.distance(b)).abs() < 1e-9);
        assert_eq!(p.existing.len(), 1);
        assert!(p.existing[0].len() > 10, "a point at every triangle edge");
        assert!(!p.modified && p.proposed == p.existing);
        for (s, z) in &p.existing[0] {
            let xy = a + (b - a) * (s / p.length);
            assert!((site.elevation_at(xy).unwrap() - z).abs() < 1e-6, "s={s}");
        }
        assert_eq!(p.existing[0][0].0, 0.0);
        assert!((p.existing[0].last().unwrap().0 - p.length).abs() < 1e-9);
        assert!(p.z_min <= p.z_max);
        // A section object without a length, or a plain line, has no profile.
        let zero = section(&mut ed, (1.0, 1.0), (1.0, 1.0), json!({}));
        assert!(profile(ed.doc(), ed.doc().object(zero).unwrap(), &site).is_none());
        let plain = add(&mut ed, Shape::Line { a: a, b: b });
        assert!(profile(ed.doc(), ed.doc().object(plain).unwrap(), &site).is_none());
        let circle = add(&mut ed, Shape::Circle { center: a, radius: 1.0 });
        assert!(profile(ed.doc(), ed.doc().object(circle).unwrap(), &site).is_none());
        assert!(data(ed.doc().object(circle).unwrap()).is_none());
    }

    proptest! {
        #[test]
        fn profile_points_are_on_the_ground(
            ax in 100.0f64..9900.0, ay in 100.0f64..9900.0, bx in 100.0f64..9900.0, by in 100.0f64..9900.0,
        ) {
            prop_assume!(DVec2::new(ax, ay).distance(DVec2::new(bx, by)) > 10.0);
            let mut ed = Editor::default();
            let sid = add(&mut ed, grid_site(11, 1000.0, |x, y| (x * y) / 5000.0 + x / 30.0));
            let sec = section(&mut ed, (ax, ay), (bx, by), json!({}));
            let site = SiteResult::compute(ed.doc(), sid).unwrap();
            let p = profile(ed.doc(), ed.doc().object(sec).unwrap(), &site).unwrap();
            let (a, b) = (DVec2::new(ax, ay), DVec2::new(bx, by));
            for (s, z) in &p.existing[0] {
                let xy = a.lerp(b, s / p.length);
                prop_assert!((site.elevation_at(xy).unwrap() - z).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn a_line_that_leaves_the_model_is_sampled_in_runs() {
        let mut ed = Editor::default();
        let sid = add(&mut ed, grid_site(5, 1000.0, |x, _| x / 10.0));
        let sec = section(&mut ed, (-2000.0, 1000.0), (6000.0, 1000.0), json!({"step": 500.0}));
        let site = SiteResult::compute(ed.doc(), sid).unwrap();
        let p = profile(ed.doc(), ed.doc().object(sec).unwrap(), &site).unwrap();
        assert_eq!(p.existing.len(), 1);
        let run = &p.existing[0];
        assert!((run[0].0 - 2000.0).abs() < 1e-9 && (run.last().unwrap().0 - 6000.0).abs() < 1e-9, "{run:?}");
        assert!(run.iter().all(|(s, z)| (z - (s - 2000.0) / 10.0).abs() < 1e-9));
        // A line that misses the model has no runs. One that enters twice has two.
        let miss = section(&mut ed, (-9000.0, 9000.0), (-5000.0, 9000.0), json!({}));
        assert!(profile(ed.doc(), ed.doc().object(miss).unwrap(), &site).unwrap().existing.is_empty());
        assert_eq!((profile(ed.doc(), ed.doc().object(miss).unwrap(), &site).unwrap().z_min, 0.0), (0.0, 0.0));
        let tin = &site.existing;
        let runs = sample(tin, DVec2::new(-1000.0, 1000.0), DVec2::new(5000.0, 1000.0), 250.0);
        assert_eq!(runs.len(), 1);
        let l = [DVec2::new(1000.0, -1000.0), DVec2::new(1000.0, 5000.0)];
        assert_eq!(sample(tin, l[0], l[1], 100.0).len(), 1);
    }

    #[test]
    fn walls_slabs_and_plants_are_cut() {
        let mut ed = Editor::default();
        let sid = add(&mut ed, grid_site(11, 1000.0, |_, _| 0.0));
        let b = cadektor_core::curve::corners(&[DVec2::new(5000.0, 3000.0), DVec2::new(6000.0, 3000.0), DVec2::new(6000.0, 7000.0), DVec2::new(5000.0, 7000.0)]);
        add(&mut ed, Shape::SiteModifier { modifier: ModifierKind::Pad { elevation: -400.0, slope: None }, boundary: b });
        let wd = wall::WallData { top: wall::Top::Fixed { elevation: 800.0 }, thickness: 300.0, ..wall::WallData::default() };
        add_with(&mut ed, path(&[(2000.0, 0.0), (2000.0, 10_000.0)]), wall::KEY, serde_json::to_value(&wd).unwrap());
        // A hardscape from x 7000 to 8000, 100 thick, raised 50 above the layer.
        let hb = cadektor_core::curve::corners(&[DVec2::new(7000.0, 4000.0), DVec2::new(8000.0, 4000.0), DVec2::new(8000.0, 6000.0), DVec2::new(7000.0, 6000.0)]);
        add(&mut ed, Shape::Hardscape { boundary: hb, material: "m".into(), thickness: Some(100.0), elevation: Some(50.0) });
        let near = add(&mut ed, Shape::Plant { at: DVec2::new(3500.0, 5500.0), species: "Oak".into(), spread: 2000.0, height: 6000.0 });
        add(&mut ed, Shape::Plant { at: DVec2::new(3500.0, 9500.0), species: "Far".into(), spread: 2000.0, height: 6000.0 });
        let sec = section(&mut ed, (0.0, 5000.0), (10_000.0, 5000.0), json!({"exaggeration": 3.0}));
        let site = SiteResult::compute(ed.doc(), sid).unwrap();
        let p = profile(ed.doc(), ed.doc().object(sec).unwrap(), &site).unwrap();
        assert!(p.modified);
        assert_eq!(p.walls.len(), 1);
        let w = p.walls[0];
        assert!((w.s - 2000.0).abs() < 1e-9 && (w.width - 300.0).abs() < 1e-9 && w.top == 800.0 && w.bottom == 0.0, "{w:?}");
        assert_eq!(p.slabs.len(), 1);
        assert!((p.slabs[0].s0 - 7000.0).abs() < 1e-9 && (p.slabs[0].s1 - 8000.0).abs() < 1e-9 && p.slabs[0].thickness == 100.0 && p.slabs[0].top0 == 50.0);
        assert_eq!(p.plants.len(), 1);
        assert_eq!((p.plants[0].id, p.plants[0].s, p.plants[0].offset), (near, 3500.0, 500.0));
        assert!(p.z_max >= 6000.0 && p.z_min <= -400.0);
        // The proposed run has the pad.
        let low = p.proposed[0].iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
        assert!((low + 400.0).abs() < 1e-6);
        // Objects can be left out.
        let sec2 = section(&mut ed, (0.0, 5000.0), (10_000.0, 5000.0), json!({"walls": false, "hardscapes": false, "plants": false}));
        let p2 = profile(ed.doc(), ed.doc().object(sec2).unwrap(), &site).unwrap();
        assert!(p2.walls.is_empty() && p2.slabs.is_empty() && p2.plants.is_empty());
        // The drawing: exaggeration 3, baseline 1 m under the lowest point.
        let d = SectionData { exaggeration: 3.0, ..SectionData::default() };
        let items = drawing(&p, &d, DVec2::new(0.0, -50_000.0), 250.0);
        let roles: Vec<&str> = items.iter().map(|i| i.role).collect();
        for r in ["axis", "label", "existing", "proposed", "wall", "hardscape", "plant"] {
            assert!(roles.contains(&r), "{r}");
        }
        let exist = items.iter().find(|i| i.role == "existing").unwrap();
        let Shape::Polyline { points, .. } = &exist.shape else { panic!() };
        // Baseline = floor(-400 / 1000) * 1000 - 1000 = -2000. Ground at 0 is 2000 above it, times 3.
        assert!((points[0].p.y - (-50_000.0 + 6000.0)).abs() < 1e-9, "{:?}", points[0]);
        let wall_item = items.iter().find(|i| i.role == "wall").unwrap();
        let Shape::Polyline { points, closed } = &wall_item.shape else { panic!() };
        assert!(*closed && (points[2].p.y - points[0].p.y - 800.0 * 3.0).abs() < 1e-9);
        assert!(items.iter().any(|i| matches!(&i.shape, Shape::Text { text, .. } if text.contains("Section A") && text.contains("x3"))));
        // Scaling to paper.
        let small: Vec<Item> = items.iter().map(|i| i.scaled(0.01)).collect();
        let Shape::Polyline { points, .. } = &small.iter().find(|i| i.role == "wall").unwrap().shape else { panic!() };
        assert!((points[2].p.y - points[0].p.y - 24.0).abs() < 1e-9);
        assert!(small.iter().any(|i| matches!(&i.shape, Shape::Text { height, .. } if (*height - 2.5).abs() < 1e-9)));
        assert!(matches!(Item { shape: Shape::Circle { center: DVec2::ZERO, radius: 1.0 }, ..items[0].clone() }.scaled(2.0).shape, Shape::Circle { .. }));
    }

    #[test]
    fn walls_at_a_slant_are_wider_and_unmodified_sites_draw_one_ground_line() {
        let mut ed = Editor::default();
        let sid = add(&mut ed, grid_site(11, 1000.0, |_, _| 0.0));
        let wd = wall::WallData { top: wall::Top::Fixed { elevation: 800.0 }, thickness: 300.0, ..wall::WallData::default() };
        add_with(&mut ed, path(&[(2000.0, 0.0), (6000.0, 10_000.0)]), wall::KEY, serde_json::to_value(&wd).unwrap());
        // A wall that runs along the section does not cut it.
        add_with(&mut ed, path(&[(0.0, 5000.0), (10_000.0, 5000.0)]), wall::KEY, serde_json::to_value(&wd).unwrap());
        let sec = section(&mut ed, (0.0, 5000.0), (10_000.0, 5000.0), json!({}));
        let site = SiteResult::compute(ed.doc(), sid).unwrap();
        let p = profile(ed.doc(), ed.doc().object(sec).unwrap(), &site).unwrap();
        assert_eq!(p.walls.len(), 1, "{:?}", p.walls);
        let sin = (10_000.0f64 / (4000.0f64.powi(2) + 10_000.0f64.powi(2)).sqrt()).abs();
        assert!((p.walls[0].width - 300.0 / sin).abs() < 1e-6);
        let items = drawing(&p, &SectionData::default(), DVec2::ZERO, 100.0);
        assert!(!items.iter().any(|i| i.role == "proposed"));
        assert_eq!(items.iter().filter(|i| i.role == "existing").count(), 1);
        assert_eq!(meters(1500.0, 1), "1.5");
    }
}
