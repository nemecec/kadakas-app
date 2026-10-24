//! Boundary representations: shells, faces, loops and edges become triangles. The points of
//! an edge are cut once and used by both faces that share it, so a closed solid has no cracks.

use std::collections::{BTreeMap, HashMap};
use std::f64::consts::{PI, TAU};
use std::rc::Rc;

use glam::{DVec3, DVec3 as V3};

use crate::ctx::{Ctx, Fail, R, attr, flag, reference, refs, skip};
use crate::curve::Curve;
use crate::geom::frame;
use crate::mesh2d::{Points, Refine, area, mesh_face};
use crate::surface::Surface;

pub type Rgba = [u8; 4];

/// The identity of a point that two faces can share.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VKey {
    /// A vertex (the number of the VERTEX_POINT, or of the point of a polygon).
    Vertex(u64),
    /// A point inside an edge: the edge and the index.
    Edge(u64, u32),
    /// A point that a face invents for itself and uses twice (a pole, a seam).
    Face(u64, u32),
}

/// The triangles of one body in the units of the file.
#[derive(Default)]
pub struct Builder {
    pub positions: Vec<DVec3>,
    keys: HashMap<VKey, u32>,
    pub groups: BTreeMap<Option<Rgba>, Vec<[u32; 3]>>,
    pending: Vec<(Option<Rgba>, [u32; 3])>,
}

impl Builder {
    fn vertex(&mut self, key: Option<VKey>, p: DVec3) -> u32 {
        if let Some(k) = key
            && let Some(i) = self.keys.get(&k)
        {
            return *i;
        }
        let i = self.positions.len() as u32;
        self.positions.push(p);
        if let Some(k) = key {
            self.keys.insert(k, i);
        }
        i
    }

    /// Moves the triangles of the faces since the last call into the groups. For a closed
    /// shell, `outward` says that the volume must be positive (false for a void). A shell
    /// with the other sign is turned inside out. Returns true when it was turned.
    pub fn commit(&mut self, outward: Option<bool>) -> bool {
        let mut turned = false;
        if let Some(out) = outward {
            let vol: f64 = self
                .pending
                .iter()
                .map(|(_, t)| {
                    let (a, b, c) = (
                        self.positions[t[0] as usize],
                        self.positions[t[1] as usize],
                        self.positions[t[2] as usize],
                    );
                    a.dot(b.cross(c))
                })
                .sum();
            if (vol > 0.0) != out && vol != 0.0 {
                turned = true;
                for (_, t) in &mut self.pending {
                    t.swap(1, 2);
                }
            }
        }
        for (c, t) in self.pending.drain(..) {
            self.groups.entry(c).or_default().push(t);
        }
        turned
    }
}

type Pt = (DVec3, VKey);

fn vertex_point(ctx: &mut Ctx, id: u64) -> R<(DVec3, VKey)> {
    let e = ctx.ent(id)?;
    match e.name.as_str() {
        "VERTEX_POINT" => {
            let p = ctx.point(reference(attr(e, 1)?).unwrap_or(0))?;
            Ok((p, VKey::Vertex(id)))
        }
        "CARTESIAN_POINT" => Ok((ctx.point(id)?, VKey::Vertex(id))),
        other => skip(format!("an edge ends at a {other}, not at a VERTEX_POINT")),
    }
}

