//! Geometry of IFC representation items: profiles, curves, extruded solids, face sets, breps,
//! mapped items and simple clipping. Everything is in the units of the file. The importer scales
//! to millimeters when it emits objects.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use cadektor_core::curve::{self, Vertex, VertexKind};
use glam::{DAffine2, DAffine3, DVec2, DVec3};

use super::ctx::{Ctx, attr, enum_of, flag, int, items, num, numbers};
use crate::step::Value;

/// An indexed triangle mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub v: Vec<DVec3>,
    pub t: Vec<[u32; 3]>,
}

impl MeshData {
    pub fn append(&mut self, o: &MeshData) {
        let base = self.v.len() as u32;
        self.v.extend_from_slice(&o.v);
        self.t
            .extend(o.t.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }

    pub fn transform(&mut self, xf: &DAffine3) {
        for p in &mut self.v {
            *p = xf.transform_point3(*p);
        }
        if xf.matrix3.determinant() < 0.0 {
            self.flip();
        }
    }

    pub fn flip(&mut self) {
        for t in &mut self.t {
            t.swap(1, 2);
        }
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        if self.v.is_empty() {
            return None;
        }
        Some(self.v.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        ))
    }

    /// Merges vertices that are closer than `tol` (on a grid) and drops triangles that collapse.
    pub fn weld(&mut self, tol: f64) {
        let q = |p: &DVec3| {
            (
                (p.x / tol).round() as i64,
                (p.y / tol).round() as i64,
                (p.z / tol).round() as i64,
            )
        };
        let mut map: HashMap<(i64, i64, i64), u32> = HashMap::with_capacity(self.v.len());
        let mut remap = Vec::with_capacity(self.v.len());
        let mut out: Vec<DVec3> = Vec::new();
        for p in &self.v {
            let id = *map.entry(q(p)).or_insert_with(|| {
                out.push(*p);
                (out.len() - 1) as u32
            });
            remap.push(id);
        }
        self.t = self
            .t
            .iter()
            .map(|t| t.map(|i| remap[i as usize]))
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect();
        self.v = out;
    }

    /// An axis-aligned box with 12 triangles facing outward.
    pub fn cuboid(lo: DVec3, hi: DVec3) -> MeshData {
        let c = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        let v = vec![
            c(false, false, false),
            c(true, false, false),
            c(true, true, false),
            c(false, true, false),
            c(false, false, true),
            c(true, false, true),
            c(true, true, true),
            c(false, true, true),
        ];
        let t = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        MeshData { v, t }
    }
}

/// One closed area: an outer outline and holes. Outlines are curved vertices (see `curve`).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Vec<Vertex>,
    pub holes: Vec<Vec<Vertex>>,
}

/// A profile pushed along a direction. `dir` is in the local frame and has the length of the depth.
#[derive(Clone, Debug, PartialEq)]
pub struct Extrusion {
    pub regions: Vec<Region>,
    pub dir: DVec3,
    pub xf: DAffine3,
    /// Largest distance between a curve and its flat segments, in file units.
    pub tol: f64,
}

impl Extrusion {
    /// True if the profile plane is horizontal and the extrusion goes straight up or down.
    pub fn is_upright(&self) -> bool {
        let m = self.xf.matrix3;
        let up = m.z_axis.normalize_or_zero();
        let flat = |v: DVec3| v.x.abs() < 1e-9 && v.y.abs() < 1e-9;
        flat(up)
            && up.z.abs() > 0.0
            && self.dir.x.abs() < 1e-9 * self.dir.length().max(1.0)
            && self.dir.y.abs() < 1e-9 * self.dir.length().max(1.0)
    }

    /// Height in world units (absolute, along Z) for an upright extrusion.
    pub fn height(&self) -> f64 {
        (self.xf.matrix3 * self.dir).z.abs()
    }

    /// The closed triangle mesh of the solid, in world coordinates.
    pub fn mesh(&self) -> MeshData {
        let mut out = MeshData::default();
        for r in &self.regions {
            let ring = |vs: &[Vertex]| curve::flatten(vs, true, self.tol.max(1e-9));
            let outer = clean(&ring(&r.outer), true);
            if outer.len() < 3 {
                continue;
            }
            let holes: Vec<Vec<DVec2>> = r
                .holes
                .iter()
                .map(|h| clean(&ring(h), true))
                .filter(|h| h.len() >= 3)
                .collect();
            let mut m = MeshData::default();
            // Caps. Holes are cut out by splitting the area into pieces without holes.
            let pieces = crate::geom::split_with_holes(&outer, &holes);
            for piece in &pieces {
                let tris = cadektor_core::mesh::triangulate(piece);
                let base = m.v.len() as u32;
                m.v.extend(piece.iter().map(|p| p.extend(0.0)));
                let top = m.v.len() as u32;
                m.v.extend(piece.iter().map(|p| p.extend(0.0) + self.dir));
                for [a, b, c] in tris {
                    // Bottom faces down (clockwise from above), top faces up.
                    m.t.push([base + a as u32, base + c as u32, base + b as u32]);
                    m.t.push([top + a as u32, top + b as u32, top + c as u32]);
                }
            }
            // Walls. The outline runs counterclockwise and the holes clockwise.
            let mut rings: Vec<Vec<DVec2>> = vec![outer.clone()];
            rings.extend(holes.iter().map(|h| h.iter().rev().copied().collect()));
            for ring in &rings {
                let base = m.v.len() as u32;
                let n = ring.len() as u32;
                m.v.extend(ring.iter().map(|p| p.extend(0.0)));
                m.v.extend(ring.iter().map(|p| p.extend(0.0) + self.dir));
                for i in 0..n {
                    let j = (i + 1) % n;
                    m.t.push([base + i, base + j, base + n + j]);
                    m.t.push([base + i, base + n + j, base + n + i]);
                }
            }
            // An extrusion against the profile normal turns the whole solid inside out.
            if self.dir.z < 0.0 {
                m.flip();
            }
            out.append(&m);
        }
        out.transform(&self.xf);
        out
    }
}

/// Removes repeated points and the closing point, and sets the winding. `None` winding is kept.
fn clean(points: &[DVec2], ccw: bool) -> Vec<DVec2> {
    let mut ring: Vec<DVec2> = Vec::with_capacity(points.len());
    for p in points {
        if !p.is_finite() {
            return Vec::new();
        }
        if ring.last().is_none_or(|l| l.distance_squared(*p) > 1e-18) {
            ring.push(*p);
        }
    }
    while ring.len() > 1 && ring[0].distance_squared(ring[ring.len() - 1]) <= 1e-18 {
        ring.pop();
    }
    if ring.len() < 3 {
        return Vec::new();
    }
    let area = crate::geom::signed_area(&ring);
    if area == 0.0 {
        return Vec::new();
    }
    if (area < 0.0) == ccw {
        ring.reverse();
    }
    ring
}

/// A curve in the world, for annotations. `exact` holds curved vertices when the curve is flat and
/// horizontal.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveData {
    pub exact: Option<(Vec<Vertex>, f64)>,
    pub pts: Vec<DVec3>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Mesh(MeshData),
    Extrusion(Extrusion),
    /// A mapped item that becomes a symbol instance. `xf` takes the source frame to the world.
    Instance {
        source: u64,
        xf: DAffine3,
    },
    Curve(CurveData),
    Point(DVec3),
}

/// Evaluates representation items.
pub struct Eval<'a> {
    pub cx: &'a Ctx<'a>,
    pub pieces: Vec<Piece>,
    pub issues: Vec<String>,
    /// True when a bounding box stands in for geometry that is not supported.
    pub approximate: bool,
    pub tris: usize,
    pub max_tris: usize,
    /// Tells if a mapped item may become a symbol instance.
    pub instancing: Option<&'a dyn Fn(u64) -> bool>,
    /// Curve tolerance in file units.
    pub tol: f64,
}

