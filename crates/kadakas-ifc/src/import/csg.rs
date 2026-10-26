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
pub const MAX_POLYGONS: usize = 150_000;
/// The most work (split tests) that one boolean operation may use. The import has a budget of its own.
pub const PER_CALL_FUEL: u64 = 60_000_000;
/// The work of all booleans of one import.
pub const IMPORT_FUEL: u64 = 1_500_000_000;
/// The deepest tree.
pub const MAX_DEPTH: usize = 400;

/// What a boolean operation does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Union,
    Difference,
    Intersection,
}

/// A work budget that is counted, not timed, so the same file always does the same work. One unit
/// is one polygon sent through the plane of one node of a tree (a split test).
#[derive(Debug)]
pub struct Fuel {
    left: std::cell::Cell<u64>,
}

impl Fuel {
    pub fn new(units: u64) -> Fuel {
        Fuel {
            left: std::cell::Cell::new(units),
        }
    }

    /// The units that are left.
    pub fn left(&self) -> u64 {
        self.left.get()
    }

    /// Uses `n` units; the budget stays at zero when it was too small.
    pub fn spend(&self, n: u64) {
        self.left.set(self.left.get().saturating_sub(n));
    }

    /// Runs `f` with a budget of at most `cap` units taken from this one; what `f` used is spent
    /// here. A single operation cannot use up the budget of the whole import.
    pub fn scoped<T>(&self, cap: u64, f: impl FnOnce(&Fuel) -> T) -> T {
        let own = Fuel::new(self.left().min(cap));
        let before = own.left();
        let r = f(&own);
        self.spend(before - own.left());
        r
    }

    fn take(&self, n: u64) -> Result<(), CsgError> {
        let l = self.left.get();
        if n > l {
            self.left.set(0);
            return Err(CsgError::OutOfFuel);
        }
        self.left.set(l - n);
        Ok(())
    }
}

/// A box in space.
#[derive(Clone, Copy, Debug)]
struct Aabb {
    lo: DVec3,
    hi: DVec3,
}

impl Aabb {
    fn of(polys: &[Polygon]) -> Aabb {
        let mut b = Aabb {
            lo: DVec3::splat(f64::INFINITY),
            hi: DVec3::splat(f64::NEG_INFINITY),
        };
        for p in polys {
            for v in &p.v {
                b.lo = b.lo.min(*v);
                b.hi = b.hi.max(*v);
            }
        }
        b
    }

    fn of_polygon(p: &Polygon) -> Aabb {
        Aabb::of(std::slice::from_ref(p))
    }

    fn touches(&self, o: &Aabb) -> bool {
        self.lo.cmple(o.hi + DVec3::splat(EPS)).all()
            && o.lo.cmple(self.hi + DVec3::splat(EPS)).all()
    }
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
    /// The work budget ([`Fuel`]) is used up.
    OutOfFuel,
}

impl std::fmt::Display for CsgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CsgError::NotClosed => "a solid is not closed",
            CsgError::Empty => "a solid has no volume",
            CsgError::TooBig => "the solids are too complex for a boolean operation",
            CsgError::OutOfFuel => "the work budget for boolean operations is used up",
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
    fn from(polygons: Vec<Polygon>, fuel: &Fuel) -> Result<Node, CsgError> {
        let mut n = Node::default();
        n.build(polygons, 0, fuel)?;
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
    fn clip_polygons(&self, polys: Vec<Polygon>, fuel: &Fuel) -> Result<Vec<Polygon>, CsgError> {
        let Some(plane) = &self.plane else {
            return Ok(polys);
        };
        fuel.take(polys.len() as u64 + 1)?;
        let (mut front, mut back) = (Vec::new(), Vec::new());
        let mut cf = Vec::new();
        let mut cb = Vec::new();
        for p in &polys {
            split(plane, p, &mut cf, &mut cb, &mut front, &mut back);
        }
        front.append(&mut cf);
        back.append(&mut cb);
        // The polygons that are alive at once are the memory of the work.
        if front.len() + back.len() > MAX_POLYGONS {
            return Err(CsgError::TooBig);
        }
        let mut front = match &self.front {
            Some(f) => f.clip_polygons(front, fuel)?,
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polygons(back, fuel)?,
            None => Vec::new(),
        };
        front.extend(back);
        Ok(front)
    }

    /// Clips the polygons of this tree by `other`. With a box, a polygon that is outside of it is
    /// left as it is (it cannot touch the solid of `other`).
    fn clip_to(&mut self, other: &Node, only: Option<&Aabb>, fuel: &Fuel) -> Result<(), CsgError> {
        let own = std::mem::take(&mut self.polygons);
        self.polygons = match only {
            None => other.clip_polygons(own, fuel)?,
            Some(b) => {
                let (near, mut far): (Vec<Polygon>, Vec<Polygon>) = own
                    .into_iter()
                    .partition(|p| Aabb::of_polygon(p).touches(b));
                far.extend(other.clip_polygons(near, fuel)?);
                far
            }
        };
        if let Some(f) = &mut self.front {
            f.clip_to(other, only, fuel)?;
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other, only, fuel)?;
        }
        Ok(())
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

    fn build(&mut self, polys: Vec<Polygon>, depth: usize, fuel: &Fuel) -> Result<(), CsgError> {
        if polys.is_empty() {
            return Ok(());
        }
        if depth > MAX_DEPTH || polys.len() > MAX_POLYGONS {
            return Err(CsgError::TooBig);
        }
        fuel.take(polys.len() as u64 + 1)?;
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
            node.build(front, depth + 1, fuel)?;
        }
        if !back.is_empty() {
            let node = self.back.get_or_insert_with(Default::default);
            node.build(back, depth + 1, fuel)?;
        }
        Ok(())
    }
}

