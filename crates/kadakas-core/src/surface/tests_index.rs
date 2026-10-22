//! The spatial index of the edits that add edges (`edit::near`) against the scans it replaces: the
//! same random edits, once with the index and once with the scans (`edit::reference`), must give
//! the same results and byte-identical models.
//!
//! `PROPTEST_CASES=3000 cargo test --release -p kadakas-core --lib surface::tests_index` runs a
//! longer search.

use glam::{DMat3, DVec3};
use proptest::prelude::*;

use super::edit::reference::{self, Mode};
use super::topo::Topo;
use super::*;

type P = [f64; 3];

/// Where a case draws: all points are turned by `turn` and moved by `offset`, so the planes of the
/// faces are not along the axes and the tolerance grows with the size of the coordinates.
#[derive(Clone, Debug)]
struct Place {
    turn: DMat3,
    offset: DVec3,
}

impl Place {
    fn at(&self, p: P) -> P {
        (self.turn * DVec3::from(p) + self.offset).to_array()
    }

    /// The length tolerance at the points of the case.
    fn tol(&self) -> f64 {
        tolerance_at(self.offset.abs().max_element() + 10_000.0)
    }
}

fn place() -> impl Strategy<Value = Place> {
    let turn = prop_oneof![
        Just(DMat3::IDENTITY),
        Just(DMat3::from_rotation_z(0.5)),
        Just(DMat3::from_rotation_x(std::f64::consts::FRAC_PI_4)),
        (0.0..6.3f64, 0.0..6.3f64, 0.0..6.3f64).prop_map(|(a, b, c)| DMat3::from_euler(
            glam::EulerRot::XYZ,
            a,
            b,
            c
        )),
    ];
    let offset = prop_oneof![
        Just(DVec3::ZERO),
        Just(DVec3::new(-2500.0, 1250.0, 333.0)),
        // Survey coordinates: 650 km from the origin (the tolerance is 0.65 mm there).
        Just(DVec3::new(6.5e8, 6.4e8, 12_000.0)),
        Just(DVec3::new(-9.9e8, 3.0e5, -2.0e4)),
    ];
    (turn, offset).prop_map(|(turn, offset)| Place { turn, offset })
}

/// A point of a coarse grid (many collinear overlaps, T-junctions and crossings), moved by a small
/// amount: none, within the tolerance (it welds) or a little over it (a near miss).
#[derive(Clone, Debug)]
struct Pt {
    grid: [i32; 3],
    jitter: [f64; 3],
    /// 0: on the grid, 1: within the tolerance, 2: over it, 3: anywhere in the box.
    kind: u8,
    free: [f64; 3],
}

fn pt() -> impl Strategy<Value = Pt> {
    (
        (0..6i32, 0..6i32, prop_oneof![3 => Just(0i32), 1 => 0..3i32]),
        (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64),
        prop_oneof![6 => Just(0u8), 2 => Just(1u8), 1 => Just(2u8), 2 => Just(3u8)],
        (
            0.0..2500.0f64,
            0.0..2500.0f64,
            prop_oneof![Just(0.0), 0.0..1000.0f64],
        ),
    )
        .prop_map(|((x, y, z), (a, b, c), kind, (fx, fy, fz))| Pt {
            grid: [x, y, z],
            jitter: [a, b, c],
            kind,
            free: [fx, fy, fz],
        })
}

impl Pt {
    fn at(&self, pl: &Place) -> P {
        let g = self.grid.map(|c| f64::from(c) * 500.0);
        let s = match self.kind {
            1 => 0.3 * pl.tol(),
            2 => 2.5 * pl.tol(),
            _ => 0.0,
        };
        let local = if self.kind == 3 {
            self.free
        } else {
            [0, 1, 2].map(|i| g[i] + self.jitter[i] * s)
        };
        pl.at(local)
    }
}

