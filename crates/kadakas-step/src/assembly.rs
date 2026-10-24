//! Products, representations and assemblies: which geometry belongs to which product, and
//! where each instance stands.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use glam::{DAffine3, DVec3};

use crate::body::{ITEM_TYPES, RepGeom};
use crate::ctx::{Ctx, Fail, R, attr, flag, list, reference, refs, skip, text};
use crate::part21::{COMPLEX, Entity};

/// One placed copy of the geometry of a product.
pub struct Instance {
    pub name: String,
    pub geom: Rc<RepGeom>,
    pub placement: DAffine3,
}

pub struct Structure {
    pub instances: Vec<Instance>,
    /// Number of products found.
    pub products: usize,
}

fn is_rep(e: &Entity) -> bool {
    e.name.ends_with("REPRESENTATION") && !e.name.ends_with("_RELATIONSHIP")
        || e.name == COMPLEX && (e.is("SHAPE_REPRESENTATION") || e.is("REPRESENTATION"))
}

struct Usage {
    nauo: u64,
    parent: u64,
    child: u64,
}

/// The product graph, for the walk.
struct Graph<'g> {
    pd_name: &'g HashMap<u64, String>,
    pd_reps: &'g HashMap<u64, Vec<u64>>,
    kids: &'g HashMap<u64, Vec<&'g Usage>>,
    tf: &'g HashMap<u64, DAffine3>,
}

/// What the walk keeps between its two runs.
#[derive(Default)]
struct Walk {
    used_reps: HashSet<u64>,
    visits: usize,
}

struct Place {
    rep_1: u64,
    rep_2: u64,
    item_1: u64,
    item_2: u64,
}

impl<'a> Ctx<'a> {
    /// The items and the context of a representation.
    fn rep_parts(&mut self, rep: u64) -> R<(Vec<u64>, u64)> {
        let e = self.ent(rep)?;
        if !is_rep(e) {
            return skip(format!("#{rep} is a {}, not a representation", e.name));
        }
        let a: &[crate::part21::Value] = if e.name == COMPLEX {
            e.part("REPRESENTATION").unwrap_or(&[])
        } else {
            &e.attrs
        };
        let items = refs(attr_at(a, 1)?);
        let context = reference(attr_at(a, 2)?).unwrap_or(0);
        Ok((items, context))
    }

    /// Millimeters per unit of a representation, and the angle unit; sets the tolerance.
    fn enter_rep(&mut self, context: u64) -> f64 {
        let (len, ang) = self.context_units(context);
        self.angle_unit = ang;
        let scale = match (self.opts.unit, len) {
            (Some(u), _) => u.mm(),
            (None, Some(l)) => l,
            (None, None) => {
                self.note("a representation names no length unit: millimeters are assumed");
                self.unit_from_file = false;
                1.0
            }
        };
        if !self.unit_scales.contains(&scale) {
            self.unit_scales.push(scale);
        }
        let tol_mm = self
            .opts
            .tolerance_mm
            .unwrap_or_else(|| (self.diag_mm(scale) / 2000.0).clamp(0.01, 0.5));
        self.tol = tol_mm / scale;
        self.tol_mm.get_or_insert(tol_mm);
        self.angle = self.opts.angle_deg.to_radians().clamp(0.02, 1.5);
        scale
    }