const MAX_DEPTH: usize = 16;

impl<'a> Eval<'a> {
    pub fn new(cx: &'a Ctx<'a>, max_tris: usize) -> Eval<'a> {
        Eval {
            cx,
            pieces: Vec::new(),
            issues: Vec::new(),
            approximate: false,
            tris: 0,
            max_tris,
            instancing: None,
            tol: 2.0 / cx.units.length_mm,
        }
    }

    fn issue(&mut self, text: impl Into<String>) {
        let t = text.into();
        if !self.issues.contains(&t) {
            self.issues.push(t);
        }
    }

    fn add_mesh(&mut self, m: MeshData) {
        if m.t.is_empty() {
            return;
        }
        self.tris += m.t.len();
        if self.tris > self.max_tris {
            self.issue("the triangle limit was reached: geometry was dropped");
            return;
        }
        self.pieces.push(Piece::Mesh(m));
    }

    /// Evaluates one representation item with the transform `xf` (item frame to world).
    pub fn item(&mut self, id: u64, xf: &DAffine3, depth: usize) {
        if depth > MAX_DEPTH {
            self.issue("representation items nest too deep");
            return;
        }
        let Some(e) = self.cx.ent(id) else {
            self.issue(format!("missing representation item #{id}"));
            return;
        };
        match e.name.as_str() {
            "IFCEXTRUDEDAREASOLID" => self.extruded(&e, xf),
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" => {
                self.triangulated(&e, xf)
            }
            "IFCPOLYGONALFACESET" => self.polygonal(&e, xf),
            "IFCFACETEDBREP" => self.shell(attr(&e, 0), xf),
            "IFCFACETEDBREPWITHVOIDS" => self.shell(attr(&e, 0), xf),
            "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                let mut m = MeshData::default();
                for s in items(attr(&e, 0)) {
                    if let Some(sh) = self.cx.get(s) {
                        self.shell_into(&sh, xf, &mut m);
                    }
                }
                m.weld(1e-9);
                self.add_mesh(m);
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" => {
                let mut m = MeshData::default();
                self.shell_into(&e, xf, &mut m);
                m.weld(1e-9);
                self.add_mesh(m);
            }
            "IFCMAPPEDITEM" => self.mapped(&e, xf, depth),
            "IFCBOOLEANRESULT" | "IFCBOOLEANCLIPPINGRESULT" => self.boolean(&e, xf, depth),
            "IFCBOUNDINGBOX" => self.bounding_box(&e, xf),
            "IFCGEOMETRICSET" | "IFCGEOMETRICCURVESET" => {
                for v in items(attr(&e, 0)) {
                    if let Some(i) = v.as_ref_id() {
                        self.item(i, xf, depth + 1);
                    }
                }
            }
            "IFCPOLYLINE"
            | "IFCINDEXEDPOLYCURVE"
            | "IFCCOMPOSITECURVE"
            | "IFCTRIMMEDCURVE"
            | "IFCCIRCLE" => self.curve_piece(id, xf),
            "IFCCARTESIANPOINT" => {
                if let Some(p) = super::ctx::point_of(&e) {
                    self.pieces.push(Piece::Point(xf.transform_point3(p)));
                }
            }
            other => self.issue(format!("unsupported geometry {other}")),
        }
    }

    // ------------------------------------------------------------ mapped items

    fn mapped(&mut self, e: &crate::step::Entity, xf: &DAffine3, depth: usize) {
        let Some(src_id) = attr(e, 0).as_ref_id() else {
            return;
        };
        let Some(src) = self.cx.ent(src_id) else {
            self.issue("a mapped item has no source");
            return;
        };
        let origin = self
            .cx
            .get(attr(&src, 0))
            .and_then(|o| self.cx.axis3(&o))
            .unwrap_or(DAffine3::IDENTITY);
        let target = self.cx.operator(attr(e, 1)).unwrap_or(DAffine3::IDENTITY);
        if origin.matrix3.determinant().abs() < 1e-18 || target.matrix3.determinant().abs() < 1e-18
        {
            self.issue("a mapped item has a flat transform");
            return;
        }
        let item_xf = *xf * target * origin.inverse();
        if let Some(ok) = self.instancing
            && ok(src_id)
        {
            // The definition lives in the frame of the map origin.
            self.pieces.push(Piece::Instance {
                source: src_id,
                xf: *xf * target,
            });
            return;
        }
        let Some(rep) = self.cx.get(attr(&src, 1)) else {
            self.issue("a mapped item has no representation");
            return;
        };
        for i in items(attr(&rep, 3)) {
            if let Some(id) = i.as_ref_id() {
                self.item(id, &item_xf, depth + 1);
            }
        }
    }

    // ------------------------------------------------------------ boolean clipping

    fn boolean(&mut self, e: &crate::step::Entity, xf: &DAffine3, depth: usize) {
        let op = enum_of(attr(e, 0)).unwrap_or("");
        let Some(first) = attr(e, 1).as_ref_id() else {
            return;
        };
        let start = self.pieces.len();
        // The first operand is evaluated in place; the clip works on what it added.
        self.item(first, xf, depth + 1);
        let second = self.cx.get(attr(e, 2));
        match (op, second) {
            ("DIFFERENCE", Some(s)) if s.name == "IFCHALFSPACESOLID" => {
                let Some(plane) = self.cx.get(attr(&s, 0)).filter(|p| p.name == "IFCPLANE") else {
                    self.issue("a half space is not bounded by a plane: not clipped");
                    return;
                };
                let Some(pos) = self.cx.get(attr(&plane, 0)).and_then(|p| self.cx.axis3(&p)) else {
                    return;
                };
                let world = *xf * pos;
                let n = world.matrix3.z_axis.normalize_or_zero();
                let o = world.translation;
                // With the agreement flag TRUE the normal points out of the removed half space.
                let keep = if flag(attr(&s, 1)).unwrap_or(true) {
                    n
                } else {
                    -n
                };
                // Convert the pieces of the first operand to meshes, then clip them.
                let tail: Vec<Piece> = self.pieces.drain(start..).collect();
                for p in tail {
                    match piece_mesh(&p) {
                        Some(m) => {
                            let clipped = clip_mesh(&m, o, keep);
                            if !clipped.t.is_empty() {
                                self.pieces.push(Piece::Mesh(clipped));
                            }
                        }
                        None => self.pieces.push(p),
                    }
                }
            }
            ("DIFFERENCE", Some(s)) if s.name == "IFCPOLYGONALBOUNDEDHALFSPACE" => {
                self.issue("a bounded half space is not applied: the solid is not clipped")
            }
            ("UNION", Some(s)) => self.item(s.id, xf, depth + 1),
            (_, Some(s)) => self.issue(format!(
                "a boolean {op} with {} is not applied: only the first operand is used",
                s.name
            )),
            _ => {}
        }
    }

    // ------------------------------------------------------------ bounding box

    fn bounding_box(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(c) = self.cx.point(attr(e, 0)) else {
            return;
        };
        let d = DVec3::new(
            num(attr(e, 1)).unwrap_or(0.0),
            num(attr(e, 2)).unwrap_or(0.0),
            num(attr(e, 3)).unwrap_or(0.0),
        );
        if !(d.is_finite() && d.min_element() > 0.0) {
            return;
        }
        let mut m = MeshData::cuboid(c, c + d);
        m.transform(xf);
        self.add_mesh(m);
    }

    // ------------------------------------------------------------ extruded solid

    fn extruded(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(regions) = self.profile(attr(e, 0).as_ref_id().unwrap_or(0)) else {
            return;
        };
        let pos = self
            .cx
            .get(attr(e, 1))
            .and_then(|p| self.cx.axis3(&p))
            .unwrap_or(DAffine3::IDENTITY);
        let dir = self
            .cx
            .direction(attr(e, 2))
            .and_then(|d| d.try_normalize())
            .unwrap_or(DVec3::Z);
        let depth = num(attr(e, 3)).unwrap_or(0.0);
        if !(depth.is_finite() && depth > 0.0) {
            self.issue("an extruded solid has no depth");
            return;
        }
        let ex = Extrusion {
            regions,
            dir: dir * depth,
            xf: *xf * pos,
            tol: self.tol,
        };
        self.tris += ex
            .regions
            .iter()
            .map(|r| r.outer.len() * 4 + 2)
            .sum::<usize>();
        if self.tris > self.max_tris {
            self.issue("the triangle limit was reached: geometry was dropped");
            return;
        }
        self.pieces.push(Piece::Extrusion(ex));
    }

    fn pos2(&self, v: &Value) -> DAffine2 {
        self.cx
            .get(v)
            .and_then(|p| self.cx.axis2(&p))
            .unwrap_or(DAffine2::IDENTITY)
    }

    /// The regions of a profile definition.
    pub fn profile(&mut self, id: u64) -> Option<Vec<Region>> {
        let e = self.cx.ent(id)?;
        let corner = |p: DVec2| Vertex::corner(p);
        let place = |a: &DAffine2, v: Vec<Vertex>| -> Vec<Vertex> {
            v.into_iter()
                .map(|x| Vertex::new(a.transform_point2(x.p), x.kind))
                .collect()
        };
        let rect = |x: f64, y: f64, r: f64| -> Vec<Vertex> {
            let (hx, hy) = (x / 2.0, y / 2.0);
            let kind = if r > 0.0 {
                VertexKind::Arc { radius: r }
            } else {
                VertexKind::Corner
            };
            [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
                .iter()
                .map(|(a, b)| Vertex::new(DVec2::new(*a, *b), kind))
                .collect()
        };
        let circle = |r: f64| -> Vec<Vertex> {
            (0..4)
                .map(|k| {
                    let a = PI / 2.0 * k as f64;
                    Vertex::new(DVec2::from_angle(a) * r, VertexKind::PointOnArc)
                })
                .collect()
        };
        let regions = match e.name.as_str() {
            "IFCRECTANGLEPROFILEDEF" | "IFCROUNDEDRECTANGLEPROFILEDEF" => {
                let (x, y) = (num(attr(&e, 3))?, num(attr(&e, 4))?);
                let r = if e.name == "IFCROUNDEDRECTANGLEPROFILEDEF" {
                    num(attr(&e, 5)).unwrap_or(0.0)
                } else {
                    0.0
                };
                if !(x > 0.0 && y > 0.0) {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                vec![Region {
                    outer: place(&a, rect(x, y, r.min(x.min(y) / 2.0))),
                    holes: vec![],
                }]
            }
            "IFCRECTANGLEHOLLOWPROFILEDEF" => {
                let (x, y, t) = (num(attr(&e, 3))?, num(attr(&e, 4))?, num(attr(&e, 5))?);
                if !(x > 0.0 && y > 0.0 && t > 0.0 && 2.0 * t < x.min(y)) {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                vec![Region {
                    outer: place(&a, rect(x, y, 0.0)),
                    holes: vec![place(&a, rect(x - 2.0 * t, y - 2.0 * t, 0.0))],
                }]
            }
            "IFCCIRCLEPROFILEDEF" | "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let r = num(attr(&e, 3))?;
                if r <= 0.0 {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                let holes = if e.name == "IFCCIRCLEHOLLOWPROFILEDEF" {
                    let t = num(attr(&e, 4))?;
                    if t <= 0.0 || t >= r {
                        return None;
                    }
                    vec![place(&a, circle(r - t))]
                } else {
                    vec![]
                };
                vec![Region {
                    outer: place(&a, circle(r)),
                    holes,
                }]
            }
            "IFCELLIPSEPROFILEDEF" => {
                let (rx, ry) = (num(attr(&e, 3))?, num(attr(&e, 4))?);
                if !(rx > 0.0 && ry > 0.0) {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                let pts = (0..48)
                    .map(|k| {
                        let t = TAU * k as f64 / 48.0;
                        corner(DVec2::new(rx * t.cos(), ry * t.sin()))
                    })
                    .collect();
                vec![Region {
                    outer: place(&a, pts),
                    holes: vec![],
                }]
            }
            "IFCISHAPEPROFILEDEF" => {
                let (w, d, tw, tf) = (
                    num(attr(&e, 3))?,
                    num(attr(&e, 4))?,
                    num(attr(&e, 5))?,
                    num(attr(&e, 6))?,
                );
                if !(w > 0.0 && d > 0.0 && tw > 0.0 && tf > 0.0 && 2.0 * tf < d && tw < w) {
                    return None;
                }
                let (hw, hd, ht) = (w / 2.0, d / 2.0, tw / 2.0);
                let a = self.pos2(attr(&e, 2));
                let pts = [
                    (-hw, -hd),
                    (hw, -hd),
                    (hw, -hd + tf),
                    (ht, -hd + tf),
                    (ht, hd - tf),
                    (hw, hd - tf),
                    (hw, hd),
                    (-hw, hd),
                    (-hw, hd - tf),
                    (-ht, hd - tf),
                    (-ht, -hd + tf),
                    (-hw, -hd + tf),
                ]
                .iter()
                .map(|(x, y)| corner(DVec2::new(*x, *y)))
                .collect();
                vec![Region {
                    outer: place(&a, pts),
                    holes: vec![],
                }]
            }
            "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                let outer = self.ring(attr(&e, 2))?;
                let mut holes = Vec::new();
                if e.name == "IFCARBITRARYPROFILEDEFWITHVOIDS" {
                    for h in items(attr(&e, 3)) {
                        if let Some(r) = self.ring(h) {
                            holes.push(r);
                        }
                    }
                }
                vec![Region { outer, holes }]
            }
            "IFCCOMPOSITEPROFILEDEF" => {
                let mut all = Vec::new();
                for p in items(attr(&e, 2)) {
                    if let Some(i) = p.as_ref_id()
                        && let Some(mut r) = self.profile(i)
                    {
                        all.append(&mut r);
                    }
                }
                all
            }
            other => {
                self.issue(format!("unsupported profile {other}"));
                return None;
            }
        };
        if regions.is_empty() {
            None
        } else {
            Some(regions)
        }
    }

    /// A closed outline from a curve reference.
    fn ring(&mut self, v: &Value) -> Option<Vec<Vertex>> {
        let id = v.as_ref_id()?;
        let mut pts = Vec::new();
        let mut zs = Vec::new();
        if !self.curve_vertices(id, &mut pts, &mut zs, 0) {
            self.issue("a profile outline is not supported");
            return None;
        }
        // Drop the closing point.
        while pts.len() > 1 && pts[0].p.distance_squared(pts[pts.len() - 1].p) < 1e-18 {
            pts.pop();
        }
        (pts.len() >= 3).then_some(pts)
    }

    // ------------------------------------------------------------ curves

    fn curve_piece(&mut self, id: u64, xf: &DAffine3) {
        let mut pts = Vec::new();
        let mut zs = Vec::new();
        if !self.curve_vertices(id, &mut pts, &mut zs, 0) || pts.len() < 2 {
            self.issue("a curve is not supported");
            return;
        }
        let closed = pts.len() > 2 && pts[0].p.distance_squared(pts[pts.len() - 1].p) < 1e-12;
        if closed {
            pts.pop();
            zs.pop();
        }
        let up = xf.matrix3.z_axis.normalize_or_zero();
        let horizontal = up.x.abs() < 1e-9 && up.y.abs() < 1e-9 && up.z > 0.0;
        let flat_z = zs.iter().all(|z| (z - zs[0]).abs() < 1e-9);
        let exact = (horizontal && flat_z && (xf.matrix3.x_axis.z.abs() < 1e-9)).then(|| {
            let mapped: Vec<Vertex> = pts
                .iter()
                .map(|v| {
                    let w = xf.transform_point3(v.p.extend(0.0));
                    Vertex::new(w.truncate(), v.kind)
                })
                .collect();
            let z = xf.transform_point3(DVec3::new(0.0, 0.0, zs[0])).z;
            (mapped, z)
        });
        // The flat points: curved vertices are flattened for the 3D case.
        let flat = curve::flatten(&pts, closed, self.tol.max(1e-9));
        let z0 = zs.first().copied().unwrap_or(0.0);
        let world: Vec<DVec3> = if flat.len() == pts.len() {
            flat.iter()
                .zip(&zs)
                .map(|(p, z)| xf.transform_point3(p.extend(*z)))
                .collect()
        } else {
            flat.iter()
                .map(|p| xf.transform_point3(p.extend(z0)))
                .collect()
        };
        self.pieces.push(Piece::Curve(CurveData {
            exact,
            pts: world,
            closed,
        }));
    }

    /// Appends the vertices of a curve. Curved segments become `PointOnArc` vertices, so the arcs stay exact.
    pub fn curve_vertices(
        &mut self,
        id: u64,
        out: &mut Vec<Vertex>,
        zs: &mut Vec<f64>,
        depth: usize,
    ) -> bool {
        if depth > 4 {
            return false;
        }
        let Some(e) = self.cx.ent(id) else {
            return false;
        };
        let push = |out: &mut Vec<Vertex>, zs: &mut Vec<f64>, p: DVec3, kind: VertexKind| {
            if let Some(last) = out.last()
                && kind == VertexKind::Corner
                && last.kind == VertexKind::Corner
                && last.p.distance_squared(p.truncate()) < 1e-18
                && (zs.last().copied().unwrap_or(0.0) - p.z).abs() < 1e-12
            {
                return;
            }
            out.push(Vertex::new(p.truncate(), kind));
            zs.push(p.z);
        };
        match e.name.as_str() {
            "IFCPOLYLINE" => {
                let mut any = false;
                for v in items(attr(&e, 0)) {
                    if let Some(p) = self.cx.point(v) {
                        push(out, zs, p, VertexKind::Corner);
                        any = true;
                    }
                }
                any
            }
            "IFCINDEXEDPOLYCURVE" => {
                let Some(list) = self.cx.get(attr(&e, 0)) else {
                    return false;
                };
                let coords = point_list(&list);
                if coords.is_empty() {
                    return false;
                }
                let at = |i: i64| -> Option<DVec3> {
                    usize::try_from(i - 1)
                        .ok()
                        .and_then(|k| coords.get(k))
                        .copied()
                };
                let segs = items(attr(&e, 1));
                if segs.is_empty() {
                    for p in &coords {
                        push(out, zs, *p, VertexKind::Corner);
                    }
                    return true;
                }
                for s in segs {
                    let (name, idx) = match s {
                        Value::Typed(n, inner) => (n.as_str(), items(inner)),
                        other => ("IFCLINEINDEX", items(other)),
                    };
                    let ids: Vec<i64> = idx.iter().filter_map(int).collect();
                    if name == "IFCARCINDEX" && ids.len() == 3 {
                        let (Some(a), Some(b), Some(c)) = (at(ids[0]), at(ids[1]), at(ids[2]))
                        else {
                            return false;
                        };
                        push(out, zs, a, VertexKind::Corner);
                        push(out, zs, b, VertexKind::PointOnArc);
                        push(out, zs, c, VertexKind::Corner);
                    } else {
                        for i in ids {
                            let Some(p) = at(i) else { return false };
                            push(out, zs, p, VertexKind::Corner);
                        }
                    }
                }
                true
            }
            "IFCCOMPOSITECURVE" => {
                let mut any = false;
                for seg in self.cx.get_all(attr(&e, 0)) {
                    let same = flag(attr(&seg, 1)).unwrap_or(true);
                    let Some(parent) = attr(&seg, 2).as_ref_id() else {
                        continue;
                    };
                    let (mut p2, mut z2) = (Vec::new(), Vec::new());
                    if !self.curve_vertices(parent, &mut p2, &mut z2, depth + 1) {
                        return false;
                    }
                    if !same {
                        p2.reverse();
                        z2.reverse();
                    }
                    for (v, z) in p2.into_iter().zip(z2) {
                        push(out, zs, v.p.extend(z), v.kind);
                    }
                    any = true;
                }
                any
            }
            "IFCTRIMMEDCURVE" => {
                let Some(basis) = self.cx.get(attr(&e, 0)) else {
                    return false;
                };
                let sense = flag(attr(&e, 3)).unwrap_or(true);
                match basis.name.as_str() {
                    "IFCCIRCLE" => {
                        let Some(path) = self.circle_path(&basis, attr(&e, 1), attr(&e, 2), sense)
                        else {
                            return false;
                        };
                        for (p, k) in path {
                            push(out, zs, p, k);
                        }
                        true
                    }
                    "IFCLINE" => {
                        let pt = |v: &Value| items(v).iter().find_map(|x| self.cx.point(x));
                        match (pt(attr(&e, 1)), pt(attr(&e, 2))) {
                            (Some(a), Some(b)) => {
                                push(out, zs, a, VertexKind::Corner);
                                push(out, zs, b, VertexKind::Corner);
                                true
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }
            "IFCCIRCLE" => {
                let Some(path) =
                    self.circle_path(&e, &Value::List(vec![]), &Value::List(vec![]), true)
                else {
                    return false;
                };
                for (p, k) in path {
                    push(out, zs, p, k);
                }
                true
            }
            _ => false,
        }
    }

    /// The vertices of an arc of a circle, in the frame of the curve. Arcs over 180 degrees are split,
    /// so that three points always fix them.
    fn circle_path(
        &self,
        circle: &crate::step::Entity,
        t1: &Value,
        t2: &Value,
        sense: bool,
    ) -> Option<Vec<(DVec3, VertexKind)>> {
        let pos = self
            .cx
            .get(attr(circle, 0))
            .and_then(|p| self.cx.axis3(&p))?;
        let r = num(attr(circle, 1))?;
        if !(r.is_finite() && r > 0.0) {
            return None;
        }
        let inv = pos.inverse();
        let angle = |trim: &Value| -> Option<f64> {
            for v in items(trim) {
                if let Some(p) = self.cx.point(v) {
                    let l = inv.transform_point3(p);
                    return Some(l.y.atan2(l.x));
                }
            }
            for v in items(trim) {
                if let Some(n) = num(v) {
                    return Some(n * self.cx.units.angle_rad);
                }
            }
            None
        };
        let (a0, a1) = match (angle(t1), angle(t2)) {
            (Some(a), Some(b)) => (a, b),
            _ => (0.0, TAU),
        };
        let mut sweep = if sense {
            (a1 - a0).rem_euclid(TAU)
        } else {
            -(a0 - a1).rem_euclid(TAU)
        };
        if sweep.abs() < 1e-9 {
            sweep = if sense { TAU } else { -TAU };
        }
        let n = if sweep.abs() > PI * 0.99 { 2 } else { 1 };
        let at = |a: f64| pos.transform_point3(DVec3::new(r * a.cos(), r * a.sin(), 0.0));
        let mut out = Vec::new();
        for k in 0..n {
            let s = a0 + sweep * k as f64 / n as f64;
            let m = a0 + sweep * (k as f64 + 0.5) / n as f64;
            out.push((at(s), VertexKind::Corner));
            out.push((at(m), VertexKind::PointOnArc));
        }
        out.push((at(a0 + sweep), VertexKind::Corner));
        Some(out)
    }

    // ------------------------------------------------------------ face sets and breps

    fn triangulated(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(list) = self.cx.get(attr(e, 0)) else {
            return;
        };
        let coords = point_list(&list);
        let pn: Vec<i64> = items(attr(e, 4)).iter().filter_map(int).collect();
        let idx = |v: &Value| -> Option<u32> {
            let mut i = int(v)?;
            if !pn.is_empty() {
                i = *pn.get(usize::try_from(i - 1).ok()?)?;
            }
            let k = usize::try_from(i - 1).ok()?;
            (k < coords.len()).then_some(k as u32)
        };
        let mut m = MeshData {
            v: coords.iter().map(|p| xf.transform_point3(*p)).collect(),
            t: Vec::new(),
        };
        for t in items(attr(e, 3)) {
            let l = items(t);
            if l.len() != 3 {
                continue;
            }
            if let (Some(a), Some(b), Some(c)) = (idx(&l[0]), idx(&l[1]), idx(&l[2])) {
                m.t.push([a, b, c]);
            }
        }
        if xf.matrix3.determinant() < 0.0 {
            m.flip();
        }
        self.add_mesh(m);
    }

    fn polygonal(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(list) = self.cx.get(attr(e, 0)) else {
            return;
        };
        let coords: Vec<DVec3> = point_list(&list)
            .iter()
            .map(|p| xf.transform_point3(*p))
            .collect();
        let pn: Vec<i64> = items(attr(e, 3)).iter().filter_map(int).collect();
        let ring = |v: &Value| -> Option<Vec<DVec3>> {
            items(v)
                .iter()
                .map(|i| {
                    let mut i = int(i)?;
                    if !pn.is_empty() {
                        i = *pn.get(usize::try_from(i - 1).ok()?)?;
                    }
                    coords.get(usize::try_from(i - 1).ok()?).copied()
                })
                .collect()
        };
        let mut m = MeshData::default();
        for f in self.cx.get_all(attr(e, 2)) {
            let Some(outer) = ring(attr(&f, 0)) else {
                continue;
            };
            let mut rings = vec![outer];
            if f.name == "IFCINDEXEDPOLYGONALFACEWITHVOIDS" {
                rings.extend(items(attr(&f, 1)).iter().filter_map(&ring));
            }
            add_polygon(&mut m, &rings);
        }
        if xf.matrix3.determinant() < 0.0 {
            m.flip();
        }
        m.weld(1e-9);
        self.add_mesh(m);
    }

    fn shell(&mut self, outer: &Value, xf: &DAffine3) {
        let Some(sh) = self.cx.get(outer) else {
            return;
        };
        let mut m = MeshData::default();
        self.shell_into(&sh, xf, &mut m);
        m.weld(1e-9);
        self.add_mesh(m);
    }

    fn shell_into(&mut self, shell: &crate::step::Entity, xf: &DAffine3, m: &mut MeshData) {
        let first_tri = m.t.len();
        for f in self.cx.get_all(attr(shell, 0)) {
            if f.name != "IFCFACE" && f.name != "IFCADVANCEDFACE" {
                continue;
            }
            let mut rings: Vec<Vec<DVec3>> = Vec::new();
            for b in self.cx.get_all(attr(&f, 0)) {
                let Some(l) = self.cx.get(attr(&b, 0)) else {
                    continue;
                };
                if l.name != "IFCPOLYLOOP" {
                    self.issue(format!("unsupported face bound {}", l.name));
                    continue;
                }
                let mut pts: Vec<DVec3> = items(attr(&l, 0))
                    .iter()
                    .filter_map(|p| self.cx.point(p))
                    .map(|p| xf.transform_point3(p))
                    .collect();
                if flag(attr(&b, 1)) == Some(false) {
                    pts.reverse();
                }
                // The outer bound comes first.
                if b.name == "IFCFACEOUTERBOUND" {
                    rings.insert(0, pts);
                } else {
                    rings.push(pts);
                }
            }
            add_polygon(m, &rings);
        }
        if xf.matrix3.determinant() < 0.0 {
            for t in &mut m.t[first_tri..] {
                t.swap(1, 2);
            }
        }
    }
}

/// The coordinates of an `IfcCartesianPointList2D` or `3D`.
fn point_list(e: &crate::step::Entity) -> Vec<DVec3> {
    items(attr(e, 0))
        .iter()
        .filter_map(|row| {
            let n = numbers(row);
            (n.len() >= 2).then(|| DVec3::new(n[0], n[1], n.get(2).copied().unwrap_or(0.0)))
        })
        .collect()
}

/// Triangulates a planar polygon in 3D with optional holes. The first ring is the outline.
pub fn add_polygon(m: &mut MeshData, rings: &[Vec<DVec3>]) {
    let Some(outer) = rings.first() else { return };
    if outer.len() < 3 {
        return;
    }
    // Newell's normal.
    let mut n = DVec3::ZERO;
    for i in 0..outer.len() {
        let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
        n += DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let Some(n) = n.try_normalize() else { return };
    let basis = super::ctx::frame(n, None);
    let (u, w) = (basis.x_axis, basis.y_axis);
    let o = outer[0];
    let flat = |r: &[DVec3]| -> Vec<DVec2> {
        r.iter()
            .map(|p| DVec2::new((*p - o).dot(u), (*p - o).dot(w)))
            .collect()
    };
    if rings.len() == 1 {
        let pts = flat(outer);
        let base = m.v.len() as u32;
        m.v.extend_from_slice(outer);
        for [a, b, c] in cadektor_core::mesh::triangulate(&pts) {
            m.t.push([base + a as u32, base + b as u32, base + c as u32]);
        }
        return;
    }
    let outer2 = clean(&flat(outer), true);
    let holes: Vec<Vec<DVec2>> = rings[1..]
        .iter()
        .map(|r| clean(&flat(r), true))
        .filter(|r| r.len() >= 3)
        .collect();
    if outer2.len() < 3 {
        return;
    }
    for piece in crate::geom::split_with_holes(&outer2, &holes) {
        let base = m.v.len() as u32;
        m.v.extend(piece.iter().map(|p| o + u * p.x + w * p.y));
        for [a, b, c] in cadektor_core::mesh::triangulate(&piece) {
            m.t.push([base + a as u32, base + b as u32, base + c as u32]);
        }
    }
}

/// The mesh of a piece, if it is solid geometry.
pub fn piece_mesh(p: &Piece) -> Option<MeshData> {
    match p {
        Piece::Mesh(m) => Some(m.clone()),
        Piece::Extrusion(e) => Some(e.mesh()),
        _ => None,
    }
}

/// Keeps the part of the mesh on the side of `normal` (where `(p - origin) . normal >= 0`) and closes
/// the cut with flat caps. A cut with nested loops (a hole in the section) is closed with its hole.
pub fn clip_mesh(m: &MeshData, origin: DVec3, normal: DVec3) -> MeshData {
    let Some(n) = normal.try_normalize() else {
        return m.clone();
    };
    let d = |p: DVec3| (p - origin).dot(n);
    let mut out = MeshData::default();
    let mut index: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let key = |p: DVec3| {
        (
            (p.x * 1e6).round() as i64,
            (p.y * 1e6).round() as i64,
            (p.z * 1e6).round() as i64,
        )
    };
    let mut vid = |out: &mut MeshData, p: DVec3| -> u32 {
        *index.entry(key(p)).or_insert_with(|| {
            out.v.push(p);
            (out.v.len() - 1) as u32
        })
    };
    let mut cut: Vec<(DVec3, DVec3)> = Vec::new();
    for t in &m.t {
        let p = t.map(|i| m.v[i as usize]);
        let ds = p.map(d);
        let eps = 1e-9;
        if ds.iter().all(|x| *x >= -eps) {
            let ids = p.map(|q| vid(&mut out, q));
            if ds.iter().all(|x| x.abs() <= eps) {
                continue;
            }
            out.t.push(ids);
            // An edge lying in the plane is part of the cut.
            for k in 0..3 {
                let (a, b) = (k, (k + 1) % 3);
                if ds[a].abs() <= eps && ds[b].abs() <= eps {
                    cut.push((p[a], p[b]));
                }
            }
            continue;
        }
        if ds.iter().all(|x| *x <= eps) {
            continue;
        }
        // The triangle crosses the plane: clip the polygon.
        let mut poly: Vec<DVec3> = Vec::new();
        for k in 0..3 {
            let (a, b) = (k, (k + 1) % 3);
            if ds[a] >= 0.0 {
                poly.push(p[a]);
            }
            if (ds[a] > 0.0 && ds[b] < 0.0) || (ds[a] < 0.0 && ds[b] > 0.0) {
                let s = ds[a] / (ds[a] - ds[b]);
                poly.push(p[a].lerp(p[b], s));
            }
        }
        if poly.len() < 3 {
            continue;
        }
        // Points of the new polygon that lie on the plane form the cut edge.
        let on: Vec<DVec3> = poly
            .iter()
            .copied()
            .filter(|q| d(*q).abs() <= 1e-7)
            .collect();
        if on.len() == 2 {
            cut.push((on[0], on[1]));
        }
        let ids: Vec<u32> = poly.iter().map(|q| vid(&mut out, *q)).collect();
        for k in 1..ids.len() - 1 {
            out.t.push([ids[0], ids[k], ids[k + 1]]);
        }
    }
    cap(&mut out, &cut, origin, n);
    out
}

/// Closes the cut of a clipped mesh. The caps face away from `keep`.
fn cap(out: &mut MeshData, cut: &[(DVec3, DVec3)], _origin: DVec3, keep: DVec3) {
    if cut.is_empty() {
        return;
    }
    let key = |p: DVec3| {
        (
            (p.x * 1e5).round() as i64,
            (p.y * 1e5).round() as i64,
            (p.z * 1e5).round() as i64,
        )
    };
    // Chain the segments into loops.
    let mut next: HashMap<(i64, i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (i, (a, b)) in cut.iter().enumerate() {
        next.entry(key(*a)).or_default().push((i, true));
        next.entry(key(*b)).or_default().push((i, false));
    }
    let mut used = vec![false; cut.len()];
    let mut loops: Vec<Vec<DVec3>> = Vec::new();
    for s in 0..cut.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let mut pts = vec![cut[s].0];
        let mut end = cut[s].1;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > cut.len() + 2 {
                break;
            }
            if key(end) == key(pts[0]) {
                break;
            }
            pts.push(end);
            let Some(cands) = next.get(&key(end)) else {
                break;
            };
            let Some(&(i, fwd)) = cands.iter().find(|(i, _)| !used[*i]) else {
                break;
            };
            used[i] = true;
            end = if fwd { cut[i].1 } else { cut[i].0 };
        }
        if pts.len() >= 3 && key(end) == key(pts[0]) {
            loops.push(pts);
        }
    }
    if loops.is_empty() {
        return;
    }
    // Plane basis: the cap faces -keep.
    let out_n = -keep;
    let basis = super::ctx::frame(out_n, None);
    let (u, w) = (basis.x_axis, basis.y_axis);
    let o = loops[0][0];
    let flat = |r: &[DVec3]| -> Vec<DVec2> {
        r.iter()
            .map(|p| DVec2::new((*p - o).dot(u), (*p - o).dot(w)))
            .collect()
    };
    let mut rings: Vec<(Vec<DVec2>, f64)> = loops
        .iter()
        .map(|l| {
            let f = flat(l);
            let a = crate::geom::signed_area(&f).abs();
            (f, a)
        })
        .collect();
    rings.sort_by(|a, b| b.1.total_cmp(&a.1));
    // A loop inside an earlier loop is a hole.
    let mut groups: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> = Vec::new();
    for (r, _) in rings {
        let c = clean(&r, true);
        if c.len() < 3 {
            continue;
        }
        let mut placed = false;
        for g in groups.iter_mut() {
            if inside(&g.0, c[0]) {
                g.1.push(c.clone());
                placed = true;
                break;
            }
        }
        if !placed {
            groups.push((c, Vec::new()));
        }
    }
    for (outer, holes) in groups {
        for piece in crate::geom::split_with_holes(&outer, &holes) {
            let base = out.v.len() as u32;
            out.v.extend(piece.iter().map(|p| o + u * p.x + w * p.y));
            for [a, b, c] in cadektor_core::mesh::triangulate(&piece) {
                out.t
                    .push([base + a as u32, base + b as u32, base + c as u32]);
            }
        }
    }
}

fn inside(poly: &[DVec2], p: DVec2) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{Limits, StepIndex};

    fn index(data: &str) -> StepIndex {
        let text = format!("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{data}\nENDSEC;\n");
        StepIndex::from_bytes(text.into_bytes(), Limits::default()).unwrap()
    }

    fn volume(m: &MeshData) -> f64 {
        m.t.iter()
            .map(|t| {
                let (a, b, c) = (m.v[t[0] as usize], m.v[t[1] as usize], m.v[t[2] as usize]);
                a.dot(b.cross(c))
            })
            .sum::<f64>()
            / 6.0
    }

    fn run(data: &str, id: u64) -> (Vec<Piece>, Vec<String>) {
        let x = index(data);
        let cx = Ctx::new(&x);
        let mut ev = Eval::new(&cx, 1_000_000);
        ev.item(id, &DAffine3::IDENTITY, 0);
        (ev.pieces, ev.issues)
    }

    const BASE: &str = "#1=IFCCARTESIANPOINT((0.,0.,0.));\n#2=IFCAXIS2PLACEMENT3D(#1,$,$);\n#3=IFCDIRECTION((0.,0.,1.));\n";

    #[test]
    fn rectangle_extrusion_is_a_closed_solid_of_the_right_volume() {
        let (p, issues) = run(
            &format!(
                "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,4.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,3.);"
            ),
            5,
        );
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert!(ex.is_upright());
        assert!((ex.height() - 3.0).abs() < 1e-12);
        let m = ex.mesh();
        assert!((volume(&m) - 24.0).abs() < 1e-9, "{}", volume(&m));
    }

    #[test]
    fn circle_and_voids_and_downward_extrusions() {
        let (p, _) = run(
            &format!(
                "{BASE}#4=IFCCIRCLEPROFILEDEF(.AREA.,$,$,1.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,2.);"
            ),
            5,
        );
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        // The circle is exact in the profile (four points on the arc) and flat in the mesh.
        assert!(
            ex.regions[0]
                .outer
                .iter()
                .all(|v| v.kind == VertexKind::PointOnArc)
        );
        let v = volume(&ex.mesh());
        assert!((v - PI * 2.0).abs() < 0.05 * PI * 2.0, "{v}");
        // A profile with a square void.
        let (p, _) = run(
            &format!(
                "{BASE}#10=IFCCARTESIANPOINT((0.,0.));\n#11=IFCCARTESIANPOINT((10.,0.));\n#12=IFCCARTESIANPOINT((10.,10.));\n#13=IFCCARTESIANPOINT((0.,10.));\n#14=IFCPOLYLINE((#10,#11,#12,#13,#10));\n#15=IFCCARTESIANPOINT((4.,4.));\n#16=IFCCARTESIANPOINT((6.,4.));\n#17=IFCCARTESIANPOINT((6.,6.));\n#18=IFCCARTESIANPOINT((4.,6.));\n#19=IFCPOLYLINE((#15,#16,#17,#18,#15));\n#20=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#14,(#19));\n#21=IFCEXTRUDEDAREASOLID(#20,#2,#3,1.);\n#22=IFCDIRECTION((0.,0.,-1.));\n#23=IFCEXTRUDEDAREASOLID(#20,#2,#22,1.);"
            ),
            21,
        );
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert_eq!(ex.regions[0].holes.len(), 1);
        assert!(
            (volume(&ex.mesh()) - 96.0).abs() < 1e-6,
            "{}",
            volume(&ex.mesh())
        );
        let (p, _) = run(
            &format!(
                "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,1.);\n#22=IFCDIRECTION((0.,0.,-1.));\n#23=IFCEXTRUDEDAREASOLID(#4,#2,#22,2.);"
            ),
            23,
        );
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert!(ex.is_upright());
        // A solid that goes down has positive volume too, and lies below.
        assert!((volume(&ex.mesh()) - 2.0).abs() < 1e-9);
        assert!(ex.mesh().bounds().unwrap().1.z < 1e-9);
    }

    #[test]
    fn composite_curve_with_arc_stays_exact() {
        // A half disc: a line along the x axis and a semicircle above it.
        let data = format!(
            "{BASE}#10=IFCCARTESIANPOINT((-1.,0.));\n#11=IFCCARTESIANPOINT((1.,0.));\n#12=IFCPOLYLINE((#10,#11));\n#13=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#12);\n#14=IFCCARTESIANPOINT((0.,0.));\n#15=IFCAXIS2PLACEMENT2D(#14,$);\n#16=IFCCIRCLE(#15,1.);\n#17=IFCTRIMMEDCURVE(#16,(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(180.)),.T.,.PARAMETER.);\n#18=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#17);\n#19=IFCCOMPOSITECURVE((#13,#18),.F.);\n#20=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#19);\n#21=IFCEXTRUDEDAREASOLID(#20,#2,#3,1.);\n#30=IFCPROJECT('g',$,'p',$,$,$,$,$,#31);\n#31=IFCUNITASSIGNMENT((#32,#33));\n#32=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#33=IFCCONVERSIONBASEDUNIT($,.PLANEANGLEUNIT.,'DEGREE',#34);\n#34=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.0174532925199433),#35);\n#35=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);"
        );
        let (p, issues) = run(&data, 21);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        let kinds: Vec<_> = ex.regions[0].outer.iter().map(|v| v.kind).collect();
        assert!(kinds.contains(&VertexKind::PointOnArc), "{kinds:?}");
        // Half disc area: pi/2 (the flattened mesh is slightly smaller).
        let v = volume(&ex.mesh());
        assert!((v - PI / 2.0).abs() < 0.01, "{v}");
    }

    #[test]
    fn indexed_poly_curve_with_an_arc_index() {
        let data = format!(
            "{BASE}#10=IFCCARTESIANPOINTLIST2D(((0.,0.),(2.,0.),(1.,1.),(0.,2.)));\n#11=IFCINDEXEDPOLYCURVE(#10,(IFCLINEINDEX((1,2)),IFCARCINDEX((2,3,4)),IFCLINEINDEX((4,1))),.F.);\n#12=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#11);\n#13=IFCEXTRUDEDAREASOLID(#12,#2,#3,1.);"
        );
        let (p, issues) = run(&data, 13);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert_eq!(ex.regions[0].outer.len(), 4);
        assert_eq!(ex.regions[0].outer[2].kind, VertexKind::PointOnArc);
    }

    #[test]
    fn face_sets_and_breps() {
        // A tetrahedron as a triangulated face set.
        let tet = "#10=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(6.,0.,0.),(0.,6.,0.),(0.,0.,6.)));\n#11=IFCTRIANGULATEDFACESET(#10,$,.T.,((1,3,2),(1,2,4),(2,3,4),(1,4,3)),$);";
        let (p, issues) = run(tet, 11);
        assert!(issues.is_empty());
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 36.0).abs() < 1e-9);
        // The same as a polygonal face set with one quad face and a hole-free triangle.
        let cube = "#10=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1.,0.,0.),(1.,1.,0.),(0.,1.,0.),(0.,0.,1.),(1.,0.,1.),(1.,1.,1.),(0.,1.,1.)));\n#11=IFCINDEXEDPOLYGONALFACE((1,4,3,2));\n#12=IFCINDEXEDPOLYGONALFACE((5,6,7,8));\n#13=IFCINDEXEDPOLYGONALFACE((1,2,6,5));\n#14=IFCINDEXEDPOLYGONALFACE((2,3,7,6));\n#15=IFCINDEXEDPOLYGONALFACE((3,4,8,7));\n#16=IFCINDEXEDPOLYGONALFACE((4,1,5,8));\n#17=IFCPOLYGONALFACESET(#10,.T.,(#11,#12,#13,#14,#15,#16),$);";
        let (p, issues) = run(cube, 17);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert_eq!(m.v.len(), 8);
        assert!((volume(m) - 1.0).abs() < 1e-9);
        // A faceted brep of the same cube (one face has a hole-free loop).
        let mut brep = String::new();
        let pts = [
            (0, 0, 0),
            (1, 0, 0),
            (1, 1, 0),
            (0, 1, 0),
            (0, 0, 1),
            (1, 0, 1),
            (1, 1, 1),
            (0, 1, 1),
        ];
        for (i, (x, y, z)) in pts.iter().enumerate() {
            brep += &format!("#{}=IFCCARTESIANPOINT(({x}.,{y}.,{z}.));\n", 100 + i);
        }
        let faces = [
            [1, 4, 3, 2],
            [5, 6, 7, 8],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 4, 8, 7],
            [4, 1, 5, 8],
        ];
        for (k, f) in faces.iter().enumerate() {
            let refs: Vec<String> = f.iter().map(|i| format!("#{}", 99 + i)).collect();
            brep += &format!(
                "#{}=IFCPOLYLOOP(({}));\n#{}=IFCFACEOUTERBOUND(#{},.T.);\n#{}=IFCFACE((#{}));\n",
                200 + k,
                refs.join(","),
                300 + k,
                200 + k,
                400 + k,
                300 + k
            );
        }
        brep += "#500=IFCCLOSEDSHELL((#400,#401,#402,#403,#404,#405));\n#501=IFCFACETEDBREP(#500);\n#502=IFCSHELLBASEDSURFACEMODEL((#500));";
        let (p, issues) = run(&brep, 501);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 1.0).abs() < 1e-9);
        let (p, _) = run(&brep, 502);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 1.0).abs() < 1e-9);
        // Reversed bound orientation turns the face around.
        // (The top face carries a third of the signed volume; the bottom face none.)
        let flipped = brep.replace(
            "#301=IFCFACEOUTERBOUND(#201,.T.)",
            "#301=IFCFACEOUTERBOUND(#201,.F.)",
        );
        let (p, _) = run(&flipped, 501);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 1.0 / 3.0).abs() < 1e-9, "{}", volume(m));
    }

    #[test]
    fn polygon_with_a_hole_in_3d() {
        let mut m = MeshData::default();
        let sq = |a: f64, z: f64| {
            vec![
                DVec3::new(-a, -a, z),
                DVec3::new(a, -a, z),
                DVec3::new(a, a, z),
                DVec3::new(-a, a, z),
            ]
        };
        let mut hole = sq(1.0, 0.0);
        hole.reverse();
        add_polygon(&mut m, &[sq(3.0, 0.0), hole]);
        let area: f64 =
            m.t.iter()
                .map(|t| {
                    let (a, b, c) = (m.v[t[0] as usize], m.v[t[1] as usize], m.v[t[2] as usize]);
                    (b - a).cross(c - a).length() / 2.0
                })
                .sum();
        assert!((area - 32.0).abs() < 1e-6, "{area}");
        // Degenerate input does nothing.
        let mut e = MeshData::default();
        add_polygon(&mut e, &[vec![DVec3::ZERO, DVec3::X]]);
        add_polygon(&mut e, &[vec![DVec3::ZERO, DVec3::X, DVec3::X * 2.0]]);
        add_polygon(&mut e, &[]);
        assert!(e.t.is_empty());
    }

    #[test]
    fn clipping_cuts_and_caps_a_box() {
        let cube = MeshData::cuboid(DVec3::ZERO, DVec3::new(2.0, 2.0, 2.0));
        assert!((volume(&cube) - 8.0).abs() < 1e-9);
        let c = clip_mesh(&cube, DVec3::new(0.0, 0.0, 1.0), -DVec3::Z);
        assert!((volume(&c) - 4.0).abs() < 1e-9, "{}", volume(&c));
        // A sloped cut.
        let c = clip_mesh(
            &cube,
            DVec3::new(1.0, 1.0, 1.0),
            DVec3::new(-1.0, 0.0, -1.0),
        );
        let v = volume(&c);
        assert!(v > 1.0 && v < 7.0, "{v}");
        // Nothing to remove, everything removed.
        assert!(
            (volume(&clip_mesh(&cube, DVec3::new(0.0, 0.0, 5.0), -DVec3::Z)) - 8.0).abs() < 1e-9
        );
        assert!(
            clip_mesh(&cube, DVec3::new(0.0, 0.0, -5.0), -DVec3::Z)
                .t
                .is_empty()
        );
        assert_eq!(clip_mesh(&cube, DVec3::ZERO, DVec3::ZERO), cube);
    }

    #[test]
    fn boolean_clipping_result_with_a_half_space() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,2.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,4.);\n#6=IFCCARTESIANPOINT((0.,0.,3.));\n#7=IFCAXIS2PLACEMENT3D(#6,$,$);\n#8=IFCPLANE(#7);\n#9=IFCHALFSPACESOLID(#8,.F.);\n#10=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#5,#9);\n#11=IFCHALFSPACESOLID(#8,.T.);\n#12=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#5,#11);\n#13=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#10,#11);"
        );
        // Flag FALSE: the half space is above the plane, so the part above is removed. 2x2x3 remains.
        let (p, issues) = run(&data, 10);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 12.0).abs() < 1e-6, "{}", volume(m));
        // Flag TRUE: the half space is below the plane: 2x2x1 remains.
        let (p, _) = run(&data, 12);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 4.0).abs() < 1e-6);
        // Nested clipping removes everything.
        let (p, _) = run(&data, 13);
        assert!(p.is_empty());
    }

    #[test]
    fn unsupported_operands_and_items_are_reported() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,2.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,4.);\n#6=IFCPOLYGONALBOUNDEDHALFSPACE($,$,$,$);\n#7=IFCBOOLEANRESULT(.DIFFERENCE.,#5,#6);\n#8=IFCBOOLEANRESULT(.INTERSECTION.,#5,#4);\n#9=IFCADVANCEDBREP($);\n#10=IFCISHAPEPROFILEDEF(.AREA.,$,$,2.,4.,0.5,0.5,0.);\n#11=IFCEXTRUDEDAREASOLID(#10,#2,#3,4.);\n#12=IFCCSHAPEPROFILEDEF(.AREA.,$,$,2.);\n#13=IFCEXTRUDEDAREASOLID(#12,#2,#3,4.);"
        );
        let (p, issues) = run(&data, 7);
        assert_eq!(p.len(), 1);
        assert!(issues[0].contains("bounded half space"), "{issues:?}");
        let (_, issues) = run(&data, 8);
        assert!(issues[0].contains("IFCRECTANGLEPROFILEDEF"), "{issues:?}");
        let (_, issues) = run(&data, 9);
        assert_eq!(issues, vec!["unsupported geometry IFCADVANCEDBREP"]);
        let (p, _) = run(&data, 11);
        assert!(
            (volume(&piece_mesh(&p[0]).unwrap()) - (2.0 * 0.5 * 2.0 + 3.0 * 0.5) * 4.0 / 1.0 * 1.0)
                .abs()
                < 2.1
        );
        let (p, issues) = run(&data, 13);
        assert!(p.is_empty());
        assert!(issues.iter().any(|i| i.contains("IFCCSHAPEPROFILEDEF")));
        let (_, issues) = run(&data, 99);
        assert!(issues[0].contains("missing"));
    }

    #[test]
    fn mapped_items_expand_or_become_instances() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,1.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,1.);\n#6=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#5));\n#7=IFCREPRESENTATIONMAP(#2,#6);\n#8=IFCCARTESIANPOINT((10.,0.,0.));\n#9=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#8,2.,$);\n#10=IFCMAPPEDITEM(#7,#9);"
        );
        let (p, _) = run(&data, 10);
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        // Scale 2 at x = 10: a 2 x 2 x 2 cube.
        assert!((volume(&ex.mesh()) - 8.0).abs() < 1e-9);
        let x = index(&data);
        let cx = Ctx::new(&x);
        let mut ev = Eval::new(&cx, 1_000_000);
        let yes = |id: u64| id == 7;
        ev.instancing = Some(&yes);
        ev.item(10, &DAffine3::IDENTITY, 0);
        let Piece::Instance { source, xf } = &ev.pieces[0] else {
            panic!()
        };
        assert_eq!(*source, 7);
        assert!((xf.transform_point3(DVec3::ZERO) - DVec3::new(10.0, 0.0, 0.0)).length() < 1e-9);
    }

    #[test]
    fn bounding_boxes_and_curves() {
        let data = format!(
            "{BASE}#4=IFCBOUNDINGBOX(#1,2.,3.,4.);\n#5=IFCBOUNDINGBOX(#1,0.,3.,4.);\n#6=IFCCARTESIANPOINT((1.,1.));\n#7=IFCCARTESIANPOINT((4.,1.));\n#8=IFCPOLYLINE((#6,#7));\n#9=IFCGEOMETRICCURVESET((#8,#1));"
        );
        let (p, _) = run(&data, 4);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 24.0).abs() < 1e-9);
        assert!(run(&data, 5).0.is_empty());
        let (p, _) = run(&data, 9);
        assert_eq!(p.len(), 2);
        let Piece::Curve(c) = &p[0] else { panic!() };
        assert_eq!(c.pts.len(), 2);
        assert!(c.exact.is_some() && !c.closed);
        assert!(matches!(p[1], Piece::Point(_)));
    }

    #[test]
    fn mesh_helpers() {
        let mut m = MeshData::cuboid(DVec3::ZERO, DVec3::ONE);
        m.transform(&DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        // A mirror keeps the solid right side out.
        assert!((volume(&m) - 1.0).abs() < 1e-9);
        let mut w = MeshData::cuboid(DVec3::ZERO, DVec3::ONE);
        w.append(&MeshData::cuboid(DVec3::ZERO, DVec3::ONE));
        assert_eq!(w.v.len(), 16);
        w.weld(1e-6);
        assert_eq!(w.v.len(), 8);
        assert!(MeshData::default().bounds().is_none());
    }
}