#[derive(Clone, Debug)]
enum Op {
    /// One edit of several edges.
    Add(Vec<(Pt, Pt)>),
    /// A rectangle in a plane z = const of the grid; with `gap` the last side comes in an edit of
    /// its own (a gap closed last).
    Rect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        z: i32,
        gap: bool,
    },
    Curve(Vec<Pt>, bool),
    Erase(Vec<usize>),
    EraseFace(usize),
    Reverse(usize),
    PushPull(usize, f64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => prop::collection::vec((pt(), pt()), 1..8).prop_map(Op::Add),
        3 => (0..5i32, 0..5i32, 1..5i32, 1..5i32, 0..3i32, any::<bool>())
            .prop_map(|(x, y, w, h, z, gap)| Op::Rect { x, y, w, h, z, gap }),
        1 => (prop::collection::vec(pt(), 2..6), any::<bool>()).prop_map(|(p, c)| Op::Curve(p, c)),
        2 => prop::collection::vec(0..64usize, 1..4).prop_map(Op::Erase),
        1 => (0..64usize).prop_map(Op::EraseFace),
        1 => (0..64usize).prop_map(Op::Reverse),
        1 => (0..64usize, prop_oneof![Just(500.0), Just(-250.0), Just(1000.0)])
            .prop_map(|(k, d)| Op::PushPull(k, d)),
    ]
}

fn run(m: &mut SurfaceModel, op: &Op, pl: &Place) -> Vec<Result<(), SurfaceError>> {
    let pick = |n: usize, k: usize| (n > 0).then(|| k % n);
    let one = |m: &mut SurfaceModel, e: Edit| vec![m.apply(&e)];
    match op {
        Op::Add(segs) => one(
            m,
            Edit::AddEdges {
                edges: segs.iter().map(|(a, b)| [a.at(pl), b.at(pl)]).collect(),
            },
        ),
        Op::Rect { x, y, w, h, z, gap } => {
            let c = |i: i32, j: i32| {
                pl.at([
                    f64::from(i) * 500.0,
                    f64::from(j) * 500.0,
                    f64::from(*z) * 500.0,
                ])
            };
            let p = [c(*x, *y), c(x + w, *y), c(x + w, y + h), c(*x, y + h)];
            let sides: Vec<[P; 2]> = (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect();
            if *gap {
                vec![
                    m.apply(&Edit::AddEdges {
                        edges: sides[..3].to_vec(),
                    }),
                    m.apply(&Edit::AddEdges {
                        edges: sides[3..].to_vec(),
                    }),
                ]
            } else {
                one(m, Edit::AddEdges { edges: sides })
            }
        }
        Op::Curve(points, closed) => one(
            m,
            Edit::AddCurve {
                points: points.iter().map(|p| p.at(pl)).collect(),
                closed: *closed,
                faceted: false,
            },
        ),
        Op::Erase(ks) => {
            let n = m.edge_count();
            let ids: Vec<EdgeId> = ks
                .iter()
                .filter_map(|k| pick(n, *k))
                .map(|i| m.edges().nth(i).unwrap().0)
                .collect();
            if ids.is_empty() {
                return Vec::new();
            }
            one(m, Edit::EraseEdges { edges: ids })
        }
        Op::EraseFace(k) | Op::Reverse(k) | Op::PushPull(k, _) => {
            let Some(i) = pick(m.face_count(), *k) else {
                return Vec::new();
            };
            let id = m.faces().nth(i).unwrap().0;
            let e = match op {
                Op::EraseFace(_) => Edit::EraseFaces { faces: vec![id] },
                Op::Reverse(_) => Edit::ReverseFaces { faces: vec![id] },
                Op::PushPull(_, d) => Edit::PushPull {
                    face: id,
                    distance: *d,
                },
                _ => unreachable!(),
            };
            one(m, e)
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(48),
        ..ProptestConfig::default()
    })]

    #[test]
    fn the_index_gives_the_results_of_the_scans(
        pl in place(),
        ops in prop::collection::vec(op(), 1..30),
    ) {
        let mut fast = SurfaceModel::new();
        let mut slow = SurfaceModel::new();
        for op in &ops {
            let a = reference::with(Mode::Index, || run(&mut fast, op, &pl));
            let b = reference::with(Mode::Scan, || run(&mut slow, op, &pl));
            prop_assert_eq!(&a, &b, "{:?}", op);
            prop_assert_eq!(&fast, &slow, "{:?}", op);
            prop_assert_eq!(
                serde_json::to_string(&fast).unwrap(),
                serde_json::to_string(&slow).unwrap()
            );
            // The bulk build of the topology index equals adding one item at a time.
            prop_assert_eq!(Topo::of(&fast), Topo::of_one_by_one(&fast));
        }
    }
}

