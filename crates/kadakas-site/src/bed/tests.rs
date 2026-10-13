use super::*;
use proptest::prelude::*;

/// A grid of `n` by `n` squares of the given size, two triangles each, rising with x and y.
fn grid(n: usize, size: f64) -> (Vec<DVec3>, Vec<u32>) {
    let mut v = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (i as f64 * size, j as f64 * size);
            v.push(DVec3::new(x, y, 0.1 * x + 0.05 * y));
        }
    }
    let at = |i: usize, j: usize| (j * (n + 1) + i) as u32;
    let mut idx = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            idx.extend([a, b, c, a, c, d]);
        }
    }
    (v, idx)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
    vec![
        DVec2::new(x0, y0),
        DVec2::new(x1, y0),
        DVec2::new(x1, y1),
        DVec2::new(x0, y1),
    ]
}

fn total_area(c: &BedCut) -> f64 {
    c.base.area() + c.beds.iter().map(PartMesh::area).sum::<f64>()
}

fn mesh_area(v: &[DVec3], idx: &[u32]) -> f64 {
    PartMesh {
        vertices: v.to_vec(),
        normals: Vec::new(),
        indices: idx.to_vec(),
    }
    .area()
}

/// The surface height at a point of the test grid.
fn height(x: f64, y: f64) -> f64 {
    0.1 * x + 0.05 * y
}

#[test]
fn a_square_bed_takes_its_area_and_keeps_the_height() {
    let (v, idx) = grid(4, 1000.0);
    let bed = rect(500.0, 500.0, 2500.0, 3500.0);
    let cut = cut_by_beds(&v, &idx, &[&bed]);
    assert!(!cut.truncated);
    assert!((cut.beds[0].area() - 2000.0 * 3000.0).abs() < 1e-3);
    assert!((total_area(&cut) - 16.0e6).abs() < 1e-3);
    // Every vertex is on the surface: the heights did not change.
    for p in cut.base.vertices.iter().chain(&cut.beds[0].vertices) {
        assert!((p.z - height(p.x, p.y)).abs() < 1e-6, "{p}");
    }
}

#[test]
fn the_front_bed_wins_where_beds_overlap() {
    let (v, idx) = grid(4, 1000.0);
    let back = rect(0.0, 0.0, 3000.0, 3000.0);
    let front = rect(1500.0, 1500.0, 4000.0, 4000.0);
    let cut = cut_by_beds(&v, &idx, &[&back, &front]);
    assert!((cut.beds[1].area() - 2500.0 * 2500.0).abs() < 1e-3);
    assert!((cut.beds[0].area() - (9.0e6 - 1500.0 * 1500.0)).abs() < 1e-3);
    assert!((cut.base.area() - (16.0e6 - 9.0e6 - 2500.0 * 2500.0 + 1500.0 * 1500.0)).abs() < 1e-3);
    // Swapping the order swaps the winner.
    let swapped = cut_by_beds(&v, &idx, &[&front, &back]);
    assert!((swapped.beds[0].area() - (6.25e6 - 1500.0 * 1500.0)).abs() < 1e-3);
    assert!((swapped.beds[1].area() - 9.0e6).abs() < 1e-3);
}

#[test]
fn a_concave_bed_leaves_its_notch_to_the_base() {
    let (v, idx) = grid(4, 1000.0);
    // A U shape: the notch (1000..3000, 2000..4000) is outside.
    let u = vec![
        DVec2::new(0.0, 0.0),
        DVec2::new(4000.0, 0.0),
        DVec2::new(4000.0, 4000.0),
        DVec2::new(3000.0, 4000.0),
        DVec2::new(3000.0, 2000.0),
        DVec2::new(1000.0, 2000.0),
        DVec2::new(1000.0, 4000.0),
        DVec2::new(0.0, 4000.0),
    ];
    let cut = cut_by_beds(&v, &idx, &[&u]);
    assert!((cut.base.area() - 2000.0 * 2000.0).abs() < 1e-3);
    assert!((cut.beds[0].area() - 12.0e6).abs() < 1e-3);
}

