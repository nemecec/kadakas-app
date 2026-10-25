//! Tests of Follow Me (`Edit::FollowMe`, MOD-08).

use glam::DVec3;
use proptest::prelude::*;

use super::*;

type P = [f64; 3];

fn volume(m: &SurfaceModel) -> f64 {
    m.faces()
        .map(|(id, f)| {
            let p0 = m.vertex(f.outer[0]).unwrap();
            m.face_area(id).unwrap() * f.plane.normal.dot(p0) / 3.0
        })
        .sum()
}

fn edge_ids_between(m: &SurfaceModel, pts: &[P]) -> Vec<EdgeId> {
    let vid = |p: P| {
        m.vertices()
            .find(|(_, q)| q.distance(DVec3::from(p)) < 1e-6)
            .map(|(id, _)| id)
            .unwrap_or_else(|| panic!("no vertex at {p:?}"))
    };
    pts.windows(2)
        .map(|w| m.edge_between(vid(w[0]), vid(w[1])).unwrap())
        .collect()
}

/// A square profile of side `s` centred on the origin in the plane x = 0, and the path `path`
/// (a polyline from the origin). Returns the model, the profile and the path edges.
fn square_on(path: &[P], s: f64) -> (SurfaceModel, FaceId, Vec<EdgeId>) {
    let mut m = SurfaceModel::new();
    let h = s / 2.0;
    let sq = [[0.0, -h, -h], [0.0, h, -h], [0.0, h, h], [0.0, -h, h]];
    m.apply(&Edit::AddEdges {
        edges: (0..4).map(|i| [sq[i], sq[(i + 1) % 4]]).collect(),
    })
    .unwrap();
    let profile = m.faces().next().unwrap().0;
    m.apply(&Edit::AddEdges {
        edges: path.windows(2).map(|w| [w[0], w[1]]).collect(),
    })
    .unwrap();
    let es = edge_ids_between(&m, path);
    (m, profile, es)
}

fn follow(
    m: &mut SurfaceModel,
    face: FaceId,
    path: &[EdgeId],
) -> Result<Vec<String>, SurfaceError> {
    let r = m.apply_with_notes(&Edit::FollowMe {
        face,
        path: path.to_vec(),
    });
    if r.is_ok() {
        let rep = m.check();
        assert!(rep.is_valid(), "{:?}", rep.problems);
    }
    r
}

fn soft_count(m: &SurfaceModel) -> usize {
    m.edges()
        .filter(|(id, _)| {
            flagged(m.edge_attrs(*id), ATTR_SOFT) && flagged(m.edge_attrs(*id), ATTR_SMOOTH)
        })
        .count()
}

#[test]
fn a_square_along_an_l_makes_a_closed_mitred_solid() {
    let path = [[0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], [2000.0, 1000.0, 0.0]];
    let (mut m, profile, es) = square_on(&path, 200.0);
    let rev = m.rev();
    follow(&mut m, profile, &es).unwrap();
    assert_eq!(m.rev(), rev + 1);
    let r = m.check();
    // The path stays as edges inside the solid: wire edges, as the check says.
    assert_eq!(r.wire_edges.len(), 2, "{r:?}");
    // 4 sides x 2 path sides + 2 caps; the top and the bottom of the L are one face each (new
    // faces side by side in one plane merge).
    assert_eq!(m.face_count(), 8);
    assert!(
        m.face(profile).is_some(),
        "the profile stays as the first cap"
    );
    // It looks away from the sweep (the path goes along +x).
    assert!(m.face(profile).unwrap().plane.normal.x < -0.9);
    // The volume: the area times the length of the path through the middle of the profile.
    assert!(
        (volume(&m) - 200.0 * 200.0 * 3000.0).abs() < 1e-3,
        "{}",
        volume(&m)
    );
    // The mitre at the corner: the corner points lie on the plane x - y = 2000 - 0 ... at 45°.
    assert!(
        m.vertices()
            .any(|(_, p)| p.distance(DVec3::new(2100.0, -100.0, 100.0)) < 1e-6)
    );
    assert!(
        m.vertices()
            .any(|(_, p)| p.distance(DVec3::new(1900.0, 100.0, 100.0)) < 1e-6)
    );
    // Without the path the solid is closed.
    m.apply(&Edit::EraseEdges { edges: es }).unwrap();
    assert!(m.check().is_closed(), "{:?}", m.check());
}

