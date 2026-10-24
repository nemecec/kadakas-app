//! Random boxes and cylinders in random places: the volume matches the analytic value, the
//! solid is closed and the exact data cuts the same solid again.

use std::collections::HashMap;
use std::f64::consts::PI;

use glam::{DMat3, DQuat, DVec3};
use kadakas_meshio::Limits;
use kadakas_step::{StepOptions, read, retessellate};
use proptest::prelude::*;

struct W {
    lines: Vec<String>,
}

impl W {
    fn add(&mut self, text: String) -> usize {
        self.lines.push(format!("#{}={};", self.lines.len() + 1, text));
        self.lines.len()
    }
    fn v(p: DVec3) -> String {
        format!("({:?},{:?},{:?})", p.x, p.y, p.z)
    }
    fn point(&mut self, p: DVec3) -> usize {
        self.add(format!("CARTESIAN_POINT('',{})", Self::v(p)))
    }
    fn dir(&mut self, d: DVec3) -> usize {
        self.add(format!("DIRECTION('',{})", Self::v(d.normalize())))
    }
    fn axis(&mut self, o: DVec3, z: DVec3, x: DVec3) -> usize {
        let (p, z, x) = (self.point(o), self.dir(z), self.dir(x));
        self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})"))
    }
    fn vertex(&mut self, p: DVec3) -> usize {
        let p = self.point(p);
        self.add(format!("VERTEX_POINT('',#{p})"))
    }
    fn line(&mut self, a: DVec3, b: DVec3) -> usize {
        let d = self.dir(b - a);
        let v = self.add(format!("VECTOR('',#{d},{:?})", (b - a).length()));
        let p = self.point(a);
        self.add(format!("LINE('',#{p},#{v})"))
    }
    fn edge(&mut self, a: usize, b: usize, c: usize) -> usize {
        self.add(format!("EDGE_CURVE('',#{a},#{b},#{c},.T.)"))
    }
    fn oe(&mut self, e: usize, fwd: bool) -> usize {
        self.add(format!("ORIENTED_EDGE('',*,*,#{e},{})", if fwd { ".T." } else { ".F." }))
    }
    fn face(&mut self, loop_: Vec<usize>, surface: usize, same: bool) -> usize {
        let l = self.add(format!("EDGE_LOOP('',({}))", loop_.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")));
        let b = self.add(format!("FACE_OUTER_BOUND('',#{l},.T.)"));
        self.add(format!("ADVANCED_FACE('',(#{b}),#{surface},{})", if same { ".T." } else { ".F." }))
    }
    fn finish(mut self, faces: Vec<usize>) -> String {
        let sh = self.add(format!("CLOSED_SHELL('',({}))", faces.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")));
        self.add(format!("MANIFOLD_SOLID_BREP('s',#{sh})"));
        format!(
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{}\nENDSEC;\nEND-ISO-10303-21;\n",
            self.lines.join("\n")
        )
    }
}

fn rotation(q: [f64; 4]) -> DMat3 {
    let q = DQuat::from_xyzw(q[0], q[1], q[2], q[3]);
    let q = if q.length_squared() < 1e-6 { DQuat::IDENTITY } else { q.normalize() };
    DMat3::from_quat(q)
}

/// A box with corners at the rotated and moved places.
fn box_step(size: DVec3, r: DMat3, t: DVec3) -> String {
    let mut w = W { lines: Vec::new() };
    let corner = |i: usize| t + r * DVec3::new((i & 1) as f64 * size.x, ((i >> 1) & 1) as f64 * size.y, ((i >> 2) & 1) as f64 * size.z);
    let faces_idx = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut vertices: HashMap<usize, usize> = HashMap::new();
    let mut edges: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    let mut faces = Vec::new();
    for f in faces_idx {
        let pts: Vec<DVec3> = f.iter().map(|i| corner(*i)).collect();
        let n = (pts[1] - pts[0]).cross(pts[2] - pts[1]);
        let ax = w.axis(pts[0], n, pts[1] - pts[0]);
        let surf = w.add(format!("PLANE('',#{ax})"));
        let mut loop_ = Vec::new();
        for k in 0..4 {
            let (a, b) = (f[k], f[(k + 1) % 4]);
            let (va, vb) = (
                *vertices.entry(a).or_insert_with(|| w.vertex(corner(a))),
                *vertices.entry(b).or_insert_with(|| w.vertex(corner(b))),
            );
            let (e, fwd) = match edges.get(&(b, a)) {
                Some((e, _)) => (*e, false),
                None => {
                    let c = w.line(corner(a), corner(b));
                    let e = w.edge(va, vb, c);
                    edges.insert((a, b), (e, 0));
                    (e, true)
                }
            };
            loop_.push(w.oe(e, fwd));
        }
        faces.push(w.face(loop_, surf, true));
    }
    w.finish(faces)
}

/// A cylinder: the axis starts at `t` and points along `r * Z`.
fn cylinder_step(radius: f64, height: f64, r: DMat3, t: DVec3) -> String {
    let mut w = W { lines: Vec::new() };
    let (x, z) = (r * DVec3::X, r * DVec3::Z);
    let top = t + z * height;
    let (p0, p1) = (t + x * radius, top + x * radius);
    let (v0, v1) = (w.vertex(p0), w.vertex(p1));
    let a0 = w.axis(t, z, x);
    let a1 = w.axis(top, z, x);
    let c0 = w.add(format!("CIRCLE('',#{a0},{radius:?})"));
    let c1 = w.add(format!("CIRCLE('',#{a1},{radius:?})"));
    let (e0, e1) = (w.edge(v0, v0, c0), w.edge(v1, v1, c1));
    let seam = w.line(p0, p1);
    let es = w.edge(v0, v1, seam);
    let side = w.add(format!("CYLINDRICAL_SURFACE('',#{a0},{radius:?})"));
    let (o0, os, o1, os2) = (w.oe(e0, true), w.oe(es, true), w.oe(e1, false), w.oe(es, false));
    let f_side = w.face(vec![o0, os, o1, os2], side, true);
    let top_s = w.add(format!("PLANE('',#{a1})"));
    let o = w.oe(e1, true);
    let f_top = w.face(vec![o], top_s, true);
    let ab = w.axis(t, -z, x);
    let bot_s = w.add(format!("PLANE('',#{ab})"));
    let o = w.oe(e0, false);
    let f_bot = w.face(vec![o], bot_s, true);
    w.finish(vec![f_side, f_top, f_bot])
}

/// Signed volume and the number of edges that are not used once in each direction.
fn measure(imp: &kadakas_step::StepImport) -> (f64, usize) {
    let mut vol = 0.0;
    let mut open = 0;
    for p in &imp.set.parts {
        let mut e: HashMap<(u32, u32), (i32, i32)> = HashMap::new();
        for t in &p.triangles {
            let q = |i: u32| DVec3::from(p.positions[i as usize]);
            vol += q(t[0]).dot(q(t[1]).cross(q(t[2]))) / 6.0;
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let c = e.entry((a.min(b), a.max(b))).or_default();
                if a < b { c.0 += 1 } else { c.1 += 1 }
            }
        }
        open += e.values().filter(|v| **v != (1, 1)).count();
    }
    (vol, open)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn random_boxes_are_exact_and_closed(
        w in 1.0f64..300.0, d in 1.0f64..300.0, h in 1.0f64..300.0,
        q in prop::array::uniform4(-1.0f64..1.0),
        t in prop::array::uniform3(-20000.0f64..20000.0),
    ) {
        let text = box_step(DVec3::new(w, d, h), rotation(q), DVec3::from(t));
        let imp = read(text.as_bytes(), &StepOptions::default()).unwrap();
        let (vol, open) = measure(&imp);
        prop_assert_eq!(open, 0);
        let want = w * d * h;
        prop_assert!((vol - want).abs() / want < 1e-6, "{} {}", vol, want);
        prop_assert_eq!(imp.set.totals.triangles, 12);
        // The exact data cuts the same box.
        let e = imp.exact[0].as_ref().unwrap();
        let r = retessellate(e, 0.1, 15.0, &Limits::default()).unwrap();
        prop_assert_eq!(r.triangles.len(), 12);
    }

    #[test]
    fn random_cylinders_match_pi_r2_h_and_are_closed(
        radius in 1.0f64..120.0, h in 1.0f64..300.0,
        q in prop::array::uniform4(-1.0f64..1.0),
        t in prop::array::uniform3(-20000.0f64..20000.0),
    ) {
        let text = cylinder_step(radius, h, rotation(q), DVec3::from(t));
        let opts = StepOptions { tolerance_mm: Some(0.02), ..Default::default() };
        let imp = read(text.as_bytes(), &opts).unwrap();
        let (vol, open) = measure(&imp);
        prop_assert_eq!(open, 0);
        let want = PI * radius * radius * h;
        // An inscribed polygon: a little less, never more.
        prop_assert!(vol <= want * (1.0 + 1e-9) && vol > want * 0.99, "{} {}", vol, want);
        // The true radius is in the exact data, in the place that the file gave.
        let e = imp.exact[0].as_ref().unwrap();
        let info = e.info(10);
        let cyl = info["list"].as_array().unwrap().iter().find(|f| f["kind"] == "cylinder").unwrap();
        prop_assert!((cyl["radius"].as_f64().unwrap() - radius).abs() < 1e-9 * radius.max(1.0));
        let fine = retessellate(e, 0.005, 10.0, &Limits::default()).unwrap();
        let pos = &fine.positions;
        let v2: f64 = fine.triangles.iter().map(|t| {
            let q = |i: u32| DVec3::from(pos[i as usize]);
            q(t[0]).dot(q(t[1]).cross(q(t[2]))) / 6.0
        }).sum();
        prop_assert!((want - v2).abs() <= (want - vol).abs() + 1e-6 * want, "{} {} {}", want, vol, v2);
    }
}