#[test]
fn an_oblique_bed_cuts_inside_the_triangles() {
    let (v, idx) = grid(1, 4000.0);
    let tri = vec![
        DVec2::new(500.0, 300.0),
        DVec2::new(3200.0, 1100.0),
        DVec2::new(1800.0, 3500.0),
    ];
    let cut = cut_by_beds(&v, &idx, &[&tri]);
    let expect = ((tri[1] - tri[0]).perp_dot(tri[2] - tri[0])).abs() / 2.0;
    assert!((cut.beds[0].area() - expect).abs() < 1e-3);
    assert!((total_area(&cut) - 16.0e6).abs() < 1e-3);
}

#[test]
fn neighbors_share_the_cut_points_of_their_edge() {
    let (v, idx) = grid(2, 2000.0);
    // The outline crosses the shared diagonals and edges.
    let bed = vec![
        DVec2::new(500.0, 700.0),
        DVec2::new(3500.0, 900.0),
        DVec2::new(3100.0, 3300.0),
        DVec2::new(900.0, 3000.0),
    ];
    let cut = cut_by_beds(&v, &idx, &[&bed]);
    for part in [&cut.base, &cut.beds[0]] {
        // No two vertices at the same place in one part: shared cut points are one vertex.
        for (i, a) in part.vertices.iter().enumerate() {
            for b in &part.vertices[i + 1..] {
                assert!((*a - *b).length() > 1e-6, "{a} twice");
            }
        }
    }
}

#[test]
fn outside_beds_and_bad_input_change_nothing() {
    let (v, idx) = grid(2, 1000.0);
    let far = rect(1.0e6, 1.0e6, 2.0e6, 2.0e6);
    let tiny = vec![DVec2::ZERO, DVec2::X];
    let cut = cut_by_beds(&v, &idx, &[&far, &tiny]);
    assert_eq!(cut.beds.len(), 2);
    assert!(cut.beds.iter().all(PartMesh::is_empty));
    assert_eq!(cut.base.indices.len(), idx.len());
    // A bed that covers everything leaves nothing to the base.
    let all = rect(-1.0e3, -1.0e3, 1.0e4, 1.0e4);
    let cut = cut_by_beds(&v, &idx, &[&all]);
    assert!(cut.base.is_empty() && (cut.beds[0].area() - 4.0e6).abs() < 1e-3);
    // Indices outside the vertices are skipped; no beds is the whole mesh in the base.
    let mut bad = idx.clone();
    bad.extend([0, 1, 99]);
    let cut = cut_by_beds(&v, &bad, &[]);
    assert_eq!(cut.base.indices.len(), idx.len());
    assert!(cut.beds.is_empty());
}

#[test]
fn more_than_64_outlines_use_the_first_64() {
    let (v, idx) = grid(1, 1000.0);
    let all = rect(-1.0, -1.0, 2000.0, 2000.0);
    let list: Vec<&[DVec2]> = (0..70).map(|_| all.as_slice()).collect();
    let cut = cut_by_beds(&v, &idx, &list);
    assert_eq!(cut.beds.len(), 64);
    assert!((cut.beds[63].area() - 1.0e6).abs() < 1e-3);
}

#[test]
fn the_bounds_stop_the_cutting_and_say_so() {
    let (v, idx) = grid(4, 1000.0);
    let bed = rect(500.0, 500.0, 2500.0, 3500.0);
    // The output cap: later triangles go whole to the base.
    let cut = cut_capped(&v, &idx, &[&bed], 3, MAX_WORK);
    assert!(cut.truncated);
    assert!((total_area(&cut) - 16.0e6).abs() < 1e-3);
    // The work budget does the same.
    let cut = cut_capped(&v, &idx, &[&bed], MAX_CUT_TRIANGLES, 1);
    assert!(cut.truncated);
    assert!((total_area(&cut) - 16.0e6).abs() < 1e-3);
    // The work counts steps, not time: the same input gives the same output.
    assert_eq!(
        cut_by_beds(&v, &idx, &[&bed]),
        cut_by_beds(&v, &idx, &[&bed])
    );
}