#[test]
fn the_profile_at_the_far_end_sweeps_from_there() {
    // The path drawn from the other end: the sweep still starts at the profile.
    let path = [[2000.0, 1000.0, 0.0], [2000.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
    let (mut m, profile, es) = square_on(&path, 200.0);
    follow(&mut m, profile, &es).unwrap();
    assert!((volume(&m) - 200.0 * 200.0 * 3000.0).abs() < 1e-3);
}

fn circle(n: usize, r: f64, z: f64) -> Vec<P> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [r * a.cos(), r * a.sin(), z]
        })
        .collect()
}

#[test]
fn a_profile_around_a_closed_circle_makes_a_ring_without_caps() {
    let mut m = SurfaceModel::new();
    // The path: a circle of 24 sides, radius 1000, as one curve (no face: drawn in the air).
    let pts = circle(24, 1000.0, 500.0);
    m.apply(&Edit::AddCurve {
        points: pts.clone(),
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    let disc = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![disc] }).unwrap();
    let path: Vec<EdgeId> = m.edges().map(|(id, _)| id).collect();
    // A square profile in the plane y = 0 around the vertex (1000, 0, 500).
    let sq = [
        [900.0, 0.0, 400.0],
        [1100.0, 0.0, 400.0],
        [1100.0, 0.0, 600.0],
        [900.0, 0.0, 600.0],
    ];
    m.apply(&Edit::AddEdges {
        edges: (0..4).map(|i| [sq[i], sq[(i + 1) % 4]]).collect(),
    })
    .unwrap();
    let profile = m.faces().next().unwrap().0;
    let notes = follow(&mut m, profile, &path).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert!(m.face(profile).is_none(), "no caps");
    // 24 outer and 24 inner sides; the top and the bottom are flat rings, one face each.
    assert_eq!(m.face_count(), 24 * 2 + 2);
    let r = m.check();
    assert_eq!(r.wire_edges.len(), 24, "the path");
    assert!(
        r.border_edges.is_empty() && r.nonmanifold_edges.is_empty(),
        "{r:?}"
    );
    // The copies of the profile at the corners of the curve are soft and smooth (the flat ones
    // went with the merge).
    assert_eq!(soft_count(&m), 24 * 2);
    // Each point of the profile runs along the curve as a curve.
    assert!(m.curves().filter(|(_, es)| es.len() == 24).count() >= 5);
}

#[test]
fn a_lathe_about_an_axis_turns_a_rectangle_into_a_cylinder() {
    let mut m = SurfaceModel::new();
    let pts = circle(24, 500.0, 0.0);
    m.apply(&Edit::AddCurve {
        points: pts,
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    let disc = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![disc] }).unwrap();
    let path: Vec<EdgeId> = m.edges().map(|(id, _)| id).collect();
    // The half section: from the axis to the path, 300 high, in the plane y = 0.
    let sec = [
        [0.0, 0.0, 0.0],
        [500.0, 0.0, 0.0],
        [500.0, 0.0, 300.0],
        [0.0, 0.0, 300.0],
    ];
    m.apply(&Edit::AddEdges {
        edges: (0..4).map(|i| [sec[i], sec[(i + 1) % 4]]).collect(),
    })
    .unwrap();
    let profile = m.faces().next().unwrap().0;
    follow(&mut m, profile, &path).unwrap();
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    // The top and the bottom are one face each (the fans merge), and 24 sides.
    assert_eq!(m.face_count(), 26);
    // A 24-gon prism of radius 500, 300 high.
    let area = 0.5 * 24.0 * 500.0 * 500.0 * (std::f64::consts::TAU / 24.0).sin();
    assert!(
        (volume(&m).abs() - area * 300.0).abs() < 1.0,
        "{}",
        volume(&m)
    );
}

#[test]
fn a_round_profile_along_a_path_shades_round_and_keeps_its_curves() {
    let mut m = SurfaceModel::new();
    let ring: Vec<P> = (0..12)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 12.0;
            [0.0, 100.0 * a.cos(), 100.0 * a.sin()]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: ring,
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    let profile = m.faces().next().unwrap().0;
    let path = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 1000.0, 0.0]];
    m.apply(&Edit::AddEdges {
        edges: path.windows(2).map(|w| [w[0], w[1]]).collect(),
    })
    .unwrap();
    let es = edge_ids_between(&m, &path);
    follow(&mut m, profile, &es).unwrap();
    // 12 x 2 sides, 2 caps; the 12 edges along each side of the path are soft and smooth.
    assert_eq!(m.face_count(), 26);
    assert_eq!(soft_count(&m), 24);
    // The ring at the corner and at the end are curves, and the profile's own.
    assert_eq!(m.curves().filter(|(_, es)| es.len() == 12).count(), 3);
}