/// Points from the start vertex to the end vertex of an edge along its curve.
pub fn discretize(
    curve: &Curve,
    sp: DVec3,
    ep: DVec3,
    same_vertex: bool,
    same_sense: bool,
    tol: f64,
    ang: f64,
) -> Vec<DVec3> {
    let closed = same_vertex || sp.distance(ep) <= 1e-9 * (1.0 + sp.length());
    let mut pts = match curve {
        Curve::Line { .. } => vec![sp, ep],
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            let period = TAU;
            let ts = curve.param_of(sp);
            let te = if closed {
                if same_sense { ts + period } else { ts - period }
            } else if same_sense {
                ts + (curve.param_of(ep) - ts).rem_euclid(period)
            } else {
                ts - (ts - curve.param_of(ep)).rem_euclid(period)
            };
            curve.sample(ts, te, tol, ang)
        }
        Curve::Trimmed { basis, t0, t1 } => {
            let forward = t1 >= t0;
            if closed && !matches!(basis.as_ref(), Curve::Circle { .. } | Curve::Ellipse { .. }) {
                let (a, b) = if forward { (*t0, *t1) } else { (*t1, *t0) };
                let mut p = basis.sample(a, b, tol, ang);
                if forward != same_sense {
                    p.reverse();
                }
                p
            } else {
                return discretize(basis, sp, ep, same_vertex, same_sense == forward, tol, ang);
            }
        }
        Curve::Spline(_) | Curve::Poly(_) => {
            let (a, b) = curve.natural_range().unwrap_or((0.0, 1.0));
            if closed {
                let p = curve.sample(a, b, tol, ang);
                if same_sense { p } else { p.into_iter().rev().collect() }
            } else {
                let ts = curve.closest(sp, a, b);
                let te = curve.closest(ep, a, b);
                curve.sample(ts, te, tol, ang)
            }
        }
        Curve::Composite(_) => {
            let mut p = curve.sample_all(tol, ang).unwrap_or_else(|| vec![sp, ep]);
            let straight = sp.distance_squared(p[0]) + ep.distance_squared(p[p.len() - 1]);
            let reversed = sp.distance_squared(p[p.len() - 1]) + ep.distance_squared(p[0]);
            if reversed < straight || (closed && !same_sense) {
                p.reverse();
            }
            p
        }
    };
    if pts.len() < 2 {
        pts = vec![sp, ep];
    }
    let n = pts.len();
    pts[0] = sp;
    pts[n - 1] = ep;
    pts
}

impl<'a> Ctx<'a> {
    /// The points of an EDGE_CURVE from its start vertex to its end vertex, cut once.
    pub fn edge_polyline(&mut self, id: u64) -> R<Rc<Vec<Pt>>> {
        if let Some(p) = self.edges.get(&id) {
            return Ok(p.clone());
        }
        let e = self.ent(id)?;
        if e.name != "EDGE_CURVE" {
            return skip(format!("an edge is a {}, not an EDGE_CURVE", e.name));
        }
        let (sp, sk) = vertex_point(self, reference(attr(e, 1)?).unwrap_or(0))?;
        let (ep, ek) = vertex_point(self, reference(attr(e, 2)?).unwrap_or(0))?;
        let same_sense = flag(attr(e, 4)?).unwrap_or(true);
        let curve_id = reference(attr(e, 3)?).unwrap_or(0);
        let pts = match self.curve(curve_id) {
            Ok(c) => discretize(&c, sp, ep, sk == ek, same_sense, self.tol, self.angle),
            Err(Fail::Skip(why)) => {
                self.skip_id(curve_id, &format!("{why}: the edge is a straight line"));
                vec![sp, ep]
            }
            Err(f) => return Err(f),
        };
        let n = pts.len();
        let out: Vec<Pt> = pts
            .into_iter()
            .enumerate()
            .map(|(k, p)| {
                let key = if k == 0 {
                    sk
                } else if k == n - 1 {
                    ek
                } else {
                    VKey::Edge(id, k as u32)
                };
                (p, key)
            })
            .collect();
        let out = Rc::new(out);
        self.edges.insert(id, out.clone());
        Ok(out)
    }

    /// The points of a loop in order. The last point is not the first again.
    fn loop_points(&mut self, id: u64) -> R<Vec<Pt>> {
        let e = self.ent(id)?;
        let mut out: Vec<Pt> = Vec::new();
        match e.name.as_str() {
            "EDGE_LOOP" => {
                let list = refs(attr(e, 1)?);
                if list.is_empty() {
                    return skip("a loop has no edges");
                }
                for oe in list {
                    let o = self.ent(oe)?;
                    let (edge, fwd) = match o.name.as_str() {
                        "ORIENTED_EDGE" => (
                            reference(attr(o, 3)?).unwrap_or(0),
                            flag(attr(o, 4)?).unwrap_or(true),
                        ),
                        "EDGE_CURVE" => (oe, true),
                        other => return skip(format!("a loop holds a {other}")),
                    };
                    let poly = self.edge_polyline(edge)?;
                    let n = poly.len();
                    if fwd {
                        out.extend(poly[..n - 1].iter().copied());
                    } else {
                        out.extend(poly[1..].iter().rev().copied());
                    }
                }
            }
            "POLY_LOOP" => {
                for p in refs(attr(e, 1)?) {
                    out.push((self.point(p)?, VKey::Vertex(p)));
                }
            }
            "VERTEX_LOOP" => return skip("a loop of one vertex has no area"),
            other => return skip(format!("the loop type {other} is not supported")),
        }
        Ok(out)
    }
}

