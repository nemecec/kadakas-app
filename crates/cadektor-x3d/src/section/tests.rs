//! Tests of the cut: boxes, flat surfaces, plants, hiding, clipping and the cache.

use glam::{DVec2, DVec3};
use proptest::prelude::*;

use cadektor_core::command::{Command, Editor};
use cadektor_core::mesh;
use cadektor_core::model::{Attributes, Document, Shape};
use cadektor_core::section::{Look, Mode};

use super::*;
use crate::tests::{document, plant, slab};

fn spec(a: (f64, f64), b: (f64, f64)) -> SectionSpec {
    SectionSpec {
        a: DVec2::new(a.0, a.1),
        b: DVec2::new(b.0, b.1),
        look: Look::Left,
        mode: Mode::Cut,
        depth: None,
        beyond: true,
    }
}

fn run(doc: &Document, s: &SectionSpec) -> SectionDrawing {
    compute(doc, &mesh::build(doc), s)
}

fn bounds_of(f: &CutFace) -> (DVec2, DVec2) {
    let pts = f.rings.iter().flatten();
    pts.fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(l, h), p| (l.min(*p), h.max(*p)),
    )
}

#[test]
fn cutting_a_box_gives_the_expected_rectangle() {
    // A slab 2000 by 1000 mm, 300 mm thick, top at height 0. The plane runs along x at y = 500.
    let doc = document(vec![slab(0.0, 0.0, 2000.0, 1000.0, 300.0, 0.0)]);
    let d = run(&doc, &spec((-1000.0, 500.0), (4000.0, 500.0)));
    assert_eq!(d.length, 5000.0);
    assert_eq!(d.faces.len(), 1);
    assert!(d.profiles.is_empty() && d.silhouettes.is_empty());
    let f = &d.faces[0];
    assert_eq!(f.rings.len(), 1);
    assert_eq!(f.rings[0].len(), 4, "a rectangle: {:?}", f.rings[0]);
    let (lo, hi) = bounds_of(f);
    // u counts from the first point: x = 0 is at u = 1000.
    assert!(
        (lo.x - 1000.0).abs() < 0.1 && (hi.x - 3000.0).abs() < 0.1,
        "{lo} {hi}"
    );
    assert!((lo.y + 300.0).abs() < 0.1 && hi.y.abs() < 0.1, "{lo} {hi}");
    assert!((d.cut_area() - 2000.0 * 300.0).abs() < 1.0);
    assert!(cut::area2(&f.rings[0]) > 0.0, "outer ring counterclockwise");
    assert_eq!(d.bounds.unwrap().0.x.round(), 1000.0);
}

#[test]
fn looking_right_mirrors_the_section() {
    let doc = document(vec![slab(0.0, 0.0, 2000.0, 1000.0, 300.0, 0.0)]);
    let mut s = spec((-1000.0, 500.0), (4000.0, 500.0));
    s.look = Look::Right;
    let d = run(&doc, &s);
    let (lo, hi) = bounds_of(&d.faces[0]);
    // The viewer looks the other way: u starts at the second point, so x = 0..2000 is u = 2000..4000.
    assert!(
        (lo.x - 2000.0).abs() < 0.1 && (hi.x - 4000.0).abs() < 0.1,
        "{lo} {hi}"
    );
    assert!((d.cut_area() - 600_000.0).abs() < 1.0);
}

#[test]
fn the_cut_is_limited_to_the_length_of_the_line() {
    let doc = document(vec![slab(0.0, 0.0, 2000.0, 1000.0, 300.0, 0.0)]);
    let d = run(&doc, &spec((500.0, 500.0), (1500.0, 500.0)));
    assert!((d.cut_area() - 1000.0 * 300.0).abs() < 1.0);
    let (lo, hi) = bounds_of(&d.faces[0]);
    assert!(lo.x >= -1e-9 && hi.x <= 1000.0 + 1e-9);
    // A line beside the slab cuts nothing.
    let d = run(&doc, &spec((3000.0, 500.0), (5000.0, 500.0)));
    assert!(d.faces.is_empty());
}

#[test]
fn a_plane_through_a_vertex_or_an_edge_still_closes_the_ring() {
    let doc = document(vec![slab(0.0, 0.0, 2000.0, 1000.0, 300.0, 0.0)]);
    // Through the side face (an edge) and along a face of the slab.
    for y in [0.0, 1000.0, 500.0, 0.0001] {
        let d = run(&doc, &spec((-100.0, y), (2100.0, y)));
        for f in &d.faces {
            assert!(f.rings.iter().all(|r| r.len() >= 3));
        }
    }
    // A plane through the middle of a 45 degree line also works (a diagonal cut).
    let d = run(&doc, &spec((-500.0, -500.0), (2500.0, 1500.0)));
    assert_eq!(d.faces.len(), 1);
    assert!(d.cut_area() > 0.0);
}