#[test]
fn the_new_faces_take_the_paint_of_the_profile() {
    let path = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]];
    let (mut m, profile, es) = square_on(&path, 200.0);
    m.apply(&Edit::SetFaceMaterial {
        faces: vec![profile],
        side: PaintSide::Front,
        material: Some("stone".into()),
    })
    .unwrap();
    let front_along = m.face(profile).unwrap().plane.normal.x > 0.0;
    follow(&mut m, profile, &es).unwrap();
    let walls = m
        .faces()
        .filter(|(_, f)| f.plane.normal.x.abs() < 1e-9)
        .filter(|(_, f)| f.attrs.get(ATTR_MATERIAL) == Some(&serde_json::json!("stone")))
        .count();
    assert_eq!(walls, 4);
    // The caps keep the paint on the same physical side as the profile.
    let side = if front_along {
        ATTR_BACK_MATERIAL
    } else {
        ATTR_MATERIAL
    };
    let caps = m
        .faces()
        .filter(|(_, f)| f.plane.normal.x.abs() > 0.9)
        .filter(|(_, f)| f.attrs.get(side) == Some(&serde_json::json!("stone")))
        .count();
    assert_eq!(caps, 2);
}

#[test]
fn follow_me_refuses_what_it_cannot_sweep() {
    let path = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]];
    let (m0, profile, es) = square_on(&path, 200.0);
    let refused = |m: &mut SurfaceModel, face: FaceId, p: Vec<EdgeId>, words: &str| {
        let before = m.clone();
        let e = m.apply(&Edit::FollowMe { face, path: p }).unwrap_err();
        assert_eq!(*m, before);
        assert!(e.to_string().contains(words), "{e}");
    };
    // A path in the plane of the profile.
    let mut m = m0.clone();
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, -100.0, -100.0], [0.0, -500.0, -100.0]]],
    })
    .unwrap();
    let along = edge_ids_between(&m, &[[0.0, -100.0, -100.0], [0.0, -500.0, -100.0]]);
    refused(&mut m, profile, along, "lies along the path");
    // Branches.
    let mut m = m0.clone();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[1000.0, 0.0, 0.0], [1000.0, 500.0, 0.0]],
            [[1000.0, 0.0, 0.0], [1000.0, -500.0, 0.0]],
        ],
    })
    .unwrap();
    let all: Vec<EdgeId> = m
        .edges()
        .filter(|(_, e)| m.vertex(e.v[0]).unwrap().x > 0.0 || m.vertex(e.v[1]).unwrap().x > 0.0)
        .map(|(id, _)| id)
        .collect();
    refused(&mut m, profile, all, "without branches");
    // An edge of the profile in the path.
    let mut m = m0.clone();
    let own = m
        .edges()
        .find(|(_, e)| m.vertex(e.v[0]).unwrap().x == 0.0 && m.vertex(e.v[1]).unwrap().x == 0.0)
        .unwrap()
        .0;
    // The edge lies in the plane of the profile, so that is the reason given first.
    refused(&mut m, profile, vec![own], "lies along the path");
    let mut m = m0.clone();
    assert_eq!(
        m.apply(&Edit::FollowMe {
            face: FaceId(999),
            path: es.clone()
        }),
        Err(SurfaceError::NoFace(FaceId(999)))
    );
}

#[test]
fn too_many_faces_are_refused_with_the_count() {
    let mut m = SurfaceModel::new();
    let ring: Vec<P> = (0..200)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 200.0;
            [0.0, 100.0 * a.cos(), 100.0 * a.sin()]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: ring,
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    let profile = m.faces().next().unwrap().0;
    let path: Vec<P> = (0..=40).map(|i| [100.0 * f64::from(i), 0.0, 0.0]).collect();
    m.apply(&Edit::AddEdges {
        edges: path.windows(2).map(|w| [w[0], w[1]]).collect(),
    })
    .unwrap();
    let es = edge_ids_between(&m, &path);
    let before = m.clone();
    let e = m
        .apply(&Edit::FollowMe {
            face: profile,
            path: es,
        })
        .unwrap_err();
    assert!(matches!(e, SurfaceError::Limit { .. }), "{e}");
    assert_eq!(m, before);
}

#[test]
fn follow_me_gives_the_same_ids_every_time_and_replays() {
    let path = [[0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], [2000.0, 1000.0, 300.0]];
    let (m0, profile, es) = square_on(&path, 200.0);
    let mut a = m0.clone();
    let mut b = m0.clone();
    follow(&mut a, profile, &es).unwrap();
    follow(&mut b, profile, &es).unwrap();
    assert_eq!(a, b);
    let entry = a.log().last().unwrap().clone();
    let mut c = m0.clone();
    sync::replay_intent(&mut c, &entry.intent).unwrap();
    assert_eq!(c.face_count(), a.face_count());
}