/// One loop in the parameters: the points and where each came from.
struct UvLoop {
    uv: Vec<[f64; 2]>,
    src: Vec<Pt>,
    /// Turns around the periodic u direction: 0 for a loop that closes by itself.
    wind: i32,
}

fn unwrap_to(prev: f64, x: f64, period: Option<f64>) -> f64 {
    match period {
        Some(t) => x + ((prev - x) / t).round() * t,
        None => x,
    }
}

/// The loop in the parameters of the surface, with the angles made continuous.
fn to_uv(surface: &Surface, pts: &[Pt], hint: &mut Option<(f64, f64)>, ang: f64) -> Option<UvLoop> {
    // Equal neighbours collapse (a pole stays one point, whatever its key).
    let mut kept: Vec<Pt> = Vec::with_capacity(pts.len());
    for &(p, k) in pts {
        if let Some(&(q, kk)) = kept.last()
            && kk == k
            && q.distance(p) <= 1e-12 * (1.0 + p.length())
        {
            continue;
        }
        kept.push((p, k));
    }
    while kept.len() > 1
        && kept[0].1 == kept[kept.len() - 1].1
        && kept[0].0.distance(kept[kept.len() - 1].0) <= 1e-12 * (1.0 + kept[0].0.length())
    {
        kept.pop();
    }
    if kept.len() < 3 {
        return None;
    }
    let pu = surface.period_u();
    let pv = surface.period_v();
    // Raw parameters. A pole has no angle.
    let mut raw: Vec<([f64; 2], bool)> = Vec::with_capacity(kept.len());
    for &(p, _) in &kept {
        let (u, v) = surface.uv_of(p, *hint)?;
        let pole = surface.at_pole(p);
        if !pole {
            *hint = Some((u, v));
        }
        raw.push(([u, v], pole));
    }
    // Continuous angles for the points that are not poles.
    let mut prev: Option<[f64; 2]> = None;
    let mut first_free: Option<usize> = None;
    for (i, (r, pole)) in raw.iter_mut().enumerate() {
        if *pole {
            continue;
        }
        if let Some(p) = prev {
            r[0] = unwrap_to(p[0], r[0], pu);
            r[1] = unwrap_to(p[1], r[1], pv);
        } else {
            first_free = Some(i);
        }
        prev = Some(*r);
    }
    first_free?;
    let poles: Vec<usize> = (0..raw.len()).filter(|i| raw[*i].1).collect();
    // The turns of the whole loop, from its first to its last point that is not a pole.
    let free: Vec<usize> = (0..raw.len()).filter(|i| !raw[*i].1).collect();
    let (f0, f1) = (free[0], free[free.len() - 1]);
    let turns = |raw: &Vec<([f64; 2], bool)>| -> (i32, i32) {
        let wu = pu.map_or(0, |t| ((raw[f1].0[0] - raw[f0].0[0]) / t).round() as i32);
        let wv = pv.map_or(0, |t| ((raw[f1].0[1] - raw[f0].0[1]) / t).round() as i32);
        (wu, wv)
    };
    let shift_after = |raw: &mut Vec<([f64; 2], bool)>, at: usize, du: f64| {
        for r in raw.iter_mut().skip(at + 1) {
            if !r.1 {
                r.0[0] += du;
            }
        }
    };
    let (wu, _) = turns(&raw);
    // A pole lets the angle jump: use that to close a loop that turns, unless the way back
    // to the start is by a pole too.
    let wrap_by_pole = poles.contains(&0) || poles.contains(&(raw.len() - 1));
    if let (Some(&p0), Some(t)) = (poles.first(), pu)
        && wu != 0
        && !wrap_by_pole
    {
        shift_after(&mut raw, p0, -(wu as f64) * t);
    }
    // Build the output with two points for each pole.
    let build = |raw: &[([f64; 2], bool)]| -> (Vec<[f64; 2]>, Vec<usize>) {
        let n = raw.len();
        let mut uv = Vec::with_capacity(n + poles.len());
        let mut src = Vec::with_capacity(n + poles.len());
        for i in 0..n {
            let (r, pole) = raw[i];
            if !pole {
                uv.push(r);
                src.push(i);
                continue;
            }
            // The nearest points that are not poles before and after, around the loop.
            let before = (1..n).map(|d| (i + n - d) % n).find(|j| !raw[*j].1).unwrap_or(i);
            let after = (1..n).map(|d| (i + d) % n).find(|j| !raw[*j].1).unwrap_or(i);
            // The pole is a line in the parameters: cut it, so that the triangles at the pole
            // are small. All its points are the same point of space.
            let (u0, u1) = (raw[before].0[0], raw[after].0[0]);
            let segs = (((u1 - u0).abs() / ang).ceil() as usize).clamp(1, 512);
            for k in 0..=segs {
                uv.push([u0 + (u1 - u0) * k as f64 / segs as f64, r[1]]);
                src.push(i);
            }
        }
        (uv, src)
    };
    let (mut uv, mut src) = build(&raw);
    if !poles.is_empty() && pu.is_some() {
        let idx: Vec<usize> = (0..uv.len()).collect();
        let a = area(&uv, &idx).abs();
        let w = uv.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max)
            - uv.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let h = uv.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max)
            - uv.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        if a <= 1e-9 * (w * h).max(1e-300) && poles.len() >= 2 {
            // A face that goes all the way around: a jump of one turn at the first pole.
            let t = pu.unwrap_or(TAU);
            let at = poles.iter().copied().find(|i| *i != 0 && *i != raw.len() - 1).unwrap_or(poles[0]);
            shift_after(&mut raw, at, t);
            (uv, src) = build(&raw);
        }
    }
    // The turns of the points that are in the list.
    let wind = if poles.is_empty() { turns(&raw).0 } else { 0 };
    if pv.is_some() && turns(&raw).1 != 0 {
        return None;
    }
    let src: Vec<Pt> = src.into_iter().map(|i| kept[i]).collect();
    Some(UvLoop { uv, src, wind })
}

