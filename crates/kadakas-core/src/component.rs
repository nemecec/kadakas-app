//! Groups and components of the 3D modeler (MOD-07, section 4.23 of `docs/feature-spec.md`).
//!
//! # The two containers
//!
//! - A **group** is a `Shape::Group` object. Its members have it as their `parent`: surface models
//!   (the geometry of the group, each with its own `transform`), other groups and component
//!   instances. The group has no transform of its own, so a member keeps one transform that is
//!   always the one from its local coordinates to the drawing. Moving a group moves its members.
//! - A **component** is a definition with instances. The definition is a symbol resource
//!   (`GraphicResource::Symbol`) whose [`SymbolDef::parts`] hold 3D content: surface models, groups
//!   and instances of other components, as a flat list in which a part names the group part that
//!   holds it. Content is in definition coordinates. An instance is a `Shape::Component`: the
//!   symbol and a transform from definition coordinates to the drawing (12 numbers, the columns of
//!   the matrix and then the move). Every user of the definition reads it at the moment of use, so
//!   all instances show the same content and no instance holds a copy of it.
//!
//! # Commands
//!
//! All are `Command` variants and one undo step each:
//!
//! - `MakeGroup`: the selected faces and edges of a surface model (and any objects of the same
//!   level) go into a new group. The geometry is copied into a surface model of its own with the
//!   same ids and placement, and taken out of the old model (`SurfaceModel::extract` and
//!   `remove`), so it does not weld to the geometry that stays. If the selection is the whole
//!   model, the model object itself moves into the group.
//! - `MakeComponent`: the same selection becomes the parts of a new definition, and (unless
//!   `replace` is off) one instance replaces it in place. The axes of the definition are the
//!   axes of the model, so the instance transform is the model transform.
//! - `Explode`: a group or an instance goes. Its surface models merge into the surface model of
//!   the level (the first surface model among the siblings; see below), welding and re-forming
//!   faces by the rules of MOD-06 (`SurfaceModel::absorb`). Groups and instances inside move up
//!   one level. An instance that was the last user of its definition takes the definition with it.
//! - `MakeUnique`: an instance gets a copy of its definition (the name gets `#1`, `#2`, ...).
//!   Instances in one call that share a definition share the copy.
//! - `EditComponent` and `CloseComponent`: edit in place. The parts of the definition become
//!   objects in an edit group at the instance (the instance is hidden meanwhile), as for 2D
//!   symbols (`symbols::EDIT_GROUP_KEY`); everything else (draw, make group, explode, nest) runs
//!   on those objects. `CloseComponent` writes the objects back as the parts of the definition,
//!   and all instances show the change at once. The other instances do not change while the edit
//!   is open. A definition is edited in one place at a time.
//!
//! # The level of a surface model
//!
//! Raw geometry lives in the surface model objects of a level: the document root (objects with no
//! parent on a layer), a group, or an edit group. Where the geometry of an exploded group goes,
//! and where drawing in a context goes, is the **raw model** of the level: the first surface
//! model among the children, in z-order ([`raw_model`]). A level with none gets one when it is
//! needed ([`EditContext::ensure_raw_model`]) or, on explode, takes the first model of the group.
//!
//! # Edit context
//!
//! [`EditContext`] is the path of ids of the groups and instances that a person has entered, with
//! the flag `dim_rest` for the UI (everything outside the entered container is shown dimmed and
//! cannot be picked). Drawing in a context converts points from the drawing into the local
//! coordinates of the raw model through its transform ([`EditContext::apply_world`]).
//!
//! # Limits
//!
//! [`MAX_GROUP_DEPTH`] groups in groups, [`MAX_COMPONENT_DEPTH`] definitions in definitions,
//! [`MAX_LEAVES`] surface models in the expansion of one definition. A command or a walk that
//! would pass one fails with a message and changes nothing (`ComponentError`); nothing is cut
//! off silently. The numbers come from the measurement in `perf_limits` in `tests/components_files.rs`.
//!
//! # Collaboration and files
//!
//! Groups and instances are objects and definitions are resources, so every command is a change
//! of objects and resources that collaboration already sends. The surface models are changed with
//! `SurfaceModel::apply`, so their `rev` and edit log (`surface::sync`) stay true. Two users who
//! edit one definition at once are told at the second `EditComponent`. A definition that is
//! saved holds its parts in `parts` of the symbol (the saved form of a surface model is the one
//! of `surface::serial`).

use std::collections::{HashMap, HashSet};

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use crate::model::{Attributes, ClassId, Document, MAX_COORD, ObjectId, Shape};
use crate::resource::{ResourceId, Resources};
use crate::surface::SurfaceModel;

mod context;
pub use context::EditContext;

/// Groups inside groups (in the document or in a definition) are followed this deep. Measured in
/// `perf_limits` in `tests/components_files.rs` (debug build): a chain of 32 groups takes about
/// 5 ms to make and 2 ms to move, far below what a person notices.
pub const MAX_GROUP_DEPTH: usize = 32;
/// Definitions inside definitions. Measured in `perf_limits` in `tests/components_files.rs`: the
/// walk of a chain of 16 definitions takes about 2 microseconds.
pub const MAX_COMPONENT_DEPTH: usize = 16;
/// Most surface models in the expansion of one definition (nested instances counted each time).
/// Measured in `perf_limits` in `tests/components_files.rs`: the walk of 20,000 models takes about
/// 4 ms, and a scene of that many has one index, not one per model.
pub const MAX_LEAVES: usize = 20_000;
/// Most parts in one definition.
pub const MAX_PARTS: usize = 10_000;
/// Most parts, groups and instances that the expansion of one definition visits, every nested
/// instance counted each time it is used. This is the cost of a walk. It is a safety guard: a
/// chain of definitions that hold no model (empty groups) has no models to count, but the number
/// of paths through it grows with each level. Over the budget the expansion is refused with the
/// name of the definition.
pub const MAX_VISITED: usize = 1_000_000;
/// Most surface models over all instances of the document together, each instance counted with
/// the models of its whole expansion. The scene of an instance holds its own copy of the points
/// (no instancing in the mesh and in the inference scene): about 4.7 KB per model, so 250,000
/// models take about 1.2 GB. Measured in `docs/modeler-limits.md`.
pub const MAX_TOTAL_MODELS: usize = 250_000;
/// Most edges in all the surface models of the parts of one definition together.
pub const MAX_DEFINITION_EDGES: usize = 100_000;
/// Most bytes of the saved text (compact JSON) of the parts of one definition.
pub const MAX_DEFINITION_BYTES: usize = 16 * 1024 * 1024;
/// Most faces that one explode may add to the surface model of its level. An explode welds all
/// the parts into one model, and the cost grows faster than the number of faces.
pub const MAX_EXPLODE_FACES: usize = 20_000;

/// A number with a comma between the thousands, for the messages: 250,000.
pub fn grouped(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Why a group or component operation was refused. Nothing changed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ComponentError {
    #[error("groups are nested at most {max} deep")]
    GroupsTooDeep { max: usize },
    #[error("components are nested at most {max} deep")]
    ComponentsTooDeep { max: usize },
    #[error("a component expands to at most {max} surface models")]
    TooMany { max: usize },
    #[error(
        "component '{name}' has more than {} parts, groups and instances when each nested component is counted every time it is used. Remove nested components or empty groups",
        grouped(*max)
    )]
    TooBig { name: String, max: usize },
    #[error("total models would be {}, the limit is {}. {hint}", grouped(*total), grouped(*max))]
    TotalModels {
        total: usize,
        max: usize,
        hint: String,
    },
    #[error("a component cannot contain itself ({0})")]
    Cycle(ResourceId),
    #[error("component {0} does not exist")]
    Missing(ResourceId),
    #[error("a {0} cannot be part of a component. Use surface models, groups and components")]
    NotAllowed(String),
    #[error("{0}")]
    Invalid(String),
}