    /// The size of the file: the diagonal of the box around all points, in millimeters.
    fn diag_mm(&self, scale: f64) -> f64 {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for e in &self.file.entities {
            if e.name == "CARTESIAN_POINT"
                && let Some(c) = e.attrs.get(1).and_then(crate::ctx::reals)
                && c.len() == 3
            {
                let p = DVec3::new(c[0], c[1], c[2]);
                if p.is_finite() && p.abs().max_element() < 1e12 {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
        if lo.x > hi.x {
            1000.0
        } else {
            ((hi - lo).length() * scale).max(1.0)
        }
    }

    /// The geometry of the items of one representation, in millimeters.
    fn rep_geometry(
        &mut self,
        rep: u64,
        cache: &mut HashMap<u64, Rc<RepGeom>>,
        depth: usize,
    ) -> R<Rc<RepGeom>> {
        if let Some(g) = cache.get(&rep) {
            return Ok(g.clone());
        }
        if depth > 8 {
            return skip("representations are nested too deep");
        }
        let (items, context) = self.rep_parts(rep)?;
        let scale = self.enter_rep(context);
        let mut geom = RepGeom::default();
        // Items that place another representation.
        let mut plain = Vec::new();
        let mut mapped = Vec::new();
        for it in items {
            match self.peek(it) {
                Some(e) if e.name == "MAPPED_ITEM" => mapped.push(it),
                _ => plain.push(it),
            }
        }
        self.read_items(&plain, scale, &mut geom)?;
        for m in mapped {
            let e = self.ent(m)?;
            match self.mapped_item(e, scale, cache, depth) {
                Ok(g) => {
                    geom.bodies.extend(g.bodies);
                    geom.curves.extend(g.curves);
                }
                Err(Fail::Skip(why)) => self.skip_entity(e, &why),
                Err(f) => return Err(f),
            }
            // The tolerance and units of this representation again.
            self.enter_rep(context);
        }
        let g = Rc::new(geom);
        cache.insert(rep, g.clone());
        Ok(g)
    }

    fn mapped_item(
        &mut self,
        e: &'a Entity,
        scale: f64,
        cache: &mut HashMap<u64, Rc<RepGeom>>,
        depth: usize,
    ) -> R<RepGeom> {
        let map = self.ent(reference(attr(e, 1)?).unwrap_or(0))?;
        if map.name != "REPRESENTATION_MAP" {
            return skip("a mapped item without a representation map");
        }
        let rep = reference(attr(map, 1)?).unwrap_or(0);
        let origin = self.axis2(reference(attr(map, 0)?).unwrap_or(0))?;
        let target = self.axis2(reference(attr(e, 2)?).unwrap_or(0))?;
        let (_, ctx_id) = self.rep_parts(rep)?;
        self.mapped_used.insert(rep);
        let inner = self.rep_geometry(rep, cache, depth + 1)?;
        let (inner_len, _) = self.context_units(ctx_id);
        let inner_scale = self.opts.unit.map_or(inner_len.unwrap_or(1.0), |u| u.mm());
        let m = mm_frame(target, scale) * mm_frame(origin, inner_scale).inverse();
        // The copy is paid for before it is made.
        inner.charge_copy(self)?;
        Ok(inner.transformed(&m))
    }

    /// Reads the whole file: products and assemblies, else all the geometry items found.
    pub fn structure(&mut self) -> R<Structure> {
        let file = self.file;
        let mut cache: HashMap<u64, Rc<RepGeom>> = HashMap::new();
        // Product definitions: name and representations.
        let mut pd_name: HashMap<u64, String> = HashMap::new();
        let mut pd_reps: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut pds_to_pd: HashMap<u64, u64> = HashMap::new();
        let mut usages: Vec<Usage> = Vec::new();
        let mut pds_of_nauo: HashMap<u64, u64> = HashMap::new();
        let mut places: HashMap<u64, Place> = HashMap::new();
        let mut srr: Vec<(u64, u64)> = Vec::new();
        for e in &file.entities {
            match e.name.as_str() {
                "PRODUCT_DEFINITION" => {
                    pd_name.insert(e.id, self.product_name(e));
                    self.use_id(e.id);
                }
                "PRODUCT_DEFINITION_SHAPE" => {
                    if let Some(d) = e.attrs.get(2).and_then(reference) {
                        pds_to_pd.insert(e.id, d);
                        pds_of_nauo.insert(d, e.id);
                    }
                    self.use_id(e.id);
                }
                n if n.ends_with("USAGE_OCCURRENCE") || n == "ASSEMBLY_COMPONENT_USAGE" => {
                    if let (Some(p), Some(c)) = (
                        e.attrs.get(3).and_then(reference),
                        e.attrs.get(4).and_then(reference),
                    ) {
                        usages.push(Usage {
                            nauo: e.id,
                            parent: p,
                            child: c,
                        });
                        self.use_id(e.id);
                    }
                }
                "SHAPE_REPRESENTATION_RELATIONSHIP" => {
                    if let (Some(a), Some(b)) = (
                        e.attrs.get(2).and_then(reference),
                        e.attrs.get(3).and_then(reference),
                    ) {
                        srr.push((a, b));
                        self.use_id(e.id);
                    }
                }
                _ => {}
            }
        }
        for e in &file.entities {
            match e.name.as_str() {
                "SHAPE_DEFINITION_REPRESENTATION" => {
                    let (Some(d), Some(r)) = (
                        e.attrs.first().and_then(reference),
                        e.attrs.get(1).and_then(reference),
                    ) else {
                        continue;
                    };
                    if let Some(pd) = pds_to_pd.get(&d)
                        && pd_name.contains_key(pd)
                    {
                        pd_reps.entry(*pd).or_default().push(r);
                        self.use_id(e.id);
                    }
                }
                COMPLEX | "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION" => {
                    if let Some(p) = self.place_of(e, &pds_to_pd) {
                        places.insert(p.0, p.1);
                    }
                }
                _ => {}
            }
        }
        // Plain relationships between representations of the same product: shared geometry.
        let rep_owner: HashMap<u64, u64> = pd_reps
            .iter()
            .flat_map(|(pd, rs)| rs.iter().map(move |r| (*r, *pd)))
            .collect();
        for (a, b) in &srr {
            match (rep_owner.get(a), rep_owner.get(b)) {
                (Some(pd), None) => pd_reps.entry(*pd).or_default().push(*b),
                (None, Some(pd)) => pd_reps.entry(*pd).or_default().push(*a),
                _ => {}
            }
        }
        let mut out = Structure {
            instances: Vec::new(),
            products: pd_name.len(),
        };
        let any_geometry = pd_reps
            .values()
            .flatten()
            .any(|r| self.peek(*r).is_some_and(is_rep));
        if !any_geometry {
            return self.loose_items(&mut cache, out);
        }
        // Transform of each usage.
        let mut tf: HashMap<u64, DAffine3> = HashMap::new();
        let (mut missing, mut unreadable) = (0u64, 0u64);
        for u in &usages {
            let Some(pl) = pds_of_nauo.get(&u.nauo).and_then(|pds| places.get(pds)) else {
                missing += 1;
                continue;
            };
            let child_reps = pd_reps.get(&u.child).cloned().unwrap_or_default();
            if let Some(m) = self.place_matrix(pl, &child_reps) {
                tf.insert(u.nauo, m);
            } else {
                unreadable += 1;
            }
        }
        if missing > 0 {
            self.note(format!(
                "{missing} assembly usages have no placement in the file: the part stands where its own coordinates put it"
            ));
        }
        if unreadable > 0 {
            self.note(format!(
                "{unreadable} assembly usages have a placement that cannot be read: the part stands where its own coordinates put it"
            ));
        }
        // A product whose representation is mapped by another representation is placed by
        // the mapped items. It is not also a root at the origin.
        let mapped_targets: HashSet<u64> = file
            .entities
            .iter()
            .filter(|e| e.name == "REPRESENTATION_MAP")
            .filter_map(|e| e.attrs.get(1).and_then(reference))
            .collect();
        let children: HashSet<u64> = usages.iter().map(|u| u.child).collect();
        let mut kids: HashMap<u64, Vec<&Usage>> = HashMap::new();
        for u in &usages {
            kids.entry(u.parent).or_default().push(u);
        }
        let mut roots: Vec<u64> = pd_name
            .keys()
            .copied()
            .filter(|p| !children.contains(p))
            .collect();
        roots.sort_unstable();
        let mut held_back: Vec<u64> = Vec::new();
        roots.retain(|p| {
            let mapped = pd_reps
                .get(p)
                .is_some_and(|rs| rs.iter().any(|r| mapped_targets.contains(r)));
            if mapped {
                held_back.push(*p);
            }
            !mapped
        });
        let graph = Graph {
            pd_name: &pd_name,
            pd_reps: &pd_reps,
            kids: &kids,
            tf: &tf,
        };
        let mut state = Walk::default();
        let first: Vec<(u64, DAffine3, usize, Vec<u64>)> = roots
            .into_iter()
            .rev()
            .map(|r| (r, DAffine3::IDENTITY, 0, vec![r]))
            .collect();
        self.walk(&graph, first, &mut state, &mut out, &mut cache)?;
        // A mapped product that no mapped item reached stands at the origin after all.
        let lost: Vec<(u64, DAffine3, usize, Vec<u64>)> = held_back
            .into_iter()
            .filter(|pd| {
                !pd_reps
                    .get(pd)
                    .is_some_and(|rs| rs.iter().any(|r| self.mapped_used.contains(r)))
            })
            .map(|pd| (pd, DAffine3::IDENTITY, 0, vec![pd]))
            .collect();
        if !lost.is_empty() {
            self.note(format!(
                "{} products that other representations map were not used by any of them: they stand at the origin",
                lost.len()
            ));
            self.walk(&graph, lost, &mut state, &mut out, &mut cache)?;
        }
        Ok(out)
    }

    /// Walks the product graph depth first from the given starts, and adds the instances.
    fn walk(
        &mut self,
        g: &Graph,
        mut stack: Vec<(u64, DAffine3, usize, Vec<u64>)>,
        st: &mut Walk,
        out: &mut Structure,
        cache: &mut HashMap<u64, Rc<RepGeom>>,
    ) -> R<()> {
        while let Some((pd, m, depth, path)) = stack.pop() {
            // Every usage that is visited counts, with or without geometry: a file of a few
            // kilobytes can name 2^24 paths through a graph of 24 levels.
            st.visits += 1;
            self.charge(1 + path.len() as u64)?;
            if st.visits > self.limits.max_instances {
                return Err(Fail::Abort(kadakas_meshio::MeshError::TooManyParts {
                    count: st.visits as u64,
                    limit: self.limits.max_instances as u64,
                }));
            }
            if depth > self.limits.max_assembly_depth {
                self.note(format!(
                    "an assembly is nested deeper than {} levels: the rest is left out",
                    self.limits.max_assembly_depth
                ));
                continue;
            }
            let name = g.pd_name.get(&pd).cloned().unwrap_or_default();
            for rep in g.pd_reps.get(&pd).cloned().unwrap_or_default() {
                let geom = match self.rep_geometry(rep, cache, 0) {
                    Ok(x) => x,
                    Err(Fail::Skip(why)) => {
                        self.skip_id(rep, &why);
                        continue;
                    }
                    Err(f) => return Err(f),
                };
                if geom.is_empty() {
                    continue;
                }
                if out.instances.len() >= self.limits.max_instances {
                    return Err(Fail::Abort(kadakas_meshio::MeshError::TooManyParts {
                        count: out.instances.len() as u64 + 1,
                        limit: self.limits.max_instances as u64,
                    }));
                }
                if !st.used_reps.insert(rep) {
                    geom.charge_copy(self)?;
                }
                out.instances.push(Instance {
                    name: name.clone(),
                    geom,
                    placement: m,
                });
            }
            if let Some(ks) = g.kids.get(&pd) {
                for u in ks.iter().rev() {
                    if path.contains(&u.child) {
                        self.note("an assembly contains itself: the loop is cut");
                        continue;
                    }
                    let t = g.tf.get(&u.nauo).copied().unwrap_or(DAffine3::IDENTITY);
                    let mut p = path.clone();
                    p.push(u.child);
                    stack.push((u.child, m * t, depth + 1, p));
                }
            }
        }
        Ok(())
    }

    /// A file with no product structure: every solid, surface model and tessellated item that
    /// is not part of another becomes a body. The units come from the first context.
    fn loose_items(
        &mut self,
        cache: &mut HashMap<u64, Rc<RepGeom>>,
        mut out: Structure,
    ) -> R<Structure> {
        let file = self.file;
        let _ = cache;
        let inner: HashSet<u64> = file
            .entities
            .iter()
            .filter(|e| e.name == "TESSELLATED_SOLID" || e.name == "TESSELLATED_SHELL")
            .flat_map(|e| e.attrs.get(1).map(refs).unwrap_or_default())
            .collect();
        let items: Vec<u64> = file
            .entities
            .iter()
            .filter(|e| ITEM_TYPES.contains(&e.name.as_str()) && !inner.contains(&e.id))
            .map(|e| e.id)
            .collect();
        if items.is_empty() {
            return Ok(out);
        }
        let ctx_id = file
            .entities
            .iter()
            .find(|e| e.is("GLOBAL_UNIT_ASSIGNED_CONTEXT"))
            .map_or(0, |e| e.id);
        let scale = self.enter_rep(ctx_id);
        let mut geom = RepGeom::default();
        self.read_items(&items, scale, &mut geom)?;
        if !geom.is_empty() {
            out.instances.push(Instance {
                name: String::new(),
                geom: Rc::new(geom),
                placement: DAffine3::IDENTITY,
            });
        }
        Ok(out)
    }

    fn product_name(&mut self, pd: &Entity) -> String {
        let name = |ctx: &mut Ctx<'a>, id: u64, idx: usize| -> Option<String> {
            let e = ctx.ent(id).ok()?;
            let s = e.attrs.get(idx).and_then(text)?.trim().to_string();
            (!s.is_empty()).then_some(s)
        };
        let formation = pd.attrs.get(2).and_then(reference);
        let product = formation
            .and_then(|f| self.ent(f).ok())
            .and_then(|f| f.attrs.get(2).and_then(reference));
        product
            .and_then(|p| name(self, p, 1).or_else(|| name(self, p, 0)))
            .or_else(|| {
                pd.attrs
                    .first()
                    .and_then(text)
                    .map(str::to_string)
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or_default()
    }

    /// The placement that a CONTEXT_DEPENDENT_SHAPE_REPRESENTATION holds: the number of its
    /// PRODUCT_DEFINITION_SHAPE and the representations and axes.
    fn place_of(&mut self, e: &Entity, pds_to_pd: &HashMap<u64, u64>) -> Option<(u64, Place)> {
        let _ = pds_to_pd;
        if e.name != "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION" {
            return None;
        }
        let rel = self.ent(e.attrs.first().and_then(reference)?).ok()?;
        let pds = e.attrs.get(1).and_then(reference)?;
        let base = rel.part("REPRESENTATION_RELATIONSHIP")?;
        let (r1, r2) = (
            base.get(2).and_then(reference)?,
            base.get(3).and_then(reference)?,
        );
        let tr = rel.part("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?;
        let idt = self.ent(tr.first().and_then(reference)?).ok()?;
        if idt.name != "ITEM_DEFINED_TRANSFORMATION" {
            return None;
        }
        let (i1, i2) = (
            idt.attrs.get(2).and_then(reference)?,
            idt.attrs.get(3).and_then(reference)?,
        );
        self.use_id(e.id);
        Some((
            pds,
            Place {
                rep_1: r1,
                rep_2: r2,
                item_1: i1,
                item_2: i2,
            },
        ))
    }

    /// The matrix (in millimeters) that moves the child into the parent.
    fn place_matrix(&mut self, p: &Place, child_reps: &[u64]) -> Option<DAffine3> {
        let scale_of = |ctx: &mut Ctx<'a>, rep: u64| -> f64 {
            let Ok((_, c)) = ctx.rep_parts(rep) else {
                return 1.0;
            };
            let (l, _) = ctx.context_units(c);
            ctx.opts.unit.map_or(l.unwrap_or(1.0), |u| u.mm())
        };
        let f1 = mm_frame(self.axis2(p.item_1).ok()?, scale_of(self, p.rep_1));
        let f2 = mm_frame(self.axis2(p.item_2).ok()?, scale_of(self, p.rep_2));
        // item 1 lies in the representation 1. If that is the child, the child's frame goes
        // to the frame in the parent.
        if child_reps.contains(&p.rep_2) && !child_reps.contains(&p.rep_1) {
            Some(f1 * f2.inverse())
        } else {
            Some(f2 * f1.inverse())
        }
    }
}

/// A frame with its origin in millimeters.
fn mm_frame(f: DAffine3, scale: f64) -> DAffine3 {
    DAffine3 {
        matrix3: f.matrix3,
        translation: f.translation * scale,
    }
}

fn attr_at(a: &[crate::part21::Value], i: usize) -> R<&crate::part21::Value> {
    a.get(i).map_or_else(|| skip("an attribute is missing"), Ok)
}

#[allow(dead_code)]
fn unused(_: &[u8]) {
    let _ = (
        attr as fn(&Entity, usize) -> R<&crate::part21::Value>,
        flag,
        list,
    );
}