/// Points on the line `u` from `v0` to `v1` (not the ends) so that the line follows the surface.
fn seam_points(s: &Surface, u: f64, v0: f64, v1: f64, tol: f64, ang: f64) -> Vec<(f64, DVec3)> {
    fn rec(s: &Surface, u: f64, a: (f64, DVec3), b: (f64, DVec3), tol: f64, ang: f64, d: u32, out: &mut Vec<(f64, DVec3)>) {
        if d > 10 || out.len() > 2000 {
            return;
        }
        let vm = (a.0 + b.0) / 2.0;
        let pm = s.eval(u, vm);
        let chord = b.1 - a.1;
        let t = if chord.length_squared() > 0.0 {
            ((pm - a.1).dot(chord) / chord.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let sag = pm.distance(a.1 + chord * t);
        let (d1, d2) = (pm - a.1, b.1 - pm);
        let turn = if d1.length() > 0.0 && d2.length() > 0.0 {
            d1.angle_between(d2)
        } else {
            0.0
        };
        if sag > tol || turn > ang {
            let m = (vm, pm);
            rec(s, u, a, m, tol, ang, d + 1, out);
            out.push(m);
            rec(s, u, m, b, tol, ang, d + 1, out);
        }
    }
    let a = (v0, s.eval(u, v0));
    let b = (v1, s.eval(u, v1));
    let mut out = Vec::new();
    rec(s, u, a, b, tol, ang, 0, &mut out);
    out
}

impl<'a> Ctx<'a> {
    /// Cuts one face and adds its triangles to the pending triangles of the builder.
    pub fn face(&mut self, b: &mut Builder, id: u64, flip_in: bool, inherited: Option<Rgba>) -> R<()> {
        let mut e = self.ent(id)?;
        let mut flip = flip_in;
        if e.name == "ORIENTED_FACE" {
            flip ^= !flag(attr(e, 3)?).unwrap_or(true);
            e = self.ent(reference(attr(e, 2)?).unwrap_or(0))?;
        }
        let color = self.colors.get(&id).or_else(|| self.colors.get(&e.id)).copied().or(inherited);
        let (geometry, same_sense) = match e.name.as_str() {
            "ADVANCED_FACE" | "FACE_SURFACE" => (
                reference(attr(e, 2)?),
                flag(attr(e, 3)?).unwrap_or(true),
            ),
            "FACE" => (None, true),
            other => return skip(format!("a shell holds a {other}, not a face")),
        };
        let result = self.face_inner(b, e.id, e, geometry, same_sense, flip, color);
        match result {
            Err(Fail::Skip(why)) => {
                self.skip_entity(e, &format!("face not read: {why}"));
                Ok(())
            }
            other => other,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn face_inner(
        &mut self,
        b: &mut Builder,
        face: u64,
        e: &'a crate::part21::Entity,
        geometry: Option<u64>,
        same_sense: bool,
        flip: bool,
        color: Option<Rgba>,
    ) -> R<()> {
        // The loops of the bounds, in the order of the file; the flag marks an outer bound.
        let mut loops: Vec<(Vec<Pt>, bool)> = Vec::new();
        for bound in refs(attr(e, 1)?) {
            let be = self.ent(bound)?;
            let outer = be.name == "FACE_OUTER_BOUND";
            if !outer && be.name != "FACE_BOUND" {
                return skip(format!("a face has a {} as a bound", be.name));
            }
            let mut pts = self.loop_points(reference(attr(be, 1)?).unwrap_or(0))?;
            if !flag(attr(be, 2)?).unwrap_or(true) {
                pts.reverse();
            }
            loops.push((pts, outer));
        }
        if loops.is_empty() {
            return skip("a face has no bounds");
        }
        let surface: Rc<Surface> = match geometry {
            Some(g) => {
                let s = self.surface(g);
                match s {
                    Ok(s) => s,
                    Err(Fail::Skip(why)) => {
                        self.skip_id(g, &why);
                        return skip(why);
                    }
                    Err(f) => return Err(f),
                }
            }
            None => Rc::new(plane_of(&loops)?),
        };
        let kind = surface.kind();
        let periodic = surface.period_u().is_some();
        let mut hint = None;
        let mut uvloops: Vec<(UvLoop, bool)> = Vec::new();
        for (pts, outer) in &loops {
            if pts.len() < 3 {
                continue;
            }
            match to_uv(&surface, pts, &mut hint, self.angle) {
                Some(l) => uvloops.push((l, *outer)),
                None => return skip("a boundary loop cannot be placed on the surface"),
            }
        }
        if uvloops.is_empty() {
            return skip("the boundary loops have fewer than three points");
        }
        let face_key = |k: u32| VKey::Face(face, k);
        // The outline: a loop that closes by itself, or the band or cap of a loop that turns.
        let windy: Vec<usize> = (0..uvloops.len()).filter(|i| uvloops[*i].0.wind != 0).collect();
        let mut points = Points::default();
        let mut srcs: Vec<Option<VKey>> = Vec::new();
        let mut loop_ix: Vec<Vec<usize>> = Vec::new();
        let add = |points: &mut Points, srcs: &mut Vec<Option<VKey>>, uv: [f64; 2], p: DVec3, k: Option<VKey>| -> usize {
            points.uv.push(uv);
            points.xyz.push(p);
            srcs.push(k);
            points.uv.len() - 1
        };
        let mut outline: Option<Vec<usize>> = None;
        let mut hole_loops: Vec<usize> = Vec::new();
        let t = surface.period_u().unwrap_or(TAU);
        let (tol, ang) = (self.tol, self.angle);
        match windy.len() {
            0 => {}
            1 if periodic => {
                let l = &uvloops[windy[0]].0;
                let n = l.uv.len();
                let first = l.uv[0];
                // The loop ends one step before its start: the start again, a turn later.
                let closing = [first[0] + l.wind as f64 * t, first[1]];
                let north = (l.wind > 0) == same_sense;
                let vp = surface
                    .pole_v(north)
                    .ok_or(())
                    .or_else(|()| skip("a boundary loop goes around the axis, but the surface has no pole on that side"))?;
                let mut ix: Vec<usize> = (0..n).map(|k| add(&mut points, &mut srcs, l.uv[k], l.src[k].0, Some(l.src[k].1))).collect();
                ix.push(add(&mut points, &mut srcs, closing, l.src[0].0, Some(l.src[0].1)));
                // Up the meridian to the pole, along the pole, down the meridian.
                let up = seam_points(&surface, closing[0], closing[1], vp, tol, ang);
                for (k, (v, p)) in up.iter().enumerate() {
                    ix.push(add(&mut points, &mut srcs, [closing[0], *v], *p, Some(face_key(10 + k as u32))));
                }
                let pole_p = surface.eval(closing[0], vp);
                let segs = (((closing[0] - first[0]).abs() / self.angle).ceil() as usize).clamp(1, 512);
                for k in 0..=segs {
                    let u = closing[0] + (first[0] - closing[0]) * k as f64 / segs as f64;
                    ix.push(add(&mut points, &mut srcs, [u, vp], pole_p, Some(face_key(0))));
                }
                for (k, (v, p)) in up.iter().enumerate().rev() {
                    ix.push(add(&mut points, &mut srcs, [first[0], *v], *p, Some(face_key(10 + k as u32))));
                }
                outline = Some(ix);
                hole_loops = (0..uvloops.len()).filter(|i| *i != windy[0]).collect();
            }
            2 if periodic && uvloops[windy[0]].0.wind * uvloops[windy[1]].0.wind < 0 => {
                let (a, bb) = (&uvloops[windy[0]].0, &uvloops[windy[1]].0);
                let (a, bb) = if a.wind > 0 { (a, bb) } else { (bb, a) };
                let (na, nb) = (a.uv.len(), bb.uv.len());
                let closing_a = [a.uv[0][0] + t, a.uv[0][1]];
                let k = ((closing_a[0] - bb.uv[0][0]) / t).round();
                let shift = k * t;
                let mut ix: Vec<usize> = (0..na).map(|j| add(&mut points, &mut srcs, a.uv[j], a.src[j].0, Some(a.src[j].1))).collect();
                ix.push(add(&mut points, &mut srcs, closing_a, a.src[0].0, Some(a.src[0].1)));
                // The right-hand meridian from the end of a to the start of b.
                let right = seam_points(&surface, closing_a[0], closing_a[1], bb.uv[0][1], tol, ang);
                for (j, (v, p)) in right.iter().enumerate() {
                    ix.push(add(&mut points, &mut srcs, [closing_a[0], *v], *p, Some(face_key(10 + j as u32))));
                }
                for j in 0..nb {
                    ix.push(add(&mut points, &mut srcs, [bb.uv[j][0] + shift, bb.uv[j][1]], bb.src[j].0, Some(bb.src[j].1)));
                }
                let left_u = bb.uv[0][0] + shift - t;
                ix.push(add(&mut points, &mut srcs, [left_u, bb.uv[0][1]], bb.src[0].0, Some(bb.src[0].1)));
                // The left-hand meridian back: the same points in the other order.
                for (j, (v, p)) in right.iter().enumerate().rev() {
                    ix.push(add(&mut points, &mut srcs, [left_u, *v], *p, Some(face_key(10 + j as u32))));
                }
                outline = Some(ix);
                hole_loops = (0..uvloops.len()).filter(|i| !windy.contains(i)).collect();
            }
            _ => return skip("the boundary loops go around the surface in a way that is not supported"),
        }
        if outline.is_none() {
            // The loop that is marked outer, else the largest.
            let pick = |ul: &[(UvLoop, bool)]| -> usize {
                let marked: Vec<usize> = (0..ul.len()).filter(|i| ul[*i].1).collect();
                let idx = |i: usize| -> f64 {
                    let ix: Vec<usize> = (0..ul[i].0.uv.len()).collect();
                    area(&ul[i].0.uv, &ix).abs()
                };
                let pool: Vec<usize> = if marked.is_empty() { (0..ul.len()).collect() } else { marked };
                pool.into_iter().max_by(|a, b| idx(*a).total_cmp(&idx(*b))).unwrap_or(0)
            };
            let o = pick(&uvloops);
            let l = &uvloops[o].0;
            outline = Some((0..l.uv.len()).map(|k| add(&mut points, &mut srcs, l.uv[k], l.src[k].0, Some(l.src[k].1))).collect());
            hole_loops = (0..uvloops.len()).filter(|i| *i != o).collect();
        }
        let n_boundary_outline = outline.as_ref().map_or(0, Vec::len);
        let _ = n_boundary_outline;
        loop_ix.push(outline.unwrap_or_default());
        for h in hole_loops {
            let l = &uvloops[h].0;
            if l.wind != 0 {
                continue;
            }
            loop_ix.push((0..l.uv.len()).map(|k| add(&mut points, &mut srcs, l.uv[k], l.src[k].0, Some(l.src[k].1))).collect());
        }
        // Outline counterclockwise, holes clockwise.
        for (i, ix) in loop_ix.iter_mut().enumerate() {
            let a = area(&points.uv, ix);
            if (i == 0 && a < 0.0) || (i > 0 && a > 0.0) {
                ix.reverse();
            }
        }
        let n_bound = points.uv.len();
        points.scale = metric(&surface, &points);
        let max_tris = self.limits.max_face_triangles.min((self.opts.limits.max_triangles.saturating_sub(self.triangles)) as usize + 1);
        let refine = Refine {
            surface: &surface,
            tol: self.tol,
            angle: self.angle,
            max_triangles: max_tris.max(8),
        };
        let Some((tris, truncated)) = mesh_face(&mut points, &loop_ix, &refine) else {
            return skip("the outline of the face cannot be cut into triangles");
        };
        if truncated {
            self.note(format!(
                "a face on a {kind} was cut at the limit of {} triangles, so the tolerance is not met there",
                self.limits.max_face_triangles
            ));
        }
        // To the vertices of the body: boundary points by key, new points one each.
        let mut global: Vec<u32> = Vec::with_capacity(points.uv.len());
        let before = b.positions.len();
        for i in 0..points.uv.len() {
            let key = if i < n_bound { srcs[i] } else { None };
            global.push(b.vertex(key, points.xyz[i]));
        }
        let reverse = flip ^ !same_sense;
        let mut count = 0u64;
        for t in tris {
            let (x, y, z) = (global[t[0] as usize], global[t[1] as usize], global[t[2] as usize]);
            if x == y || y == z || x == z {
                continue;
            }
            b.pending.push((color, if reverse { [x, z, y] } else { [x, y, z] }));
            count += 1;
        }
        self.count((b.positions.len() - before) as u64, count)?;
        self.tessellated_face(kind, count);
        Ok(())
    }
}

/// The average lengths in space per unit of u and of v over the points of the boundary.
fn metric(s: &Surface, p: &Points) -> [f64; 2] {
    let n = p.uv.len();
    let step = (n / 64).max(1);
    let (mut su, mut sv, mut k) = (0.0, 0.0, 0.0);
    for uv in p.uv.iter().step_by(step) {
        let (a, b) = s.partials(uv[0], uv[1]);
        if a.is_finite() && b.is_finite() {
            su += a.length();
            sv += b.length();
            k += 1.0;
        }
    }
    if k > 0.0 && su > 0.0 && sv > 0.0 { [su / k, sv / k] } else { [1.0, 1.0] }
}

/// The plane of a face that has no surface (a face of a faceted solid), from the Newell normal
/// of its largest loop.
fn plane_of(loops: &[(Vec<Pt>, bool)]) -> R<Surface> {
    let mut best: Option<(f64, V3, V3, V3)> = None;
    for (pts, _) in loops {
        if pts.len() < 3 {
            continue;
        }
        let mut n = V3::ZERO;
        let mut c = V3::ZERO;
        for i in 0..pts.len() {
            let (a, b) = (pts[i].0, pts[(i + 1) % pts.len()].0);
            n += a.cross(b);
            c += a;
        }
        let m = n.length();
        if best.is_none_or(|(bm, ..)| m > bm) {
            best = Some((m, n, c / pts.len() as f64, pts[1].0 - pts[0].0));
        }
    }
    match best {
        Some((m, n, c, dir)) if m > 1e-18 && n.is_finite() => Ok(Surface::Plane {
            f: frame(c, Some(n / m), Some(dir)),
        }),
        _ => skip("a face without a surface has a boundary with no area"),
    }
}

#[allow(dead_code)]
const _: f64 = PI;
