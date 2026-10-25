//! Boolean operations on closed triangle meshes (union, difference, intersection), for the IFC
//! import: `IfcRelVoidsElement` openings, `IfcBooleanResult` with a solid and `IfcCsgSolid`.
//!
//! The method is the binary space partition of Naylor et al.: every operand becomes a tree of
//! planes, and the polygons of one operand are clipped by the tree of the other. It is exact up to a
//! distance tolerance, it keeps coplanar faces (an opening that is flush with the wall face) apart by
//! the direction of their normals, and it needs no connectivity. The result is a polygon soup that may
//! have T-junctions; its volume and its look are right.
//!
//! Limits (an input over a limit is refused with a reason, never cut wrongly): both operands must be
//! closed meshes (every edge has one partner after welding); the number of polygons the work may
//! create is capped ([`MAX_POLYGONS`]); a tree deeper than [`MAX_DEPTH`] is refused, which keeps the
//! recursion small.

use std::collections::HashMap;

use glam::DVec3;

use super::geom::MeshData;

/// Distance (file units) under which a point is on a plane.
pub const EPS: f64 = 1e-7;
/// The most polygons that one operation may hold at any time.
pub const MAX_POLYGONS: usize = 400_000;
/// The deepest tree.
pub const MAX_DEPTH: usize = 400;

/// What a boolean operation does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Union,
    Difference,
    Intersection,
}

/// Why an operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CsgError {
    /// An operand has an open edge or an edge with more than two faces.
    NotClosed,
    /// An operand has no volume.
    Empty,
    /// The work would pass [`MAX_POLYGONS`] or [`MAX_DEPTH`].
    TooBig,
}

impl std::fmt::Display for CsgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CsgError::NotClosed => "a solid is not closed",
            CsgError::Empty => "a solid has no volume",
            CsgError::TooBig => "the solids are too complex for a boolean operation",
        })
    }
}

#[derive(Clone, Debug)]
struct Plane {
    n: DVec3,
    w: f64,
}

impl Plane {
    fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }
}

#[derive(Clone, Debug)]
struct Polygon {
    v: Vec<DVec3>,
    plane: Plane,
}