/// One part of a component definition: a surface model, a group or an instance of another
/// component, in definition coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// `Shape::SurfaceModel` (placed by its `transform`), `Shape::Group` or `Shape::Component`.
    pub shape: Shape,
    #[serde(default)]
    pub attrs: Attributes,
    /// The class of the part. Without one the part takes the class of the instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassId>,
    /// The index of the group part that holds this part. It is smaller than the index of this
    /// part, so the list is a tree and cannot have a cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
}

impl Part {
    /// A part with no attributes and no class, at the top.
    pub fn new(shape: Shape) -> Part {
        Part {
            shape,
            attrs: Attributes::default(),
            class: None,
            parent: None,
        }
    }
}

/// The transform of the twelve numbers in a `Shape::Component` or in a saved surface model.
pub fn affine(raw: &[f64; 12]) -> DAffine3 {
    DAffine3::from_cols_array(raw)
}

/// The twelve numbers of a transform.
pub fn raw(a: &DAffine3) -> [f64; 12] {
    a.to_cols_array()
}

/// The transform of an instance after a change of the map (a CRS change): its origin goes to
/// `origin`, the plan axes x and y become `dx` and `dy` (in drawing units per local unit), and
/// heights are scaled by `z_scale` (as `SurfaceModel::remap_plan`).
pub fn remap_plan(
    transform: &[f64; 12],
    origin: DVec3,
    dx: glam::DVec2,
    dy: glam::DVec2,
    z_scale: f64,
) -> [f64; 12] {
    let a = affine(transform);
    let map = glam::DMat3::from_cols(dx.extend(0.0), dy.extend(0.0), DVec3::Z * z_scale);
    raw(&DAffine3 {
        matrix3: map * a.matrix3,
        translation: origin,
    })
}

/// The definition and the transform of an instance, or `None` for another shape.
pub fn placement(shape: &Shape) -> Option<(ResourceId, DAffine3)> {
    match shape {
        Shape::Component { symbol, transform } => Some((*symbol, affine(transform))),
        _ => None,
    }
}

/// A transform that can be inverted and has no number out of range (an instance with a scale of
/// zero cannot be edited in place or exploded).
pub fn is_regular(a: &DAffine3) -> bool {
    a.is_finite()
        && a.translation.abs().max_element() <= MAX_COORD
        && a.matrix3.determinant().abs() > 1e-12
}

/// The first problem of the parts of a definition, or `None`.
pub fn parts_problem(parts: &[Part]) -> Option<String> {
    if parts.len() > MAX_PARTS {
        return Some(format!("a component has at most {MAX_PARTS} parts"));
    }
    let mut depth: Vec<usize> = Vec::with_capacity(parts.len());
    for (i, p) in parts.iter().enumerate() {
        let d = match p.parent {
            None => 1,
            Some(q) => {
                let q = q as usize;
                if q >= i || !matches!(parts[q].shape, Shape::Group { .. }) {
                    return Some(format!(
                        "part {i} is not inside a group that comes before it"
                    ));
                }
                depth[q] + 1
            }
        };
        if d > MAX_GROUP_DEPTH {
            return Some(format!("groups are nested at most {MAX_GROUP_DEPTH} deep"));
        }
        depth.push(d);
        if !p.attrs.is_finite() {
            return Some(format!("part {i} has a number that is not valid"));
        }
        match &p.shape {
            Shape::SurfaceModel(m) => {
                if let Some(msg) = m.problem() {
                    return Some(format!("part {i}: {msg}"));
                }
                if !m.is_finite() || !p.shape.is_in_range() {
                    return Some(format!("part {i} has a number that is not valid"));
                }
            }
            Shape::Component { transform, .. } => {
                if !is_regular(&affine(transform)) {
                    return Some(format!("part {i} has a transform that cannot be inverted"));
                }
            }
            Shape::Group { name } => {
                if name.chars().count() > 200 {
                    return Some(format!("part {i}: a group name has at most 200 characters"));
                }
            }
            other => {
                return Some(format!(
                    "part {i} is a {}. A component holds surface models, groups and components",
                    other.kind()
                ));
            }
        }
    }
    None
}

/// The symbols that the parts use (nested instances).
pub fn part_refs(parts: &[Part]) -> impl Iterator<Item = ResourceId> + '_ {
    parts.iter().filter_map(|p| match &p.shape {
        Shape::Component { symbol, .. } => Some(*symbol),
        _ => None,
    })
}

/// A surface model of the expansion of a definition: shared with the definition, not copied.
#[derive(Clone, Copy, Debug)]
pub struct Leaf<'a> {
    pub model: &'a SurfaceModel,
    /// From the local coordinates of the model to the coordinates of the outermost definition: the
    /// transforms of the nested instances and the own transform of the model.
    pub transform: DAffine3,
    pub class: Option<ClassId>,
    pub attrs: &'a Attributes,
}

/// What the expansion of a definition costs, counted once per definition (not once per path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expansion {
    /// Surface models in the expansion, nested components counted each time they are used.
    pub models: usize,
    /// Parts, groups and instances visited by a walk of the expansion ([`MAX_VISITED`]).
    pub nodes: usize,
    /// 1 for a definition with no instance inside, else 1 more than the deepest one inside.
    pub depth: usize,
}

/// The definition that fails, and why.
pub type ExpansionError = (ResourceId, ComponentError);

/// The memo of the expansion sizes of the definitions of one `Resources`. The size of a
/// definition is computed once from the sizes of the definitions it uses, so the work is
/// O(definitions + parts) however many paths run through the graph (a chain of eight instances
/// per level has 8 to the power of the levels paths). Every walk over a definition asks here
/// first: a walk that would be too long, too deep, too big, cyclic or incomplete is refused
/// before it starts.
///
/// A size is kept only when the definition is within the limits. A failure is kept per asked
/// definition. The cache is valid for one state of the resources: make a new one after a change.
pub struct ExpansionCache<'a> {
    res: &'a Resources,
    done: HashMap<ResourceId, Expansion>,
    failed: HashMap<ResourceId, ExpansionError>,
    open: Vec<ResourceId>,
}

impl<'a> ExpansionCache<'a> {
    pub fn new(res: &'a Resources) -> Self {
        ExpansionCache {
            res,
            done: HashMap::new(),
            failed: HashMap::new(),
            open: Vec::new(),
        }
    }

