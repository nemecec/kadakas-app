//! The triangulated irregular network (TIN): a constrained Delaunay triangulation of source
//! points, with elevation lookup, slope and aspect.

use std::collections::{HashMap, HashSet};

use glam::{DVec2, DVec3};
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};

use crate::SiteError;
use crate::grid::GridIndex;

/// Source data of a surface: loose points, and polylines whose segments the triangulation keeps
/// (contours, breaklines, 3D polygons).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceData {
    pub points: Vec<DVec3>,
    pub lines: Vec<SourceLine>,
}

/// A polyline of source data. All its vertices are source points too.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceLine {
    pub points: Vec<DVec3>,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug)]
struct V {
    pos: Point2<f64>,
    z: f64,
}

impl HasPosition for V {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.pos
    }
}

type Handle = spade::handles::FixedVertexHandle;

/// Collects points and constraint edges, then makes a `Tin`. Used by `Tin::build` and by the
/// code that builds the proposed surface.
pub struct TinBuilder {
    cdt: ConstrainedDelaunayTriangulation<V>,
    /// Handle of the point at each XY, so that the first point at a place wins.
    seen: HashMap<(u64, u64), Handle>,
}

impl Default for TinBuilder {
    fn default() -> Self {
        TinBuilder { cdt: ConstrainedDelaunayTriangulation::new(), seen: HashMap::new() }
    }
}

impl TinBuilder {
    /// Adds a point. A point at the same XY as an earlier one is ignored, so the first one wins.
    /// Returns a handle for `add_edge`.
    pub fn add_point(&mut self, p: DVec3) -> Result<Handle, SiteError> {
        if !p.is_finite() {
            return Err(SiteError::NotFinite);
        }
        // Adding 0.0 turns -0.0 into 0.0, so both spellings of zero are one place.
        let key = ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits());
        if let Some(h) = self.seen.get(&key) {
            return Ok(*h);
        }
        let h = self.cdt.insert(V { pos: Point2::new(p.x, p.y), z: p.z }).map_err(|_| SiteError::NotFinite)?;
        self.seen.insert(key, h);
        Ok(h)
    }

    /// Keeps the segment between two points as an edge of the triangulation. Returns false, and
    /// does nothing, if the segment would cross an edge that is already kept.
    pub fn add_edge(&mut self, a: Handle, b: Handle) -> bool {
        if a == b || !self.cdt.can_add_constraint(a, b) {
            return false;
        }
        self.cdt.add_constraint(a, b);
        true
    }

    /// Adds a polyline as points and edges. Returns the number of edges that were refused.
    pub fn add_line(&mut self, points: &[DVec3], closed: bool) -> Result<usize, SiteError> {
        let handles = points.iter().map(|p| self.add_point(*p)).collect::<Result<Vec<_>, _>>()?;
        let n = handles.len();
        let segs = if closed && n > 2 { n } else { n.saturating_sub(1) };
        let mut refused = 0;
        for i in 0..segs {
            if !self.add_edge(handles[i], handles[(i + 1) % n]) && handles[i] != handles[(i + 1) % n] {
                refused += 1;
            }
        }
        Ok(refused)
    }

    /// Makes the TIN. Fails if all points lie on one line.
    pub fn finish(self) -> Result<Tin, SiteError> {
        let mut vertices = vec![DVec3::ZERO; self.cdt.num_vertices()];
        for v in self.cdt.vertices() {
            let d = v.data();
            vertices[v.fix().index()] = DVec3::new(d.pos.x, d.pos.y, d.z);
        }
        let triangles: Vec<[u32; 3]> = self
            .cdt
            .inner_faces()
            .map(|f| {
                let [a, b, c] = f.vertices().map(|v| v.fix().index() as u32);
                // Keep every triangle counterclockwise.
                let (pa, pb, pc) = (vertices[a as usize].truncate(), vertices[b as usize].truncate(), vertices[c as usize].truncate());
                if (pb - pa).perp_dot(pc - pa) < 0.0 { [a, c, b] } else { [a, b, c] }
            })
            .collect();
        Tin::from_parts(vertices, triangles)
    }
}

