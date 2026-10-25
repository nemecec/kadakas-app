//! STEP entities of a boundary representation (shells, faces, loops, edges) into the exact
//! payload of the core ([`ExactSolid`]). The triangles are cut from the payload by `tess`.

use std::collections::HashMap;

use kadakas_core::exact::{
    ExactEdge, ExactFace, ExactLoop, ExactShell, ExactSolid, LoopItem, VERSION,
};

use crate::ctx::{Ctx, Fail, R, attr, flag, reference, refs, skip};
use crate::tess::Rgba;

/// A payload that grows while the entities are read. Entities are numbered once.
pub struct ExactBuilder {
    pub solid: ExactSolid,
    vertices: HashMap<u64, u32>,
    edges: HashMap<u64, u32>,
}

impl Default for ExactBuilder {
    fn default() -> Self {
        ExactBuilder {
            solid: ExactSolid {
                version: VERSION,
                product: String::new(),
                unit_mm: 1.0,
                tolerance_mm: 0.0,
                angle_deg: 0.0,
                vertices: Vec::new(),
                edges: Vec::new(),
                shells: Vec::new(),
                faces: Vec::new(),
                mesh_hash: 0,
                extra: Default::default(),
            },
            vertices: HashMap::new(),
            edges: HashMap::new(),
        }
    }
}

impl ExactBuilder {
    pub fn begin_shell(&mut self, closed: bool, outward: Option<bool>) -> u32 {
        self.solid.shells.push(ExactShell { closed, outward });
        (self.solid.shells.len() - 1) as u32
    }

    fn vertex(&mut self, ctx: &mut Ctx, id: u64) -> R<u32> {
        if let Some(v) = self.vertices.get(&id) {
            return Ok(*v);
        }
        let e = ctx.ent(id)?;
        let p = match e.name.as_str() {
            "VERTEX_POINT" => ctx.point(reference(attr(e, 1)?).unwrap_or(0))?,
            "CARTESIAN_POINT" => ctx.point(id)?,
            other => return skip(format!("an edge ends at a {other}, not at a VERTEX_POINT")),
        };
        self.solid.vertices.push(p);
        let i = (self.solid.vertices.len() - 1) as u32;
        self.vertices.insert(id, i);
        Ok(i)
    }

    /// A vertex for a point of a polygon loop (the number is the entity of the point).
    fn corner(&mut self, ctx: &mut Ctx, id: u64) -> R<u32> {
        if let Some(v) = self.vertices.get(&id) {
            return Ok(*v);
        }
        let p = ctx.point(id)?;
        self.solid.vertices.push(p);
        let i = (self.solid.vertices.len() - 1) as u32;
        self.vertices.insert(id, i);
        Ok(i)
    }

    fn edge(&mut self, ctx: &mut Ctx, id: u64) -> R<u32> {
        if let Some(v) = self.edges.get(&id) {
            return Ok(*v);
        }
        let e = ctx.ent(id)?;
        if e.name != "EDGE_CURVE" {
            return skip(format!("an edge is a {}, not an EDGE_CURVE", e.name));
        }
        let start = self.vertex(ctx, reference(attr(e, 1)?).unwrap_or(0))?;
        let end = self.vertex(ctx, reference(attr(e, 2)?).unwrap_or(0))?;
        let same_sense = flag(attr(e, 4)?).unwrap_or(true);
        let curve_id = reference(attr(e, 3)?).unwrap_or(0);
        let curve = match ctx.curve(curve_id) {
            Ok(c) => Some((*c).clone()),
            Err(Fail::Skip(why)) => {
                ctx.skip_id(curve_id, &format!("{why}: the edge is a straight line"));
                None
            }
            Err(f) => return Err(f),
        };
        self.solid.edges.push(ExactEdge {
            curve,
            start,
            end,
            same_sense,
        });
        let i = (self.solid.edges.len() - 1) as u32;
        self.edges.insert(id, i);
        Ok(i)
    }
}