    /// The resources that this cache reads.
    pub fn resources(&self) -> &'a Resources {
        self.res
    }

    /// The size of the expansion of `id`. On a problem: the definition that fails (`id` or one
    /// it uses), and why.
    pub fn get(&mut self, id: ResourceId) -> Result<Expansion, ExpansionError> {
        if let Some(e) = self.done.get(&id) {
            return Ok(*e);
        }
        if let Some(f) = self.failed.get(&id) {
            return Err(f.clone());
        }
        self.open.clear();
        let r = self.measure(id);
        if let Err(e) = &r {
            self.failed.insert(id, e.clone());
        }
        r
    }

    fn measure(&mut self, id: ResourceId) -> Result<Expansion, ExpansionError> {
        if let Some(v) = self.done.get(&id) {
            return Ok(*v);
        }
        if self.open.contains(&id) {
            return Err((id, ComponentError::Cycle(id)));
        }
        let res = self.res;
        let def = res.symbol(id).ok_or((id, ComponentError::Missing(id)))?;
        // Stack safety: a chain deeper than the limit is refused before it is followed.
        if self.open.len() >= MAX_COMPONENT_DEPTH {
            return Err((
                id,
                ComponentError::ComponentsTooDeep {
                    max: MAX_COMPONENT_DEPTH,
                },
            ));
        }
        self.open.push(id);
        let mut models = 0usize;
        let mut nodes = def.parts.len();
        let mut depth = 1usize;
        for p in &def.parts {
            match &p.shape {
                Shape::SurfaceModel(_) => models += 1,
                Shape::Component { symbol, .. } => match self.measure(*symbol) {
                    Ok(inner) => {
                        models = models.saturating_add(inner.models);
                        nodes = nodes.saturating_add(inner.nodes);
                        depth = depth.max(inner.depth + 1);
                    }
                    Err(e) => {
                        self.open.pop();
                        return Err(e);
                    }
                },
                _ => {}
            }
        }
        self.open.pop();
        if models > MAX_LEAVES {
            return Err((id, ComponentError::TooMany { max: MAX_LEAVES }));
        }
        if depth > MAX_COMPONENT_DEPTH {
            return Err((
                id,
                ComponentError::ComponentsTooDeep {
                    max: MAX_COMPONENT_DEPTH,
                },
            ));
        }
        if nodes > MAX_VISITED {
            let name = res.get(id).map_or_else(String::new, |r| r.name.clone());
            return Err((
                id,
                ComponentError::TooBig {
                    name,
                    max: MAX_VISITED,
                },
            ));
        }
        let e = Expansion {
            models,
            nodes,
            depth,
        };
        self.done.insert(id, e);
        Ok(e)
    }
}

/// The surface models of a definition, nested components included, each with the transform from
/// its own coordinates to the coordinates of `symbol`. The models are those of the definition (a
/// reference), so a definition with many instances is read, not copied. Depth-first in the order
/// of the parts. Refuses a missing definition, a cycle, nesting over [`MAX_COMPONENT_DEPTH`],
/// more than [`MAX_LEAVES`] models and an expansion that visits more than [`MAX_VISITED`] parts.
pub fn leaves(res: &Resources, symbol: ResourceId) -> Result<Vec<Leaf<'_>>, ComponentError> {
    leaves_in(&mut ExpansionCache::new(res), symbol)
}

/// Like [`leaves`], with the sizes of the definitions kept in `cache` (for many calls on one
/// document: each definition is measured once).
pub fn leaves_in<'a>(
    cache: &mut ExpansionCache<'a>,
    symbol: ResourceId,
) -> Result<Vec<Leaf<'a>>, ComponentError> {
    let size = cache.get(symbol).map_err(|(_, e)| e)?;
    let res = cache.res;
    let mut out = Vec::with_capacity(size.models);
    let mut stack = vec![symbol];
    let mut visited = 0usize;
    walk(
        res,
        symbol,
        DAffine3::IDENTITY,
        &mut stack,
        &mut visited,
        &mut out,
    )?;
    Ok(out)
}

