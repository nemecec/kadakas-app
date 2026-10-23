//! Tests of the conversion of a triangle mesh into a surface model (MOD-12).

use glam::DVec3;

use super::*;

fn cube(size: f64) -> (Vec<DVec3>, Vec<[u32; 3]>) {
    let v: Vec<DVec3> = (0..8)
        .map(|i| {
            DVec3::new(
                (i & 1) as f64 * size,
                ((i >> 1) & 1) as f64 * size,
                ((i >> 2) & 1) as f64 * size,
            )
        })
        .collect();
    // Counterclockwise seen from outside.
    let quads: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let t = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect();
    (v, t)
}

/// A flat grid of n by n squares, two triangles each, in the plane z = 0.
fn grid(n: usize, height: impl Fn(usize, usize) -> f64) -> (Vec<DVec3>, Vec<[u32; 3]>) {
    let mut v = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            v.push(DVec3::new(i as f64 * 100.0, j as f64 * 100.0, height(i, j)));
        }
    }
    let at = |i: usize, j: usize| (j * (n + 1) + i) as u32;
    let mut t = Vec::new();
    for j in 0..n {
        for i in 0..n {
            t.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            t.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    (v, t)
}

fn sphere(rings: usize, around: usize, r: f64) -> (Vec<DVec3>, Vec<[u32; 3]>) {
    let mut v = vec![DVec3::new(0.0, 0.0, r)];
    for i in 1..rings {
        let th = std::f64::consts::PI * i as f64 / rings as f64;
        for j in 0..around {
            let ph = 2.0 * std::f64::consts::PI * j as f64 / around as f64;
            v.push(DVec3::new(
                r * th.sin() * ph.cos(),
                r * th.sin() * ph.sin(),
                r * th.cos(),
            ));
        }
    }
    v.push(DVec3::new(0.0, 0.0, -r));
    let south = (v.len() - 1) as u32;
    let ring = |i: usize, j: usize| (1 + (i - 1) * around + j % around) as u32;
    let mut t = Vec::new();
    for j in 0..around {
        t.push([0, ring(1, j), ring(1, j + 1)]);
    }
    for i in 1..rings - 1 {
        for j in 0..around {
            t.push([ring(i, j), ring(i + 1, j), ring(i + 1, j + 1)]);
            t.push([ring(i, j), ring(i + 1, j + 1), ring(i, j + 1)]);
        }
    }
    for j in 0..around {
        t.push([ring(rings - 1, j), south, ring(rings - 1, j + 1)]);
    }
    (v, t)
}

fn tri_area(v: &[DVec3], t: &[[u32; 3]]) -> f64 {
    t.iter()
        .map(|t| {
            let p = |i: u32| v[i as usize];
            (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
        })
        .sum()
}

#[test]
fn a_cube_becomes_six_faces_and_a_closed_solid() {
    let (v, t) = cube(1000.0);
    let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    let rep = &r.report;
    assert_eq!((rep.vertices, rep.edges, rep.faces), (8, 12, 6));
    assert_eq!(rep.source_triangles, 12);
    assert!(rep.closed);
    assert!((rep.volume_mm3.unwrap() - 1.0e9).abs() < 1.0);
    assert!((rep.area_mm2 - 6.0e6).abs() < 1e-3);
    assert_eq!(rep.healed_vertices, 0);
    assert!(r.model.check().is_valid());
    assert!(r.model.check().is_closed());
    assert!(
        r.model
            .solid_report()
            .shells
            .iter()
            .all(|s| s.state == ShellState::Solid)
    );
    // Every face looks outward: its normal points away from the center.
    for (_, f) in r.model.faces() {
        let c = r.model.loop_points(&f.outer).iter().sum::<DVec3>() / 4.0;
        assert!(f.plane.normal.dot(c) > 0.0);
    }
    // The origin is the center of the box.
    assert_eq!(r.origin, DVec3::splat(500.0));
    // Merge off: twelve faces.
    let off = ConvertOptions {
        merge: false,
        ..Default::default()
    };
    let r = surface_from_triangles(&v, &t, &off).unwrap();
    assert_eq!((r.report.faces, r.report.edges), (12, 18));
    assert!(r.model.check().is_valid());
}

#[test]
fn unindexed_corners_weld() {
    let (v, t) = cube(1000.0);
    // Every triangle has its own three vertices, as an STL file has them.
    let mut v2 = Vec::new();
    let mut t2 = Vec::new();
    for tri in &t {
        let base = v2.len() as u32;
        v2.extend(tri.iter().map(|i| v[*i as usize]));
        t2.push([base, base + 1, base + 2]);
    }
    let r = surface_from_triangles(&v2, &t2, &ConvertOptions::default()).unwrap();
    assert_eq!(
        (r.report.vertices, r.report.edges, r.report.faces),
        (8, 12, 6)
    );
    assert_eq!(r.report.welded_vertices, 36 - 8);
    // A little noise below the tolerance still welds.
    let noisy: Vec<DVec3> = v2
        .iter()
        .enumerate()
        .map(|(i, p)| *p + DVec3::splat(((i % 3) as f64 - 1.0) * 0.002))
        .collect();
    let r = surface_from_triangles(&noisy, &t2, &ConvertOptions::default()).unwrap();
    assert_eq!(r.report.vertices, 8);
    assert!(r.report.faces >= 6);
}

#[test]
fn a_flat_grid_is_one_face_with_four_corners() {
    let (v, t) = grid(10, |_, _| 0.0);
    let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    assert_eq!(
        (r.report.vertices, r.report.edges, r.report.faces),
        (4, 4, 1)
    );
    assert_eq!(r.report.merged_triangles, 200);
    assert!(r.report.healed_vertices > 0);
    assert!(!r.report.closed);
    assert_eq!(r.report.border_edges, 4);
    assert!((r.report.area_mm2 - 1.0e6).abs() < 1e-3);
    assert!(r.model.check().is_valid());
}

#[test]
fn a_grid_with_a_hole_is_a_face_with_a_hole() {
    let (v, mut t) = grid(6, |_, _| 0.0);
    // Remove the squares of the cells 2..4 by 2..4: a hole of 2 by 2 cells.
    let at = |i: usize, j: usize| (j * 7 + i) as u32;
    let inside = |tri: &[u32; 3]| {
        tri.iter().all(|c| {
            let (i, j) = (*c as usize % 7, *c as usize / 7);
            (2..=4).contains(&i) && (2..=4).contains(&j)
        })
    };
    t.retain(|tri| !inside(tri));
    let _ = at;
    let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    assert_eq!(r.report.faces, 1);
    assert_eq!(r.report.faces_with_holes, 1);
    assert!((r.report.area_mm2 - (36.0 - 4.0) * 1.0e4).abs() < 1e-3);
    let (_, f) = r.model.faces().next().unwrap();
    assert_eq!((f.outer.len(), f.holes.len(), f.holes[0].len()), (4, 1, 4));
    assert!(r.model.check().is_valid());
}

#[test]
fn a_pinched_region_stays_triangles() {
    // 3 by 3 cells without the cells (0,0) and (1,1): the hole boundary touches at a vertex.
    let (v, mut t) = grid(3, |_, _| 0.0);
    let cells = [(0usize, 0usize), (1, 1)];
    let mut keep = Vec::new();
    for (k, tri) in t.iter().enumerate() {
        let cell = k / 2;
        let (i, j) = (cell % 3, cell / 3);
        if !cells.contains(&(i, j)) {
            keep.push(*tri);
        }
    }
    t = keep;
    let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    assert_eq!(r.report.unmerged_regions, 1);
    assert_eq!(r.report.faces, t.len());
    assert!(r.model.check().is_valid());
}

#[test]
fn a_triangle_with_no_area_is_the_glue_of_a_triangulated_face() {
    // A pentagon with a reflex corner on the line between two others. Its triangulation holds
    // a triangle of three points in a line, which joins the other two triangles.
    let v = vec![
        DVec3::new(-250.0, -250.0, 0.0),
        DVec3::new(-250.0, 250.0, 0.0),
        DVec3::new(250.0, 750.0, 0.0),
        DVec3::new(-750.0, 750.0, 0.0),
        DVec3::new(-750.0, -250.0, 0.0),
    ];
    let t = vec![[0, 1, 4], [2, 3, 4], [2, 4, 1]];
    let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    assert_eq!((r.report.faces, r.report.vertices), (1, 5));
    assert_eq!(r.report.joined_slivers, 1);
    assert_eq!(r.report.degenerate_triangles, 0);
    assert_eq!(r.report.merged_triangles, 2);
    assert!((r.report.area_mm2 - 625_000.0).abs() < 1e-6);
    assert!(r.model.check().is_valid());
    // Without the merge, the sliver cannot be a face and is dropped.
    let off = ConvertOptions {
        merge: false,
        ..Default::default()
    };
    let r = surface_from_triangles(&v, &t, &off).unwrap();
    assert_eq!((r.report.faces, r.report.degenerate_triangles), (2, 1));
    // A sliver alone, or next to a triangle in another plane, is dropped.
    let lone = vec![DVec3::ZERO, DVec3::X * 100.0, DVec3::X * 200.0];
    assert_eq!(
        surface_from_triangles(&lone, &[[0, 1, 2]], &ConvertOptions::default()).unwrap_err(),
        ConvertError::Empty
    );
    let mut v2 = v.clone();
    v2.push(DVec3::new(-250.0, 250.0, 500.0));
    let tilted = vec![[0, 1, 4], [4, 1, 5], [2, 4, 1]];
    let r = surface_from_triangles(&v2, &tilted, &ConvertOptions::default()).unwrap();
    assert!(r.report.degenerate_triangles + r.report.joined_slivers >= 1);
    assert!(r.model.check().is_valid());
}

#[test]
fn a_sphere_is_closed_and_bands_merge_into_quads() {
    let (v, t) = sphere(12, 16, 1000.0);
    let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    // The four corners of a band between two rings lie in one plane, so each pair of triangles
    // is one quad: 16 + 16 triangles at the poles and 10 bands of 16 quads.
    assert_eq!(r.report.faces, 32 + 10 * 16);
    assert!(r.report.closed);
    let exact = tri_area(&v, &t);
    assert!((r.report.area_mm2 - exact).abs() < 1e-6 * exact);
    assert!(r.model.check().is_closed());
}

#[test]
fn softening_counts_the_edges_under_the_angle() {
    let (v, t) = cube(1000.0);
    let none = surface_from_triangles(
        &v,
        &t,
        &ConvertOptions {
            soften_below_angle: Some(20.0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(none.report.softened_edges, 0);
    assert_eq!(none.report.considered_edges, 12);
    let all = surface_from_triangles(
        &v,
        &t,
        &ConvertOptions {
            soften_below_angle: Some(100.0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(all.report.softened_edges, 12);
    assert_eq!(all.report.soften_below_angle, Some(100.0));
    assert!(
        all.model
            .edges()
            .all(|(id, _)| all.model.edge_soft(id) && all.model.edge_smooth(id))
    );
    // A sphere with 22.5 degree steps: the edges under 30 degrees soften.
    let (sv, st) = sphere(8, 16, 1000.0);
    let r = surface_from_triangles(
        &sv,
        &st,
        &ConvertOptions {
            soften_below_angle: Some(30.0),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.report.softened_edges > r.report.edges / 2);
    assert_eq!(softened_count(&r.model, 30.0), r.report.softened_edges);
    assert!(softened_count(&r.model, 1.0) < r.report.softened_edges);
    // Without an angle no edge is soft.
    let plain = surface_from_triangles(&sv, &st, &ConvertOptions::default()).unwrap();
    assert_eq!(plain.report.softened_edges, 0);
    assert!(
        plain
            .model
            .edges()
            .all(|(id, _)| !plain.model.edge_soft(id))
    );
    assert!(softened_count(&plain.model, 30.0) > 0);
}

#[test]
fn bad_triangles_are_counted_and_bad_input_is_refused() {
    let (v, mut t) = cube(1000.0);
    t.push([0, 0, 1]);
    t.push([0, 1, 1]);
    // A repeat and a sliver.
    t.push(t[0]);
    let mut v2 = v.clone();
    v2.push(DVec3::new(500.0, 0.0, 0.0));
    t.push([0, 1, 8]);
    let r = surface_from_triangles(&v2, &t, &ConvertOptions::default()).unwrap();
    assert_eq!(r.report.degenerate_triangles, 3);
    assert_eq!(r.report.duplicate_triangles, 1);
    assert_eq!(r.report.faces, 6);
    assert!(r.notes.iter().any(|n| n.contains("no area")));
    assert!(r.notes.iter().any(|n| n.contains("repeated")));
    let o = ConvertOptions::default();
    assert_eq!(
        surface_from_triangles(&[], &[], &o).unwrap_err(),
        ConvertError::Empty
    );
    assert_eq!(
        surface_from_triangles(&v, &[[0, 0, 1]], &o).unwrap_err(),
        ConvertError::Empty
    );
    assert!(matches!(
        surface_from_triangles(&v, &[[0, 1, 99]], &o),
        Err(ConvertError::BadIndex { index: 99, .. })
    ));
    let mut nan = v.clone();
    nan[0].x = f64::NAN;
    assert_eq!(
        surface_from_triangles(&nan, &t, &o).unwrap_err(),
        ConvertError::BadVertex
    );
    let mut far = v.clone();
    far[0].x = 1.0e12;
    assert_eq!(
        surface_from_triangles(&far, &t, &o).unwrap_err(),
        ConvertError::BadVertex
    );
    for bad in [-1.0, 181.0, f64::NAN] {
        let e = surface_from_triangles(
            &v,
            &t,
            &ConvertOptions {
                soften_below_angle: Some(bad),
                ..Default::default()
            },
        );
        assert!(matches!(e, Err(ConvertError::Options(_))), "{bad}");
    }
    for bad in [0.0, 5000.0, f64::NAN] {
        let e = surface_from_triangles(
            &v,
            &t,
            &ConvertOptions {
                tolerance: Some(bad),
                ..Default::default()
            },
        );
        assert!(matches!(e, Err(ConvertError::Options(_))), "{bad}");
    }
    // A big tolerance welds more.
    let r = surface_from_triangles(
        &v,
        &t[..12],
        &ConvertOptions {
            tolerance: Some(1500.0),
            ..Default::default()
        },
    );
    assert!(r.is_err());
}

#[test]
fn a_mesh_over_the_limits_is_refused_with_the_counts() {
    // 260 by 260 squares with a wave: 67,600 squares, 135,200 triangles, 67,861 vertices.
    let (v, t) = grid(260, |i, j| {
        (((i * 73_856_093) ^ (j * 19_349_663)) % 1000) as f64
    });
    let e = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap_err();
    match &e {
        ConvertError::OverLimit { over, counts } => {
            let names: Vec<&str> = over.iter().map(|o| o.what).collect();
            assert!(
                names.contains(&"vertices") && names.contains(&"faces"),
                "{names:?}"
            );
            assert_eq!(counts.vertices, 261 * 261);
            assert_eq!(counts.faces, 260 * 260 * 2);
            let text = e.to_string();
            assert!(
                text.contains("68121 vertices") || text.contains("68121"),
                "{text}"
            );
            assert!(text.contains("limit 55000"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    // Too many triangles for one conversion.
    let big: Vec<[u32; 3]> = vec![[0, 1, 2]; MAX_CONVERT_TRIANGLES + 1];
    assert!(matches!(
        surface_from_triangles(&v, &big, &ConvertOptions::default()),
        Err(ConvertError::TooManyTriangles { .. })
    ));
}

#[test]
fn a_coplanar_mesh_over_the_limits_still_converts_after_merging() {
    // 150 by 150 squares in one plane: 45,000 triangles that become one face.
    let (v, t) = grid(150, |_, _| 5.0);
    let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    assert_eq!(r.report.faces, 1);
    assert_eq!(r.report.vertices, 4);
}

#[test]
fn the_result_is_deterministic_and_survives_edits() {
    let (v, t) = sphere(10, 12, 800.0);
    assert!(t.len() > 100);
    let a = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    let b = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
    assert_eq!(a.model, b.model);
    // The model saves and loads.
    let text = serde_json::to_string(&a.model).unwrap();
    let back: SurfaceModel = serde_json::from_str(&text).unwrap();
    assert_eq!(back.face_count(), a.model.face_count());
    // An edit on the converted model works: erase one face, the area shrinks.
    let mut m = a.model.clone();
    let (fid, _) = m.faces().next().unwrap();
    m.apply(&Edit::EraseFaces { faces: vec![fid] }).unwrap();
    assert!(m.area() < a.model.area());
}

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        // Random terrain with flat patches: the area is kept, the model is valid, and no face
        // count passes the triangle count.
        #[test]
        fn random_terrain_keeps_its_area(
            n in 2usize..9,
            h in proptest::collection::vec(0u8..3, 100),
            weld in any::<bool>(),
        ) {
            let (v, t) = grid(n, |i, j| h[(j * 10 + i) % 100] as f64 * 50.0);
            let (v, t) = if weld {
                (v, t)
            } else {
                let mut v2 = Vec::new();
                let mut t2 = Vec::new();
                for tri in &t {
                    let base = v2.len() as u32;
                    v2.extend(tri.iter().map(|i| v[*i as usize]));
                    t2.push([base, base + 1, base + 2]);
                }
                (v2, t2)
            };
            let exact = tri_area(&v, &t);
            let r = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
            prop_assert!((r.report.area_mm2 - exact).abs() < 1e-6 * exact.max(1.0),
                "{} {}", r.report.area_mm2, exact);
            prop_assert!(r.report.faces <= t.len());
            prop_assert!(r.model.check().is_valid(), "{:?}", r.model.check().problems);
            // Merging never changes the area compared with no merging.
            let off = surface_from_triangles(&v, &t, &ConvertOptions { merge: false, ..Default::default() }).unwrap();
            prop_assert!((off.report.area_mm2 - r.report.area_mm2).abs() < 1e-6 * exact.max(1.0));
            prop_assert!(off.report.faces >= r.report.faces);
        }

        // Random spheres are closed, with the volume of the triangles.
        #[test]
        fn random_spheres_are_closed(rings in 3usize..10, around in 3usize..14, r in 100.0f64..5000.0) {
            let (v, t) = sphere(rings, around, r);
            let c = surface_from_triangles(&v, &t, &ConvertOptions::default()).unwrap();
            prop_assert!(c.report.closed);
            let vol: f64 = t.iter().map(|t| {
                let p = |i: u32| v[i as usize];
                p(t[0]).dot(p(t[1]).cross(p(t[2])))
            }).sum::<f64>() / 6.0;
            prop_assert!((c.report.volume_mm3.unwrap() - vol.abs()).abs() < 1e-6 * vol.abs());
            prop_assert!(c.model.check().is_valid());
        }
    }
}
