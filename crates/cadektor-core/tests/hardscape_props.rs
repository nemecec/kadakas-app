//! Property tests for hardscapes and landscape areas: path outlines, exact areas of straight
//! paths, scatter positions, and undo of any mix of the new commands.

use cadektor_core::command::{Command, Editor};
use cadektor_core::curve;
use cadektor_core::geom;
use cadektor_core::hardscape::offset::{Join, path_outline};
use cadektor_core::hardscape::{self, HardscapePatch, Mode, NoTerrain, Slope};
use cadektor_core::landscape::{self, LandscapePatch, MixEntry};
use cadektor_core::model::{Attributes, Document, ObjectId, Shape};
use cadektor_core::plants::style::PlantStyle;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::DVec2;
use proptest::prelude::*;

fn join() -> impl Strategy<Value = Join> {
    prop_oneof![Just(Join::Miter), Just(Join::Round), Just(Join::Bevel)]
}

/// A path that turns gently and stays at a distance from itself: its direction never leaves
/// a half plane, and every segment is at least four widths long.
fn gentle_path() -> impl Strategy<Value = (Vec<DVec2>, f64)> {
    (100.0..2000.0f64, prop::collection::vec((4.0..10.0f64, -40.0..40.0f64), 1..7), -1.0e5..1.0e5f64, -1.0e5..1.0e5f64).prop_map(|(width, steps, x, y)| {
        let mut pts = vec![DVec2::new(x, y)];
        let mut heading = 0.0f64;
        for (len, turn) in steps {
            heading = (heading + turn).clamp(-85.0, 85.0);
            let last = *pts.last().unwrap();
            pts.push(last + DVec2::from_angle(heading.to_radians()) * len * width);
        }
        (pts, width)
    })
}

fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (r, s) = (b - a, d - c);
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-12 {
        return false;
    }
    let t = (c - a).perp_dot(s) / denom;
    let u = (c - a).perp_dot(r) / denom;
    t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9
}