/// Larger edits than the random ones: grids and a terrain in turned planes far from the origin,
/// drawn over each other, with the index and with the scans.
#[test]
fn large_edits_give_the_results_of_the_scans() {
    let pl = Place {
        turn: DMat3::from_euler(glam::EulerRot::XYZ, 0.3, -0.7, 1.1),
        offset: DVec3::new(6.5e8, 6.4e8, 12_000.0),
    };
    let grid = |n: i32, x0: f64, y0: f64, s: f64| -> Vec<[P; 2]> {
        let mut out = Vec::new();
        for i in 0..=n {
            for j in 0..n {
                let (a, b) = (f64::from(i), f64::from(j));
                out.push([
                    pl.at([x0 + a * s, y0 + b * s, 0.0]),
                    pl.at([x0 + a * s, y0 + (b + 1.0) * s, 0.0]),
                ]);
                out.push([
                    pl.at([x0 + b * s, y0 + a * s, 0.0]),
                    pl.at([x0 + (b + 1.0) * s, y0 + a * s, 0.0]),
                ]);
            }
        }
        out
    };
    // Above the grids (an edge through a face is a problem of the full check).
    let height = |x: f64, y: f64| 1000.0 + 300.0 * (x / 900.0).sin() * (y / 700.0).cos();
    let mut terrain = Vec::new();
    for i in 0..12 {
        for j in 0..12 {
            let p = |a: i32, b: i32| {
                let (x, y) = (f64::from(a) * 400.0 + 100.0, f64::from(b) * 400.0 + 100.0);
                pl.at([x, y, height(x, y)])
            };
            terrain.push([p(i, j), p(i + 1, j)]);
            terrain.push([p(i, j), p(i, j + 1)]);
            terrain.push([p(i, j), p(i + 1, j + 1)]);
        }
    }
    let edits = vec![
        Edit::AddEdges {
            edges: grid(15, 0.0, 0.0, 300.0),
        },
        // A grid that welds to the first and crosses it.
        Edit::AddEdges {
            edges: grid(10, 1500.0, 1650.0, 450.0),
        },
        Edit::AddEdges { edges: terrain },
        // Long lines across everything.
        Edit::AddEdges {
            edges: (0..8)
                .map(|i| {
                    let t = f64::from(i) * 610.0 + 37.0;
                    [pl.at([t, -100.0, 0.0]), pl.at([t + 900.0, 6000.0, 0.0])]
                })
                .collect(),
        },
        Edit::AddCurve {
            points: (0..48)
                .map(|i| {
                    let a = f64::from(i) / 48.0 * std::f64::consts::TAU;
                    pl.at([2400.0 + 1300.0 * a.cos(), 2400.0 + 1300.0 * a.sin(), 0.0])
                })
                .collect(),
            closed: true,
            faceted: false,
        },
    ];
    let mut fast = SurfaceModel::new();
    let mut slow = SurfaceModel::new();
    for e in &edits {
        let a = reference::with(Mode::Index, || fast.apply(e));
        let b = reference::with(Mode::Scan, || slow.apply(e));
        assert_eq!(a, b);
        assert!(a.is_ok(), "{a:?}");
        assert_eq!(
            serde_json::to_string(&fast).unwrap(),
            serde_json::to_string(&slow).unwrap()
        );
    }
    assert!(fast.face_count() > 500, "{} faces", fast.face_count());
    // Erase some edges and draw them again.
    // (Not the edges of the curve: erasing two edges of one curve leaves a curve that is no chain.)
    let ids: Vec<EdgeId> = fast
        .edges()
        .map(|(id, _)| id)
        .filter(|id| fast.curve_of(*id).is_none())
        .step_by(7)
        .collect();
    let back: Vec<[P; 2]> = ids
        .iter()
        .map(|id| {
            let e = fast.edge(*id).unwrap();
            e.v.map(|v| fast.vertex(v).unwrap().to_array())
        })
        .collect();
    for e in [
        Edit::EraseEdges { edges: ids },
        Edit::AddEdges { edges: back },
    ] {
        let a = reference::with(Mode::Index, || fast.apply(&e));
        let b = reference::with(Mode::Scan, || slow.apply(&e));
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&fast).unwrap(),
            serde_json::to_string(&slow).unwrap()
        );
    }
    let r = fast.check();
    assert!(r.is_valid(), "{:?}", &r.problems[..r.problems.len().min(5)]);
}