fn walk<'a>(
    res: &'a Resources,
    symbol: ResourceId,
    xf: DAffine3,
    stack: &mut Vec<ResourceId>,
    visited: &mut usize,
    out: &mut Vec<Leaf<'a>>,
) -> Result<(), ComponentError> {
    let def = res.symbol(symbol).ok_or(ComponentError::Missing(symbol))?;
    for p in &def.parts {
        // The cache has measured the expansion, so these checks cannot fail. They stay as a
        // second wall: a walk is never unbounded.
        *visited += 1;
        if *visited > MAX_VISITED {
            let name = res
                .get(stack[0])
                .map_or_else(String::new, |r| r.name.clone());
            return Err(ComponentError::TooBig {
                name,
                max: MAX_VISITED,
            });
        }
        match &p.shape {
            Shape::SurfaceModel(m) => {
                if out.len() >= MAX_LEAVES {
                    return Err(ComponentError::TooMany { max: MAX_LEAVES });
                }
                out.push(Leaf {
                    model: m,
                    transform: xf * m.transform,
                    class: p.class,
                    attrs: &p.attrs,
                });
            }
            Shape::Component {
                symbol: inner,
                transform,
            } => {
                if stack.contains(inner) {
                    return Err(ComponentError::Cycle(*inner));
                }
                if stack.len() >= MAX_COMPONENT_DEPTH {
                    return Err(ComponentError::ComponentsTooDeep {
                        max: MAX_COMPONENT_DEPTH,
                    });
                }
                stack.push(*inner);
                let r = walk(res, *inner, xf * affine(transform), stack, visited, out);
                stack.pop();
                r?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Checks, after the definition `changed` was added or changed, that it and every component
/// definition that uses it (directly or through others) still expands within the limits: no
/// missing definition, no cycle, nesting at most [`MAX_COMPONENT_DEPTH`], at most
/// [`MAX_LEAVES`] surface models and at most [`MAX_VISITED`] visited parts. One pass over the
/// graph, with the numbers of each definition counted once (no walk over the paths, however many
/// definitions use the same base). On a problem: the definition that fails, and why.
pub fn check_expansion(
    res: &Resources,
    changed: ResourceId,
) -> Result<(), (ResourceId, ComponentError)> {
    // The reverse graph of the nested instances: who uses a definition.
    let mut users: HashMap<ResourceId, Vec<ResourceId>> = HashMap::new();
    for r in res.iter() {
        if let Some(d) = res.symbol(r.id) {
            for inner in part_refs(&d.parts) {
                users.entry(inner).or_default().push(r.id);
            }
        }
    }
    // The definition and all that use it.
    let mut affected: Vec<ResourceId> = vec![changed];
    let mut seen: HashSet<ResourceId> = HashSet::from([changed]);
    let mut i = 0;
    while i < affected.len() {
        for u in users.get(&affected[i]).into_iter().flatten() {
            if seen.insert(*u) {
                affected.push(*u);
            }
        }
        i += 1;
    }
    let mut cache = ExpansionCache::new(res);
    for id in affected {
        if res.symbol(id).is_some_and(|d| d.is_component()) {
            cache.get(id)?;
        }
    }
    Ok(())
}

/// How deep the definition nests other definitions: 1 for a definition with no instance inside.
/// A cycle or a missing definition adds nothing. The depth of each definition is found once, so
/// the work is the size of the graph and not the number of paths.
pub fn definition_depth(res: &Resources, symbol: ResourceId) -> usize {
    max_definition_depth(res, [symbol])
}

/// The deepest of [`definition_depth`] over several definitions, with the depths shared.
pub fn max_definition_depth(
    res: &Resources,
    symbols: impl IntoIterator<Item = ResourceId>,
) -> usize {
    fn go(
        res: &Resources,
        symbol: ResourceId,
        open: &mut Vec<ResourceId>,
        memo: &mut HashMap<ResourceId, usize>,
    ) -> usize {
        if let Some(d) = memo.get(&symbol) {
            return *d;
        }
        // A chain that is already too deep is not followed further.
        if open.len() > MAX_COMPONENT_DEPTH {
            return MAX_COMPONENT_DEPTH + 1;
        }
        // A cycle and a missing definition add no level.
        if open.contains(&symbol) {
            return 0;
        }
        let Some(def) = res.symbol(symbol) else {
            return 0;
        };
        open.push(symbol);
        let mut d = 1;
        for inner in part_refs(&def.parts) {
            d = d.max(1 + go(res, inner, open, memo));
        }
        open.pop();
        memo.insert(symbol, d);
        d
    }
    let mut memo = HashMap::new();
    symbols
        .into_iter()
        .map(|s| go(res, s, &mut Vec::new(), &mut memo))
        .max()
        .unwrap_or(0)
}

/// The box (min, max) around the surface models of a definition, in the coordinates of the
/// definition. `None` for an empty or damaged definition.
pub fn definition_bounds(res: &Resources, symbol: ResourceId) -> Option<(DVec3, DVec3)> {
    definition_bounds_in(&mut ExpansionCache::new(res), symbol)
}

/// Like [`definition_bounds`], with the sizes of the definitions kept in `cache`.
pub fn definition_bounds_in(
    cache: &mut ExpansionCache<'_>,
    symbol: ResourceId,
) -> Option<(DVec3, DVec3)> {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for leaf in leaves_in(cache, symbol).ok()? {
        let (a, b) = leaf.model.local_bounds()?;
        for c in box_corners(a, b) {
            let p = leaf.transform.transform_point3(c);
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

fn box_corners(a: DVec3, b: DVec3) -> impl Iterator<Item = DVec3> {
    (0..8).map(move |i| {
        DVec3::new(
            if i & 1 == 0 { a.x } else { b.x },
            if i & 2 == 0 { a.y } else { b.y },
            if i & 4 == 0 { a.z } else { b.z },
        )
    })
}

/// The box (min, max) around an instance in the drawing: the box of the definition, with its
/// eight corners placed by the instance transform. `None` for an empty or damaged definition.
/// (A turned instance gets the box of the turned box: the plan, the SVG and the print all use
/// this, so they agree.)
pub fn instance_bounds(
    res: &Resources,
    symbol: ResourceId,
    xf: &DAffine3,
) -> Option<(DVec3, DVec3)> {
    place_box(definition_bounds(res, symbol)?, xf)
}

fn place_box((a, b): (DVec3, DVec3), xf: &DAffine3) -> Option<(DVec3, DVec3)> {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for c in box_corners(a, b) {
        let p = xf.transform_point3(c);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

/// How many groups the group at `id` has above it.
pub fn group_depth(doc: &Document, id: ObjectId) -> usize {
    doc.ancestors(id).len()
}

/// How many levels of groups are in the tree under `id`, with `id` itself counted as one level
/// when it is a group (0 for an object that is no group).
pub fn group_height(doc: &Document, id: ObjectId) -> usize {
    if !doc.is_group(id) {
        return 0;
    }
    1 + doc
        .children(id)
        .into_iter()
        .map(|c| group_height(doc, c))
        .max()
        .unwrap_or(0)
}

/// The class that the plain models of a level have: the class of the group (or edit group), and
/// at the root the active class.
fn level_class(doc: &Document, container: Option<ObjectId>) -> ClassId {
    container
        .and_then(|c| doc.object(c))
        .map_or(doc.active_class, |o| o.class)
}

/// True if a surface model object has no look of its own: no attributes, and the class of its
/// level (or no class). Only a plain model can be the raw model of a level, so geometry that is
/// drawn or exploded into the level never takes the colour of a styled model.
pub fn is_plain_model(doc: &Document, id: ObjectId, container: Option<ObjectId>) -> bool {
    doc.object(id).is_some_and(|o| {
        matches!(o.shape, Shape::SurfaceModel(_))
            && o.attrs == Attributes::default()
            && (o.class == crate::model::NONE_CLASS || o.class == level_class(doc, container))
    })
}

/// The raw model of a level: the first plain surface model ([`is_plain_model`]) among the
/// children of `container` (a group), or among the objects with no parent on `layer` when
/// `container` is `None`. A level that has only styled models has none.
pub fn raw_model(
    doc: &Document,
    container: Option<ObjectId>,
    layer: crate::model::LayerId,
) -> Option<ObjectId> {
    doc.objects
        .values()
        .find(|o| {
            o.parent == container
                && (container.is_some() || o.layer == layer)
                && is_plain_model(doc, o.id, container)
        })
        .map(|o| o.id)
}

/// Turns objects into the parts of a definition. `roots` (in the order of the drawing) and the
/// groups among them with all their members are converted; the surface models and instances get
/// `to_def * transform`. Only surface models, groups and instances can be parts.
pub fn parts_from_objects(
    doc: &Document,
    roots: &[ObjectId],
    to_def: &DAffine3,
) -> Result<Vec<Part>, ComponentError> {
    let wanted: HashSet<ObjectId> = roots.iter().copied().collect();
    let ordered: Vec<ObjectId> = doc
        .objects
        .keys()
        .copied()
        .filter(|id| wanted.contains(id))
        .collect();
    let mut parts = Vec::new();
    for id in ordered {
        add_part(doc, id, None, to_def, 1, &mut parts)?;
    }
    if let Some(m) = parts_problem(&parts) {
        return Err(ComponentError::Invalid(m));
    }
    Ok(parts)
}

fn add_part(
    doc: &Document,
    id: ObjectId,
    parent: Option<u32>,
    to_def: &DAffine3,
    depth: usize,
    parts: &mut Vec<Part>,
) -> Result<(), ComponentError> {
    let o = doc
        .object(id)
        .ok_or_else(|| ComponentError::Invalid(format!("object {id} does not exist")))?;
    if depth > MAX_GROUP_DEPTH {
        return Err(ComponentError::GroupsTooDeep {
            max: MAX_GROUP_DEPTH,
        });
    }
    if parts.len() >= MAX_PARTS {
        return Err(ComponentError::Invalid(format!(
            "a component has at most {MAX_PARTS} parts"
        )));
    }
    let shape = match &o.shape {
        Shape::SurfaceModel(m) => {
            let mut m = (**m).clone();
            // An identity chain keeps the numbers as they are (no rounding).
            if *to_def != DAffine3::IDENTITY {
                m.transform = *to_def * m.transform;
            }
            Shape::SurfaceModel(Box::new(m))
        }
        Shape::Component { symbol, transform } => Shape::Component {
            symbol: *symbol,
            transform: raw(&(*to_def * affine(transform))),
        },
        Shape::Group { name } => Shape::Group { name: name.clone() },
        other => return Err(ComponentError::NotAllowed(other.kind().to_string())),
    };
    let is_group = matches!(shape, Shape::Group { .. });
    let index = parts.len() as u32;
    parts.push(Part {
        shape,
        attrs: o.attrs.clone(),
        class: (o.class != crate::model::NONE_CLASS).then_some(o.class),
        parent,
    });
    if is_group {
        for c in doc.children(id) {
            add_part(doc, c, Some(index), to_def, depth + 1, parts)?;
        }
    }
    Ok(())
}

/// A part turned into what a new object needs: the shape placed by `xf`, and the index of the
/// part that holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub shape: Shape,
    pub attrs: Attributes,
    pub class: Option<ClassId>,
    pub parent: Option<usize>,
}

/// The parts as shapes in the drawing: surface models and instances placed by `xf * transform`.
/// The result has the order of the parts, so `parent` indexes into it.
pub fn materialize(parts: &[Part], xf: &DAffine3) -> Vec<Made> {
    parts
        .iter()
        .map(|p| Made {
            shape: match &p.shape {
                Shape::SurfaceModel(m) => {
                    let mut m = (**m).clone();
                    if *xf != DAffine3::IDENTITY {
                        m.transform = *xf * m.transform;
                    }
                    Shape::SurfaceModel(Box::new(m))
                }
                Shape::Component { symbol, transform } => Shape::Component {
                    symbol: *symbol,
                    transform: if *xf == DAffine3::IDENTITY {
                        *transform
                    } else {
                        raw(&(*xf * affine(transform)))
                    },
                },
                other => other.clone(),
            },
            attrs: p.attrs.clone(),
            class: p.class,
            parent: p.parent.map(|q| q as usize),
        })
        .collect()
}

/// What to do when a component was changed by somebody else while it was open for edit
/// (`Command::CloseComponent`). Without an answer the close is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Resolve {
    /// Keep my edit as a new component (`Name#1`); the instance uses it. The other
    /// version of the definition stays as it is.
    KeepAsNew,
    /// Throw my edit away; the definition stays as it is. Works with or without a conflict.
    Discard,
    /// Replace the definition with my edit, also if somebody else changed it (or if it is not
    /// known whether they did). The other change is lost: a person says so on purpose.
    Overwrite,
}

/// A version of a definition: 53 bits of a hash of its parts. Two definitions with the same
/// parts have the same version. The edit group of an open component keeps the version that it
/// started from, to find a change by somebody else (`Resolve`).
pub fn definition_version(def: &crate::graphics::SymbolDef) -> u64 {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(serde_json::to_vec(&def.parts).unwrap_or_default());
    u64::from_le_bytes(d[..8].try_into().expect("8 bytes")) & ((1 << 53) - 1)
}

/// A cheap version of the expansion of a definition, nested components included: it changes when
/// a surface model of the definition changes (revision, last edit, sizes, placement, the
/// coordinates) or when the parts change. Scene caches key on it, so an undo, a change by
/// another user and a change of a nested component all show. `None` if the definition cannot be
/// expanded (see [`leaves`]).
pub fn version(res: &Resources, symbol: ResourceId) -> Option<u64> {
    version_in(&mut ExpansionCache::new(res), symbol)
}

/// Like [`version`], with the sizes of the definitions kept in `cache`.
pub fn version_in(cache: &mut ExpansionCache<'_>, symbol: ResourceId) -> Option<u64> {
    // Refuses a missing definition, a cycle and a size over the limits.
    cache.get(symbol).ok()?;
    version_of(cache.res, symbol, &mut HashMap::new())
}

/// The version of one definition. The version of a nested definition is found once and used
/// each time it is nested.
fn version_of(
    res: &Resources,
    symbol: ResourceId,
    memo: &mut HashMap<ResourceId, u64>,
) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    if let Some(v) = memo.get(&symbol) {
        return Some(*v);
    }
    let def = res.symbol(symbol)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let h = &mut hasher;
    for p in &def.parts {
        p.parent.hash(h);
        p.class.hash(h);
        match &p.shape {
            Shape::SurfaceModel(m) => {
                (m.rev(), m.log().last().map(|e| e.stamp), m.next_id()).hash(h);
                (m.vertex_count(), m.edge_count(), m.face_count()).hash(h);
                for x in m.transform.to_cols_array() {
                    x.to_bits().hash(h);
                }
                let mut sum = 0u64;
                for (id, p) in m.vertices() {
                    let bits = p.x.to_bits()
                        ^ p.y.to_bits().rotate_left(21)
                        ^ p.z.to_bits().rotate_left(42);
                    sum = sum.wrapping_add(u64::from(id.0).wrapping_mul(bits | 1));
                }
                sum.hash(h);
            }
            Shape::Component { symbol, transform } => {
                for x in transform {
                    x.to_bits().hash(h);
                }
                symbol.hash(h);
                version_of(res, *symbol, memo)?.hash(h);
            }
            Shape::Group { name } => name.hash(h),
            _ => {}
        }
    }
    let v = hasher.finish();
    memo.insert(symbol, v);
    Some(v)
}

/// The edges in the surface models of the parts of a definition (nested components not
/// counted: they are definitions of their own).
pub fn definition_edges(parts: &[Part]) -> usize {
    parts
        .iter()
        .map(|p| match &p.shape {
            Shape::SurfaceModel(m) => m.edge_count(),
            _ => 0,
        })
        .sum()
}

/// The bytes of the saved text (compact JSON) of the parts of a definition, counted without
/// making the text.
pub fn definition_bytes(parts: &[Part]) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut c = Count(0);
    // A part that cannot be written counts what was written before it.
    let _ = serde_json::to_writer(&mut c, parts);
    c.0
}

/// Why the parts of a definition are too big to keep, or `None`: at most
/// [`MAX_DEFINITION_EDGES`] edges in all its surface models and at most [`MAX_DEFINITION_BYTES`]
/// bytes of saved text. A definition is read by every instance and sent whole to every user of a
/// shared document, so its size is bounded as a whole and not by the count of its parts.
pub fn definition_size_problem(parts: &[Part]) -> Option<String> {
    let edges = definition_edges(parts);
    if edges > MAX_DEFINITION_EDGES {
        return Some(format!(
            "a component can have at most {} edges in all its surface models. This one has {}. Split it into two components or remove detail",
            grouped(MAX_DEFINITION_EDGES),
            grouped(edges)
        ));
    }
    // A definition with few parts and few edges is far below the size in bytes.
    let bytes = definition_bytes(parts);
    if bytes > MAX_DEFINITION_BYTES {
        return Some(format!(
            "a component can have at most {} MiB of saved text. This one has {:.1} MiB. Split it into two components or remove detail",
            MAX_DEFINITION_BYTES / (1024 * 1024),
            bytes as f64 / (1024.0 * 1024.0)
        ));
    }
    None
}

/// A problem of a component instance or of a symbol instance, for the person to see.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub object: ObjectId,
    pub message: String,
}

/// Why the instance cannot be drawn or used, or `None` if it is fine. An instance of a symbol
/// (2D) of a definition that is only a component is a problem too.
pub fn instance_problem(doc: &Document, shape: &Shape) -> Option<String> {
    instance_problem_in(doc, shape, &mut ExpansionCache::new(&doc.resources))
}

/// Like [`instance_problem`], with the sizes of the definitions kept in `cache`.
pub fn instance_problem_in(
    doc: &Document,
    shape: &Shape,
    cache: &mut ExpansionCache<'_>,
) -> Option<String> {
    match shape {
        Shape::Component { symbol, .. } => match doc.resources.get(*symbol) {
            None => Some(ComponentError::Missing(*symbol).to_string()),
            Some(_) => match doc.resources.symbol(*symbol) {
                None => Some(format!("resource {symbol} is not a component")),
                Some(d) if !d.is_component() => Some(format!(
                    "resource {symbol} is not a component (it has no 3D parts)"
                )),
                Some(_) => cache.get(*symbol).err().map(|(_, e)| e.to_string()),
            },
        },
        Shape::Symbol { symbol, .. } => doc
            .resources
            .symbol(*symbol)
            .filter(|d| d.is_component() && d.items.is_empty() && d.solids.is_empty())
            .map(|_| {
                format!(
                    "resource {symbol} is a 3D component. Use a component instance, not a symbol instance"
                )
            }),
        _ => None,
    }
}

/// The instances that cannot be drawn or used, with the reason: an instance that cannot be
/// expanded ([`instance_problem`]), and an instance beyond the total of [`MAX_TOTAL_MODELS`]
/// ([`Budget`]). The 3D scene draws a marked placeholder for each. (The plan draws the box of
/// the definition for an instance beyond the total: it needs no memory per model.)
pub fn findings(doc: &Document) -> Vec<Finding> {
    let mut cache = ExpansionCache::new(&doc.resources);
    let mut out = instance_findings(doc, &mut cache);
    out.extend(budget_findings(doc, &mut cache));
    out
}

/// The part of [`findings`] about instances that cannot be expanded.
pub fn instance_findings(doc: &Document, cache: &mut ExpansionCache<'_>) -> Vec<Finding> {
    doc.objects
        .values()
        .filter_map(|o| {
            instance_problem_in(doc, &o.shape, cache).map(|m| Finding {
                object: o.id,
                message: format!("object {}: {m}", o.id),
            })
        })
        .collect()
}

/// The part of [`findings`] about instances beyond the total of models.
pub fn budget_findings(doc: &Document, cache: &mut ExpansionCache<'_>) -> Vec<Finding> {
    Budget::of(doc, cache)
        .over
        .into_iter()
        .map(|id| Finding {
            object: id,
            message: format!(
                "object {id}: the document has more than {} models in all its instances. This instance is drawn as a placeholder in the 3D view",
                grouped(MAX_TOTAL_MODELS)
            ),
        })
        .collect()
}

/// The models that an object adds to the total of the document ([`Budget`]): the models of the
/// expansion of its definition, if it is a component instance that is not open for edit and can
/// be expanded. `None` for every other object.
pub fn instance_models(
    obj: &crate::model::Object,
    cache: &mut ExpansionCache<'_>,
) -> Option<usize> {
    let Shape::Component { symbol, .. } = &obj.shape else {
        return None;
    };
    if crate::symbols::is_being_edited(obj) {
        return None;
    }
    cache.get(*symbol).ok().map(|e| e.models)
}

/// The models over all the instances of a document, against [`MAX_TOTAL_MODELS`].
///
/// Each instance counts the models of its whole expansion. An instance that cannot be expanded
/// (a finding of its own) and an instance whose definition is open for edit count nothing. The
/// instances are taken in the order of the document: an instance whose models would take the
/// running total over the limit is `over` (drawn as a placeholder), and the later instances that
/// still fit are drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    /// The models of all counted instances together (may be over the limit).
    pub total: usize,
    /// The instances beyond the limit, in the order of the document.
    pub over: Vec<ObjectId>,
}

impl Budget {
    pub fn of(doc: &Document, cache: &mut ExpansionCache<'_>) -> Budget {
        let mut b = Budget::default();
        let mut drawn = 0usize;
        for o in doc.objects.values() {
            let Some(models) = instance_models(o, cache) else {
                continue;
            };
            b.total = b.total.saturating_add(models);
            if drawn.saturating_add(models) > MAX_TOTAL_MODELS {
                b.over.push(o.id);
            } else {
                drawn += models;
            }
        }
        b
    }

    /// The error for a document over the limit, or `None`. The text names the component that
    /// adds the most models, so the person knows what to remove.
    pub fn error(&self, doc: &Document, cache: &mut ExpansionCache<'_>) -> Option<ComponentError> {
        (self.total > MAX_TOTAL_MODELS).then(|| ComponentError::TotalModels {
            total: self.total,
            max: MAX_TOTAL_MODELS,
            hint: biggest_user(doc, cache),
        })
    }
}

/// "The most models come from component X (N instances of M models). Delete some instances,
/// explode them or take parts out of the component."
fn biggest_user(doc: &Document, cache: &mut ExpansionCache<'_>) -> String {
    let mut by: HashMap<ResourceId, (usize, usize)> = HashMap::new();
    for o in doc.objects.values() {
        if let Shape::Component { symbol, .. } = &o.shape
            && !crate::symbols::is_being_edited(o)
            && let Ok(size) = cache.get(*symbol)
            && size.models > 0
        {
            by.entry(*symbol).or_insert((0, size.models)).0 += 1;
        }
    }
    let Some((id, (n, models))) = by
        .into_iter()
        .max_by_key(|(id, (n, m))| (n.saturating_mul(*m), std::cmp::Reverse(id.0)))
    else {
        return "Remove instances of components.".into();
    };
    let name = doc.resources.get(id).map_or("", |r| r.name.as_str());
    format!(
        "Most of them come from component '{name}': {} instances with {} models each. Delete some instances or take parts out of the component",
        grouped(n),
        grouped(models)
    )
}

/// Refuses a document with more than [`MAX_TOTAL_MODELS`] models over all its instances. The
/// text names the component that adds the most.
pub fn check_total_models(doc: &Document) -> Result<(), ComponentError> {
    let mut cache = ExpansionCache::new(&doc.resources);
    let budget = Budget::of(doc, &mut cache);
    match budget.error(doc, &mut cache) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// The models over all the instances of the document ([`Budget::total`]).
pub fn total_models(doc: &Document) -> usize {
    Budget::of(doc, &mut ExpansionCache::new(&doc.resources)).total
}

/// The definitions that cannot be expanded (a cycle, nesting or size over the limits, a missing
/// nested definition), a definition that is too big to keep ([`definition_size_problem`]) and
/// the instances beyond the total of models, as notes for the person. A file with such a
/// definition still loads and nothing is cut off; its instances are drawn as marked
/// placeholders ([`plan_mark`]).
pub fn definition_findings(doc: &Document) -> Vec<String> {
    let mut cache = ExpansionCache::new(&doc.resources);
    let mut notes = Vec::new();
    for r in doc.resources.iter() {
        let Some(d) = doc.resources.symbol(r.id).filter(|d| d.is_component()) else {
            continue;
        };
        if let Err((_, e)) = cache.get(r.id) {
            notes.push(format!("component '{}' ({}): {e}", r.name, r.id));
        }
        if let Some(why) = definition_size_problem(&d.parts) {
            notes.push(format!("component '{}' ({}): {why}", r.name, r.id));
        }
    }
    let budget = Budget::of(doc, &mut cache);
    if !budget.over.is_empty() {
        notes.push(format!(
            "the document has {} models in all its instances, the limit is {}. {} instances are drawn as placeholders in the 3D view (the first is object {}). {}",
            grouped(budget.total),
            grouped(MAX_TOTAL_MODELS),
            grouped(budget.over.len()),
            budget.over[0],
            biggest_user(doc, &mut cache)
        ));
    }
    notes
}

/// The notes of a loaded document about components: [`definition_findings`] and the instances
/// that cannot be expanded. (The instances beyond the total of models are one note, in
/// [`definition_findings`].)
pub fn load_findings(doc: &Document) -> Vec<String> {
    let mut cache = ExpansionCache::new(&doc.resources);
    let mut notes = definition_findings(doc);
    notes.extend(
        instance_findings(doc, &mut cache)
            .into_iter()
            .map(|f| f.message),
    );
    notes
}

/// The colour (RGBA) of the mark of an instance that cannot be drawn: in the plan, the 3D scene
/// and the exports.
pub const WARNING_COLOR: [u8; 4] = [230, 126, 34, 255];

/// How an instance shows in the plan.
#[derive(Clone, Debug, PartialEq)]
pub enum PlanMark {
    /// The box around the instance: the corners, counterclockwise, in the drawing.
    Box([glam::DVec2; 4]),
    /// An instance that cannot be drawn: the mark (a cross) at its origin, and the reason.
    Broken(glam::DVec2, String),
    /// A definition with no geometry: nothing to draw.
    Empty,
}

/// What the plan, the SVG and the print draw for an instance (the same everywhere, AGR-01).
///
/// The work that depends only on the definition (the expansion, its problem, its box) is kept
/// per definition and per content version ([`version`]), so many instances and many frames do
/// not walk the models again. Inside a [`plan_frame`] the version of a definition is read once;
/// outside, on every call.
pub fn plan_mark(doc: &Document, shape: &Shape) -> Option<PlanMark> {
    let Shape::Component { symbol, transform } = shape else {
        return None;
    };
    let broken = |why: String| {
        Some(PlanMark::Broken(
            glam::DVec2::new(transform[9], transform[10]),
            why,
        ))
    };
    // The kind of the definition is cheap to read; the expansion is the cached part.
    match doc.resources.get(*symbol) {
        None => return broken(ComponentError::Missing(*symbol).to_string()),
        Some(_) => match doc.resources.symbol(*symbol) {
            None => return broken(format!("resource {symbol} is not a component")),
            Some(d) if !d.is_component() => {
                return broken(format!(
                    "resource {symbol} is not a component (it has no 3D parts)"
                ));
            }
            Some(_) => {}
        },
    }
    let info = PLAN.with(|p| p.borrow_mut().info(&doc.resources, *symbol));
    if let Some(why) = &info.problem {
        return broken(why.clone());
    }
    Some(
        match info.bounds.and_then(|b| place_box(b, &affine(transform))) {
            Some((lo, hi)) => PlanMark::Box([
                glam::DVec2::new(lo.x, lo.y),
                glam::DVec2::new(hi.x, lo.y),
                glam::DVec2::new(hi.x, hi.y),
                glam::DVec2::new(lo.x, hi.y),
            ]),
            None => PlanMark::Empty,
        },
    )
}

/// What is kept for a definition: why it cannot be expanded, and its box.
struct DefInfo {
    problem: Option<String>,
    bounds: Option<(DVec3, DVec3)>,
}

struct Entry {
    version: Option<u64>,
    info: std::rc::Rc<DefInfo>,
    /// The frame in which the version was read.
    frame: u64,
}

#[derive(Default)]
struct PlanState {
    /// Nesting of `plan_frame` guards; 0 outside any frame.
    open: usize,
    frame: u64,
    map: std::collections::HashMap<ResourceId, Entry>,
}

impl PlanState {
    fn info(&mut self, res: &Resources, symbol: ResourceId) -> std::rc::Rc<DefInfo> {
        if self.open > 0
            && let Some(e) = self.map.get(&symbol)
            && e.frame == self.frame
        {
            return e.info.clone();
        }
        let version = version_in(&mut ExpansionCache::new(res), symbol);
        if let Some(e) = self.map.get_mut(&symbol)
            && e.version == version
            && version.is_some()
        {
            e.frame = self.frame;
            return e.info.clone();
        }
        let mut cache = ExpansionCache::new(res);
        let problem = cache.get(symbol).err().map(|(_, e)| e.to_string());
        let info = std::rc::Rc::new(DefInfo {
            bounds: if problem.is_some() {
                None
            } else {
                definition_bounds_in(&mut cache, symbol)
            },
            problem,
        });
        self.map.insert(
            symbol,
            Entry {
                version,
                info: info.clone(),
                frame: self.frame,
            },
        );
        info
    }
}

thread_local! {
    static PLAN: std::cell::RefCell<PlanState> = std::cell::RefCell::new(PlanState::default());
}

/// While this guard lives, [`plan_mark`] reads the version of each definition once. Take one for
/// the drawing of a plan or an export, where the document does not change. Outside a frame every
/// call reads the version.
pub struct PlanFrame(());

pub fn plan_frame() -> PlanFrame {
    PLAN.with(|p| {
        let mut p = p.borrow_mut();
        if p.open == 0 {
            p.frame += 1;
            // Do not keep the definitions of a document that is closed.
            if p.map.len() > 4096 {
                p.map.clear();
            }
        }
        p.open += 1;
    });
    PlanFrame(())
}

impl Drop for PlanFrame {
    fn drop(&mut self) {
        PLAN.with(|p| p.borrow_mut().open -= 1);
    }
}

/// What Object Info shows for an instance.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceInfo {
    /// The name of the definition (empty if it is missing).
    pub name: String,
    pub position: DVec3,
    /// The turn about the z axis, in degrees.
    pub rotation_deg: f64,
    /// The length of each axis of the instance.
    pub scale: DVec3,
}

/// The name, position, rotation and scale of an instance.
pub fn instance_info(doc: &Document, shape: &Shape) -> Option<InstanceInfo> {
    let (symbol, a) = placement(shape)?;
    Some(InstanceInfo {
        name: doc
            .resources
            .get(symbol)
            .map(|r| r.name.clone())
            .unwrap_or_default(),
        position: a.translation,
        rotation_deg: a.matrix3.x_axis.y.atan2(a.matrix3.x_axis.x).to_degrees(),
        scale: DVec3::new(
            a.matrix3.x_axis.length(),
            a.matrix3.y_axis.length(),
            a.matrix3.z_axis.length(),
        ),
    })
}

/// The box (min, max) around an object in the drawing: its surface models and instances, groups
/// followed to their members. `None` if there is no geometry.
pub fn object_bounds3(doc: &Document, id: ObjectId) -> Option<(DVec3, DVec3)> {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for leaf in doc.leaves(&[id]) {
        let b = match &doc.object(leaf)?.shape {
            Shape::SurfaceModel(m) => m.bounds(),
            Shape::Component { symbol, transform } => {
                instance_bounds(&doc.resources, *symbol, &affine(transform))
            }
            _ => None,
        };
        if let Some((a, b)) = b {
            lo = lo.min(a);
            hi = hi.max(b);
        }
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

#[cfg(test)]
mod tests {
    //! The memoized walks are checked against plain recursive ones (every path walked, as the
    //! code did before the memo) on random small graphs of definitions: with cycles, missing
    //! definitions, groups and models.
    use super::*;
    use crate::graphics::{GraphicResource, SymbolDef};
    use crate::resource::{Resource, ResourceBody};

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    fn model_part() -> Part {
        Part::new(Shape::SurfaceModel(Box::new(SurfaceModel::new())))
    }

    fn instance(symbol: ResourceId, k: usize) -> Part {
        Part::new(Shape::Component {
            symbol,
            transform: raw(&DAffine3::from_translation(DVec3::new(k as f64, 0.0, 0.0))),
        })
    }

    /// `n` definitions. A part is a model, a group or an instance. An instance names a definition
    /// below this one (a graph without a cycle) or, with `wild`, now and then any definition (a
    /// cycle) or one that does not exist. `fan` is the most parts of a definition.
    fn random_graph(
        rng: &mut Rng,
        n: usize,
        fan: usize,
        wild: bool,
    ) -> (Resources, Vec<ResourceId>) {
        let mut doc = Document::with_sequential_ids();
        let ids: Vec<ResourceId> = (0..n + 1).map(|_| ResourceId(doc.alloc_id())).collect();
        // ids[n] is never pushed: a missing definition.
        let mut res = Resources::default();
        for i in 0..n {
            let mut parts = Vec::new();
            for k in 0..rng.below(fan + 1) {
                parts.push(match rng.below(5) {
                    0 => model_part(),
                    1 => Part::new(Shape::Group { name: "g".into() }),
                    _ if i == 0 && !wild => model_part(),
                    _ => {
                        let target = if wild && rng.below(8) == 0 {
                            rng.below(n + 1)
                        } else {
                            rng.below(i.max(1))
                        };
                        instance(ids[target], k)
                    }
                });
            }
            res.push(Resource {
                id: ids[i],
                name: format!("D{i}"),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                    parts,
                    ..SymbolDef::default()
                }))),
            });
        }
        (res, ids)
    }

    #[derive(Debug, PartialEq)]
    enum Why {
        Missing,
        Cycle,
    }

    type Walked = (usize, usize, usize, f64);

    /// The plain walk: every path, nothing kept. (models, nodes, depth, sum of the x of the
    /// models) or the reason.
    fn plain(
        res: &Resources,
        id: ResourceId,
        stack: &mut Vec<ResourceId>,
        x: f64,
    ) -> Result<Walked, Why> {
        let def = res.symbol(id).ok_or(Why::Missing)?;
        stack.push(id);
        let (mut models, mut nodes, mut depth, mut sum) = (0, def.parts.len(), 1, 0.0);
        for p in &def.parts {
            match &p.shape {
                Shape::SurfaceModel(_) => {
                    models += 1;
                    sum += x;
                }
                Shape::Component { symbol, transform } => {
                    if stack.contains(symbol) {
                        stack.pop();
                        return Err(Why::Cycle);
                    }
                    match plain(res, *symbol, stack, x + transform[9]) {
                        Ok((m, n, d, s)) => {
                            models += m;
                            nodes += n;
                            depth = depth.max(d + 1);
                            sum += s;
                        }
                        Err(e) => {
                            stack.pop();
                            return Err(e);
                        }
                    }
                }
                _ => {}
            }
        }
        stack.pop();
        Ok((models, nodes, depth, sum))
    }

    /// The old `definition_depth`, a walk over every path.
    fn plain_depth(res: &Resources, symbol: ResourceId, seen: &mut Vec<ResourceId>) -> usize {
        if seen.contains(&symbol) || seen.len() > MAX_COMPONENT_DEPTH + 1 {
            return seen.len();
        }
        let Some(def) = res.symbol(symbol) else {
            return seen.len();
        };
        seen.push(symbol);
        let d = part_refs(&def.parts)
            .map(|s| plain_depth(res, s, seen))
            .max()
            .unwrap_or(seen.len());
        seen.pop();
        d.max(seen.len() + 1)
    }

    /// What `symbol_reaches` means, found by trying every path.
    fn plain_reaches(
        res: &Resources,
        from: ResourceId,
        target: ResourceId,
        stack: &mut Vec<ResourceId>,
    ) -> bool {
        if from == target {
            return true;
        }
        if stack.contains(&from) {
            return false;
        }
        stack.push(from);
        let r = res
            .symbol(from)
            .is_some_and(|d| part_refs(&d.parts).any(|s| plain_reaches(res, s, target, stack)));
        stack.pop();
        r
    }

    fn a_limit(e: &ComponentError) -> bool {
        matches!(
            e,
            ComponentError::TooMany { .. }
                | ComponentError::TooBig { .. }
                | ComponentError::ComponentsTooDeep { .. }
        )
    }

    fn check_graph(res: &Resources, ids: &[ResourceId]) {
        for &id in ids {
            let reference = plain(res, id, &mut Vec::new(), 0.0);
            let within =
                |r: &Walked| r.0 <= MAX_LEAVES && r.1 <= MAX_VISITED && r.2 <= MAX_COMPONENT_DEPTH;
            let got = ExpansionCache::new(res).get(id);
            match (&reference, &got) {
                (Ok(r), Ok(g)) => {
                    assert!(within(r), "the memo accepted what is over a limit: {r:?}");
                    assert_eq!((r.0, r.1, r.2), (g.models, g.nodes, g.depth));
                    // The leaves are the same models in the same places.
                    let l = leaves(res, id).expect("within the limits");
                    assert_eq!(l.len(), r.0);
                    let sum: f64 = l.iter().map(|l| l.transform.translation.x).sum();
                    assert!((sum - r.3).abs() < 1e-9, "{sum} != {}", r.3);
                    assert_eq!(definition_depth(res, id), r.2, "depth");
                    assert!(version(res, id).is_some());
                    // The definitions that use this one may be broken: then the one that fails
                    // is named, and it is really broken.
                    if let Err((bad, _)) = check_expansion(res, id) {
                        assert!(ExpansionCache::new(res).get(bad).is_err());
                    }
                }
                (Ok(r), Err((_, e))) => {
                    assert!(!within(r), "the memo refused {e:?} for {r:?}");
                    assert!(a_limit(e));
                }
                (Err(why), Err((_, e))) => {
                    // The same kind of problem (a limit may come first in a graph with both).
                    let same = match why {
                        Why::Missing => matches!(e, ComponentError::Missing(_)),
                        Why::Cycle => matches!(e, ComponentError::Cycle(_)),
                    };
                    assert!(same || a_limit(e), "{why:?} against {e:?}");
                    assert!(leaves(res, id).is_err());
                    assert!(version(res, id).is_none());
                }
                (Err(why), Ok(_)) => panic!("the memo accepted a graph with a problem: {why:?}"),
            }
            // The depth of a graph without a problem, and the limit on a graph with one.
            if reference.is_ok() {
                let old = plain_depth(res, id, &mut Vec::new());
                let new = definition_depth(res, id);
                assert_eq!(old > MAX_COMPONENT_DEPTH, new > MAX_COMPONENT_DEPTH);
                if old <= MAX_COMPONENT_DEPTH {
                    assert_eq!(old, new);
                }
            }
            for &other in ids {
                assert_eq!(
                    plain_reaches(res, id, other, &mut Vec::new()),
                    res.symbol_reaches(id, other),
                    "reaches {id:?} -> {other:?}"
                );
            }
        }
    }

    #[test]
    fn the_memoized_walks_equal_the_plain_walks_on_random_graphs() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for case in 0..600 {
            let n = 1 + rng.below(7);
            let fan = 2 + rng.below(4);
            let (res, ids) = random_graph(&mut rng, n, fan, case % 3 == 0);
            check_graph(&res, &ids);
        }
    }

    #[test]
    fn graphs_near_the_limits_are_met_too() {
        // Wider graphs: some are over 20,000 models or 16 levels, most are not.
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let (mut ok, mut refused) = (0, 0);
        for _ in 0..200 {
            let n = 8 + rng.below(12);
            let (res, ids) = random_graph(&mut rng, n, 5, false);
            check_graph(&res, &ids);
            for &id in &ids {
                if ExpansionCache::new(&res).get(id).is_ok() {
                    ok += 1;
                } else {
                    refused += 1;
                }
            }
        }
        assert!(ok > 100, "ok {ok}, refused {refused}");
    }

    #[test]
    fn numbers_are_written_with_commas() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(250_000), "250,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
