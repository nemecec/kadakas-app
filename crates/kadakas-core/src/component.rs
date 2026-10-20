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

use std::collections::HashSet;

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

/// Why a group or component operation was refused. Nothing changed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ComponentError {
    #[error("groups are nested at most {max} deep")]
    GroupsTooDeep { max: usize },
    #[error("components are nested at most {max} deep")]
    ComponentsTooDeep { max: usize },
    #[error("a component expands to at most {max} surface models")]
    TooMany { max: usize },
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

/// The surface models of a definition, nested components included, each with the transform from
/// its own coordinates to the coordinates of `symbol`. The models are those of the definition (a
/// reference), so a definition with many instances is read, not copied. Depth-first in the order
/// of the parts. Refuses a missing definition, a cycle, nesting over [`MAX_COMPONENT_DEPTH`] and
/// more than [`MAX_LEAVES`] models.
pub fn leaves(res: &Resources, symbol: ResourceId) -> Result<Vec<Leaf<'_>>, ComponentError> {
    let mut out = Vec::new();
    let mut stack = vec![symbol];
    walk(res, symbol, DAffine3::IDENTITY, &mut stack, &mut out)?;
    Ok(out)
}

fn walk<'a>(
    res: &'a Resources,
    symbol: ResourceId,
    xf: DAffine3,
    stack: &mut Vec<ResourceId>,
    out: &mut Vec<Leaf<'a>>,
) -> Result<(), ComponentError> {
    let def = res.symbol(symbol).ok_or(ComponentError::Missing(symbol))?;
    for p in &def.parts {
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
                let r = walk(res, *inner, xf * affine(transform), stack, out);
                stack.pop();
                r?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// How deep the definition nests other definitions: 1 for a definition with no instance inside.
/// A cycle or a missing definition counts as the depth reached.
pub fn definition_depth(res: &Resources, symbol: ResourceId) -> usize {
    fn go(res: &Resources, symbol: ResourceId, seen: &mut Vec<ResourceId>) -> usize {
        if seen.contains(&symbol) || seen.len() > MAX_COMPONENT_DEPTH + 1 {
            return seen.len();
        }
        let Some(def) = res.symbol(symbol) else {
            return seen.len();
        };
        seen.push(symbol);
        let d = part_refs(&def.parts)
            .map(|s| go(res, s, seen))
            .max()
            .unwrap_or(seen.len());
        seen.pop();
        d.max(seen.len() + 1)
    }
    go(res, symbol, &mut Vec::new())
}

/// The box (min, max) around an instance in the drawing, or `None` for an empty or damaged
/// definition.
pub fn instance_bounds(
    res: &Resources,
    symbol: ResourceId,
    xf: &DAffine3,
) -> Option<(DVec3, DVec3)> {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for leaf in leaves(res, symbol).ok()? {
        let (a, b) = leaf.model.local_bounds()?;
        for i in 0..8 {
            let c = DVec3::new(
                if i & 1 == 0 { a.x } else { b.x },
                if i & 2 == 0 { a.y } else { b.y },
                if i & 4 == 0 { a.z } else { b.z },
            );
            let p = (*xf * leaf.transform).transform_point3(c);
            lo = lo.min(p);
            hi = hi.max(p);
        }
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

/// The raw model of a level: the first surface model among the children of `container` (a
/// group), or among the objects with no parent on `layer` when `container` is `None`.
pub fn raw_model(
    doc: &Document,
    container: Option<ObjectId>,
    layer: crate::model::LayerId,
) -> Option<ObjectId> {
    doc.objects
        .values()
        .find(|o| {
            o.parent == container
                && matches!(o.shape, Shape::SurfaceModel(_))
                && (container.is_some() || o.layer == layer)
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