#[test]
fn an_outline_with_many_edges_is_cut_with_its_grid() {
    // A circle of 3000 edges over a coarse mesh.
    let ring: Vec<DVec2> = (0..3000)
        .map(|i| {
            let a = i as f64 / 3000.0 * std::f64::consts::TAU;
            DVec2::new(2000.0 + 1500.0 * a.cos(), 2000.0 + 1500.0 * a.sin())
        })
        .collect();
    let (v, idx) = grid(8, 500.0);
    let cut = cut_by_beds(&v, &idx, &[&ring]);
    let circle = std::f64::consts::PI * 1500.0 * 1500.0;
    assert!((cut.beds[0].area() - circle).abs() < circle * 1e-4);
    assert!((total_area(&cut) - 16.0e6).abs() < 1e-2);
}

fn contains(poly: &[DVec2], p: DVec2) -> bool {
    let mut w = 0;
    Outline::new(poly).0.contains(p, &mut w)
}

fn dist_to_outline(poly: &[DVec2], p: DVec2) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let t = ((p - a).dot(b - a) / (b - a).length_squared().max(1e-12)).clamp(0.0, 1.0);
            (p - (a + (b - a) * t)).length()
        })
        .fold(f64::INFINITY, f64::min)
}

fn arb_poly() -> impl Strategy<Value = Vec<DVec2>> {
    // A star-shaped (or self touching) polygon around a center, in the mesh region.
    (
        (500.0..3500.0f64, 500.0..3500.0f64),
        prop::collection::vec((0.0..1.0f64, 100.0..2500.0f64), 3..12),
    )
        .prop_map(|((cx, cy), spokes)| {
            let n = spokes.len();
            spokes
                .iter()
                .enumerate()
                .map(|(i, (jitter, r))| {
                    let a = (i as f64 + jitter * 0.9) / n as f64 * std::f64::consts::TAU;
                    DVec2::new(cx + r * a.cos(), cy + r * a.sin())
                })
                .collect()
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn the_cut_keeps_the_area_and_gives_every_piece_one_owner(
        beds in prop::collection::vec(arb_poly(), 1..4),
        n in 1usize..5,
    ) {
        let (v, idx) = grid(n, 4000.0 / n as f64);
        let outlines: Vec<&[DVec2]> = beds.iter().map(Vec::as_slice).collect();
        let cut = cut_by_beds(&v, &idx, &outlines);
        prop_assert!(!cut.truncated);
        // The area is kept.
        let want = mesh_area(&v, &idx);
        prop_assert!((total_area(&cut) - want).abs() <= want * 1e-9 + 1e-6);
        // No number is not a number, and the height is the height of the surface.
        let parts = std::iter::once(&cut.base).chain(&cut.beds);
        for part in parts.clone() {
            for p in &part.vertices {
                prop_assert!(p.is_finite());
                prop_assert!((p.z - height(p.x, p.y)).abs() < 1e-6);
            }
            prop_assert!(part.indices.iter().all(|i| (*i as usize) < part.vertices.len()));
        }
        // The owner of a piece is the frontmost bed that holds its middle, or the base.
        for (k, part) in parts.enumerate() {
            for t in part.indices.as_chunks::<3>().0 {
                let [a, b, c] = t.map(|i| part.vertices[i as usize].truncate());
                let mid = (a + b + c) / 3.0;
                let near = outlines.iter().map(|o| dist_to_outline(o, mid)).fold(f64::INFINITY, f64::min);
                if near < 1e-3 || ((b - a).perp_dot(c - a)).abs() / 2.0 < 1e-3 {
                    continue;
                }
                let owner = (0..outlines.len()).rev().find(|i| contains(outlines[*i], mid));
                prop_assert_eq!(owner.map_or(0, |i| i + 1), k);
            }
        }
    }
}

/// A zigzag outline of `teeth` teeth across a square of `size` mm: every edge is long.
fn zigzag(teeth: usize, size: f64) -> Vec<DVec2> {
    let mut pts = Vec::new();
    for i in 0..teeth {
        let x = size * i as f64 / teeth as f64;
        pts.push(DVec2::new(x, if i % 2 == 0 { 0.0 } else { size }));
    }
    pts.push(DVec2::new(size, size / 2.0));
    pts
}

fn grid_entries(o: &Outline) -> usize {
    o.grid.iter().map(Vec::len).sum()
}

#[test]
fn a_zigzag_outline_has_a_capped_grid_and_counted_work() {
    // 4095 teeth, each edge as long as the square: a bounding box grid would hold a copy of
    // every edge in nearly every cell (about 4 million entries).
    let z = zigzag(4095, 40_000.0);
    let (o, work) = Outline::new(&z);
    assert!(grid_entries(&o) <= MAX_GRID_ENTRIES, "{}", grid_entries(&o));
    assert!(work >= z.len() as u64 && work < 3 * MAX_GRID_ENTRIES as u64 + z.len() as u64);
    // The cut with it keeps the area, does not truncate, and reports its work.
    let (v, idx) = grid(4, 10_000.0);
    let cut = cut_by_beds(&v, &idx, &[&z]);
    assert!(!cut.truncated);
    assert!((total_area(&cut) - 16.0e8).abs() < 1.0);
    assert!(cut.work > work && cut.work < MAX_WORK);
    // The two parts together are the whole, and the bed has a share.
    assert!(cut.beds[0].area() > 0.0 && cut.base.area() > 0.0);
}

#[test]
fn near_lists_each_edge_once_and_counts_every_entry_it_visits() {
    let z = zigzag(400, 40_000.0);
    let (mut o, _) = Outline::new(&z);
    let mut work = 0;
    let all = o.near(DVec2::splat(-1.0), DVec2::splat(50_000.0), &mut work);
    let mut sorted = all.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "an edge twice");
    assert_eq!(all.len(), z.len(), "every edge is somewhere in the box");
    assert!(work >= (grid_entries(&o) + o.cells * o.cells) as u64);
    // A small box finds only the edges near it, and a second call starts fresh.
    let mut w2 = 0;
    let few = o.near(DVec2::new(100.0, 100.0), DVec2::new(200.0, 200.0), &mut w2);
    assert!(few.len() < all.len() && !few.is_empty());
    assert_eq!(
        few,
        o.near(DVec2::new(100.0, 100.0), DVec2::new(200.0, 200.0), &mut w2)
    );
}

#[test]
fn a_deep_chain_of_cuts_uses_a_stack_not_the_call_stack() {
    // One big triangle, and 400 long edges that all cross it: a long chain of cuts. With
    // recursion this overflows a small stack; the work stack does not.
    let v = vec![
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(100_000.0, 0.0, 0.0),
        DVec3::new(0.0, 100_000.0, 0.0),
    ];
    let z = zigzag(400, 90_000.0);
    let big = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let cut = cut_by_beds(&v, &[0, 1, 2], &[&z]);
            (cut.truncated, total_area(&cut), cut.base.area())
        })
        .unwrap()
        .join()
        .expect("no stack overflow");
    assert!(!big.0);
    assert!((big.1 - 5.0e9).abs() < 5.0e9 * 1e-9, "{}", big.1);
    assert!(big.2 > 0.0);
}