#[test]
fn a_flat_surface_gives_a_profile_line() {
    let area = Shape::LandscapeArea {
        boundary: crate::tests::rect_ring(0.0, 0.0, 6000.0, 4000.0),
        name: String::new(),
    };
    let doc = document(vec![area]);
    let d = run(&doc, &spec((1000.0, 2000.0), (5000.0, 2000.0)));
    assert!(d.faces.is_empty());
    assert_eq!(d.profiles.len(), 1);
    let p = &d.profiles[0].points;
    assert_eq!(p.len(), 2);
    assert!(p.iter().all(|q| q.y.abs() < 1e-6));
    let xs: Vec<f64> = p.iter().map(|q| q.x).collect();
    assert!(xs.contains(&0.0) || xs.iter().any(|x| x.abs() < 1e-6));
    assert!(xs.iter().any(|x| (x - 4000.0).abs() < 1e-6));
}

#[test]
fn plants_are_silhouettes_and_do_not_hide_with_a_hatch() {
    let doc = document(vec![plant(2000.0, 500.0, "Acer")]);
    let d = run(&doc, &spec((0.0, 500.0), (4000.0, 500.0)));
    // The trunk is a solid: it gives a closed cut face, with no loose profile lines. The canopy
    // only gives a silhouette.
    assert_eq!(d.faces.len(), 1);
    let (tlo, thi) = bounds_of(&d.faces[0]);
    assert!(thi.x - tlo.x < 700.0 && thi.y < 6000.0 * 0.7, "{tlo} {thi}");
    assert!(d.profiles.is_empty(), "{:?}", d.profiles);
    assert_eq!(d.silhouettes.len(), 1);
    let (lo, hi) = bounds_of(&d.silhouettes[0]);
    // The canopy is 3000 wide at its widest and the plant is 6000 tall.
    assert!(
        hi.x - lo.x > 2500.0 && hi.x - lo.x <= 3000.0 + 1.0,
        "{lo} {hi}"
    );
    assert!(hi.y > 5000.0 && hi.y <= 6000.0 + 1.0);
}

#[test]
fn elevation_draws_only_the_visible_edges() {
    // A box in front, and a smaller one behind it that the first hides completely.
    let front = slab(0.0, 1000.0, 2000.0, 2000.0, 300.0, 0.0);
    let hidden = slab(500.0, 3000.0, 1500.0, 4000.0, 100.0, -100.0);
    let doc = document(vec![front, hidden]);
    let ids: Vec<_> = doc.objects.keys().copied().collect();
    let mut s = spec((-1000.0, 0.0), (4000.0, 0.0));
    s.mode = Mode::Elevation;
    let d = run(&doc, &s);
    assert!(d.faces.is_empty() && d.profiles.is_empty());
    assert!(!d.edges.is_empty());
    assert!(
        d.edges.iter().all(|e| e.object == ids[0]),
        "the box behind is hidden"
    );
    let (lo, hi) = d.bounds.unwrap();
    assert!(
        (lo.x - 1000.0).abs() < 0.1 && (hi.x - 3000.0).abs() < 0.1,
        "{lo} {hi}"
    );
    assert!((lo.y + 300.0).abs() < 0.1 && hi.y.abs() < 0.1);
    // Edges lie in front of the far clip only.
    assert!(d.edges.iter().all(|e| e.depth >= 1000.0 - 1e-6));
    // Without the first box, the second one shows.
    let doc2 = document(vec![slab(500.0, 3000.0, 1500.0, 4000.0, 100.0, -100.0)]);
    let d2 = run(&doc2, &s);
    assert!(!d2.edges.is_empty());
}

#[test]
fn the_far_clip_removes_distant_objects() {
    let doc = document(vec![
        slab(0.0, 1000.0, 2000.0, 2000.0, 300.0, 0.0),
        slab(3000.0, 6000.0, 4000.0, 7000.0, 300.0, 0.0),
    ]);
    let ids: Vec<_> = doc.objects.keys().copied().collect();
    let mut s = spec((-1000.0, 0.0), (5000.0, 0.0));
    s.mode = Mode::Elevation;
    let all = run(&doc, &s);
    assert!(all.edges.iter().any(|e| e.object == ids[1]));
    s.depth = Some(3000.0);
    let near = run(&doc, &s);
    assert!(near.edges.iter().all(|e| e.object == ids[0]));
    assert!(!near.edges.is_empty());
}

