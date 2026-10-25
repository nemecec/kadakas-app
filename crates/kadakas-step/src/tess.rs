//! Triangles from an exact solid: the faces are cut one by one, the points of an edge are cut
//! once and used by both faces that share it, so a closed solid has no cracks. This works on
//! the payload ([`ExactSolid`]) and knows nothing of STEP files, so the same code cuts the
//! faces of a STEP import and those of a later `retessellate`.

use std::collections::{BTreeMap, HashMap};
use std::f64::consts::TAU;
use std::sync::Arc;

use glam::DVec3;
use kadakas_core::exact::{Curve, ExactSolid, LoopItem, Surface};
use kadakas_meshio::MeshError;

use crate::geom::frame;
use crate::mesh2d::{Points, Refine, area, mesh_face};

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

/// The triangles of one body.
#[derive(Default)]
pub struct Builder {
    pub positions: Vec<DVec3>,
    keys: HashMap<VKey, u32>,
    pub groups: BTreeMap<Option<Rgba>, Vec<[u32; 3]>>,
    pending: Vec<(Option<Rgba>, [u32; 3])>,
}

impl Builder {
    pub(crate) fn vertex(&mut self, key: Option<VKey>, p: DVec3) -> u32 {
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

    /// Is every edge of the pending triangles used once in each direction?
    fn pending_is_closed(&self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let mut e: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
        for (_, t) in &self.pending {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let c = e.entry((a.min(b), a.max(b))).or_default();
                if a < b {
                    c.0 += 1;
                } else {
                    c.1 += 1;
                }
            }
        }
        e.values().all(|c| *c == (1, 1))
    }