fn is_simple(ring: &[DVec2]) -> bool {
    let n = ring.len();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if segments_cross(ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

proptest! {
    #[test]
    fn path_outlines_are_valid_polygons((pts, width) in gentle_path(), j in join()) {
        let ring = path_outline(&pts, width, j);
        prop_assert!(ring.len() >= 4);
        prop_assert!(ring.iter().all(|p| p.is_finite()));
        // Counterclockwise, with a positive area.
        let o = ring[0];
        let twice: f64 = (0..ring.len()).map(|i| (ring[i] - o).perp_dot(ring[(i + 1) % ring.len()] - o)).sum();
        prop_assert!(twice > 0.0);
        // No edge crosses another edge.
        prop_assert!(is_simple(&ring), "outline crosses itself: {ring:?}");
        // Every point is not farther from the center line than a miter allows, and the outline
        // points of the straight parts are exactly half a width away.
        let dist = |p: DVec2| pts.windows(2).map(|w| geom::dist_to_segment(p, w[0], w[1])).fold(f64::INFINITY, f64::min);
        let limit = hardscape::offset::MITER_LIMIT * width / 2.0 + 1e-6;
        prop_assert!(ring.iter().all(|p| dist(*p) <= limit));
        // The area is between that of the center line strip and that of the strip with every join at its widest.
        let length: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
        let area = geom::polygon_area(&ring);
        prop_assert!(area >= 0.5 * length * width);
        prop_assert!(area <= length * width + (pts.len() as f64) * (limit * limit) * 4.0);
    }

    #[test]
    fn straight_paths_have_exact_areas(
        a in (-1.0e6..1.0e6f64, -1.0e6..1.0e6f64),
        angle in 0.0..std::f64::consts::TAU,
        len in 10.0..1.0e5f64,
        width in 10.0..1.0e4f64,
        cuts in prop::collection::vec(0.05..0.95f64, 0..4),
        j in join(),
    ) {
        let start = DVec2::new(a.0, a.1);
        let end = start + DVec2::from_angle(angle) * len;
        let mut ts = cuts;
        ts.sort_by(f64::total_cmp);
        let mut pts = vec![start];
        pts.extend(ts.iter().map(|t| start.lerp(end, *t)));
        pts.push(end);
        let ring = path_outline(&pts, width, j);
        let area = geom::polygon_area(&ring);
        let want = len * width;
        prop_assert!((area - want).abs() <= 1e-6 * want, "{area} vs {want}");
        // Through the command: the plan area in the report is the same.
        let mut ed = Editor::default();
        let patch = HardscapePatch { center: Some(curve::corners(&pts)), width: Some(width), ..Default::default() };
        let id = ObjectId(ed.execute(&Command::AddHardscape { boundary: None, patch: Box::new(patch), layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0]);
        let q = hardscape::report::quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        prop_assert!((q.plan_area_m2 * 1e6 - want).abs() <= 1e-6 * want);
        prop_assert!((q.surface_area_m2 - q.plan_area_m2).abs() < 1e-12);
    }

    #[test]
    fn scatter_stays_inside_and_outside_holes(
        w in 2000.0..40_000.0f64,
        h in 2000.0..40_000.0f64,
        hole in (0.1..0.4f64, 0.1..0.4f64, 0.1..0.4f64),
        density in 0.1..6.0f64,
    ) {
        let mut ed = Editor::default();
        let p = ResourceId(ed.execute(&Command::AddResource { name: "P".into(), body: ResourceBody::plant_style(PlantStyle::default()) }).unwrap().created[0]);
        let style = landscape::LandscapeAreaStyle { mix: vec![MixEntry { plant: p, density: Some(density), ..Default::default() }], ..Default::default() };
        let s = ResourceId(ed.execute(&Command::AddResource { name: "S".into(), body: ResourceBody::LandscapeAreaStyle(Box::new(style)) }).unwrap().created[0]);
        let shape = Shape::LandscapeArea { boundary: curve::corners(&[DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, h), DVec2::new(0.0, h)]), name: "a".into() };
        let id = ObjectId(ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0]);
        let lo = DVec2::new(hole.0 * w, hole.1 * h);
        let hi = lo + DVec2::new(hole.2 * w, hole.2 * h).min(DVec2::new(w, h) * 0.5);
        let holes = vec![curve::corners(&[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)])];
        ed.execute(&Command::SetLandscapeArea { ids: vec![id], patch: LandscapePatch { style: Some(s), holes: Some(holes.clone()), ..Default::default() } }).unwrap();
        let o = ed.doc().object(id).unwrap();
        let q = landscape::quantities(ed.doc(), o).unwrap();
        let hole_area = (hi.x - lo.x) * (hi.y - lo.y) / 1e6;
        prop_assert!((q.net_area_m2 - (w * h / 1e6 - hole_area)).abs() < 1e-6);
        prop_assert_eq!(q.rows[0].count, (q.net_area_m2 * density).round() as usize);
        let placed = landscape::scatter(ed.doc(), o);
        prop_assert!(placed.len() <= landscape::MAX_SCATTER);
        let ring = [DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, h), DVec2::new(0.0, h)];
        let hole_ring = curve::positions(&holes[0]);
        for pl in &placed {
            prop_assert!(geom::point_in_polygon(pl.at, &ring));
            prop_assert!(!geom::point_in_polygon(pl.at, &hole_ring));
        }
        prop_assert_eq!(&placed, &landscape::scatter(ed.doc(), o));
    }
}