#[test]
fn objects_in_front_of_the_plane_are_not_drawn() {
    let doc = document(vec![slab(0.0, -3000.0, 2000.0, -2000.0, 300.0, 0.0)]);
    let mut s = spec((-1000.0, 0.0), (4000.0, 0.0));
    let d = run(&doc, &s);
    assert!(d.is_empty() && d.bounds.is_none());
    s.mode = Mode::Elevation;
    assert!(run(&doc, &s).is_empty());
    // Looking the other way, the same object is beyond.
    s.look = Look::Right;
    assert!(!run(&doc, &s).edges.is_empty());
}

#[test]
fn the_cut_face_hides_what_is_behind_it() {
    // The plane cuts the big slab. A smaller slab right behind it lies in the shadow of the cut.
    let doc = document(vec![
        slab(0.0, 0.0, 2000.0, 2000.0, 300.0, 0.0),
        slab(500.0, 3000.0, 1500.0, 4000.0, 100.0, -100.0),
    ]);
    let ids: Vec<_> = doc.objects.keys().copied().collect();
    let d = run(&doc, &spec((-1000.0, 1000.0), (4000.0, 1000.0)));
    assert_eq!(d.faces.len(), 1);
    assert!(d.edges.iter().all(|e| e.object != ids[1]), "{:?}", d.edges);
    // With the objects beyond switched off there are no edges at all.
    let mut s = spec((-1000.0, 1000.0), (4000.0, 1000.0));
    s.beyond = false;
    assert!(run(&doc, &s).edges.is_empty());
}