#[test]
#[ignore = "timing"]
fn follow_me_timing() {
    let mut m = SurfaceModel::new();
    let ring: Vec<P> = (0..48)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 48.0;
            [0.0, 100.0 * a.cos(), 100.0 * a.sin()]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: ring,
        closed: true,
        faceted: false,
        analytic: None,
    })
    .unwrap();
    let profile = m.faces().next().unwrap().0;
    let path: Vec<P> = (0..=100)
        .map(|i| {
            let t = f64::from(i) / 100.0 * 3.0;
            [5000.0 * t.sin(), 5000.0 * (1.0 - t.cos()), 0.0]
        })
        .collect();
    m.apply(&Edit::AddEdges {
        edges: path.windows(2).map(|w| [w[0], w[1]]).collect(),
    })
    .unwrap();
    let es = edge_ids_between(&m, &path);
    let t = std::time::Instant::now();
    m.apply(&Edit::FollowMe {
        face: profile,
        path: es,
    })
    .unwrap();
    let el = t.elapsed();
    println!(
        "follow me 48 x 100: {:.0} ms, {} faces",
        el.as_secs_f64() * 1e3,
        m.face_count()
    );
    assert!(el.as_secs_f64() < 10.0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// A square swept along an L of any size: valid, and the volume is the area times the length.
    #[test]
    fn an_l_sweep_has_the_volume_of_its_length(
        a in 300.0f64..5000.0,
        b in 300.0f64..5000.0,
        s in 20.0f64..250.0,
        up in -2000.0f64..2000.0,
    ) {
        let path = [[0.0, 0.0, 0.0], [a, 0.0, 0.0], [a, b, 0.0]];
        let (mut m, profile, es) = square_on(&path, s);
        let m0 = m.clone();
        follow(&mut m, profile, &es).unwrap();
        prop_assert!((volume(&m) - s * s * (a + b)).abs() < 1e-6 * s * s * (a + b));
        let mut again = m0.clone();
        follow(&mut again, profile, &es).unwrap();
        prop_assert_eq!(again, m);
        // A path that climbs: still valid.
        let path = [[0.0, 0.0, 0.0], [a, 0.0, 0.0], [a, b, up]];
        let (mut m, profile, es) = square_on(&path, s);
        follow(&mut m, profile, &es).unwrap();
    }
}

#[test]
fn a_profile_between_two_corners_of_a_closed_path_is_moved_to_the_nearest_with_a_note() {
    let mut m = SurfaceModel::new();
    // A square path 2000 x 2000 on the ground (no face).
    let sq: Vec<P> = vec![
        [0.0, 0.0, 0.0],
        [2000.0, 0.0, 0.0],
        [2000.0, 2000.0, 0.0],
        [0.0, 2000.0, 0.0],
    ];
    m.apply(&Edit::AddEdges {
        edges: (0..4).map(|i| [sq[i], sq[(i + 1) % 4]]).collect(),
    })
    .unwrap();
    let floor = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![floor] }).unwrap();
    let path: Vec<EdgeId> = m.edges().map(|(id, _)| id).collect();
    // A small square profile across the first side, near its start, 10 above it (a profile
    // that touches the path inside a side is refused: the sweep would run along the path).
    let p = [
        [300.0, -50.0, 10.0],
        [300.0, 50.0, 10.0],
        [300.0, 50.0, 110.0],
        [300.0, -50.0, 110.0],
    ];
    m.apply(&Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    })
    .unwrap();
    let profile = m.faces().next().unwrap().0;
    let notes = follow(&mut m, profile, &path).unwrap();
    assert!(
        notes.iter().any(|n| n.contains("not at a corner")),
        "{notes:?}"
    );
    assert!(m.face(profile).is_none());
    // A frame: 4 x 4 sides, the top and the bottom one ring face each, the inner and outer
    // walls one face per side.
    let r = m.check();
    assert!(
        r.border_edges.is_empty() && r.nonmanifold_edges.is_empty(),
        "{r:?}"
    );
    // The volume of a frame 100 wide and 100 high around the path.
    let v = (2100.0 * 2100.0 - 1900.0 * 1900.0) * 100.0;
    assert!((volume(&m).abs() - v).abs() < 1e-3, "{}", volume(&m));
}