impl<'a> Ctx<'a> {
    fn read_loop(&mut self, b: &mut ExactBuilder, id: u64) -> R<Vec<LoopItem>> {
        let e = self.ent(id)?;
        let mut items = Vec::new();
        match e.name.as_str() {
            "EDGE_LOOP" => {
                let list = refs(attr(e, 1)?);
                if list.is_empty() {
                    return skip("a loop has no edges");
                }
                for oe in list {
                    let o = self.ent(oe)?;
                    let (edge, forward) = match o.name.as_str() {
                        "ORIENTED_EDGE" => (
                            reference(attr(o, 3)?).unwrap_or(0),
                            flag(attr(o, 4)?).unwrap_or(true),
                        ),
                        "EDGE_CURVE" => (oe, true),
                        other => return skip(format!("a loop holds a {other}")),
                    };
                    let edge = b.edge(self, edge)?;
                    items.push(LoopItem::Edge { edge, forward });
                }
            }
            "POLY_LOOP" => {
                for p in refs(attr(e, 1)?) {
                    let vertex = b.corner(self, p)?;
                    items.push(LoopItem::Vertex { vertex });
                }
            }
            "VERTEX_LOOP" => return skip("a loop of one vertex has no area"),
            other => return skip(format!("the loop type {other} is not supported")),
        }
        Ok(items)
    }

    /// Reads one face into the payload. A face that cannot be read is counted as skipped.
    pub fn read_face(
        &mut self,
        b: &mut ExactBuilder,
        shell: u32,
        id: u64,
        flip_in: bool,
        inherited: Option<Rgba>,
    ) -> R<()> {
        let mut e = self.ent(id)?;
        let mut flip = flip_in;
        if e.name == "ORIENTED_FACE" {
            flip ^= !flag(attr(e, 3)?).unwrap_or(true);
            e = self.ent(reference(attr(e, 2)?).unwrap_or(0))?;
        }
        let color = self
            .colors
            .get(&id)
            .or_else(|| self.colors.get(&e.id))
            .copied()
            .or(inherited);
        let (geometry, same_sense) = match e.name.as_str() {
            "ADVANCED_FACE" | "FACE_SURFACE" => {
                (reference(attr(e, 2)?), flag(attr(e, 3)?).unwrap_or(true))
            }
            "FACE" => (None, true),
            other => return skip(format!("a shell holds a {other}, not a face")),
        };
        // Keep the payload as it was if the face fails half way.
        let (nv, ne) = (b.solid.vertices.len(), b.solid.edges.len());
        let result = self.read_face_inner(b, shell, e, geometry, same_sense, flip, color);
        match result {
            Err(Fail::Skip(why)) => {
                // Edges and vertices that were added stay (another face may use them); the
                // maps keep their numbers valid. Only the face is missing.
                let _ = (nv, ne);
                self.skip_entity(e, &format!("face not read: {why}"));
                Ok(())
            }
            other => other,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn read_face_inner(
        &mut self,
        b: &mut ExactBuilder,
        shell: u32,
        e: &'a crate::part21::Entity,
        geometry: Option<u64>,
        same_sense: bool,
        flip: bool,
        color: Option<Rgba>,
    ) -> R<()> {
        let mut loops: Vec<ExactLoop> = Vec::new();
        for bound in refs(attr(e, 1)?) {
            let be = self.ent(bound)?;
            let outer = be.name == "FACE_OUTER_BOUND";
            if !outer && be.name != "FACE_BOUND" {
                return skip(format!("a face has a {} as a bound", be.name));
            }
            let mut items = self.read_loop(b, reference(attr(be, 1)?).unwrap_or(0))?;
            if !flag(attr(be, 2)?).unwrap_or(true) {
                items.reverse();
                for it in &mut items {
                    if let LoopItem::Edge { forward, .. } = it {
                        *forward = !*forward;
                    }
                }
            }
            loops.push(ExactLoop { outer, items });
        }
        if loops.is_empty() {
            return skip("a face has no bounds");
        }
        let surface = match geometry {
            Some(g) => match self.surface(g) {
                Ok(s) => Some((*s).clone()),
                Err(Fail::Skip(why)) => {
                    self.skip_id(g, &why);
                    return skip(why);
                }
                Err(f) => return Err(f),
            },
            None => None,
        };
        b.solid.faces.push(ExactFace {
            shell,
            surface,
            same_sense,
            flip,
            loops,
            color,
            source: e.id,
        });
        Ok(())
    }
}