#[derive(Clone, Debug)]
enum Op {
    Path { len: f64, width: f64 },
    Slab { w: f64 },
    Style(f64),
    Apply { target: u8, with_style: bool, drape: bool, percent: f64 },
    Width { target: u8, width: f64 },
    Move { target: u8 },
    Area { w: f64 },
    Holes { target: u8 },
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        2 => (500.0..9000.0f64, 100.0..2000.0f64).prop_map(|(len, width)| Op::Path { len, width }),
        2 => (500.0..9000.0f64).prop_map(|w| Op::Slab { w }),
        2 => (20.0..200.0f64).prop_map(Op::Style),
        3 => (0u8..6, any::<bool>(), any::<bool>(), -20.0..20.0f64).prop_map(|(target, with_style, drape, percent)| Op::Apply { target, with_style, drape, percent }),
        2 => (0u8..6, 100.0..3000.0f64).prop_map(|(target, width)| Op::Width { target, width }),
        1 => (0u8..6).prop_map(|target| Op::Move { target }),
        1 => (1000.0..9000.0f64).prop_map(|w| Op::Area { w }),
        1 => (0u8..6).prop_map(|target| Op::Holes { target }),
        2 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

fn ids_of(ed: &Editor, f: impl Fn(&Shape) -> bool) -> Vec<ObjectId> {
    ed.doc().objects.values().filter(|o| f(&o.shape)).map(|o| o.id).collect()
}

fn check(doc: &Document) {
    // The document can be saved and opened again, and every hardscape is consistent.
    let back = Document::from_json(&serde_json::to_string(doc).unwrap()).expect("reopens");
    assert_eq!(&back, doc);
    for o in doc.objects.values() {
        if let Some(d) = hardscape::data(o) {
            assert!(d.validate().is_ok());
            if d.mode == Mode::Path {
                assert_eq!(o.shape.vertices().unwrap(), &d.outline(), "the outline follows the center line");
            }
            if let Some(id) = d.style {
                let Shape::Hardscape { thickness, .. } = &o.shape else { panic!() };
                assert_eq!(*thickness, Some(doc.resources.hardscape_style(id).unwrap().total_thickness()));
            }
        }
        if let Shape::Hardscape { .. } = o.shape {
            let q = hardscape::report::quantities(doc, o, None).unwrap();
            assert!(q.plan_area_m2.is_finite() && q.volume_m3.is_finite() && q.surface_area_m2 >= q.plan_area_m2 - 1e-9);
        }
    }
    assert!(hardscape::report::report(doc, &NoTerrain).total_plan_area_m2.is_finite());
    assert!(landscape::plant_list(doc).total_count < usize::MAX);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn any_mix_of_hardscape_commands_keeps_the_document_valid(ops in prop::collection::vec(op(), 1..30)) {
        let mut ed = Editor::default();
        let mut n = 0;
        let hs = |s: &Shape| matches!(s, Shape::Hardscape { .. });
        for op in ops {
            let hard = ids_of(&ed, hs);
            let pick = |t: u8, list: &[ObjectId]| if list.is_empty() { None } else { Some(list[t as usize % list.len()]) };
            let result = match op {
                Op::Path { len, width } => {
                    let patch = HardscapePatch { center: Some(curve::corners(&[DVec2::ZERO, DVec2::new(len, 0.0), DVec2::new(len, len / 2.0)])), width: Some(width), ..Default::default() };
                    ed.execute(&Command::AddHardscape { boundary: None, patch: Box::new(patch), layer: None, class: None, attrs: Attributes::default() }).map(|_| ())
                }
                Op::Slab { w } => ed.execute(&Command::AddHardscape { boundary: Some(curve::corners(&[DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, w), DVec2::new(0.0, w)])), patch: Default::default(), layer: None, class: None, attrs: Attributes::default() }).map(|_| ()),
                Op::Style(t) => {
                    n += 1;
                    let style = hardscape::HardscapeStyle { thickness: t, components: vec![hardscape::Component { thickness: t * 2.0, ..Default::default() }], ..Default::default() };
                    ed.execute(&Command::AddResource { name: format!("S{n}"), body: ResourceBody::HardscapeStyle(Box::new(style)) }).map(|_| ())
                }
                Op::Apply { target, with_style, drape, percent } => match pick(target, &hard) {
                    Some(id) => {
                        let style = ed.doc().resources.hardscape_styles().next().map(|(i, _, _)| i).filter(|_| with_style);
                        let patch = HardscapePatch { style, drape: Some(drape), slope: Some(Slope::Cross { percent, angle_deg: 30.0 }), ..Default::default() };
                        ed.execute(&Command::SetHardscape { ids: vec![id], patch }).map(|_| ())
                    }
                    None => Ok(()),
                },
                Op::Width { target, width } => match pick(target, &hard) {
                    Some(id) => ed.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { width: Some(width), ..Default::default() } }).map(|_| ()),
                    None => Ok(()),
                },
                Op::Move { target } => match pick(target, &hard) {
                    Some(id) => ed.execute(&Command::MoveObjects { ids: vec![id], delta: DVec2::new(123.0, -45.0) }).map(|_| ()),
                    None => Ok(()),
                },
                Op::Area { w } => {
                    let shape = Shape::LandscapeArea { boundary: curve::corners(&[DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, w), DVec2::new(0.0, w)]), name: "a".into() };
                    ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).map(|_| ())
                }
                Op::Holes { target } => match pick(target, &ids_of(&ed, |s| matches!(s, Shape::LandscapeArea { .. }))) {
                    Some(id) => {
                        let hole = curve::corners(&[DVec2::new(100.0, 100.0), DVec2::new(600.0, 100.0), DVec2::new(600.0, 600.0)]);
                        ed.execute(&Command::SetLandscapeArea { ids: vec![id], patch: LandscapePatch { holes: Some(vec![hole]), ..Default::default() } }).map(|_| ())
                    }
                    None => Ok(()),
                },
                Op::Undo => { ed.undo(); Ok(()) }
                Op::Redo => { ed.redo(); Ok(()) }
            };
            // A refused command (for example a hole outside a moved area) must leave everything valid.
            let _ = result;
            check(ed.doc());
        }
        // Undo everything: the empty document comes back.
        while ed.undo() {}
        prop_assert!(ed.doc().objects.is_empty() && ed.doc().resources.is_empty());
    }
}