/// The polygons of a closed mesh, facing outward. `Err` when the mesh is open or has no volume.
fn polygons_of(m: &MeshData, fuel: &Fuel) -> Result<Vec<Polygon>, CsgError> {
    let mut mesh = m.clone();
    mesh.weld(EPS);
    // An operand that cannot be healed is not used: the budget is used up.
    heal_t_junctions(&mut mesh, fuel, HEAL_CELLS)?;
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

fn mesh_of(polys: &[Polygon], fuel: &Fuel) -> MeshData {
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
    // A result that cannot be healed (the budget is used up) is kept as it is, not closed: the
    // caller checks `is_closed` and the import says so.
    let _ = heal_t_junctions(&mut m, fuel, heal_limit());
    m
}

/// Splits the triangles at the vertices that lie on their edges, so that the faces of a solid
/// that came out of a boolean operation meet edge to edge (the result is closed again and can be
/// the operand of the next operation).
///
/// The vertices are put in a grid whose cell size follows the size of the mesh in each direction
/// (a long thin wall gets many cells along its length and one across). An edge looks only at the
/// cells of its box, so the work follows the number of triangles and the vertices near their edges,
/// not the product of the two. The cells that edges look at are charged to `fuel` (one unit each,
/// the budget of the import). It gives up, with the mesh unchanged, when they pass the fuel or
/// [`HEAL_CELLS`] for one mesh; the caller finds out with `is_closed` (a result that is not closed is
/// marked and counted). The budget is counted, not timed.
fn heal_t_junctions(m: &mut MeshData, fuel: &Fuel, cap: u64) -> Result<(), CsgError> {
    use std::collections::HashMap;
    let n = m.v.len();
    let Some((lo, hi)) = m.bounds() else {
        return Ok(());
    };
    let extent = hi - lo;
    let longest = extent.max_element().max(1e-9);
    // Cells: at most 2048 along the longest side, and about one vertex in each.
    let floor = longest / 2048.0;
    let e = extent.max(DVec3::splat(floor));
    let size = ((e.x * e.y * e.z) / n.max(1) as f64).cbrt().max(floor);
    let cell = |p: DVec3| -> (i64, i64, i64) {
        let q = (p - lo) / size;
        (q.x.floor() as i64, q.y.floor() as i64, q.z.floor() as i64)
    };
    let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::with_capacity(n);
    for (i, p) in m.v.iter().enumerate() {
        grid.entry(cell(*p)).or_default().push(i as u32);
    }
    let mut visited = 0u64;
    let mut out: Vec<[u32; 3]> = Vec::with_capacity(m.t.len());
    let mut centres: Vec<DVec3> = Vec::new();
    for t in &m.t {
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
            let (c0, c1) = (
                cell(pa.min(pb) - DVec3::splat(EPS)),
                cell(pa.max(pb) + DVec3::splat(EPS)),
            );
            let count = ((c1.0 - c0.0 + 1) * (c1.1 - c0.1 + 1) * (c1.2 - c0.2 + 1)).max(1) as u64;
            visited += count;
            // The cells of the edges before this one are charged already: giving up costs no more.
            if visited > cap || count > fuel.left() {
                return Err(CsgError::OutOfFuel);
            }
            fuel.spend(count);
            let mut extra: Vec<(f64, u32)> = Vec::new();
            for x in c0.0..=c1.0 {
                for y in c0.1..=c1.1 {
                    for z in c0.2..=c1.2 {
                        let Some(list) = grid.get(&(x, y, z)) else {
                            continue;
                        };
                        for &c in list {
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
                    }
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
        let c = (n + centres.len()) as u32;
        centres.push(centre);
        for k in 0..ring.len() {
            out.push([c, ring[k], ring[(k + 1) % ring.len()]]);
        }
    }
    m.v.extend(centres);
    m.t = out;
    Ok(())
}

/// Cells that the healing of T-junctions may look at (a cell with a few vertices costs a few steps).
const HEAL_CELLS: u64 = 400_000_000;

#[cfg(test)]
thread_local! {
    /// A test can lower the limit to see what an import does with a result that is not healed.
    pub(crate) static HEAL_LIMIT: std::cell::Cell<u64> = const { std::cell::Cell::new(HEAL_CELLS) };
}

/// Lowers the limit until it is dropped (also when the test fails).
#[cfg(test)]
pub(crate) struct HealLimitGuard;

#[cfg(test)]
impl HealLimitGuard {
    pub(crate) fn set(n: u64) -> HealLimitGuard {
        HEAL_LIMIT.with(|c| c.set(n));
        HealLimitGuard
    }
}

#[cfg(test)]
impl Drop for HealLimitGuard {
    fn drop(&mut self) {
        HEAL_LIMIT.with(|c| c.set(HEAL_CELLS));
    }
}

#[cfg(test)]
fn heal_limit() -> u64 {
    HEAL_LIMIT.with(|c| c.get())
}

#[cfg(not(test))]
fn heal_limit() -> u64 {
    HEAL_CELLS
}

/// The operation on two polygon sets that face outward.
fn run(pa: Vec<Polygon>, pb: Vec<Polygon>, op: Op, fuel: &Fuel) -> Result<Vec<Polygon>, CsgError> {
    if pa.len() + pb.len() > MAX_POLYGONS {
        return Err(CsgError::TooBig);
    }
    let mut a = Node::from(pa, fuel)?;
    let mut b = Node::from(pb, fuel)?;
    match op {
        Op::Union => {
            a.clip_to(&b, None, fuel)?;
            b.clip_to(&a, None, fuel)?;
            b.invert();
            b.clip_to(&a, None, fuel)?;
            b.invert();
            let mut rest = Vec::new();
            b.all(&mut rest);
            a.build(rest, 0, fuel)?;
        }
        Op::Difference => {
            a.invert();
            a.clip_to(&b, None, fuel)?;
            b.clip_to(&a, None, fuel)?;
            b.invert();
            b.clip_to(&a, None, fuel)?;
            b.invert();
            let mut rest = Vec::new();
            b.all(&mut rest);
            a.build(rest, 0, fuel)?;
            a.invert();
        }
        Op::Intersection => {
            a.invert();
            b.clip_to(&a, None, fuel)?;
            b.invert();
            a.clip_to(&b, None, fuel)?;
            b.clip_to(&a, None, fuel)?;
            let mut rest = Vec::new();
            b.all(&mut rest);
            a.build(rest, 0, fuel)?;
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

/// The middle of the box of a mesh. The operands move there before the work and the result moves
/// back: far from the origin (6.5e9 mm) a double has no room left for the tolerance.
fn centre_of(m: &MeshData) -> DVec3 {
    m.bounds()
        .map_or(DVec3::ZERO, |(lo, hi)| ((lo + hi) / 2.0).round())
}

fn moved(m: &MeshData, by: DVec3) -> MeshData {
    let mut c = m.clone();
    c.exact = None;
    c.v.iter_mut().for_each(|p| *p -= by);
    c
}

fn put_back(mut m: MeshData, by: DVec3) -> MeshData {
    m.v.iter_mut().for_each(|p| *p += by);
    m
}

/// `a` combined with `b`. Both must be closed. The result faces outward.
pub fn boolean(a: &MeshData, b: &MeshData, op: Op, fuel: &Fuel) -> Result<MeshData, CsgError> {
    let c = centre_of(a);
    let out = run(
        polygons_of(&moved(a, c), fuel)?,
        polygons_of(&moved(b, c), fuel)?,
        op,
        fuel,
    )?;
    Ok(put_back(mesh_of(&out, fuel), c))
}

/// `host` less every cutter. The host and each cutter must be closed. Cutters that do not touch
/// each other (the openings of a wall) are cut in one pass: the tree of the host is built once and
/// each cutter only meets the polygons near it. Cutters that overlap are cut one after the other.
pub fn subtract_all(
    host: &MeshData,
    cutters: &[MeshData],
    fuel: &Fuel,
) -> Result<MeshData, CsgError> {
    let c = centre_of(host);
    let host_polys = polygons_of(&moved(host, c), fuel)?;
    let mut cutter_polys = Vec::with_capacity(cutters.len());
    for k in cutters {
        cutter_polys.push(polygons_of(&moved(k, c), fuel)?);
    }
    let boxes: Vec<Aabb> = cutter_polys.iter().map(|p| Aabb::of(p)).collect();
    let apart =
        (0..boxes.len()).all(|i| (i + 1..boxes.len()).all(|j| !boxes[i].touches(&boxes[j])));
    let out = if apart && !cutter_polys.is_empty() {
        // One pass: the host tree is inverted once, every cutter clips it, and the cutters are
        // clipped by it, as in the pair operation.
        let mut a = Node::from(host_polys, fuel)?;
        a.invert();
        let mut nodes = Vec::with_capacity(cutter_polys.len());
        for p in cutter_polys {
            nodes.push(Node::from(p, fuel)?);
        }
        for (n, b) in nodes.iter().zip(&boxes) {
            a.clip_to(n, Some(b), fuel)?;
        }
        let mut result = Vec::new();
        a.all(&mut result);
        for n in &mut nodes {
            n.clip_to(&a, None, fuel)?;
            n.invert();
            n.clip_to(&a, None, fuel)?;
            n.invert();
            n.all(&mut result);
        }
        result.iter_mut().for_each(Polygon::flip);
        if result.len() > MAX_POLYGONS {
            return Err(CsgError::TooBig);
        }
        result
    } else {
        let mut cur = host_polys;
        for p in cutter_polys {
            cur = run(cur, p, Op::Difference, fuel)?;
            if cur.is_empty() {
                return Err(CsgError::Empty);
            }
        }
        cur
    };
    if out.is_empty() {
        return Err(CsgError::Empty);
    }
    Ok(put_back(mesh_of(&out, fuel), c))
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

    fn fuel() -> Fuel {
        Fuel::new(u64::MAX / 2)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn a_hole_through_a_wall_takes_its_volume() {
        // Wall 5000 x 250 x 3000; opening 1000 x 250 x 1200 flush with both faces.
        let wall = bx([0.0, 0.0, 0.0], [5000.0, 250.0, 3000.0]);
        let open = bx([1000.0, 0.0, 900.0], [2000.0, 250.0, 2100.0]);
        let r = boolean(&wall, &open, Op::Difference, &fuel()).unwrap();
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
        let r2 = boolean(&wall, &open2, Op::Difference, &fuel()).unwrap();
        assert!(close(vol(&r2), vol(&r)));
    }

    #[test]
    fn blind_pockets_notches_and_missing_the_host() {
        let host = bx([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let pocket = bx([2.0, 2.0, 5.0], [4.0, 4.0, 12.0]);
        let r = boolean(&host, &pocket, Op::Difference, &fuel()).unwrap();
        assert!(close(vol(&r), 1000.0 - 2.0 * 2.0 * 5.0));
        // A corner notch.
        let notch = bx([5.0, 5.0, -1.0], [11.0, 11.0, 11.0]);
        let r = boolean(&host, &notch, Op::Difference, &fuel()).unwrap();
        assert!(close(vol(&r), 1000.0 - 5.0 * 5.0 * 10.0));
        // Far away: nothing changes.
        let far = bx([20.0, 20.0, 20.0], [30.0, 30.0, 30.0]);
        assert!(close(
            vol(&boolean(&host, &far, Op::Difference, &fuel()).unwrap()),
            1000.0
        ));
        // Two holes one after the other.
        let h1 = bx([1.0, 1.0, -1.0], [2.0, 2.0, 11.0]);
        let h2 = bx([6.0, 6.0, -1.0], [8.0, 8.0, 11.0]);
        let r = subtract_all(&host, &[h1.clone(), h2.clone()], &fuel()).unwrap();
        assert!(close(vol(&r), 1000.0 - 10.0 - 40.0));
        assert!(is_closed(&r));
        // The result is a valid operand of the next operation.
        let again = boolean(
            &r,
            &bx([0.0, 0.0, 0.0], [10.0, 10.0, 1.0]),
            Op::Difference,
            &fuel(),
        )
        .unwrap();
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
            vol(&boolean(&a, &b, Op::Union, &fuel()).unwrap()),
            64.0 + 64.0 - 8.0
        ));
        assert!(close(
            vol(&boolean(&a, &b, Op::Intersection, &fuel()).unwrap()),
            8.0
        ));
        // Boxes that share a face.
        let c = bx([4.0, 0.0, 0.0], [8.0, 4.0, 4.0]);
        assert!(close(
            vol(&boolean(&a, &c, Op::Union, &fuel()).unwrap()),
            128.0
        ));
    }

    #[test]
    fn the_orientation_of_the_inputs_does_not_matter() {
        let host = bx([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let mut inside_out = bx([2.0, 2.0, -1.0], [4.0, 4.0, 11.0]);
        inside_out.flip();
        let r = boolean(&host, &inside_out, Op::Difference, &fuel()).unwrap();
        assert!(close(vol(&r), 1000.0 - 40.0));
    }

    #[test]
    fn open_and_empty_meshes_are_refused() {
        let host = bx([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let mut open = host.clone();
        open.t.pop();
        assert_eq!(
            boolean(&host, &open, Op::Difference, &fuel()).unwrap_err(),
            CsgError::NotClosed
        );
        assert_eq!(
            boolean(&host, &MeshData::default(), Op::Union, &fuel()).unwrap_err(),
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
        let out = boolean(&host, &cutter, Op::Difference, &fuel()).unwrap();
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
        let out = boolean(&host, &prism, Op::Difference, &fuel()).unwrap();
        assert!(
            close(vol(&out), 1000.0 - area * 10.0),
            "{} {}",
            vol(&out),
            area
        );
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    fn bx(lo: [f64; 3], hi: [f64; 3]) -> MeshData {
        MeshData::cuboid(DVec3::from(lo), DVec3::from(hi))
    }

    /// The volume, computed near the origin: far from it the sum of products loses digits.
    fn vol(m: &MeshData) -> f64 {
        signed_volume(&moved(m, centre_of(m)))
    }

    /// A wall `len` long with `n` openings of 1000 x (thickness + 20) x 1000, 2000 apart.
    fn wall_with(n: usize, len: f64, at: DVec3) -> (MeshData, Vec<MeshData>) {
        let wall = MeshData::cuboid(at, at + DVec3::new(len, 250.0, 3000.0));
        let cutters = (0..n)
            .map(|i| {
                let x = 1000.0 + 2000.0 * i as f64;
                MeshData::cuboid(
                    at + DVec3::new(x, -10.0, 900.0),
                    at + DVec3::new(x + 1000.0, 260.0, 1900.0),
                )
            })
            .collect();
        (wall, cutters)
    }

    #[test]
    fn many_openings_are_cut_in_one_pass_with_a_counted_budget() {
        for n in [3usize, 300, 1000, 3000] {
            let (wall, cutters) = wall_with(n, 2000.0 * n as f64 + 1000.0, DVec3::ZERO);
            let fuel = Fuel::new(IMPORT_FUEL);
            let started = std::time::Instant::now();
            let out = subtract_all(&wall, &cutters, &fuel).unwrap();
            println!("{n} openings took {:?}", started.elapsed());
            let want = vol(&wall) - n as f64 * 1000.0 * 250.0 * 1000.0;
            assert!(
                (vol(&out) - want).abs() < 1e-6 * want,
                "{n}: {} {want}",
                vol(&out)
            );
            assert!(is_closed(&out), "{n}: the result must be closed");
            let used = IMPORT_FUEL - fuel.left();
            println!("{n} openings used {used} units of work");
            assert!(used < PER_CALL_FUEL, "{n}: {used}");
            // A hang guard only (measured for 3,000 openings: 2.8 s in a debug build, 0.23 s in a release build).
            assert!(started.elapsed().as_secs() < 300);
        }
    }

    #[test]
    fn the_cells_of_the_healing_are_charged_to_the_fuel() {
        let (wall, _) = wall_with(3, 7000.0, DVec3::ZERO);
        let fuel = Fuel::new(IMPORT_FUEL);
        let mut m = wall.clone();
        heal_t_junctions(&mut m, &fuel, HEAL_CELLS).unwrap();
        let used = IMPORT_FUEL - fuel.left();
        assert!(used > 0, "the healing used no fuel");
        // Without fuel it gives up and the mesh stays as it was.
        let none = Fuel::new(0);
        let mut m = wall.clone();
        assert!(matches!(
            heal_t_junctions(&mut m, &none, HEAL_CELLS),
            Err(CsgError::OutOfFuel)
        ));
        assert_eq!(m, wall);
        // A budget that is too small: it gives up with the mesh unchanged, and what it spent is
        // the cells of the edges before the one that did not fit, never more than the budget
        // and less than the whole healing needs.
        let small = Fuel::new(used / 2);
        let mut m = wall.clone();
        assert!(heal_t_junctions(&mut m, &small, HEAL_CELLS).is_err());
        assert_eq!(m, wall);
        let spent = used / 2 - small.left();
        assert!(
            spent > 0 && spent <= used / 2 && small.left() < used,
            "{spent}"
        );
    }

    #[test]
    fn a_wall_far_from_the_origin_is_cut() {
        // A wall at (6.5e8, 6.5e9) mm: a double has a step of about 1e-6 there, more than EPS.
        let at = DVec3::new(6.5e8, 6.5e9, 0.0);
        for n in [1usize, 3] {
            let (wall, cutters) = wall_with(n, 2000.0 * n as f64 + 1000.0, at);
            let out = subtract_all(&wall, &cutters, &Fuel::new(IMPORT_FUEL)).unwrap();
            let want = vol(&wall) - n as f64 * 1000.0 * 250.0 * 1000.0;
            assert!(
                (vol(&out) - want).abs() < 1e-6 * want.abs(),
                "{n}: {} {want}",
                vol(&out)
            );
            let (lo, _) = out.bounds().unwrap();
            assert!(
                (lo - at).length() < 1e-3,
                "the result is back in place: {lo}"
            );
        }
        // The pair operation too, with a turned wall.
        let mut wall = MeshData::cuboid(DVec3::ZERO, DVec3::new(5000.0, 250.0, 3000.0));
        wall.transform(&glam::DAffine3::from_rotation_translation(
            glam::DQuat::from_rotation_z(0.6),
            at,
        ));
        let mut hole = MeshData::cuboid(
            DVec3::new(1000.0, -10.0, 900.0),
            DVec3::new(2000.0, 260.0, 1900.0),
        );
        hole.transform(&glam::DAffine3::from_rotation_translation(
            glam::DQuat::from_rotation_z(0.6),
            at,
        ));
        let out = boolean(&wall, &hole, Op::Difference, &Fuel::new(IMPORT_FUEL)).unwrap();
        let want = 5000.0 * 250.0 * 3000.0 - 1000.0 * 250.0 * 1000.0;
        assert!(
            (vol(&out) - want).abs() < 1e-6 * want,
            "{} {want}",
            vol(&out)
        );
    }

    #[test]
    fn overlapping_cutters_are_cut_one_after_the_other() {
        let host = bx([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let a = bx([2.0, 2.0, -1.0], [6.0, 6.0, 11.0]);
        let b = bx([4.0, 4.0, -1.0], [8.0, 8.0, 11.0]);
        let out = subtract_all(&host, &[a, b], &Fuel::new(IMPORT_FUEL)).unwrap();
        // 16 + 16 - 4 = 28 of the base, 10 high.
        assert!((vol(&out) - (1000.0 - 280.0)).abs() < 1e-6, "{}", vol(&out));
    }

    #[test]
    fn a_used_up_budget_stops_the_work_safely() {
        let (wall, cutters) = wall_with(50, 100_000.0, DVec3::ZERO);
        let fuel = Fuel::new(200);
        assert_eq!(
            subtract_all(&wall, &cutters, &fuel).unwrap_err(),
            CsgError::OutOfFuel
        );
        assert_eq!(fuel.left(), 0);
        assert!(CsgError::OutOfFuel.to_string().contains("budget"));
        // A scope cannot use more than its cap, and spends what it used.
        let g = Fuel::new(1_000_000);
        let r = g.scoped(10, |own| {
            assert_eq!(own.left(), 10);
            own.take(4).unwrap();
            own.take(100)
        });
        assert_eq!(r, Err(CsgError::OutOfFuel));
        assert_eq!(g.left(), 1_000_000 - 10);
    }
}