impl Polygon {
    fn flip(&mut self) {
        self.v.reverse();
        self.plane.flip();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Sends the polygon to one of four lists; a spanning polygon is cut.
fn split(
    plane: &Plane,
    poly: &Polygon,
    coplanar_front: &mut Vec<Polygon>,
    coplanar_back: &mut Vec<Polygon>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
) {
    let mut kind = COPLANAR;
    let types: Vec<u8> = poly
        .v
        .iter()
        .map(|p| {
            let t = plane.n.dot(*p) - plane.w;
            let ty = if t < -EPS {
                BACK
            } else if t > EPS {
                FRONT
            } else {
                COPLANAR
            };
            kind |= ty;
            ty
        })
        .collect();
    match kind {
        COPLANAR => {
            if plane.n.dot(poly.plane.n) > 0.0 {
                coplanar_front.push(poly.clone());
            } else {
                coplanar_back.push(poly.clone());
            }
        }
        FRONT => front.push(poly.clone()),
        BACK => back.push(poly.clone()),
        _ => {
            let (mut f, mut b): (Vec<DVec3>, Vec<DVec3>) = (Vec::new(), Vec::new());
            let n = poly.v.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.v[i], poly.v[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.n.dot(vi)) / plane.n.dot(vj - vi);
                    let v = vi.lerp(vj, t);
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                front.push(Polygon {
                    v: f,
                    plane: poly.plane.clone(),
                });
            }
            if b.len() >= 3 {
                back.push(Polygon {
                    v: b,
                    plane: poly.plane.clone(),
                });
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polygons: Vec<Polygon>,
}

impl Node {
    fn from(polygons: Vec<Polygon>) -> Result<Node, CsgError> {
        let mut n = Node::default();
        n.build(polygons, 0)?;
        Ok(n)
    }

    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some(p) = &mut self.plane {
            p.flip();
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// The polygons that are not inside this tree.
    fn clip_polygons(&self, polys: Vec<Polygon>) -> Vec<Polygon> {
        let Some(plane) = &self.plane else {
            return polys;
        };
        let (mut front, mut back) = (Vec::new(), Vec::new());
        let mut cf = Vec::new();
        let mut cb = Vec::new();
        for p in &polys {
            split(plane, p, &mut cf, &mut cb, &mut front, &mut back);
        }
        front.append(&mut cf);
        back.append(&mut cb);
        let mut front = match &self.front {
            Some(f) => f.clip_polygons(front),
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polygons(back),
            None => Vec::new(),
        };
        front.extend(back);
        front
    }

    fn clip_to(&mut self, other: &Node) {
        self.polygons = other.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(f) = &mut self.front {
            f.clip_to(other);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other);
        }
    }

    fn all(&self, out: &mut Vec<Polygon>) {
        out.extend(self.polygons.iter().cloned());
        if let Some(f) = &self.front {
            f.all(out);
        }
        if let Some(b) = &self.back {
            b.all(out);
        }
    }

    fn count(&self) -> usize {
        self.polygons.len()
            + self.front.as_ref().map_or(0, |n| n.count())
            + self.back.as_ref().map_or(0, |n| n.count())
    }

    fn build(&mut self, polys: Vec<Polygon>, depth: usize) -> Result<(), CsgError> {
        if polys.is_empty() {
            return Ok(());
        }
        if depth > MAX_DEPTH || polys.len() > MAX_POLYGONS {
            return Err(CsgError::TooBig);
        }
        let plane = match &self.plane {
            Some(p) => p.clone(),
            None => polys[0].plane.clone(),
        };
        let (mut front, mut back) = (Vec::new(), Vec::new());
        let mut own = Vec::new();
        for p in &polys {
            let mut cb = Vec::new();
            split(&plane, p, &mut own, &mut cb, &mut front, &mut back);
            own.append(&mut cb);
        }
        self.plane = Some(plane);
        self.polygons.append(&mut own);
        if !front.is_empty() {
            let node = self.front.get_or_insert_with(Default::default);
            node.build(front, depth + 1)?;
        }
        if !back.is_empty() {
            let node = self.back.get_or_insert_with(Default::default);
            node.build(back, depth + 1)?;
        }
        Ok(())
    }
}

/// The polygons of a closed mesh, facing outward. `Err` when the mesh is open or has no volume.
fn polygons_of(m: &MeshData) -> Result<Vec<Polygon>, CsgError> {
    let mut mesh = m.clone();
    mesh.weld(EPS);
    if mesh.t.is_empty() {
        return Err(CsgError::Empty);
    }
    if !is_closed(&mesh) {
        return Err(CsgError::NotClosed);
    }
    let vol = signed_volume(&mesh);
    if vol.abs() < 1e-18 {
        return Err(CsgError::Empty);
    }
    if vol < 0.0 {
        mesh.flip();
    }
    let mut out = Vec::with_capacity(mesh.t.len());
    for t in &mesh.t {
        let (a, b, c) = (
            mesh.v[t[0] as usize],
            mesh.v[t[1] as usize],
            mesh.v[t[2] as usize],
        );
        let n = (b - a).cross(c - a);
        let len = n.length();
        if len < 1e-14 {
            continue;
        }
        let n = n / len;
        out.push(Polygon {
            v: vec![a, b, c],
            plane: Plane { n, w: n.dot(a) },
        });
    }
    Ok(out)
}

/// True if every directed edge of the mesh has exactly one opposite partner.
pub fn is_closed(m: &MeshData) -> bool {
    let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
    for t in &m.t {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a == b {
                continue;
            }
            *edges.entry((a, b)).or_default() += 1;
        }
    }
    edges
        .iter()
        .all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1))
}

/// The volume of a mesh (positive when it faces outward).
pub fn signed_volume(m: &MeshData) -> f64 {
    m.t.iter()
        .map(|t| {
            let (a, b, c) = (m.v[t[0] as usize], m.v[t[1] as usize], m.v[t[2] as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

fn mesh_of(polys: &[Polygon]) -> MeshData {
    let mut m = MeshData::default();
    for p in polys {
        let base = m.v.len() as u32;
        m.v.extend_from_slice(&p.v);
        for i in 1..p.v.len() as u32 - 1 {
            let t = [base, base + i, base + i + 1];
            let (a, b, c) = (p.v[0], p.v[i as usize], p.v[i as usize + 1]);
            if (b - a).cross(c - a).length() > 1e-14 {
                m.t.push(t);
            }
        }
    }
    m.weld(EPS);
    m.compact();
    heal_t_junctions(&mut m);
    m
}

/// Splits the triangles at the vertices that lie on their edges, so that the faces of a solid
/// that came out of a boolean operation meet edge to edge (the result is closed again and can be
/// the operand of the next operation). Work over [`HEAL_BUDGET`] (triangles times vertices) is skipped.
fn heal_t_junctions(m: &mut MeshData) {
    let n = m.v.len();
    if n == 0 || m.t.len().saturating_mul(n) > HEAL_BUDGET {
        return;
    }
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|a, b| m.v[*a as usize].x.total_cmp(&m.v[*b as usize].x));
    let xs: Vec<f64> = order.iter().map(|i| m.v[*i as usize].x).collect();
    let mut out: Vec<[u32; 3]> = Vec::with_capacity(m.t.len());
    let triangles = std::mem::take(&mut m.t);
    for t in &triangles {
        let mut ring: Vec<u32> = Vec::with_capacity(6);
        let mut split_any = false;
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            ring.push(a);
            let (pa, pb) = (m.v[a as usize], m.v[b as usize]);
            let ab = pb - pa;
            let len2 = ab.length_squared();
            if len2 < 1e-24 {
                continue;
            }
            let (lo, hi) = (pa.x.min(pb.x) - EPS, pa.x.max(pb.x) + EPS);
            let mut extra: Vec<(f64, u32)> = Vec::new();
            for j in xs.partition_point(|x| *x < lo)..n {
                if xs[j] > hi {
                    break;
                }
                let c = order[j];
                if c == a || c == b {
                    continue;
                }
                let pc = m.v[c as usize];
                let s = (pc - pa).dot(ab) / len2;
                if s <= 0.0 || s >= 1.0 {
                    continue;
                }
                if (pc - (pa + ab * s)).length() <= EPS
                    && (pc - pa).length() > EPS
                    && (pc - pb).length() > EPS
                {
                    extra.push((s, c));
                }
            }
            extra.sort_by(|x, y| x.0.total_cmp(&y.0));
            split_any |= !extra.is_empty();
            ring.extend(extra.into_iter().map(|(_, c)| c));
        }
        if !split_any {
            out.push(*t);
            continue;
        }
        // A fan around the centre of the triangle: the triangle is convex, so every piece has an
        // area, even where several points lie on one edge.
        let centre = (m.v[t[0] as usize] + m.v[t[1] as usize] + m.v[t[2] as usize]) / 3.0;
        let c = m.v.len() as u32;
        m.v.push(centre);
        for k in 0..ring.len() {
            out.push([c, ring[k], ring[(k + 1) % ring.len()]]);
        }
    }
    m.t = out;
}

/// Triangles times vertices that the healing of T-junctions accepts.
const HEAL_BUDGET: usize = 60_000_000;

/// The operation on two polygon sets that face outward.
fn run(pa: Vec<Polygon>, pb: Vec<Polygon>, op: Op) -> Result<Vec<Polygon>, CsgError> {
    if pa.len() + pb.len() > MAX_POLYGONS {
        return Err(CsgError::TooBig);
    }
    let mut a = Node::from(pa)?;
    let mut b = Node::from(pb)?;
    match op {
        Op::Union => {
            a.clip_to(&b);
            b.clip_to(&a);
            b.invert();
            b.clip_to(&a);
            b.invert();
            let mut rest = Vec::new();
            b.all(&mut rest);
            a.build(rest, 0)?;
        }
        Op::Difference => {
            a.invert();
            a.clip_to(&b);
            b.clip_to(&a);
            b.invert();
            b.clip_to(&a);
            b.invert();
            let mut rest = Vec::new();
            b.all(&mut rest);
            a.build(rest, 0)?;
            a.invert();
        }
        Op::Intersection => {
            a.invert();
            b.clip_to(&a);
            b.invert();
            a.clip_to(&b);
            b.clip_to(&a);
            let mut rest = Vec::new();
            b.all(&mut rest);
            a.build(rest, 0)?;
            a.invert();
        }
    }
    if a.count() > MAX_POLYGONS {
        return Err(CsgError::TooBig);
    }
    let mut out = Vec::new();
    a.all(&mut out);
    Ok(out)
}

/// `a` combined with `b`. Both must be closed. The result faces outward.
pub fn boolean(a: &MeshData, b: &MeshData, op: Op) -> Result<MeshData, CsgError> {
    let out = run(polygons_of(a)?, polygons_of(b)?, op)?;
    Ok(mesh_of(&out))
}

/// `host` less every cutter, one after the other. The host and each cutter must be closed; the
/// intermediate results are polygon sets (they may have T-junctions) and are not checked again.
pub fn subtract_all(host: &MeshData, cutters: &[MeshData]) -> Result<MeshData, CsgError> {
    let mut cur = polygons_of(host)?;
    for c in cutters {
        cur = run(cur, polygons_of(c)?, Op::Difference)?;
        if cur.is_empty() {
            return Err(CsgError::Empty);
        }
    }
    Ok(mesh_of(&cur))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(lo: [f64; 3], hi: [f64; 3]) -> MeshData {
        MeshData::cuboid(DVec3::from(lo), DVec3::from(hi))
    }

    fn vol(m: &MeshData) -> f64 {
        signed_volume(m)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn a_hole_through_a_wall_takes_its_volume() {
        // Wall 5000 x 250 x 3000; opening 1000 x 250 x 1200 flush with both faces.
        let wall = bx([0.0, 0.0, 0.0], [5000.0, 250.0, 3000.0]);
        let open = bx([1000.0, 0.0, 900.0], [2000.0, 250.0, 2100.0]);
        let r = boolean(&wall, &open, Op::Difference).unwrap();
        assert!(
            close(vol(&r), 5000.0 * 250.0 * 3000.0 - 1000.0 * 250.0 * 1200.0),
            "{}",
            vol(&r)
        );
        assert!(
            is_closed(&r),
            "the result of a cut is closed (no T-junction is left)"
        );
        // An opening that sticks out of both faces gives the same result.
        let open2 = bx([1000.0, -10.0, 900.0], [2000.0, 260.0, 2100.0]);
        let r2 = boolean(&wall, &open2, Op::Difference).unwrap();
        assert!(close(vol(&r2), vol(&r)));
    }

    #[test]
    fn blind_pockets_notches_and_missing_the_host() {
        let host = bx([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let pocket = bx([2.0, 2.0, 5.0], [4.0, 4.0, 12.0]);
        let r = boolean(&host, &pocket, Op::Difference).unwrap();
        assert!(close(vol(&r), 1000.0 - 2.0 * 2.0 * 5.0));
        // A corner notch.
        let notch = bx([5.0, 5.0, -1.0], [11.0, 11.0, 11.0]);
        let r = boolean(&host, &notch, Op::Difference).unwrap();
        assert!(close(vol(&r), 1000.0 - 5.0 * 5.0 * 10.0));
        // Far away: nothing changes.
        let far = bx([20.0, 20.0, 20.0], [30.0, 30.0, 30.0]);
        assert!(close(
            vol(&boolean(&host, &far, Op::Difference).unwrap()),
            1000.0
        ));
        // Two holes one after the other.
        let h1 = bx([1.0, 1.0, -1.0], [2.0, 2.0, 11.0]);
        let h2 = bx([6.0, 6.0, -1.0], [8.0, 8.0, 11.0]);
        let r = subtract_all(&host, &[h1.clone(), h2.clone()]).unwrap();
        assert!(close(vol(&r), 1000.0 - 10.0 - 40.0));
        assert!(is_closed(&r));
        // The result is a valid operand of the next operation.
        let again = boolean(&r, &bx([0.0, 0.0, 0.0], [10.0, 10.0, 1.0]), Op::Difference).unwrap();
        assert!(
            close(
                vol(&again),
                900.0 - 10.0 * 0.9 - 40.0 * 0.9 + 0.0 * 0.0 - 0.0
            ),
            "{}",
            vol(&again)
        );
    }

    #[test]
    fn union_and_intersection_of_overlapping_boxes() {
        let a = bx([0.0, 0.0, 0.0], [4.0, 4.0, 4.0]);
        let b = bx([2.0, 2.0, 2.0], [6.0, 6.0, 6.0]);
        assert!(close(
            vol(&boolean(&a, &b, Op::Union).unwrap()),
            64.0 + 64.0 - 8.0
        ));
        assert!(close(vol(&boolean(&a, &b, Op::Intersection).unwrap()), 8.0));
        // Boxes that share a face.
        let c = bx([4.0, 0.0, 0.0], [8.0, 4.0, 4.0]);
        assert!(close(vol(&boolean(&a, &c, Op::Union).unwrap()), 128.0));
    }

    #[test]
    fn the_orientation_of_the_inputs_does_not_matter() {
        let host = bx([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let mut inside_out = bx([2.0, 2.0, -1.0], [4.0, 4.0, 11.0]);
        inside_out.flip();
        let r = boolean(&host, &inside_out, Op::Difference).unwrap();
        assert!(close(vol(&r), 1000.0 - 40.0));
    }

    #[test]
    fn open_and_empty_meshes_are_refused() {
        let host = bx([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let mut open = host.clone();
        open.t.pop();
        assert_eq!(
            boolean(&host, &open, Op::Difference).unwrap_err(),
            CsgError::NotClosed
        );
        assert_eq!(
            boolean(&host, &MeshData::default(), Op::Union).unwrap_err(),
            CsgError::Empty
        );
        assert!(CsgError::TooBig.to_string().contains("complex"));
        assert!(CsgError::NotClosed.to_string().contains("closed"));
        assert!(CsgError::Empty.to_string().contains("volume"));
    }

    #[test]
    fn turned_boxes_and_a_cylinder_in_a_box() {
        // A box turned by 30 degrees about Z cut by an upright box.
        let mut host = bx([-5.0, -5.0, 0.0], [5.0, 5.0, 4.0]);
        let r = glam::DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_6);
        host.transform(&r);
        let cutter = bx([-1.0, -1.0, -1.0], [1.0, 1.0, 5.0]);
        let out = boolean(&host, &cutter, Op::Difference).unwrap();
        assert!(close(vol(&out), 400.0 - 2.0 * 2.0 * 4.0), "{}", vol(&out));
        // A 24-sided prism in a box.
        let mut prism = MeshData::default();
        let n = 24;
        let ring: Vec<DVec3> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                DVec3::new(2.0 * a.cos(), 2.0 * a.sin(), 0.0)
            })
            .collect();
        let area = 0.5 * n as f64 * 4.0 * (std::f64::consts::TAU / n as f64).sin();
        prism.v.extend(ring.iter().copied());
        prism.v.extend(ring.iter().map(|p| *p + DVec3::Z * 20.0));
        for i in 0..n as u32 {
            let j = (i + 1) % n as u32;
            prism.t.push([i, j, n as u32 + j]);
            prism.t.push([i, n as u32 + j, n as u32 + i]);
        }
        let c = prism.v.len() as u32;
        prism.v.push(DVec3::ZERO);
        prism.v.push(DVec3::Z * 20.0);
        for i in 0..n as u32 {
            let j = (i + 1) % n as u32;
            prism.t.push([c, j, i]);
            prism.t.push([c + 1, n as u32 + i, n as u32 + j]);
        }
        let host = bx([-5.0, -5.0, 2.0], [5.0, 5.0, 12.0]);
        let out = boolean(&host, &prism, Op::Difference).unwrap();
        assert!(
            close(vol(&out), 1000.0 - area * 10.0),
            "{} {}",
            vol(&out),
            area
        );
    }
}