#[test]
fn a_triangle_that_needs_too_many_cuts_goes_whole_to_the_base() {
    let v = vec![
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(100_000.0, 0.0, 0.0),
        DVec3::new(0.0, 100_000.0, 0.0),
    ];
    let z = zigzag(300, 90_000.0);
    // Three triangles: the big one overshoots a small limit; the tiny one far away does not.
    let mut verts = v.clone();
    verts.extend([
        DVec3::new(500_000.0, 0.0, 0.0),
        DVec3::new(500_100.0, 0.0, 0.0),
        DVec3::new(500_000.0, 100.0, 0.0),
    ]);
    let idx = [0, 1, 2, 3, 4, 5];
    let limits = Limits {
        cap: MAX_CUT_TRIANGLES,
        max_work: MAX_WORK,
        max_pieces: 20,
    };
    let cut = run(&verts, &idx, &[&z], None, limits);
    assert!(cut.truncated);
    // The big triangle is in the base as one whole triangle, not as half-cut pieces.
    assert!(cut.beds[0].is_empty());
    assert_eq!(cut.base.indices.len(), 6);
    assert!((total_area(&cut) - (5.0e9 + 5_000.0)).abs() < 1e-3);
    // The same input with the normal limit cuts.
    let full = cut_by_beds(&verts, &idx, &[&z]);
    assert!(!full.truncated && !full.beds[0].is_empty());
    assert!((total_area(&full) - (5.0e9 + 5_000.0)).abs() < 1.0);
}