/// Many loops with holes in one plane (the regions find their holes with a box tree then): each
/// frame gets its hole, drawn in one edit.
#[test]
fn many_holes_in_one_plane_find_their_frames() {
    let mut edges = Vec::new();
    let square = |x: f64, y: f64, w: f64| {
        let p = [
            [x, y, 0.0],
            [x + w, y, 0.0],
            [x + w, y + w, 0.0],
            [x, y + w, 0.0],
        ];
        (0..4).map(move |i| [p[i], p[(i + 1) % 4]])
    };
    for k in 0..48 {
        let (x, y) = (f64::from(k % 8) * 3000.0, f64::from(k / 8) * 3000.0);
        edges.extend(square(x, y, 2000.0));
        edges.extend(square(x + 500.0, y + 500.0, 1000.0));
    }
    let mut fast = SurfaceModel::new();
    let mut slow = SurfaceModel::new();
    let e = Edit::AddEdges { edges };
    reference::with(Mode::Index, || fast.apply(&e)).unwrap();
    reference::with(Mode::Scan, || slow.apply(&e)).unwrap();
    assert_eq!(fast, slow);
    assert_eq!(fast.face_count(), 96);
    let framed = fast.faces().filter(|(_, f)| f.holes.len() == 1).count();
    assert_eq!(framed, 48);
    assert!(fast.check().is_valid());
}

/// An edge that passes through the inside of a face in another plane is accepted, as a sketch
/// does (found by `random_edits_keep_the_model_valid_and_are_deterministic`): drawing does not
/// split it, the check warns (`NotIntersected`, not a problem), and Intersect Faces splits it. It
/// happens both ways: a face made over an edge that is already there, and an edge drawn through a
/// face. The index and the scans give the same models.
#[test]
fn an_edge_through_a_face_in_another_plane_is_a_warning_until_intersected() {
    let tri = [
        ([0.0, 3000.0, 500.0], [0.0, 2000.0, 1000.0]),
        ([0.0, 3000.0, 500.0], [2500.0, 2000.0, 1000.0]),
        ([2500.0, 2000.0, 1000.0], [0.0, 2000.0, 1000.0]),
    ];
    let through = ([0.0, 0.0, 500.0], [500.0, 2500.0, 1000.0]);
    // The edge first, then the face that closes over it; and the face first.
    for order in [
        vec![tri[0], tri[1], through, tri[2]],
        vec![tri[0], tri[1], tri[2], through],
    ] {
        let mut models = Vec::new();
        for mode in [Mode::Index, Mode::Scan] {
            let mut m = SurfaceModel::new();
            for (a, b) in &order {
                reference::with(mode, || {
                    m.apply(&Edit::AddEdges {
                        edges: vec![[*a, *b]],
                    })
                })
                .unwrap();
            }
            assert_eq!((m.edge_count(), m.face_count()), (4, 1));
            let r = m.check();
            assert!(r.is_valid(), "{:?}", r.problems);
            assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
            assert!(matches!(r.warnings[0], check::Problem::NotIntersected(..)));
            reference::with(mode, || {
                m.apply(&Edit::IntersectFaces {
                    scope: IntersectScope::Model,
                    faces: Vec::new(),
                    edges: Vec::new(),
                })
            })
            .unwrap();
            let r = m.check();
            assert!(r.is_valid() && r.warnings.is_empty(), "{r:?}");
            assert_eq!((m.edge_count(), m.face_count()), (5, 1));
            models.push(m);
        }
        assert_eq!(models[0], models[1]);
    }
}
