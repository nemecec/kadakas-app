//! The geometry items of a representation: solids, surface models, tessellated items and curve
//! sets, read into bodies in millimeters.

use std::collections::BTreeMap;

use glam::{DAffine3, DVec3};

use crate::brep::{Builder, Rgba};
use crate::ctx::{Ctx, Fail, R, attr, flag, int, list, num, reference, refs, skip};
use crate::part21::{Entity, Value};

/// Triangles with a color, in millimeters.
#[derive(Clone, Debug, Default)]
pub struct Body {
    pub name: String,
    pub positions: Vec<DVec3>,
    pub groups: BTreeMap<Option<Rgba>, Vec<[u32; 3]>>,
}

impl Body {
    pub fn triangles(&self) -> usize {
        self.groups.values().map(Vec::len).sum()
    }

    /// The body moved by `m` (a transform with a mirror turns the triangles inside out).
    pub fn transformed(&self, m: &DAffine3) -> Body {
        let mirror = m.matrix3.determinant() < 0.0;
        Body {
            name: self.name.clone(),
            positions: self.positions.iter().map(|p| m.transform_point3(*p)).collect(),
            groups: self
                .groups
                .iter()
                .map(|(c, t)| {
                    (
                        *c,
                        t.iter()
                            .map(|t| if mirror { [t[0], t[2], t[1]] } else { *t })
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

/// A polyline of a curve set, in millimeters.
#[derive(Clone, Debug)]
pub struct CurveBody {
    pub name: String,
    pub points: Vec<DVec3>,
    pub closed: bool,
    pub color: Option<Rgba>,
}

/// What a representation holds.
#[derive(Clone, Debug, Default)]
pub struct RepGeom {
    pub bodies: Vec<Body>,
    pub curves: Vec<CurveBody>,
}

impl RepGeom {
    pub fn triangles(&self) -> u64 {
        self.bodies.iter().map(|b| b.triangles() as u64).sum()
    }

    pub fn vertices(&self) -> u64 {
        self.bodies.iter().map(|b| b.positions.len() as u64).sum::<u64>()
            + self.curves.iter().map(|c| c.points.len() as u64).sum::<u64>()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.curves.is_empty()
    }

    pub fn transformed(&self, m: &DAffine3) -> RepGeom {
        RepGeom {
            bodies: self.bodies.iter().map(|b| b.transformed(m)).collect(),
            curves: self
                .curves
                .iter()
                .map(|c| CurveBody {
                    points: c.points.iter().map(|p| m.transform_point3(*p)).collect(),
                    ..c.clone()
                })
                .collect(),
        }
    }
}

/// The names of the entity types that are geometry items of a representation.
pub const ITEM_TYPES: &[&str] = &[
    "MANIFOLD_SOLID_BREP",
    "BREP_WITH_VOIDS",
    "FACETED_BREP",
    "SHELL_BASED_SURFACE_MODEL",
    "FACE_BASED_SURFACE_MODEL",
    "TESSELLATED_SOLID",
    "TESSELLATED_SHELL",
    "TRIANGULATED_FACE",
    "TRIANGULATED_FACE_SET",
    "TRIANGULATED_SURFACE_SET",
    "COMPLEX_TRIANGULATED_FACE",
    "COMPLEX_TRIANGULATED_SURFACE_SET",
    "GEOMETRIC_CURVE_SET",
    "GEOMETRIC_SET",
];

fn scaled(b: Builder, name: String, scale: f64) -> Body {
    Body {
        name,
        positions: b.positions.iter().map(|p| *p * scale).collect(),
        groups: b.groups,
    }
}

impl<'a> Ctx<'a> {
    /// The faces of a shell with the flip of each.
    fn shell_faces(&mut self, id: u64, flip: bool, depth: usize) -> R<Vec<(u64, bool)>> {
        if depth > 4 {
            return skip("shells are nested too deep");
        }
        let e = self.ent(id)?;
        match e.name.as_str() {
            "CLOSED_SHELL" | "OPEN_SHELL" | "CONNECTED_FACE_SET" | "CONNECTED_FACE_SUB_SET" => {
                Ok(refs(attr(e, 1)?).into_iter().map(|f| (f, flip)).collect())
            }
            "ORIENTED_CLOSED_SHELL" | "ORIENTED_OPEN_SHELL" => {
                let o = flag(attr(e, 3)?).unwrap_or(true);
                self.shell_faces(reference(attr(e, 2)?).unwrap_or(0), flip ^ !o, depth + 1)
            }
            other => skip(format!("a {other} is not a shell")),
        }
    }

    /// Adds the faces of a shell to the pending triangles of the builder.
    fn shell_into(
        &mut self,
        b: &mut Builder,
        shell: u64,
        flip: bool,
        outward: Option<bool>,
        inherited: Option<Rgba>,
    ) -> R<()> {
        let faces = match self.shell_faces(shell, flip, 0) {
            Ok(f) => f,
            Err(Fail::Skip(why)) => {
                self.skip_id(shell, &format!("shell not read: {why}"));
                return Ok(());
            }
            Err(f) => return Err(f),
        };
        let color = self.colors.get(&shell).copied().or(inherited);
        for (f, fl) in faces {
            self.face(b, f, fl, color)?;
        }
        if b.commit(outward) {
            self.note("a closed shell had its normals inward: it was turned inside out");
        }
        Ok(())
    }

    fn solid(&mut self, e: &'a Entity, scale: f64) -> R<Body> {
        let mut b = Builder::default();
        let outer = reference(attr(e, 1)?).unwrap_or(0);
        let color = self.colors.get(&e.id).copied();
        self.shell_into(&mut b, outer, false, Some(true), color)?;
        if e.name == "BREP_WITH_VOIDS" {
            for v in refs(attr(e, 2)?) {
                self.shell_into(&mut b, v, false, Some(false), color)?;
            }
        }
        Ok(scaled(b, text_of(e), scale))
    }

    fn surface_model(&mut self, e: &'a Entity, scale: f64) -> R<Body> {
        let mut b = Builder::default();
        let color = self.colors.get(&e.id).copied();
        for s in refs(attr(e, 1)?) {
            self.shell_into(&mut b, s, false, None, color)?;
        }
        Ok(scaled(b, text_of(e), scale))
    }

    /// The coordinates of a COORDINATES_LIST in millimeters.
    fn coordinates(&mut self, id: u64, scale: f64) -> R<Vec<DVec3>> {
        let e = self.ent(id)?;
        if e.name != "COORDINATES_LIST" {
            return skip(format!("tessellated points are in a {}", e.name));
        }
        let rows = list(attr(e, 2)?).ok_or(()).or_else(|()| skip("a coordinate list has no points"))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let c = list(r).map(|l| l.iter().filter_map(num).collect::<Vec<_>>()).unwrap_or_default();
            if c.len() != 3 || c.iter().any(|x| x.abs() > 1e12) {
                return skip("a tessellated point is not three usable numbers");
            }
            out.push(DVec3::new(c[0], c[1], c[2]) * scale);
        }
        Ok(out)
    }

    /// A triangulated face or set: the given triangles.
    fn triangulated(&mut self, e: &'a Entity, scale: f64) -> R<Body> {
        let coords_id = e
            .attrs
            .iter()
            .filter_map(reference)
            .find(|r| self.peek(*r).is_some_and(|x| x.name == "COORDINATES_LIST"))
            .ok_or(())
            .or_else(|()| skip("a tessellated item has no coordinate list"))?;
        let pts = self.coordinates(coords_id, scale)?;
        let ints = |v: &Value| -> Option<Vec<i64>> { list(v)?.iter().map(int).collect() };
        let lol = |v: &Value| -> Option<Vec<Vec<i64>>> { list(v)?.iter().map(ints).collect() };
        // The flat list of integers is the point index (maybe empty). The lists of lists are
        // the triangles, or the strips and the fans.
        let mut pnindex: Vec<i64> = Vec::new();
        let mut sets: Vec<Vec<Vec<i64>>> = Vec::new();
        for a in &e.attrs {
            if let Value::List(items) = a {
                if items.iter().all(|i| matches!(i, Value::Int(_))) {
                    pnindex = ints(a).unwrap_or_default();
                } else if items.iter().all(|i| matches!(i, Value::List(l) if l.iter().all(|x| matches!(x, Value::Int(_))))) {
                    if let Some(l) = lol(a) {
                        sets.push(l);
                    }
                }
            }
        }
        let n = pts.len() as i64;
        let point = |i: i64| -> Option<u32> {
            let j = if pnindex.is_empty() {
                i
            } else {
                *pnindex.get(usize::try_from(i - 1).ok()?)?
            };
            (1..=n).contains(&j).then(|| (j - 1) as u32)
        };
        let complex = e.name.starts_with("COMPLEX_");
        let mut tris: Vec<[u32; 3]> = Vec::new();
        let mut bad = 0u64;
        if complex {
            // Strips then fans.
            let mut it = sets.into_iter();
            let strips = it.next().unwrap_or_default();
            let fans = it.next().unwrap_or_default();
            for s in strips {
                for k in 0..s.len().saturating_sub(2) {
                    let (a, b, c) = (point(s[k]), point(s[k + 1]), point(s[k + 2]));
                    match (a, b, c) {
                        (Some(a), Some(b), Some(c)) => {
                            tris.push(if k % 2 == 0 { [a, b, c] } else { [b, a, c] })
                        }
                        _ => bad += 1,
                    }
                }
            }
            for f in fans {
                for k in 1..f.len().saturating_sub(1) {
                    match (point(f[0]), point(f[k]), point(f[k + 1])) {
                        (Some(a), Some(b), Some(c)) => tris.push([a, b, c]),
                        _ => bad += 1,
                    }
                }
            }
        } else {
            for t in sets.pop().unwrap_or_default() {
                match t.as_slice() {
                    [a, b, c] => match (point(*a), point(*b), point(*c)) {
                        (Some(a), Some(b), Some(c)) => tris.push([a, b, c]),
                        _ => bad += 1,
                    },
                    _ => bad += 1,
                }
            }
        }
        if bad > 0 {
            self.note(format!("{bad} tessellated triangles name a point that does not exist and were left out"));
        }
        self.count(pts.len() as u64, tris.len() as u64)?;
        self.tessellated_face("TESSELLATED (given triangles)", tris.len() as u64);
        let color = self.colors.get(&e.id).copied();
        let mut groups = BTreeMap::new();
        groups.insert(color, tris);
        Ok(Body { name: text_of(e), positions: pts, groups })
    }

    fn tessellated_group(&mut self, e: &'a Entity, scale: f64, out: &mut RepGeom) -> R<()> {
        let color = self.colors.get(&e.id).copied();
        let mut merged = Body { name: text_of(e), ..Default::default() };
        for item in refs(attr(e, 1)?) {
            let ie = self.ent(item)?;
            if !ie.name.contains("TRIANGULATED") {
                self.skip_entity(ie, "tessellated item that is not a set of triangles");
                continue;
            }
            match self.triangulated(ie, scale) {
                Ok(b) => {
                    let base = merged.positions.len() as u32;
                    merged.positions.extend(b.positions);
                    for (c, t) in b.groups {
                        merged
                            .groups
                            .entry(c.or(color))
                            .or_default()
                            .extend(t.into_iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
                    }
                }
                Err(Fail::Skip(why)) => self.skip_entity(ie, &why),
                Err(f) => return Err(f),
            }
        }
        out.bodies.push(merged);
        Ok(())
    }

    fn curve_set(&mut self, e: &'a Entity, scale: f64, out: &mut RepGeom) -> R<()> {
        let color = self.colors.get(&e.id).copied();
        for el in refs(attr(e, 1)?) {
            let ce = self.ent(el)?;
            let c = match self.curve(el) {
                Ok(c) => c,
                Err(Fail::Skip(why)) => {
                    self.skip_entity(ce, &why);
                    continue;
                }
                Err(f) => return Err(f),
            };
            let Some(pts) = c.sample_all(self.tol, self.angle) else {
                self.skip_entity(ce, "a curve without ends cannot be drawn");
                continue;
            };
            self.count(pts.len() as u64, 0)?;
            let closed = pts.len() > 2 && pts[0].distance(pts[pts.len() - 1]) <= 1e-9 * (1.0 + pts[0].length());
            let mut points: Vec<DVec3> = pts.iter().map(|p| *p * scale).collect();
            if closed {
                points.pop();
            }
            out.curves.push(CurveBody {
                name: text_of(e),
                points,
                closed,
                color: self.colors.get(&el).copied().or(color),
            });
        }
        Ok(())
    }

    /// Reads the items of a representation (their numbers) into `out`, in millimeters. The
    /// coordinates in the file are multiplied by `scale`.
    pub fn read_items(&mut self, items: &[u64], scale: f64, out: &mut RepGeom) -> R<()> {
        for &id in items {
            let e = match self.ent(id) {
                Ok(e) => e,
                Err(Fail::Skip(_)) => continue,
                Err(f) => return Err(f),
            };
            let r = match e.name.as_str() {
                "MANIFOLD_SOLID_BREP" | "FACETED_BREP" | "BREP_WITH_VOIDS" => {
                    self.solid(e, scale).map(|b| out.bodies.push(b))
                }
                "SHELL_BASED_SURFACE_MODEL" | "FACE_BASED_SURFACE_MODEL" => {
                    self.surface_model(e, scale).map(|b| out.bodies.push(b))
                }
                "TRIANGULATED_FACE" | "TRIANGULATED_FACE_SET" | "TRIANGULATED_SURFACE_SET"
                | "COMPLEX_TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_SURFACE_SET" => {
                    self.triangulated(e, scale).map(|b| out.bodies.push(b))
                }
                "TESSELLATED_SOLID" | "TESSELLATED_SHELL" => self.tessellated_group(e, scale, out),
                "GEOMETRIC_CURVE_SET" | "GEOMETRIC_SET" => self.curve_set(e, scale, out),
                "AXIS2_PLACEMENT_3D" | "AXIS1_PLACEMENT" | "AXIS2_PLACEMENT_2D" => Ok(()),
                other => {
                    self.skip_entity(e, &format!("the item type {other} is not supported"));
                    Ok(())
                }
            };
            match r {
                Ok(()) => {}
                Err(Fail::Skip(why)) => self.skip_entity(e, &why),
                Err(f) => return Err(f),
            }
        }
        Ok(())
    }
}

fn text_of(e: &Entity) -> String {
    e.attrs
        .first()
        .and_then(crate::ctx::text)
        .unwrap_or("")
        .trim()
        .to_string()
}
