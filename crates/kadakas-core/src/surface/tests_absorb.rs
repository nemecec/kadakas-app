//! `Absorber` (a model that takes many models and keeps its indexes) against one `absorb` after
//! the other (`absorb_reference`, the plain version): the same random models taken in the same
//! order must give the same model to the bit, with the same ids, attributes, curves, revision
//! and log.
//!
//! `PROPTEST_CASES=3000 cargo test --release -p kadakas-core --lib surface::tests_absorb` runs a
//! longer search.

use glam::{DAffine3, DVec3};
use proptest::prelude::*;

use super::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The edges of a rectangle at a point of a coarse grid (so the parts meet, overlap and touch).
fn rect(x: f64, y: f64, w: f64, h: f64, z: f64) -> Vec<[[f64; 3]; 2]> {
    let p = [[x, y, z], [x + w, y, z], [x + w, y + h, z], [x, y + h, z]];
    (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect()
}

fn cube(x: f64, y: f64, s: f64) -> Vec<[[f64; 3]; 2]> {
    let c = |i: usize| {
        [
            x + if i & 1 == 1 { s } else { 0.0 },
            y + if i & 2 == 2 { s } else { 0.0 },
            if i & 4 == 4 { s } else { 0.0 },
        ]
    };
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push([c(i), c(i | bit)]);
            }
        }
    }
    edges
}

/// A model of some rectangles and cubes on a coarse grid, with attributes on some faces and edges.
fn random_model(rng: &mut Rng, with_attrs: bool) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    let mut edges = Vec::new();
    for _ in 0..1 + rng.below(3) {
        let (x, y) = (rng.below(5) as f64 * 1000.0, rng.below(5) as f64 * 1000.0);
        match rng.below(3) {
            0 => edges.extend(cube(x, y, 1000.0 * (1 + rng.below(2)) as f64)),
            1 => edges.extend(rect(
                x,
                y,
                1000.0 * (1 + rng.below(3)) as f64,
                1000.0 * (1 + rng.below(3)) as f64,
                0.0,
            )),
            _ => edges.extend(rect(x, y, 2000.0, 1000.0, 1000.0 * rng.below(2) as f64)),
        }
    }
    m.apply(&Edit::AddEdges { edges }).expect("draws");
    if with_attrs {
        let ids: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
        for id in ids {
            if rng.below(2) == 0 {
                m.faces
                    .get_mut(&id)
                    .expect("exists")
                    .attrs
                    .insert("material".into(), format!("m{}", rng.below(3)).into());
            }
        }
        let eids: Vec<EdgeId> = m.edges().map(|(id, _)| id).collect();
        for id in eids {
            if rng.below(6) == 0 {
                let mut a = Attrs::default();
                a.insert("hidden".into(), true.into());
                m.edge_attrs.insert(id, a);
            }
        }
    }
    m
}

fn random_xf(rng: &mut Rng) -> Option<DAffine3> {
    match rng.below(5) {
        0 | 1 => None,
        2 => Some(DAffine3::from_translation(DVec3::new(
            rng.below(4) as f64 * 1000.0,
            rng.below(4) as f64 * 1000.0,
            0.0,
        ))),
        3 => Some(DAffine3::from_scale(DVec3::new(1.0, 1.0, -1.0))),
        _ => Some(DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2)),
    }
}

fn check(seed: u64) {
    let mut rng = Rng(seed | 1);
    let with_attrs = rng.below(2) == 0;
    let start = if rng.below(4) == 0 {
        SurfaceModel::new()
    } else {
        random_model(&mut rng, with_attrs)
    };
    let mut plain = start.clone();
    let mut fast = Absorber::new(start);
    for step in 0..1 + rng.below(5) {
        let other = random_model(&mut rng, with_attrs);
        let xf = random_xf(&mut rng);
        let a = plain.absorb_reference(&other, xf);
        let b = fast.absorb(&other, xf);
        assert_eq!(a.is_ok(), b.is_ok(), "seed {seed} step {step}: {a:?} {b:?}");
        if a.is_err() {
            return;
        }
        assert!(
            *fast.model() == plain,
            "seed {seed} step {step}: the models differ"
        );
    }
    // The public `absorb` is the absorber with one step.
    let finished = fast.finish();
    assert_eq!(finished, plain);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(250))]

    #[test]
    fn the_absorber_equals_one_absorb_after_the_other(seed in any::<u64>()) {
        check(seed);
    }
}

#[test]
fn the_absorber_equals_one_absorb_after_the_other_on_fixed_seeds() {
    for seed in 1..400u64 {
        check(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
}

#[test]
fn the_public_absorb_is_the_same_as_the_plain_one() {
    for seed in 1..100u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        let a = random_model(&mut rng, true);
        let b = random_model(&mut rng, true);
        let (mut x, mut y) = (a.clone(), a);
        x.absorb(&b, None).unwrap();
        y.absorb_reference(&b, None).unwrap();
        assert_eq!(x, y, "seed {seed}");
    }
}

#[test]
fn the_lookups_through_the_index_find_what_the_scans_find() {
    use super::edit::Session;
    for seed in 1..60u64 {
        let mut rng = Rng(seed.wrapping_mul(0xA24B_AED4_963E_E407) | 1);
        let mut s = Session::new(random_model(&mut rng, false));
        s.add_edges(&rect(500.0, 500.0, 2000.0, 1500.0, 0.0))
            .unwrap();
        let m = s.model().clone();
        for _ in 0..200 {
            let p = DVec3::new(
                rng.below(6000) as f64,
                rng.below(6000) as f64,
                1000.0 * rng.below(3) as f64,
            );
            assert_eq!(s.face_at(p), sync::face_at(&m, p), "face at {p}");
            assert_eq!(s.edge_at(p), sync::edge_at(&m, p), "edge at {p}");
        }
        for (_, e) in m.edges() {
            let mid = (m.vertices[&e.v[0]] + m.vertices[&e.v[1]]) / 2.0;
            assert_eq!(s.edge_at(mid), sync::edge_at(&m, mid));
        }
    }
}

#[test]
fn the_absorber_gives_the_same_with_the_scans_instead_of_the_index() {
    use super::edit::reference::{self, Mode};
    reference::with(Mode::Scan, || {
        for seed in 1..80u64 {
            check(seed.wrapping_mul(0xA24B_AED4_963E_E407));
        }
        // The lookups fall back to the scans too.
        let mut rng = Rng(77);
        let mut s = super::edit::Session::new(random_model(&mut rng, false));
        s.add_edges(&rect(0.0, 0.0, 2000.0, 1000.0, 0.0)).unwrap();
        let m = s.model().clone();
        let p = DVec3::new(500.0, 500.0, 0.0);
        assert_eq!(s.face_at(p), sync::face_at(&m, p));
        assert_eq!(s.edge_at(p), sync::edge_at(&m, p));
    });
}