/// Slope and aspect of a surface at a place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlopeAspect {
    /// Rise over run: 0.1 is a 10 percent slope.
    pub slope: f64,
    /// Direction that the surface faces (downhill), in radians clockwise from +Y (north).
    /// `None` on flat ground.
    pub aspect: Option<f64>,
}

impl SlopeAspect {
    pub fn percent(&self) -> f64 {
        self.slope * 100.0
    }

    /// The slope angle in radians.
    pub fn angle(&self) -> f64 {
        self.slope.atan()
    }
}

/// A triangulated surface. Triangles are counterclockwise seen from above.
#[derive(Clone, Debug)]
pub struct Tin {
    pub vertices: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    index: GridIndex,
}

impl Tin {
    /// Triangulates source data with the segments of its lines as constraints. A segment that
    /// would cross an earlier one is dropped, and its two points stay.
    pub fn build(src: &SourceData) -> Result<Tin, SiteError> {
        let mut b = TinBuilder::default();
        for p in &src.points {
            b.add_point(*p)?;
        }
        for l in &src.lines {
            b.add_line(&l.points, l.closed)?;
        }
        b.finish()
    }

    /// A TIN from vertices and triangles. Triangles that are clockwise are turned around, and
    /// triangles with no area are dropped.
    pub fn from_parts(vertices: Vec<DVec3>, triangles: Vec<[u32; 3]>) -> Result<Tin, SiteError> {
        let mut tris = Vec::with_capacity(triangles.len());
        for [a, b, c] in triangles {
            let p = |i: u32| vertices.get(i as usize).map(|v| v.truncate());
            let (Some(pa), Some(pb), Some(pc)) = (p(a), p(b), p(c)) else {
                return Err(SiteError::BadTriangle);
            };
            let cross = (pb - pa).perp_dot(pc - pa);
            if cross > 0.0 {
                tris.push([a, b, c]);
            } else if cross < 0.0 {
                tris.push([a, c, b]);
            }
        }
        if tris.is_empty() {
            return Err(SiteError::NoSurface);
        }
        let boxes: Vec<(DVec2, DVec2)> = tris
            .iter()
            .map(|t| {
                let p = t.map(|i| vertices[i as usize].truncate());
                (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]))
            })
            .collect();
        let index = GridIndex::new(&boxes);
        Ok(Tin { vertices, triangles: tris, index })
    }

    /// The three corners of triangle `i`.
    pub fn corners(&self, i: usize) -> [DVec3; 3] {
        self.triangles[i].map(|v| self.vertices[v as usize])
    }

    /// The triangle that contains `xy` and the barycentric weights of the corners. A point on
    /// an edge belongs to one of the triangles that share it.
    pub fn locate(&self, xy: DVec2) -> Option<(usize, [f64; 3])> {
        if !xy.is_finite() {
            return None;
        }
        for &t in self.index.near_point(xy) {
            let [a, b, c] = self.corners(t as usize).map(|p| p.truncate());
            let area = (b - a).perp_dot(c - a);
            let w0 = (b - xy).perp_dot(c - xy) / area;
            let w1 = (c - xy).perp_dot(a - xy) / area;
            let w2 = 1.0 - w0 - w1;
            const EPS: f64 = -1e-12;
            if w0 >= EPS && w1 >= EPS && w2 >= EPS {
                return Some((t as usize, [w0, w1, w2]));
            }
        }
        None
    }

    /// Elevation at a point, or `None` outside the surface.
    pub fn elevation_at(&self, xy: DVec2) -> Option<f64> {
        let (t, w) = self.locate(xy)?;
        let c = self.corners(t);
        Some(w[0] * c[0].z + w[1] * c[1].z + w[2] * c[2].z)
    }

    /// Slope and aspect of triangle `i`.
    pub fn triangle_slope(&self, i: usize) -> SlopeAspect {
        let [a, b, c] = self.corners(i);
        let (u, v) = (b - a, c - a);
        let det = u.x * v.y - u.y * v.x;
        // Gradient of z: solve u.xy . g = u.z and v.xy . g = v.z.
        let gx = (u.z * v.y - v.z * u.y) / det;
        let gy = (v.z * u.x - u.z * v.x) / det;
        let slope = gx.hypot(gy);
        let aspect = if slope < 1e-12 { None } else { Some((-gx).atan2(-gy).rem_euclid(std::f64::consts::TAU)) };
        SlopeAspect { slope, aspect }
    }

    /// Slope and aspect at a point, or `None` outside the surface.
    pub fn slope_at(&self, xy: DVec2) -> Option<SlopeAspect> {
        self.locate(xy).map(|(t, _)| self.triangle_slope(t))
    }

    /// Lowest and highest elevation.
    pub fn z_range(&self) -> (f64, f64) {
        self.vertices.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v.z), hi.max(v.z)))
    }

    /// Box around the surface in XY.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        self.vertices.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), v| {
            (lo.min(v.truncate()), hi.max(v.truncate()))
        })
    }

    /// Area of the surface seen from above.
    pub fn area_xy(&self) -> f64 {
        (0..self.triangles.len())
            .map(|i| {
                let [a, b, c] = self.corners(i);
                (b - a).truncate().perp_dot((c - a).truncate()) / 2.0
            })
            .sum()
    }

    /// Edges that belong to one triangle, as (from, to) in the direction of that triangle.
    pub fn boundary_edges(&self) -> Vec<(u32, u32)> {
        let mut count: HashMap<(u32, u32), (u32, (u32, u32))> = HashMap::with_capacity(self.triangles.len() * 2);
        for t in &self.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let e = count.entry((a.min(b), a.max(b))).or_insert((0, (a, b)));
                e.0 += 1;
            }
        }
        count.into_values().filter(|(n, _)| *n == 1).map(|(_, e)| e).collect()
    }

    /// Vertices on the outer edge of the surface (edges that belong to one triangle).
    pub fn boundary_vertices(&self) -> HashSet<u32> {
        self.boundary_edges().into_iter().flat_map(|(a, b)| [a, b]).collect()
    }

    /// Vertical walls: pairs of outer edges that have the same place in XY and different heights
    /// (the edge of a pad without a grade limit). Each wall is a quad a1, b1, b2, a2.
    pub fn walls(&self) -> Vec<[DVec3; 4]> {
        let key = |p: DVec3| ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits());
        type Place = ((u64, u64), (u64, u64));
        let mut by_place: HashMap<Place, Vec<(DVec3, DVec3)>> = HashMap::new();
        for (a, b) in self.boundary_edges() {
            let (pa, pb) = (self.vertices[a as usize], self.vertices[b as usize]);
            let (ka, kb) = (key(pa), key(pb));
            if ka <= kb {
                by_place.entry((ka, kb)).or_default().push((pa, pb));
            } else {
                by_place.entry((kb, ka)).or_default().push((pb, pa));
            }
        }
        let mut out = Vec::new();
        for pairs in by_place.into_values() {
            if let [(a1, b1), (a2, b2)] = pairs[..]
                && (a1.z != a2.z || b1.z != b2.z)
            {
                out.push([a1, b1, b2, a2]);
            }
        }
        out
    }

    /// Candidate triangles for a box, without repeats.
    pub(crate) fn triangles_in_box(&self, lo: DVec2, hi: DVec2) -> Vec<u32> {
        let mut out = Vec::new();
        self.index.for_each_in_box(lo, hi, |i| out.push(i));
        out.sort_unstable();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn grid_points(n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> Vec<DVec3> {
        let mut v = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 * step, j as f64 * step);
                v.push(DVec3::new(x, y, f(x, y)));
            }
        }
        v
    }

    #[test]
    fn plane_is_reproduced_everywhere() {
        let f = |x: f64, y: f64| 100.0 + 0.1 * x - 0.05 * y;
        let tin = Tin::build(&SourceData { points: grid_points(6, 10.0, f), lines: vec![] }).unwrap();
        assert_eq!(tin.triangles.len(), 2 * 25);
        for &(x, y) in &[(0.0, 0.0), (13.7, 22.1), (50.0, 50.0), (49.99, 0.01)] {
            let z = tin.elevation_at(DVec2::new(x, y)).unwrap();
            assert!((z - f(x, y)).abs() < 1e-9, "{x} {y}");
        }
        assert_eq!(tin.elevation_at(DVec2::new(-1.0, 5.0)), None);
        assert_eq!(tin.elevation_at(DVec2::new(f64::NAN, 5.0)), None);
        assert!((tin.area_xy() - 2500.0).abs() < 1e-9);
        assert_eq!(tin.bounds(), (DVec2::ZERO, DVec2::splat(50.0)));
        assert_eq!(tin.z_range(), (f(0.0, 50.0), f(50.0, 0.0)));
    }

    #[test]
    fn source_points_keep_their_elevation() {
        let pts = grid_points(5, 7.0, |x, y| (x * 0.3).sin() * 4.0 + y * 0.2);
        let tin = Tin::build(&SourceData { points: pts.clone(), lines: vec![] }).unwrap();
        for p in pts {
            assert!((tin.elevation_at(p.truncate()).unwrap() - p.z).abs() < 1e-9);
        }
    }

    #[test]
    fn duplicate_xy_keeps_the_first_elevation() {
        let pts = vec![DVec3::new(0.0, 0.0, 1.0), DVec3::new(0.0, 0.0, 9.0), DVec3::new(10.0, 0.0, 1.0), DVec3::new(0.0, 10.0, 1.0)];
        let tin = Tin::build(&SourceData { points: pts, lines: vec![] }).unwrap();
        assert_eq!(tin.vertices.len(), 3);
        assert!((tin.elevation_at(DVec2::ZERO).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn collinear_and_few_points_make_no_surface() {
        let line = (0..5).map(|i| DVec3::new(i as f64, 0.0, 0.0)).collect();
        assert_eq!(Tin::build(&SourceData { points: line, lines: vec![] }).unwrap_err(), SiteError::NoSurface);
        assert_eq!(Tin::build(&SourceData::default()).unwrap_err(), SiteError::NoSurface);
    }

    #[test]
    fn non_finite_points_are_refused() {
        let src = SourceData { points: vec![DVec3::new(f64::NAN, 0.0, 0.0)], lines: vec![] };
        assert_eq!(Tin::build(&src).unwrap_err(), SiteError::NotFinite);
        let src = SourceData { points: vec![], lines: vec![SourceLine { points: vec![DVec3::new(0.0, f64::INFINITY, 0.0)], closed: false }] };
        assert_eq!(Tin::build(&src).unwrap_err(), SiteError::NotFinite);
    }

    #[test]
    fn constraint_segments_become_edges() {
        // A long diagonal that Delaunay would not choose by itself.
        let pts = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 1.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
        ];
        let line = SourceLine { points: vec![pts[0], pts[2]], closed: false };
        let tin = Tin::build(&SourceData { points: pts, lines: vec![line] }).unwrap();
        let has_edge = tin.triangles.iter().any(|t| t.contains(&0) && t.contains(&2));
        assert!(has_edge);
        let edges: HashSet<(u32, u32)> = tin
            .triangles
            .iter()
            .flat_map(|t| (0..3).map(move |k| (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]))))
            .collect();
        let idx = |p: DVec3| tin.vertices.iter().position(|v| *v == p).unwrap() as u32;
        let (a, b) = (idx(DVec3::new(0.0, 0.0, 0.0)), idx(DVec3::new(10.0, 1.0, 0.0)));
        assert!(edges.contains(&(a.min(b), a.max(b))));
    }

    #[test]
    fn crossing_constraints_are_dropped_and_counted() {
        let mut b = TinBuilder::default();
        let refused = b.add_line(&[DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 10.0, 0.0)], false).unwrap();
        assert_eq!(refused, 0);
        let refused = b.add_line(&[DVec3::new(0.0, 10.0, 0.0), DVec3::new(10.0, 0.0, 0.0)], false).unwrap();
        assert_eq!(refused, 1);
        // A closed ring, and a repeated point that makes a zero-length segment.
        let ring = [DVec3::new(20.0, 0.0, 0.0), DVec3::new(30.0, 0.0, 0.0), DVec3::new(30.0, 0.0, 0.0), DVec3::new(25.0, 9.0, 0.0)];
        assert_eq!(b.add_line(&ring, true).unwrap(), 0);
        assert!(b.finish().is_ok());
    }

    #[test]
    fn slope_and_aspect_of_a_tilted_plane() {
        // Falls toward +X by 10 percent: the surface faces east, 90 degrees clockwise from north.
        let f = |x: f64, _y: f64| 50.0 - 0.1 * x;
        let tin = Tin::build(&SourceData { points: grid_points(4, 10.0, f), lines: vec![] }).unwrap();
        let s = tin.slope_at(DVec2::new(12.0, 7.0)).unwrap();
        assert!((s.slope - 0.1).abs() < 1e-12 && (s.percent() - 10.0).abs() < 1e-9);
        assert!((s.aspect.unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!((s.angle() - 0.1f64.atan()).abs() < 1e-12);
        // Falls toward +Y: faces north, aspect 0.
        let tin = Tin::build(&SourceData { points: grid_points(4, 10.0, |_, y| -y), lines: vec![] }).unwrap();
        let a = tin.slope_at(DVec2::new(5.0, 5.0)).unwrap().aspect.unwrap();
        assert!(a.abs() < 1e-9 || (a - std::f64::consts::TAU).abs() < 1e-9);
        // Falls toward -Y: faces south.
        let tin = Tin::build(&SourceData { points: grid_points(4, 10.0, |_, y| y), lines: vec![] }).unwrap();
        assert!((tin.slope_at(DVec2::new(5.0, 5.0)).unwrap().aspect.unwrap() - std::f64::consts::PI).abs() < 1e-9);
        // Flat ground has no aspect.
        let tin = Tin::build(&SourceData { points: grid_points(3, 10.0, |_, _| 5.0), lines: vec![] }).unwrap();
        let flat = tin.slope_at(DVec2::new(5.0, 5.0)).unwrap();
        assert_eq!((flat.slope, flat.aspect), (0.0, None));
        assert!(tin.slope_at(DVec2::new(99.0, 5.0)).is_none());
    }

    #[test]
    fn boundary_vertices_are_the_hull() {
        let tin = Tin::build(&SourceData { points: grid_points(4, 10.0, |_, _| 0.0), lines: vec![] }).unwrap();
        assert_eq!(tin.boundary_vertices().len(), 12);
    }

    #[test]
    fn from_parts_fixes_winding_and_checks_indices() {
        let v = vec![DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(0.0, 0.0, 0.0)];
        let tin = Tin::from_parts(v.clone(), vec![[0, 2, 1], [0, 0, 1]]).unwrap();
        assert_eq!(tin.triangles, vec![[0, 1, 2]]);
        assert_eq!(Tin::from_parts(v.clone(), vec![[0, 1, 9]]).unwrap_err(), SiteError::BadTriangle);
        assert_eq!(Tin::from_parts(v, vec![]).unwrap_err(), SiteError::NoSurface);
    }
}