#[test]
fn section_lines_do_not_cut_themselves() {
    let mut ed = Editor::default();
    ed.execute(&Command::AddObject {
        shape: slab(0.0, 0.0, 2000.0, 1000.0, 300.0, 0.0),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::new(-1000.0, 500.0),
                b: DVec2::new(4000.0, 500.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let id = cadektor_core::model::ObjectId(out.created[0]);
    ed.execute(&Command::SetData {
        id,
        set: [("section_line".to_string(), serde_json::json!({"name": "A"}))].into(),
        remove: vec![],
    })
    .unwrap();
    let doc = ed.doc().clone();
    let s = SectionSpec::from_line(doc.object(id).unwrap()).unwrap();
    assert_eq!(s.length(), 5000.0);
    let d = run(&doc, &s);
    assert_eq!(d.faces.len(), 1);
    assert!(d.edges.iter().all(|e| e.object != id));
    assert!(SectionSpec::from_line(doc.objects.values().next().unwrap()).is_none());
}

#[test]
fn bad_specs_give_empty_drawings() {
    let doc = document(vec![slab(0.0, 0.0, 2000.0, 1000.0, 300.0, 0.0)]);
    let mut s = spec((0.0, 0.0), (0.5, 0.0));
    assert!(s.problem().unwrap().contains("too short"));
    assert!(run(&doc, &s).is_empty());
    s = spec((f64::NAN, 0.0), (10.0, 0.0));
    assert!(s.problem().unwrap().contains("finite"));
    s = spec((0.0, 0.0), (1000.0, 0.0));
    s.depth = Some(-1.0);
    assert!(s.problem().unwrap().contains("depth"));
    assert!(run(&doc, &s).is_empty());
    s.depth = None;
    assert!(s.problem().is_none());
}

#[test]
fn lines_without_surfaces_show_beyond() {
    let doc = document(vec![Shape::Polyline3d {
        points: vec![
            DVec3::new(500.0, 2000.0, 0.0),
            DVec3::new(1500.0, 2000.0, 1000.0),
        ],
        closed: false,
    }]);
    let d = run(&doc, &spec((0.0, 0.0), (2000.0, 0.0)));
    assert_eq!(d.edges.len(), 1);
    assert!((d.edges[0].depth - 2000.0).abs() < 1e-6);
}

/// A hip-roofed block 8000 by 6000 mm, eaves at 3000 mm, whose roof peaks 4000 mm behind the plane.
fn hip_house() -> Document {
    use cadektor_core::massing::{Massing, RoofStyle};
    let ring = [
        (0.0, 1000.0),
        (8000.0, 1000.0),
        (8000.0, 7000.0),
        (0.0, 7000.0),
    ];
    let mut m = Massing::new(
        ring.iter()
            .map(|(x, y)| cadektor_core::model::Vertex::corner(DVec2::new(*x, *y)))
            .collect(),
        3000.0,
    );
    m.roof = RoofStyle::Hip;
    m.pitch_deg = 30.0;
    m.base = Some(0.0);
    document(vec![Shape::Massing(Box::new(m))])
}

#[test]
fn the_far_clip_cuts_triangles_at_the_limit() {
    let doc = hip_house();
    let top = |d: &SectionDrawing| {
        d.edges
            .iter()
            .map(|e| e.a.y.max(e.b.y))
            .fold(f64::MIN, f64::max)
    };
    let mut s = spec((-1000.0, 0.0), (9000.0, 0.0));
    s.mode = Mode::Elevation;
    let all = run(&doc, &s);
    assert!(top(&all) > 4500.0, "the peak: {}", top(&all));
    // The block starts 1000 mm behind the line. At 2000 mm only the first 1000 mm of the roof
    // slope shows: it is lower than the eaves plus the slope, and no line rises above it.
    s.depth = Some(2000.0);
    let near = run(&doc, &s);
    let slope = 3000.0 + 1000.0 * 30f64.to_radians().tan();
    assert!(!near.edges.is_empty());
    assert!(top(&near) <= slope + 1.0, "{} > {slope}", top(&near));
    assert!(near.edges.iter().all(|e| e.depth <= 2000.0 + 1e-6));
}

#[test]
fn triangles_outside_the_length_are_not_drawn_and_the_cut_stays_closed() {
    let doc = document(vec![
        slab(0.0, 0.0, 9000.0, 1000.0, 300.0, 0.0),
        slab(20_000.0, 2000.0, 22_000.0, 3000.0, 300.0, 0.0),
    ]);
    let ids: Vec<_> = doc.objects.keys().copied().collect();
    // The line is shorter than the first slab: its ring is clipped, not left open.
    let d = run(&doc, &spec((1000.0, 500.0), (3000.0, 500.0)));
    assert_eq!(d.faces.len(), 1);
    assert!(d.profiles.is_empty());
    assert!((d.cut_area() - 2000.0 * 300.0).abs() < 1.0);
    assert!(d.edges.iter().all(|e| e.object != ids[1]));
    assert!(
        d.edges
            .iter()
            .all(|e| e.a.x >= -1e-6 && e.b.x <= 2000.0 + 1e-6)
    );
}

#[test]
fn the_cache_builds_once_per_revision_and_key() {
    let mut c = SectionCache::default();
    let make = || SectionDrawing {
        length: 7.0,
        ..SectionDrawing::default()
    };
    let a = c.get(1, 10, make);
    let b = c.get(1, 10, || panic!("must be cached"));
    assert!(Arc::ptr_eq(&a, &b));
    c.get(1, 11, make);
    assert_eq!(c.builds, 2);
    c.get(2, 10, make);
    assert_eq!(c.builds, 3);
    c.clear();
    c.get(2, 10, make);
    assert_eq!(c.builds, 4);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn cutting_a_box_always_gives_its_area(
        x0 in -5000.0f64..5000.0, y0 in -5000.0f64..5000.0,
        w in 500.0f64..6000.0, h in 500.0f64..6000.0,
        t in 50.0f64..800.0, angle in 0.0f64..3.0, frac in 0.1f64..0.9,
    ) {
        // A line at any angle through a point inside the box, long enough to cross it.
        let doc = document(vec![slab(x0, y0, x0 + w, y0 + h, t, 0.0)]);
        let c = DVec2::new(x0 + w * frac, y0 + h * frac);
        let dir = DVec2::from_angle(angle);
        let s = SectionSpec {
            a: c - dir * 20_000.0,
            b: c + dir * 20_000.0,
            look: Look::Left,
            mode: Mode::Cut,
            depth: None,
            beyond: false,
        };
        let d = compute(&doc, &mesh::build(&doc), &s);
        prop_assert_eq!(d.faces.len(), 1);
        // The chord of the rectangle along the line, times the thickness.
        let chord = chord(c, dir, x0, y0, w, h);
        prop_assert!((d.cut_area() - chord * t).abs() < 1.0 + chord * t * 1e-4, "area {} expected {}", d.cut_area(), chord * t);
        let (lo, hi) = bounds_of(&d.faces[0]);
        prop_assert!((hi.y - lo.y - t).abs() < 0.01);
    }
}

/// Length of the part of the line through `c` with direction `dir` that lies inside the rectangle.
fn chord(c: DVec2, dir: DVec2, x0: f64, y0: f64, w: f64, h: f64) -> f64 {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for (p, lo, hi, d) in [(c.x, x0, x0 + w, dir.x), (c.y, y0, y0 + h, dir.y)] {
        if d.abs() < 1e-12 {
            continue;
        }
        let (a, b) = ((lo - p) / d, (hi - p) / d);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    t1 - t0
}