    /// Moves the triangles of the faces since the last call into the groups. For a closed
    /// shell, `outward` says that the volume must be positive (false for a void). A closed
    /// shell with the other sign is turned inside out, and the result is true. A shell with
    /// a face missing is never turned: its volume means nothing.
    pub fn commit(&mut self, outward: Option<bool>) -> bool {
        let mut turned = false;
        if let Some(out) = outward
            && self.pending_is_closed()
        {
            // The volume is measured from the middle of the shell, so a solid far from the
            // origin is not misjudged by rounding.
            let mut c = DVec3::ZERO;
            for (_, t) in &self.pending {
                for i in t {
                    c += self.positions[*i as usize];
                }
            }
            c /= (3 * self.pending.len()) as f64;
            let vol: f64 = self
                .pending
                .iter()
                .map(|(_, t)| {
                    let (a, b, d) = (
                        self.positions[t[0] as usize] - c,
                        self.positions[t[1] as usize] - c,
                        self.positions[t[2] as usize] - c,
                    );
                    a.dot(b.cross(d))
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
        Curve::Spline(_) | Curve::Poly { .. } => {
            let (a, b) = curve.natural_range().unwrap_or((0.0, 1.0));
            if closed {
                let p = curve.sample(a, b, tol, ang);
                if same_sense {
                    p
                } else {
                    p.into_iter().rev().collect()
                }
            } else {
                let ts = curve.closest(sp, a, b);
                let te = curve.closest(ep, a, b);
                curve.sample(ts, te, tol, ang)
            }
        }
        Curve::Composite { .. } => {
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

type Pt = (DVec3, VKey);

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
            let before = (1..n)
                .map(|d| (i + n - d) % n)
                .find(|j| !raw[*j].1)
                .unwrap_or(i);
            let after = (1..n)
                .map(|d| (i + d) % n)
                .find(|j| !raw[*j].1)
                .unwrap_or(i);
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
            let at = poles
                .iter()
                .copied()
                .find(|i| *i != 0 && *i != raw.len() - 1)
                .unwrap_or(poles[0]);
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
    #[allow(clippy::too_many_arguments)]
    fn rec(
        s: &Surface,
        u: f64,
        a: (f64, DVec3),
        b: (f64, DVec3),
        tol: f64,
        ang: f64,
        d: u32,
        out: &mut Vec<(f64, DVec3)>,
    ) {
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
    if k > 0.0 && su > 0.0 && sv > 0.0 {
        [su / k, sv / k]
    } else {
        [1.0, 1.0]
    }
}

/// The plane of a face that has no surface (a face of a faceted solid), from the Newell normal
/// of its largest loop.
fn plane_of(loops: &[(Vec<Pt>, bool)]) -> Result<Surface, TErr> {
    let mut best: Option<(f64, DVec3, DVec3, DVec3)> = None;
    for (pts, _) in loops {
        if pts.len() < 3 {
            continue;
        }
        let mut n = DVec3::ZERO;
        let mut c = DVec3::ZERO;
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

/// Why a face is not cut, or why the whole job stops.
#[derive(Debug)]
pub enum TErr {
    Skip(String),
    Abort(MeshError),
}

fn skip<T>(why: impl Into<String>) -> Result<T, TErr> {
    Err(TErr::Skip(why.into()))
}

impl From<MeshError> for TErr {
    fn from(e: MeshError) -> TErr {
        TErr::Abort(e)
    }
}

/// How fine the faces are cut and how much the job may make.
#[derive(Clone, Copy, Debug)]
pub struct TessParams {
    /// The largest distance of a triangle from its surface (the unit of the solid).
    pub tol: f64,
    /// The largest turn of the normal between neighbouring points, in radians.
    pub angle: f64,
    pub max_face_triangles: usize,
    /// The triangles that the whole job may still make.
    pub triangles_left: usize,
    /// The steps that the cutting of a polygon may take: this many, and this many times the
    /// price. A face that needs more is left out (see [`CUT_FUEL`]).
    pub cut_fuel: (u64, u64),
}

/// The usual limit for the cutting of a polygon: two million steps and four times the price.
pub const CUT_FUEL: (u64, u64) = (2_000_000, 4);

/// What a step of the job costs: output that is kept, or work that is done.
#[derive(Clone, Copy, Debug)]
pub enum Cost {
    /// Vertices and triangles that were made.
    Output(u64, u64),
    /// Steps of work (see `Ctx::work`).
    Work(u64),
}

/// The most points of the boundary of one face.
/// Steps of the cutting of a polygon that one unit of work is worth, and the price of a hole.
const OPS_PER_UNIT: u64 = 10;
const CUT_PER_HOLE: u64 = 1_200;
const CUT_PER_POINT: u64 = 260;

pub const MAX_FACE_POINTS: usize = crate::mesh2d::MAX_POLYGON_POINTS;

/// The result of cutting a solid.
#[derive(Default)]
pub struct Tessellation {
    pub positions: Vec<DVec3>,
    pub groups: BTreeMap<Option<Rgba>, Vec<[u32; 3]>>,
    /// Faces that were not cut, with the reason.
    pub skipped: Vec<(usize, String)>,
    pub notes: Vec<String>,
    /// Faces and triangles by kind of surface.
    pub kinds: BTreeMap<String, (u64, u64)>,
    /// The shells whose faces were turned inside out, because their volume had the wrong sign.
    pub turned: Vec<u32>,
}

/// Cuts every face of the solid. `spend(vertices, triangles)` is called after each face and
/// stops the job with its error.
pub fn tessellate(
    solid: &ExactSolid,
    p: &TessParams,
    spend: &mut dyn FnMut(Cost) -> Result<(), MeshError>,
) -> Result<Tessellation, MeshError> {
    let mut t = Tessellator {
        solid,
        p: *p,
        mesh: Builder::default(),
        edges: HashMap::new(),
        notes: Vec::new(),
        kinds: BTreeMap::new(),
        spend,
        used: 0,
    };
    let mut out = Tessellation::default();
    for (si, shell) in solid.shells.iter().enumerate() {
        for (fi, f) in solid.faces.iter().enumerate() {
            if f.shell as usize != si {
                continue;
            }
            match t.face_mesh(fi) {
                Ok(()) => {}
                Err(TErr::Skip(why)) => out.skipped.push((fi, why)),
                Err(TErr::Abort(e)) => return Err(e),
            }
        }
        if t.mesh.commit(shell.outward) {
            out.turned.push(si as u32);
        }
    }
    out.positions = t.mesh.positions;
    out.groups = t.mesh.groups;
    out.notes = t.notes;
    out.kinds = t.kinds;
    Ok(out)
}

struct Tessellator<'a> {
    solid: &'a ExactSolid,
    p: TessParams,
    mesh: Builder,
    edges: HashMap<u32, Arc<Vec<Pt>>>,
    notes: Vec<String>,
    kinds: BTreeMap<String, (u64, u64)>,
    spend: &'a mut dyn FnMut(Cost) -> Result<(), MeshError>,
    used: usize,
}

impl Tessellator<'_> {
    fn left_triangles(&self) -> usize {
        self.p.triangles_left.saturating_sub(self.used)
    }

    fn spend(&mut self, vertices: u64, triangles: u64) -> Result<(), TErr> {
        self.used += triangles as usize;
        (self.spend)(Cost::Output(vertices, triangles))?;
        Ok(())
    }

    fn work(&mut self, n: u64) -> Result<(), TErr> {
        (self.spend)(Cost::Work(n))?;
        Ok(())
    }

    /// The points of an edge from its start vertex to its end vertex, cut once.
    fn edge_points(&mut self, ei: u32) -> Result<Arc<Vec<Pt>>, TErr> {
        if let Some(p) = self.edges.get(&ei) {
            return Ok(p.clone());
        }
        let e = &self.solid.edges[ei as usize];
        let sp = self.solid.vertices[e.start as usize];
        let ep = self.solid.vertices[e.end as usize];
        let pts = match &e.curve {
            Some(c) => discretize(
                c,
                sp,
                ep,
                e.start == e.end,
                e.same_sense,
                self.p.tol,
                self.p.angle,
            ),
            None => vec![sp, ep],
        };
        // A spline, a polyline or a composite curve is searched for its ends: that costs.
        let search = match &e.curve {
            Some(Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. }) | None => 0,
            Some(_) => 4_000,
        };
        self.work(pts.len() as u64)?;
        self.work(search)?;
        let n = pts.len();
        let out: Vec<Pt> = pts
            .into_iter()
            .enumerate()
            .map(|(k, p)| {
                let key = if k == 0 {
                    VKey::Vertex(u64::from(e.start))
                } else if k == n - 1 {
                    VKey::Vertex(u64::from(e.end))
                } else {
                    VKey::Edge(u64::from(ei), k as u32)
                };
                (p, key)
            })
            .collect();
        let out = Arc::new(out);
        self.edges.insert(ei, out.clone());
        Ok(out)
    }

    /// The points of a loop in order. The last point is not the first again. `total` counts the
    /// points of the face so far: a face over the limit is refused before the points are made.
    fn loop_points(&mut self, items: &[LoopItem], total: &mut usize) -> Result<Vec<Pt>, TErr> {
        // First the count, so that the vector is made once at its size: a vector that grows by
        // doubling holds up to three times its content for a while.
        let mut count = 0usize;
        let mut polys: Vec<Option<Arc<Vec<Pt>>>> = Vec::with_capacity(items.len());
        for it in items {
            match it {
                LoopItem::Edge { edge, .. } => {
                    let poly = self.edge_points(*edge)?;
                    count += poly.len() - 1;
                    *total += poly.len() - 1;
                    if *total > MAX_FACE_POINTS {
                        return skip(format!(
                            "the boundary of the face has more than {MAX_FACE_POINTS} points"
                        ));
                    }
                    self.work(poly.len() as u64)?;
                    polys.push(Some(poly));
                }
                LoopItem::Vertex { .. } => {
                    count += 1;
                    *total += 1;
                    if *total > MAX_FACE_POINTS {
                        return skip(format!(
                            "the boundary of the face has more than {MAX_FACE_POINTS} points"
                        ));
                    }
                    polys.push(None);
                }
            }
        }
        let mut out: Vec<Pt> = Vec::with_capacity(count);
        for (it, poly) in items.iter().zip(polys) {
            match (it, poly) {
                (LoopItem::Edge { forward, .. }, Some(poly)) => {
                    let n = poly.len();
                    if *forward {
                        out.extend(poly[..n - 1].iter().copied());
                    } else {
                        out.extend(poly[1..].iter().rev().copied());
                    }
                }
                (LoopItem::Vertex { vertex }, _) => out.push((
                    self.solid.vertices[*vertex as usize],
                    VKey::Vertex(u64::from(*vertex)),
                )),
                _ => {}
            }
        }
        Ok(out)
    }

    /// Cuts one face and adds its triangles to the pending triangles of the mesh.
    fn face_mesh(&mut self, fi: usize) -> Result<(), TErr> {
        let solid = self.solid;
        let f = &solid.faces[fi];
        let (face, same_sense, flip, color) = (fi as u64, f.same_sense, f.flip, f.color);
        self.work(50)?;
        let mut loops: Vec<(Vec<Pt>, bool)> = Vec::new();
        let mut total = 0usize;
        for l in &f.loops {
            let pts = self.loop_points(&l.items, &mut total)?;
            loops.push((pts, l.outer));
        }
        if loops.is_empty() {
            return skip("a face has no bounds");
        }
        let owned;
        let surface: &Surface = match &f.surface {
            Some(s) => s,
            None => {
                owned = plane_of(&loops)?;
                &owned
            }
        };
        // A mirror leaves a left-handed frame: the angle runs the other way and the cutting of
        // the pole and the seam would be wrong. The same surface in a right-handed frame has
        // the opposite normal, so the sense of the face turns too.
        let canonical = surface.right_handed_form();
        let (surface, same_sense) = match &canonical {
            Some(s) => (s, !same_sense),
            None => (surface, same_sense),
        };
        let kind = surface.kind();
        let periodic = surface.period_u().is_some();
        let mut hint = None;
        let mut uvloops: Vec<(UvLoop, bool)> = Vec::new();
        for (pts, outer) in &loops {
            if pts.len() < 3 {
                continue;
            }
            self.work(pts.len() as u64 * surface.uv_cost())?;
            match to_uv(surface, pts, &mut hint, self.p.angle) {
                Some(l) => uvloops.push((l, *outer)),
                None => return skip("a boundary loop cannot be placed on the surface"),
            }
        }
        if uvloops.is_empty() {
            return skip("the boundary loops have fewer than three points");
        }
        let face_key = |k: u32| VKey::Face(face, k);
        // The outline: a loop that closes by itself, or the band or cap of a loop that turns.
        let windy: Vec<usize> = (0..uvloops.len())
            .filter(|i| uvloops[*i].0.wind != 0)
            .collect();
        let mut points = Points::default();
        let mut srcs: Vec<Option<VKey>> = Vec::new();
        // The points of the boundary are known: make the vectors once.
        points.uv.reserve(total + total / 8 + 16);
        points.xyz.reserve(total + total / 8 + 16);
        srcs.reserve(total + total / 8 + 16);
        let mut loop_ix: Vec<Vec<usize>> = Vec::new();
        let add = |points: &mut Points,
                   srcs: &mut Vec<Option<VKey>>,
                   uv: [f64; 2],
                   p: DVec3,
                   k: Option<VKey>|
         -> usize {
            points.uv.push(uv);
            points.xyz.push(p);
            srcs.push(k);
            points.uv.len() - 1
        };
        let mut outline: Option<Vec<usize>> = None;
        let mut hole_loops: Vec<usize> = Vec::new();
        let t = surface.period_u().unwrap_or(TAU);
        let (tol, ang) = (self.p.tol, self.p.angle);
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
                let mut ix: Vec<usize> = (0..n)
                    .map(|k| {
                        add(
                            &mut points,
                            &mut srcs,
                            l.uv[k],
                            l.src[k].0,
                            Some(l.src[k].1),
                        )
                    })
                    .collect();
                ix.push(add(
                    &mut points,
                    &mut srcs,
                    closing,
                    l.src[0].0,
                    Some(l.src[0].1),
                ));
                // Up the meridian to the pole, along the pole, down the meridian.
                let up = seam_points(surface, closing[0], closing[1], vp, tol, ang);
                for (k, (v, p)) in up.iter().enumerate() {
                    ix.push(add(
                        &mut points,
                        &mut srcs,
                        [closing[0], *v],
                        *p,
                        Some(face_key(10 + k as u32)),
                    ));
                }
                let pole_p = surface.eval(closing[0], vp);
                let segs =
                    (((closing[0] - first[0]).abs() / self.p.angle).ceil() as usize).clamp(1, 512);
                for k in 0..=segs {
                    let u = closing[0] + (first[0] - closing[0]) * k as f64 / segs as f64;
                    ix.push(add(
                        &mut points,
                        &mut srcs,
                        [u, vp],
                        pole_p,
                        Some(face_key(0)),
                    ));
                }
                for (k, (v, p)) in up.iter().enumerate().rev() {
                    ix.push(add(
                        &mut points,
                        &mut srcs,
                        [first[0], *v],
                        *p,
                        Some(face_key(10 + k as u32)),
                    ));
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
                let mut ix: Vec<usize> = (0..na)
                    .map(|j| {
                        add(
                            &mut points,
                            &mut srcs,
                            a.uv[j],
                            a.src[j].0,
                            Some(a.src[j].1),
                        )
                    })
                    .collect();
                ix.push(add(
                    &mut points,
                    &mut srcs,
                    closing_a,
                    a.src[0].0,
                    Some(a.src[0].1),
                ));
                // The right-hand meridian from the end of a to the start of b.
                let right = seam_points(surface, closing_a[0], closing_a[1], bb.uv[0][1], tol, ang);
                for (j, (v, p)) in right.iter().enumerate() {
                    ix.push(add(
                        &mut points,
                        &mut srcs,
                        [closing_a[0], *v],
                        *p,
                        Some(face_key(10 + j as u32)),
                    ));
                }
                for j in 0..nb {
                    ix.push(add(
                        &mut points,
                        &mut srcs,
                        [bb.uv[j][0] + shift, bb.uv[j][1]],
                        bb.src[j].0,
                        Some(bb.src[j].1),
                    ));
                }
                let left_u = bb.uv[0][0] + shift - t;
                ix.push(add(
                    &mut points,
                    &mut srcs,
                    [left_u, bb.uv[0][1]],
                    bb.src[0].0,
                    Some(bb.src[0].1),
                ));
                // The left-hand meridian back: the same points in the other order.
                for (j, (v, p)) in right.iter().enumerate().rev() {
                    ix.push(add(
                        &mut points,
                        &mut srcs,
                        [left_u, *v],
                        *p,
                        Some(face_key(10 + j as u32)),
                    ));
                }
                outline = Some(ix);
                hole_loops = (0..uvloops.len()).filter(|i| !windy.contains(i)).collect();
            }
            _ => {
                return skip(
                    "the boundary loops go around the surface in a way that is not supported",
                );
            }
        }
        if outline.is_none() {
            // The loop that is marked outer, else the largest.
            let pick = |ul: &[(UvLoop, bool)]| -> usize {
                let marked: Vec<usize> = (0..ul.len()).filter(|i| ul[*i].1).collect();
                let idx = |i: usize| -> f64 {
                    let ix: Vec<usize> = (0..ul[i].0.uv.len()).collect();
                    area(&ul[i].0.uv, &ix).abs()
                };
                let pool: Vec<usize> = if marked.is_empty() {
                    (0..ul.len()).collect()
                } else {
                    marked
                };
                pool.into_iter()
                    .max_by(|a, b| idx(*a).total_cmp(&idx(*b)))
                    .unwrap_or(0)
            };
            let o = pick(&uvloops);
            let l = &uvloops[o].0;
            outline = Some(
                (0..l.uv.len())
                    .map(|k| {
                        add(
                            &mut points,
                            &mut srcs,
                            l.uv[k],
                            l.src[k].0,
                            Some(l.src[k].1),
                        )
                    })
                    .collect(),
            );
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
            loop_ix.push(
                (0..l.uv.len())
                    .map(|k| {
                        add(
                            &mut points,
                            &mut srcs,
                            l.uv[k],
                            l.src[k].0,
                            Some(l.src[k].1),
                        )
                    })
                    .collect(),
            );
        }
        // Outline counterclockwise, holes clockwise.
        for (i, ix) in loop_ix.iter_mut().enumerate() {
            let a = area(&points.uv, ix);
            if (i == 0 && a < 0.0) || (i > 0 && a > 0.0) {
                ix.reverse();
            }
        }
        let n_bound = points.uv.len();
        points.scale = metric(surface, &points);
        let max_tris = self.p.max_face_triangles.min(self.left_triangles() + 1);
        // The cutting of the polygon (the joining of the holes and the ear clipping) counts its
        // steps, and they are paid for when they are known. The price below is for a polygon
        // of the usual kind, measured on holes in rows, on diagonals and in grids, and on
        // circles, stars, combs and zigzags. The cutting stops at four times the price: a
        // face that needs more is left out, with a note, and what it took counts.
        let holes = loop_ix.len().saturating_sub(1) as u64;
        let est_ops = holes * CUT_PER_HOLE + points.uv.len() as u64 * CUT_PER_POINT;
        let fuel = self.p.cut_fuel.0 + self.p.cut_fuel.1 * est_ops;
        let refine = Refine {
            surface,
            tol: self.p.tol,
            // A little more than the step of the edges, so that a face with edges at exactly
            // the angle step is not cut again for the rounding of the last digit.
            angle: self.p.angle * 1.1,
            max_triangles: max_tris.max(8),
            fuel,
        };
        let seed = crate::mesh2d::seed_cost(&points, &loop_ix);
        // A seed grid over the limit is not made, and costs nothing. One test of a point against
        // an edge is a few nanoseconds: a step is 16 of them.
        let seed_units = if seed > crate::mesh2d::SEED_MAX_COST {
            0
        } else {
            seed / 256
        };
        self.work(seed_units)?;
        let (tris, truncated, ops) = match mesh_face(&mut points, &loop_ix, &refine) {
            Ok(r) => r,
            Err(crate::mesh2d::Cut::Cannot) => {
                return skip("the outline of the face cannot be cut into triangles");
            }
            Err(crate::mesh2d::Cut::Fuel(ops)) => {
                // The face is left out. What it took counts against the whole job, which stops
                // when that is more than the job may take.
                self.work(ops / OPS_PER_UNIT)?;
                return skip(format!(
                    "the face needs more than {fuel} steps to be cut into triangles"
                ));
            }
        };
        self.work(ops / OPS_PER_UNIT)?;
        self.work(
            tris.len() as u64
                * if matches!(surface, Surface::Plane { .. }) {
                    12
                } else {
                    40
                },
        )?;
        self.work(points.uv.len() as u64 * (5 + 4 * surface.eval_cost()))?;
        if truncated {
            self.notes.push(format!(
                "a face on a {kind} was cut at the limit of {} triangles, so the tolerance is not met there",
                self.p.max_face_triangles
            ));
        }
        // To the vertices of the body: boundary points by key, new points one each.
        let mut global: Vec<u32> = Vec::with_capacity(points.uv.len());
        let before = self.mesh.positions.len();
        for (i, p) in points.xyz.iter().enumerate() {
            let key = if i < n_bound { srcs[i] } else { None };
            global.push(self.mesh.vertex(key, *p));
        }
        let reverse = flip ^ !same_sense;
        let mut count = 0u64;
        for t in tris {
            let (x, y, z) = (
                global[t[0] as usize],
                global[t[1] as usize],
                global[t[2] as usize],
            );
            if x == y || y == z || x == z {
                continue;
            }
            self.mesh
                .pending
                .push((color, if reverse { [x, z, y] } else { [x, y, z] }));
            count += 1;
        }
        self.spend((self.mesh.positions.len() - before) as u64, count)?;
        let k = self.kinds.entry(kind.to_string()).or_default();
        k.0 += 1;
        k.1 += count;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StepOptions, read};

    #[test]
    fn a_face_that_needs_more_steps_than_its_fuel_is_left_out_with_a_reason() {
        let bytes = include_bytes!("../../../tests/data/step/plate_hole.step");
        let imp = read(bytes, &StepOptions::default()).unwrap();
        let solid = imp.exact[0].as_ref().unwrap();
        let mut params = TessParams {
            tol: 0.1,
            angle: 0.3,
            max_face_triangles: 100_000,
            triangles_left: 1_000_000,
            cut_fuel: CUT_FUEL,
        };
        let mut work = 0u64;
        let mut spend = |c: Cost| -> Result<(), MeshError> {
            if let Cost::Work(n) = c {
                work += n;
            }
            Ok(())
        };
        let whole = tessellate(solid, &params, &mut spend).unwrap();
        assert!(whole.skipped.is_empty());
        // With no fuel at all every face is left out, and each says why.
        params.cut_fuel = (0, 0);
        let none = tessellate(solid, &params, &mut spend).unwrap();
        assert_eq!(none.skipped.len(), solid.faces.len());
        assert!(
            none.skipped
                .iter()
                .all(|(_, why)| why.contains("needs more than"))
        );
        assert!(work > 0);
    }
}