#[test]
fn running_out_of_work_in_the_middle_of_a_triangle_sends_it_whole_to_the_base() {
    let (v, idx) = grid(3, 10_000.0);
    let z = zigzag(500, 30_000.0);
    let full = cut_by_beds(&v, &idx, &[&z]);
    assert!(!full.truncated);
    // Half of the work: some triangles are done, the rest go whole to the base.
    let half = run(
        &v,
        &idx,
        &[&z],
        None,
        Limits {
            cap: MAX_CUT_TRIANGLES,
            max_work: full.work / 2,
            max_pieces: MAX_PIECES_PER_TRIANGLE,
        },
    );
    assert!(half.truncated);
    assert!((total_area(&half) - 9.0e8).abs() < 1e-3);
    assert!(half.beds[0].area() < full.beds[0].area());
    // Every triangle of the base that came from a stopped triangle is a source triangle: none
    // of its vertices is a cut point. All source vertices are in the grid at multiples of 10 m.
    let whole = half
        .base
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| {
            t.iter().all(|i| {
                let p = half.base.vertices[*i as usize];
                p.x % 10_000.0 == 0.0 && p.y % 10_000.0 == 0.0
            })
        })
        .count();
    assert!(whole > 0);
    // The budget counts work, not time: the same limit gives the same result.
    let again = run(
        &v,
        &idx,
        &[&z],
        None,
        Limits {
            cap: MAX_CUT_TRIANGLES,
            max_work: full.work / 2,
            max_pieces: MAX_PIECES_PER_TRIANGLE,
        },
    );
    assert_eq!(half, again);
}

#[test]
fn the_pieces_get_normals_interpolated_from_the_source_mesh() {
    // A bumpy surface, so the normals differ from vertex to vertex.
    let (mut v, idx) = grid(3, 1000.0);
    for (i, p) in v.iter_mut().enumerate() {
        p.z = ((i * 7) % 5) as f64 * 120.0;
    }
    let normals = smooth_normals(&v, &idx);
    assert!(
        normals
            .iter()
            .all(|n| (n.length() - 1.0).abs() < 1e-9 && n.z > 0.0)
    );
    let bed = vec![
        DVec2::new(300.0, 400.0),
        DVec2::new(2700.0, 500.0),
        DVec2::new(2200.0, 2600.0),
    ];
    let cut = cut_with_normals(&v, &idx, &[&bed], Some(&normals));
    let parts = [&cut.base, &cut.beds[0]];
    let mut at_source = 0;
    for part in parts {
        assert_eq!(part.normals.len(), part.vertices.len());
        for (p, n) in part.vertices.iter().zip(&part.normals) {
            assert!((n.length() - 1.0).abs() < 1e-9 && n.is_finite());
            // Source vertices keep their normal.
            if let Some(i) = v.iter().position(|q| (*q - *p).length() < 1e-9) {
                assert!((*n - normals[i]).length() < 1e-9);
                at_source += 1;
            }
        }
    }
    assert!(at_source > 4);
    // The same point on the two sides of an outline has the same normal: no shading seam.
    for p in &cut.beds[0]
        .vertices
        .iter()
        .zip(&cut.beds[0].normals)
        .collect::<Vec<_>>()
    {
        for (q, m) in cut.base.vertices.iter().zip(&cut.base.normals) {
            if (*p.0 - *q).length() < 1e-9 {
                assert!((*p.1 - *m).length() < 1e-9, "seam at {q}");
            }
        }
    }
    // Without normals, or with the wrong number of them, the parts have none.
    assert!(cut_by_beds(&v, &idx, &[&bed]).base.normals.is_empty());
    assert!(
        cut_with_normals(&v, &idx, &[&bed], Some(&normals[1..]))
            .base
            .normals
            .is_empty()
    );
}
