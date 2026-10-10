//! Symbol instances: placement, expansion into plain shapes, bounds, hit testing, and the
//! 3D solids of an instance.
//!
//! A symbol instance (`Shape::Symbol`) keeps only the ID of its definition, a position, a rotation
//! and a scale. Every user of an instance asks for the shapes of the definition at the moment of
//! use, so a change of the definition shows in all instances at once.

use glam::{DAffine2, DMat2, DVec2};

use crate::curve::{self, Vertex, VertexKind};
use crate::geom::{dist_to_segment, point_in_polygon};
use crate::graphics::{Solid3d, SymbolDef};
use crate::model::{Attributes, ClassId, Document, Object, ObjectId, Shape, arc_sweep};
use crate::resource::{ResourceId, Resources};

/// Symbols inside symbols are followed this deep. Deeper ones are left out.
pub const MAX_DEPTH: usize = 8;
/// Most shapes that one instance expands to. More are left out.
pub const MAX_EXPANDED: usize = 100_000;
/// Chord tolerance when curves are cut into lines, in model units.
const FLAT: f64 = 0.05;
/// The key in `Object::data` that marks a symbol instance whose definition is being edited. It is hidden meanwhile.
pub const EDITING_KEY: &str = "symbol_editing";
/// The key in `Object::data` of the group that holds the components of a symbol that is being edited.
pub const EDIT_GROUP_KEY: &str = "edit_symbol";
/// The key in `Object::data` of the objects that draw a mark of a line type. They get no pattern and no line type of their own.
pub const NO_PATTERN_KEY: &str = "no_pattern";
/// Data keys that only the symbol commands write. `SetData` refuses them.
pub const RESERVED_KEYS: [&str; 3] = [EDITING_KEY, EDIT_GROUP_KEY, NO_PATTERN_KEY];

/// A shape of a symbol definition after the transform, with the attributes it brings.
#[derive(Clone, Debug, PartialEq)]
pub struct Expanded {
    pub shape: Shape,
    /// The attributes of the component (over those of the instance).
    pub attrs: Attributes,
    /// The class of the component, if it has one. Otherwise the class of the instance applies.
    pub class: Option<ClassId>,
}

/// The pieces of an instance: the definition, the position, the rotation and the scale in x and y.
pub fn placement(shape: &Shape) -> Option<(ResourceId, DVec2, f64, DVec2)> {
    match shape {
        Shape::Symbol {
            symbol,
            at,
            rotation,
            scale,
            scale_y,
        } => Some((
            *symbol,
            *at,
            *rotation,
            DVec2::new(*scale, scale_y.unwrap_or(*scale)),
        )),
        _ => None,
    }
}

/// The transform from definition coordinates to the drawing: the insertion point goes to `at`, then
/// the scale and the rotation turn the instance about it.
pub fn transform(def: &SymbolDef, at: DVec2, rotation: f64, scale: DVec2) -> DAffine2 {
    DAffine2::from_translation(at)
        * DAffine2::from_angle(rotation)
        * DAffine2::from_scale(scale)
        * DAffine2::from_translation(-def.insertion)
}

/// The transform of an instance object, or `None` if the object is not an instance of a symbol that exists.
pub fn instance_transform(
    res: &Resources,
    shape: &Shape,
    paper_scale: f64,
) -> Option<(ResourceId, DAffine2)> {
    let (id, at, rot, scale) = placement(shape)?;
    let def = res.symbol(id)?;
    // A symbol in paper units is as big as its components say on the sheet: one paper mm is `paper_scale` model mm.
    let k = if def.units == crate::graphics::PatternUnits::Paper
        && paper_scale.is_finite()
        && paper_scale > 0.0
    {
        paper_scale
    } else {
        1.0
    };
    Some((id, transform(def, at, rot, scale * k)))
}

/// Model mm per paper mm for an object: the scale of its layer (1 on a sheet layer).
pub fn layer_paper_scale(doc: &Document, obj: &Object) -> f64 {
    doc.layer(obj.layer).map_or(1.0, |l| {
        if l.kind == crate::model::LayerKind::Sheet {
            1.0
        } else {
            l.scale
        }
    })
}

fn merge(a: &Attributes, b: &Attributes) -> Attributes {
    a.over(b)
}

/// True if the matrix keeps angles: a turn, a mirror and one scale for both axes.
fn is_similarity(m: &DMat2) -> bool {
    let (a, b) = (m.x_axis, m.y_axis);
    let (la, lb) = (a.length(), b.length());
    let big = la.max(lb).max(f64::MIN_POSITIVE);
    (la - lb).abs() <= 1e-9 * big && a.dot(b).abs() <= 1e-9 * big * big
}

fn angle_of(v: DVec2) -> f64 {
    v.y.atan2(v.x)
}

/// A shape of a definition under a transform. Rects, circles and arcs stay what they are when the
/// transform keeps angles (and the rect stays axis-aligned). Otherwise they become polylines.
/// Text keeps its direction: it moves and scales only. A nested symbol is not a plain shape, so it gives `None`.
pub fn transform_shape(shape: &Shape, xf: &DAffine2) -> Option<Shape> {
    let m = xf.matrix2;
    let p = |q: DVec2| xf.transform_point2(q);
    let similar = is_similarity(&m);
    let s = m.x_axis.length();
    let corner_poly = |pts: Vec<DVec2>| Shape::Polyline {
        points: pts.into_iter().map(Vertex::corner).collect(),
        closed: true,
    };
    Some(match shape {
        Shape::Line { a, b } => Shape::Line { a: p(*a), b: p(*b) },
        Shape::Rect { min, max } => {
            let axis_aligned = m.x_axis.y.abs() <= 1e-12 * m.x_axis.length()
                && m.y_axis.x.abs() <= 1e-12 * m.y_axis.length();
            if axis_aligned {
                let (a, b) = (p(*min), p(*max));
                Shape::Rect {
                    min: a.min(b),
                    max: a.max(b),
                }
            } else {
                let (lo, hi) = (min.min(*max), min.max(*max));
                corner_poly(
                    [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
                        .into_iter()
                        .map(p)
                        .collect(),
                )
            }
        }
        Shape::Circle { center, radius } if similar => Shape::Circle {
            center: p(*center),
            radius: radius * s,
        },
        Shape::Circle { center, radius } => corner_poly(
            (0..72)
                .map(|k| {
                    p(*center
                        + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 72.0) * *radius)
                })
                .collect(),
        ),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = arc_sweep(*start, *end);
            if similar {
                let a0 = angle_of(m * DVec2::from_angle(*start));
                let a1 = angle_of(m * DVec2::from_angle(*start + sweep));
                let (ns, ne) = if m.determinant() < 0.0 {
                    (a1, a0)
                } else {
                    (a0, a1)
                };
                let ne = if sweep >= std::f64::consts::TAU {
                    ns + std::f64::consts::TAU
                } else {
                    ne
                };
                Shape::Arc {
                    center: p(*center),
                    radius: radius * s,
                    start: ns,
                    end: ne,
                }
            } else {
                let n = 8 + (sweep / std::f64::consts::TAU * 72.0).ceil() as usize;
                Shape::Polyline {
                    points: (0..=n)
                        .map(|k| {
                            Vertex::corner(p(*center
                                + DVec2::from_angle(*start + sweep * k as f64 / n as f64)
                                    * *radius))
                        })
                        .collect(),
                    closed: false,
                }
            }
        }
        Shape::Polyline { points, closed } => {
            let curved = points.iter().any(|v| {
                matches!(v.kind, VertexKind::Arc { radius } if radius > 0.0)
                    || v.kind == VertexKind::PointOnArc
            });
            if curved && !similar {
                let flat = curve::flatten(points, *closed, FLAT);
                Shape::Polyline {
                    points: flat.into_iter().map(|q| Vertex::corner(p(q))).collect(),
                    closed: *closed,
                }
            } else {
                let points = points
                    .iter()
                    .map(|v| Vertex {
                        p: p(v.p),
                        kind: if let VertexKind::Arc { radius } = v.kind {
                            VertexKind::Arc { radius: radius * s }
                        } else {
                            v.kind
                        },
                    })
                    .collect();
                Shape::Polyline {
                    points,
                    closed: *closed,
                }
            }
        }
        Shape::Text { at, text, height } => Shape::Text {
            at: p(*at),
            text: text.clone(),
            height: height * m.y_axis.length(),
        },
        _ => return None,
    })
}

/// Limits for one expansion: how many components may be visited, and whether some were left out.
struct Walk {
    left: usize,
    truncated: bool,
}

/// The shapes of a symbol definition under a transform, with nested symbols expanded. `class` and
/// `inherit` are what a parent component brings down to its children. Every visited component
/// counts against the budget, so a deep tree of nested instances cannot run away.
#[allow(clippy::too_many_arguments)]
fn expand_into(
    res: &Resources,
    symbol: ResourceId,
    xf: &DAffine2,
    inherit: &Attributes,
    class: Option<ClassId>,
    depth: usize,
    walk: &mut Walk,
    out: &mut Vec<Expanded>,
) {
    let Some(def) = res.symbol(symbol) else {
        return;
    };
    if depth > MAX_DEPTH {
        walk.truncated = true;
        return;
    }
    for it in &def.items {
        if walk.left == 0 {
            walk.truncated = true;
            return;
        }
        walk.left -= 1;
        let attrs = merge(&it.attrs, inherit);
        let class = it.class.or(class);
        match &it.shape {
            Shape::Symbol {
                symbol: nested,
                at,
                rotation,
                scale,
                scale_y,
            } => {
                let Some(inner) = res.symbol(*nested) else {
                    continue;
                };
                let local = transform(
                    inner,
                    *at,
                    *rotation,
                    DVec2::new(*scale, scale_y.unwrap_or(*scale)),
                );
                expand_into(
                    res,
                    *nested,
                    &(*xf * local),
                    &attrs,
                    class,
                    depth + 1,
                    walk,
                    out,
                );
            }
            shape => {
                if let Some(shape) = transform_shape(shape, xf) {
                    out.push(Expanded {
                        shape,
                        attrs,
                        class,
                    });
                }
            }
        }
    }
}

/// Like `expand`, and true if some components were left out because the symbol is too big or too deep.
pub fn expand_report(res: &Resources, symbol: ResourceId, xf: &DAffine2) -> (Vec<Expanded>, bool) {
    let mut out = Vec::new();
    let mut walk = Walk {
        left: MAX_EXPANDED,
        truncated: false,
    };
    expand_into(
        res,
        symbol,
        xf,
        &Attributes::default(),
        None,
        0,
        &mut walk,
        &mut out,
    );
    (out, walk.truncated)
}

/// The plain shapes of a symbol under a transform.
pub fn expand(res: &Resources, symbol: ResourceId, xf: &DAffine2) -> Vec<Expanded> {
    expand_report(res, symbol, xf).0
}

/// The plain shapes of an instance as objects, in the drawing. They have the layer and the class of the
/// instance (a component with its own class keeps it), and the object ID of the instance. Draw, print,
/// export and pick them like any other object. An object that is not an instance, or whose symbol is
/// missing, gives nothing.
pub fn instance_objects(doc: &Document, obj: &Object) -> Vec<Object> {
    instance_objects_at(doc, obj, layer_paper_scale(doc, obj))
}

/// Like `instance_objects`, with the scale of the drawing that shows it (model mm per paper mm):
/// a symbol in paper units follows it. A viewport at 1:50 passes 50.
pub fn instance_objects_at(doc: &Document, obj: &Object, paper_scale: f64) -> Vec<Object> {
    let Some((symbol, xf)) = instance_transform(&doc.resources, &obj.shape, paper_scale) else {
        return Vec::new();
    };
    let plain = obj.data.contains_key(NO_PATTERN_KEY);
    expand(&doc.resources, symbol, &xf)
        .into_iter()
        .map(|e| {
            let mut attrs = merge(&e.attrs, &obj.attrs);
            let mut data = std::collections::BTreeMap::new();
            if plain {
                attrs.fill_pattern = None;
                attrs.line_type = None;
                data.insert(NO_PATTERN_KEY.to_string(), true.into());
            }
            Object {
                extra: Default::default(),
                id: obj.id,
                layer: obj.layer,
                class: e.class.unwrap_or(obj.class),
                shape: e.shape,
                attrs,
                data,
                parent: obj.parent,
            }
        })
        .collect()
}

/// The object that draws one mark of a line type: an instance of the symbol that never gets a pattern
/// or a line type, so marks cannot make marks.
pub fn mark_object(
    owner: &Object,
    pen: Option<crate::model::Color>,
    line_weight: Option<f64>,
    symbol: ResourceId,
    at: DVec2,
    angle: f64,
    scale: f64,
) -> Object {
    let shape = Shape::Symbol {
        symbol,
        at,
        rotation: angle,
        scale,
        scale_y: None,
    };
    let mut data = std::collections::BTreeMap::new();
    data.insert(NO_PATTERN_KEY.to_string(), true.into());
    Object {
        extra: Default::default(),
        id: owner.id,
        layer: owner.layer,
        class: owner.class,
        shape,
        attrs: Attributes {
            pen,
            line_weight,
            ..Attributes::default()
        },
        data,
        parent: None,
    }
}

/// The 3D components of an instance as rings in the drawing, with the bottom, the height and the color.
pub fn instance_solids(
    doc: &Document,
    obj: &Object,
) -> Vec<(Vec<DVec2>, f64, f64, Option<crate::model::Color>)> {
    let Some((id, xf)) =
        instance_transform(&doc.resources, &obj.shape, layer_paper_scale(doc, obj))
    else {
        return Vec::new();
    };
    let Some((_, _, _, scale)) = placement(&obj.shape) else {
        return Vec::new();
    };
    let Some(def) = doc.resources.symbol(id) else {
        return Vec::new();
    };
    let z = (scale.x * scale.y).abs().sqrt();
    def.solids
        .iter()
        .map(|s: &Solid3d| {
            let (base, height) = s.span();
            (
                s.outline_points()
                    .into_iter()
                    .map(|q| xf.transform_point2(q))
                    .collect(),
                base * z,
                height * z,
                s.color(),
            )
        })
        .collect()
}

fn union(a: (DVec2, DVec2), b: (DVec2, DVec2)) -> (DVec2, DVec2) {
    (a.0.min(b.0), a.1.max(b.1))
}

fn finite(b: &(DVec2, DVec2)) -> bool {
    b.0.is_finite() && b.1.is_finite()
}

impl Document {
    /// Bounding box of an object as it looks in the drawing: a symbol instance covers its expanded
    /// shapes and 3D outlines, and a group covers its members. Other shapes give `Shape::bounds`.
    /// An empty group gives (INF, -INF).
    pub fn object_bounds(&self, obj: &Object) -> (DVec2, DVec2) {
        self.object_bounds_deep(obj, 0)
    }

    fn object_bounds_deep(&self, obj: &Object, depth: usize) -> (DVec2, DVec2) {
        let empty = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        match &obj.shape {
            Shape::Group { .. } if depth < 32 => self
                .children(obj.id)
                .into_iter()
                .filter_map(|c| self.object(c))
                .map(|c| self.object_bounds_deep(c, depth + 1))
                .fold(empty, union),
            Shape::Group { .. } => empty,
            Shape::Symbol { at, .. } => {
                let mut b = instance_objects(self, obj)
                    .iter()
                    .map(|o| o.shape.bounds())
                    .filter(finite)
                    .fold(empty, union);
                for (ring, ..) in instance_solids(self, obj) {
                    b = ring.into_iter().fold(b, |b, q| (b.0.min(q), b.1.max(q)));
                }
                if finite(&b) { b } else { (*at, *at) }
            }
            shape => shape.bounds(),
        }
    }
}

fn edge_dist(points: &[DVec2], closed: bool, p: DVec2) -> f64 {
    let n = points.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs)
        .map(|i| dist_to_segment(p, points[i], points[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

/// True if `p` is within `tol` of a plain shape (a line, rect, circle, arc, polyline or text), or
/// inside it when it is `filled`.
pub fn hit_plain(shape: &Shape, filled: bool, p: DVec2, tol: f64) -> bool {
    match shape {
        Shape::Line { a, b } => dist_to_segment(p, *a, *b) <= tol,
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            edge_dist(&corners, true, p) <= tol
                || (filled && p.cmpge(lo).all() && p.cmple(hi).all())
        }
        Shape::Circle { center, radius } => {
            let d = p.distance(*center);
            (d - radius).abs() <= tol || (filled && d < *radius)
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let v = p - *center;
            (v.length() - radius).abs() <= tol
                && (v.y.atan2(v.x) - start).rem_euclid(std::f64::consts::TAU)
                    <= arc_sweep(*start, *end)
        }
        Shape::Polyline { points, closed } => {
            let flat = curve::flatten(points, *closed, (tol / 10.0).max(1e-3));
            edge_dist(&flat, *closed, p) <= tol || (*closed && filled && point_in_polygon(p, &flat))
        }
        Shape::Text { .. } => {
            let (lo, hi) = shape.bounds();
            p.cmpge(lo - tol).all() && p.cmple(hi + tol).all()
        }
        _ => false,
    }
}

/// True if `p` is within `tol` of an instance: of an expanded outline, or inside a filled one.
/// The instance is picked inside a shape whose fill is not transparent.
pub fn hit_instance(doc: &Document, obj: &Object, p: DVec2, tol: f64) -> bool {
    instance_objects(doc, obj).iter().any(|v| {
        let filled = doc.resolved_attrs(v).fill.is_some_and(|c| c.0[3] > 0);
        hit_plain(&v.shape, filled, p, tol)
    })
}

/// True if the document has an instance of this symbol, or a symbol or line type that uses it.
pub fn is_used_by_objects(doc: &Document, symbol: ResourceId) -> bool {
    doc.objects
        .values()
        .any(|o| matches!(o.shape, Shape::Symbol { symbol: s, .. } if s == symbol))
}

/// Repairs a document after a load: an instance that is marked as being edited, but whose edit group
/// is gone, is shown again.
pub fn repair_edits(doc: &mut Document) {
    use crate::uid::AsUid;
    let editing: std::collections::HashSet<crate::uid::Uid> = doc
        .objects
        .values()
        .filter_map(|g| g.data.get(EDIT_GROUP_KEY)?.get("instance")?.as_uid())
        .collect();
    for o in doc.objects.values_mut() {
        if o.data.contains_key(EDITING_KEY) && !editing.contains(&o.id.0) {
            o.data.remove(EDITING_KEY);
        }
    }
}

/// True if the object is a symbol instance whose definition is being edited. Such an instance is hidden.
pub fn is_being_edited(obj: &Object) -> bool {
    obj.data.contains_key(EDITING_KEY)
}

/// The plain shapes of a symbol as polylines (and whether each is closed), in definition coordinates
/// relative to the insertion point and scaled by `scale`. Text is left out.
pub fn symbol_polylines(
    res: &Resources,
    symbol: ResourceId,
    scale: f64,
) -> Vec<(Vec<DVec2>, bool)> {
    let Some(def) = res.symbol(symbol) else {
        return Vec::new();
    };
    let xf = DAffine2::from_scale(DVec2::splat(scale)) * DAffine2::from_translation(-def.insertion);
    expand(res, symbol, &xf)
        .iter()
        .flat_map(|e| shape_polylines(&e.shape))
        .collect()
}

/// A plain shape as polylines: lines, rect and circle outlines, arcs and flattened polylines.
pub fn shape_polylines(shape: &Shape) -> Vec<(Vec<DVec2>, bool)> {
    match shape {
        Shape::Line { a, b } => vec![(vec![*a, *b], false)],
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            vec![(
                vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)],
                true,
            )]
        }
        Shape::Circle { center, radius } => vec![(
            (0..64)
                .map(|k| {
                    *center + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 64.0) * *radius
                })
                .collect(),
            true,
        )],
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = arc_sweep(*start, *end);
            let n = ((sweep / std::f64::consts::TAU * 64.0).ceil() as usize).max(2);
            vec![(
                (0..=n)
                    .map(|k| {
                        *center + DVec2::from_angle(*start + sweep * k as f64 / n as f64) * *radius
                    })
                    .collect(),
                false,
            )]
        }
        Shape::Polyline { points, closed } => {
            vec![(curve::flatten(points, *closed, FLAT), *closed)]
        }
        Shape::LandscapeArea { boundary, .. }
        | Shape::Hardscape { boundary, .. }
        | Shape::SiteModifier { boundary, .. } => {
            vec![(curve::flatten(boundary, true, FLAT), true)]
        }
        _ => Vec::new(),
    }
}

/// The instance objects that draw a plant with a plan symbol, scaled so the symbol fits the canopy.
pub fn plant_symbol_objects(
    doc: &Document,
    plant: &Object,
    symbol: ResourceId,
    at: DVec2,
    spread: f64,
    rotation: f64,
) -> Vec<Object> {
    let Some(def) = doc.resources.symbol(symbol) else {
        return Vec::new();
    };
    let size = {
        let b = expand(
            &doc.resources,
            symbol,
            &DAffine2::from_translation(-def.insertion),
        )
        .iter()
        .map(|e| e.shape.bounds())
        .filter(finite)
        .fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            union,
        );
        if finite(&b) {
            (b.1 - b.0).max_element()
        } else {
            0.0
        }
    };
    if size.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        || !spread.is_finite()
        || spread <= 0.0
    {
        return Vec::new();
    }
    let k = spread / size;
    let shape = Shape::Symbol {
        symbol,
        at,
        rotation,
        scale: k,
        scale_y: None,
    };
    let inst = Object {
        extra: Default::default(),
        id: plant.id,
        layer: plant.layer,
        class: plant.class,
        shape,
        attrs: plant.attrs.clone(),
        data: Default::default(),
        parent: plant.parent,
    };
    instance_objects(doc, &inst)
}

/// Identifies objects of the instance for tests and messages.
pub fn instance_ids(doc: &Document, symbol: ResourceId) -> Vec<ObjectId> {
    doc.objects
        .values()
        .filter(|o| matches!(o.shape, Shape::Symbol { symbol: s, .. } if s == symbol))
        .map(|o| o.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::graphics::{GraphicResource, SymbolItem};
    use crate::resource::ResourceBody;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-6
    }

    /// A symbol: a 10 x 20 rect with the insertion point at its center, and a circle.
    fn doc_with_symbol() -> (Editor, ResourceId) {
        let mut ed = Editor::default();
        let def = SymbolDef {
            insertion: v(5.0, 10.0),
            items: vec![
                SymbolItem {
                    shape: Shape::Rect {
                        min: v(0.0, 0.0),
                        max: v(10.0, 20.0),
                    },
                    attrs: Attributes::default(),
                    class: None,
                },
                SymbolItem {
                    shape: Shape::Circle {
                        center: v(5.0, 10.0),
                        radius: 3.0,
                    },
                    attrs: Attributes::default(),
                    class: None,
                },
            ],
            ..Default::default()
        };
        let out = ed
            .execute(&Command::AddResource {
                name: "Bench".into(),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
            })
            .unwrap();
        (ed, ResourceId(out.created[0]))
    }

    fn place(
        ed: &mut Editor,
        symbol: ResourceId,
        at: DVec2,
        rotation: f64,
        scale: f64,
        scale_y: Option<f64>,
    ) -> ObjectId {
        let shape = Shape::Symbol {
            symbol,
            at,
            rotation,
            scale,
            scale_y,
        };
        ObjectId(
            ed.execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    #[test]
    fn the_insertion_point_goes_to_the_position() {
        let (mut ed, sym) = doc_with_symbol();
        let id = place(&mut ed, sym, v(100.0, 50.0), 0.0, 1.0, None);
        let o = ed.doc().object(id).unwrap();
        let (lo, hi) = ed.doc().object_bounds(o);
        assert!(
            close(lo, v(95.0, 40.0)) && close(hi, v(105.0, 60.0)),
            "{lo} {hi}"
        );
        let shapes = instance_objects(ed.doc(), o);
        assert_eq!(shapes.len(), 2);
        assert_eq!(
            shapes[1].shape,
            Shape::Circle {
                center: v(100.0, 50.0),
                radius: 3.0
            }
        );
        assert!(
            shapes
                .iter()
                .all(|s| s.id == id && s.layer == o.layer && s.class == o.class)
        );
    }

    #[test]
    fn rotation_and_scale_about_the_insertion_point() {
        let (mut ed, sym) = doc_with_symbol();
        let id = place(&mut ed, sym, v(0.0, 0.0), FRAC_PI_2, 2.0, None);
        let (lo, hi) = ed.doc().object_bounds(ed.doc().object(id).unwrap());
        // 10 x 20 doubled is 20 x 40, turned a quarter: 40 x 20 about the origin.
        assert!(
            close(lo, v(-20.0, -10.0)) && close(hi, v(20.0, 10.0)),
            "{lo} {hi}"
        );
        let shapes = instance_objects(ed.doc(), ed.doc().object(id).unwrap());
        // A turned rect is a closed polyline.
        assert!(
            matches!(&shapes[0].shape, Shape::Polyline { closed: true, points } if points.len() == 4)
        );
        assert!(
            matches!(shapes[1].shape, Shape::Circle { radius, .. } if (radius - 6.0).abs() < 1e-9)
        );
    }

    #[test]
    fn asymmetric_scale_makes_polylines_of_circles() {
        let (mut ed, sym) = doc_with_symbol();
        let id = place(&mut ed, sym, v(0.0, 0.0), 0.0, 2.0, Some(1.0));
        let o = ed.doc().object(id).unwrap();
        let (lo, hi) = ed.doc().object_bounds(o);
        assert!(close(lo, v(-10.0, -10.0)) && close(hi, v(10.0, 10.0)));
        let shapes = instance_objects(ed.doc(), o);
        assert!(
            matches!(shapes[0].shape, Shape::Rect { .. }),
            "an axis-aligned rect stays a rect"
        );
        let Shape::Polyline {
            points,
            closed: true,
        } = &shapes[1].shape
        else {
            panic!("{:?}", shapes[1].shape)
        };
        let (cx, cy) = (
            points.iter().map(|q| q.p.x).fold(0.0, f64::max),
            points.iter().map(|q| q.p.y).fold(0.0, f64::max),
        );
        assert!(
            (cx - 6.0).abs() < 1e-9 && (cy - 3.0).abs() < 0.01,
            "an ellipse 12 x 6: {cx} {cy}"
        );
    }

    #[test]
    fn a_negative_scale_mirrors_arcs() {
        let arc = Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start: 0.0,
            end: FRAC_PI_2,
        };
        let mirror = DAffine2::from_scale(v(-1.0, 1.0));
        let Some(Shape::Arc {
            center,
            radius,
            start,
            end,
        }) = transform_shape(&arc, &mirror)
        else {
            panic!()
        };
        assert!(close(center, DVec2::ZERO) && (radius - 10.0).abs() < 1e-9);
        // The quarter in the first quadrant becomes the quarter in the second.
        assert!(
            (start - FRAC_PI_2).abs() < 1e-9 && (end - PI).abs() < 1e-9,
            "{start} {end}"
        );
        assert!((arc_sweep(start, end) - FRAC_PI_2).abs() < 1e-9);
        let full = Shape::Arc {
            center: DVec2::ZERO,
            radius: 1.0,
            start: 0.3,
            end: 0.3 + std::f64::consts::TAU,
        };
        let Some(Shape::Arc { start, end, .. }) =
            transform_shape(&full, &DAffine2::from_angle(1.0))
        else {
            panic!()
        };
        assert!((end - start - std::f64::consts::TAU).abs() < 1e-9);
        // Under an asymmetric scale an arc is a polyline.
        assert!(matches!(
            transform_shape(&arc, &DAffine2::from_scale(v(2.0, 1.0))),
            Some(Shape::Polyline { closed: false, .. })
        ));
    }

    #[test]
    fn editing_the_definition_changes_all_instances() {
        let (mut ed, sym) = doc_with_symbol();
        let a = place(&mut ed, sym, v(0.0, 0.0), 0.0, 1.0, None);
        let b = place(&mut ed, sym, v(1000.0, 0.0), PI, 1.0, None);
        let size = |ed: &Editor, id| {
            let (lo, hi) = ed.doc().object_bounds(ed.doc().object(id).unwrap());
            hi - lo
        };
        assert!(close(size(&ed, a), v(10.0, 20.0)) && close(size(&ed, b), v(10.0, 20.0)));
        let def = SymbolDef {
            insertion: v(5.0, 10.0),
            items: vec![SymbolItem {
                shape: Shape::Rect {
                    min: v(0.0, 0.0),
                    max: v(40.0, 20.0),
                },
                attrs: Attributes::default(),
                class: None,
            }],
            ..Default::default()
        };
        ed.execute(&Command::ModifyResource {
            id: sym,
            name: None,
            body: Some(ResourceBody::Graphic(Box::new(GraphicResource::Symbol(
                def,
            )))),
        })
        .unwrap();
        assert!(close(size(&ed, a), v(40.0, 20.0)) && close(size(&ed, b), v(40.0, 20.0)));
        assert!(ed.undo());
        assert!(close(size(&ed, a), v(10.0, 20.0)));
    }

    #[test]
    fn nested_symbols_compose_their_transforms() {
        let (mut ed, inner) = doc_with_symbol();
        let nested = Shape::Symbol {
            symbol: inner,
            at: v(100.0, 0.0),
            rotation: 0.0,
            scale: 1.0,
            scale_y: None,
        };
        let def = SymbolDef {
            items: vec![SymbolItem {
                shape: nested,
                attrs: Attributes {
                    pen: Some(crate::model::Color([9, 9, 9, 255])),
                    ..Default::default()
                },
                class: None,
            }],
            ..Default::default()
        };
        let outer = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Pair".into(),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
            })
            .unwrap()
            .created[0],
        );
        let id = place(&mut ed, outer, v(0.0, 0.0), 0.0, 2.0, None);
        let o = ed.doc().object(id).unwrap();
        let shapes = instance_objects(ed.doc(), o);
        assert_eq!(shapes.len(), 2);
        // The inner circle is at 100 * 2 along x, with a radius of 6.
        assert_eq!(
            shapes[1].shape,
            Shape::Circle {
                center: v(200.0, 0.0),
                radius: 6.0
            }
        );
        // The pen of the nested component goes down to its shapes.
        assert_eq!(
            shapes[0].attrs.pen,
            Some(crate::model::Color([9, 9, 9, 255]))
        );
        assert_eq!(instance_ids(ed.doc(), outer), vec![id]);
        assert!(is_used_by_objects(ed.doc(), outer));
        assert!(!is_used_by_objects(ed.doc(), ResourceId::from_num(9999)));
    }

    #[test]
    fn hit_testing_follows_the_transform() {
        let (mut ed, sym) = doc_with_symbol();
        let id = place(&mut ed, sym, v(0.0, 0.0), FRAC_PI_2, 1.0, None);
        let o = ed.doc().object(id).unwrap();
        // The rect is 20 wide and 10 high after the turn: its right edge is at x = 10.
        assert!(hit_instance(ed.doc(), o, v(10.0, 0.0), 0.5));
        assert!(
            !hit_instance(ed.doc(), o, v(0.0, 8.0), 0.5),
            "the old top edge is gone"
        );
        assert!(
            hit_instance(ed.doc(), o, v(0.0, 5.0), 0.5),
            "the new top edge"
        );
        assert!(hit_instance(ed.doc(), o, v(3.0, 0.0), 0.5), "the circle");
        assert!(
            !hit_instance(ed.doc(), o, v(1.0, 1.0), 0.5),
            "the inside of an unfilled shape is not a hit"
        );
        assert!(!hit_instance(ed.doc(), o, v(500.0, 500.0), 0.5));
    }

    #[test]
    fn plain_hits() {
        let line = Shape::Line {
            a: v(0.0, 0.0),
            b: v(10.0, 0.0),
        };
        assert!(
            hit_plain(&line, false, v(5.0, 0.4), 0.5) && !hit_plain(&line, false, v(5.0, 2.0), 0.5)
        );
        let arc = Shape::Arc {
            center: DVec2::ZERO,
            radius: 5.0,
            start: 0.0,
            end: FRAC_PI_2,
        };
        assert!(
            hit_plain(&arc, false, v(3.5, 3.5), 0.2) && !hit_plain(&arc, false, v(-3.5, 3.5), 0.2)
        );
        let circle = Shape::Circle {
            center: DVec2::ZERO,
            radius: 5.0,
        };
        assert!(
            hit_plain(&circle, true, v(1.0, 1.0), 0.1)
                && !hit_plain(&circle, false, v(1.0, 1.0), 0.1)
        );
        let poly = Shape::Polyline {
            points: vec![
                Vertex::corner(v(0.0, 0.0)),
                Vertex::corner(v(10.0, 0.0)),
                Vertex::corner(v(10.0, 10.0)),
            ],
            closed: true,
        };
        assert!(
            hit_plain(&poly, true, v(8.0, 3.0), 0.1) && !hit_plain(&poly, false, v(8.0, 3.0), 0.1)
        );
        let text = Shape::Text {
            at: v(0.0, 0.0),
            text: "ab".into(),
            height: 10.0,
        };
        assert!(hit_plain(&text, false, v(3.0, 5.0), 0.1));
        assert!(!hit_plain(
            &Shape::Group {
                name: String::new()
            },
            false,
            v(0.0, 0.0),
            1.0
        ));
    }

    #[test]
    fn missing_symbols_draw_nothing() {
        let mut ed = Editor::default();
        let o = Object::new(
            ObjectId::from_num(50),
            crate::model::FIRST_LAYER,
            crate::model::NONE_CLASS,
            Shape::Symbol {
                symbol: ResourceId::from_num(77),
                at: v(3.0, 4.0),
                rotation: 0.0,
                scale: 1.0,
                scale_y: None,
            },
        );
        assert!(
            instance_objects(ed.doc(), &o).is_empty() && instance_solids(ed.doc(), &o).is_empty()
        );
        assert_eq!(ed.doc().object_bounds(&o), (v(3.0, 4.0), v(3.0, 4.0)));
        assert!(
            instance_transform(
                &ed.doc().resources,
                &Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::X
                },
                1.0
            )
            .is_none()
        );
        assert!(
            placement(&Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X
            })
            .is_none()
        );
        assert!(!is_being_edited(&o));
        let _ = &mut ed;
    }

    #[test]
    fn solids_move_with_the_instance() {
        let (mut ed, _) = doc_with_symbol();
        let def = SymbolDef {
            insertion: DVec2::ZERO,
            items: vec![],
            solids: vec![Solid3d::Extrusion {
                outline: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)],
                base: 5.0,
                height: 20.0,
                color: None,
            }],
            ..Default::default()
        };
        let sym = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Post".into(),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
            })
            .unwrap()
            .created[0],
        );
        let id = place(&mut ed, sym, v(100.0, 100.0), 0.0, 2.0, None);
        let o = ed.doc().object(id).unwrap();
        let solids = instance_solids(ed.doc(), o);
        assert_eq!(solids.len(), 1);
        let (ring, base, height, _) = &solids[0];
        assert!(
            close(ring[2], v(120.0, 120.0))
                && (*base - 10.0).abs() < 1e-9
                && (*height - 40.0).abs() < 1e-9
        );
        // A 3D only symbol has bounds from its solids.
        let (lo, hi) = ed.doc().object_bounds(o);
        assert!(close(lo, v(100.0, 100.0)) && close(hi, v(120.0, 120.0)));
    }

    #[test]
    fn polylines_of_shapes() {
        let (ed, sym) = doc_with_symbol();
        let lines = symbol_polylines(&ed.doc().resources, sym, 2.0);
        assert_eq!(lines.len(), 2);
        // Relative to the insertion point and scaled by 2: the rect spans -10..10 in x.
        let xs: Vec<f64> = lines[0].0.iter().map(|p| p.x).collect();
        assert!(
            xs.iter().cloned().fold(f64::INFINITY, f64::min) == -10.0
                && xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) == 10.0
        );
        assert!(symbol_polylines(&ed.doc().resources, ResourceId::from_num(5555), 1.0).is_empty());
        assert!(
            shape_polylines(&Shape::Arc {
                center: DVec2::ZERO,
                radius: 1.0,
                start: 0.0,
                end: PI
            })
            .len()
                == 1
        );
        assert!(
            shape_polylines(&Shape::Text {
                at: DVec2::ZERO,
                text: "x".into(),
                height: 1.0
            })
            .is_empty()
        );
    }

    #[test]
    fn a_plant_symbol_fits_the_canopy() {
        let (ed, sym) = doc_with_symbol();
        let plant = Object::new(
            ObjectId::from_num(90),
            crate::model::FIRST_LAYER,
            crate::model::NONE_CLASS,
            Shape::Plant {
                at: v(0.0, 0.0),
                species: "x".into(),
                spread: 40.0,
                height: 1.0,
            },
        );
        let objs = plant_symbol_objects(ed.doc(), &plant, sym, v(100.0, 100.0), 40.0, 0.0);
        assert_eq!(objs.len(), 2);
        // The symbol is 20 high at most: scaled by 2 to 40.
        let b = objs.iter().map(|o| o.shape.bounds()).fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            union,
        );
        assert!(((b.1 - b.0).max_element() - 40.0).abs() < 1e-6);
        assert!(
            plant_symbol_objects(
                ed.doc(),
                &plant,
                ResourceId::from_num(7777),
                DVec2::ZERO,
                40.0,
                0.0
            )
            .is_empty()
        );
        assert!(plant_symbol_objects(ed.doc(), &plant, sym, DVec2::ZERO, 0.0, 0.0).is_empty());
    }

    #[test]
    fn polylines_keep_straight_and_curved_vertices_under_each_transform() {
        let poly = Shape::Polyline {
            points: vec![
                Vertex::corner(v(0.0, 0.0)),
                Vertex::new(v(10.0, 0.0), VertexKind::Arc { radius: 2.0 }),
                Vertex::corner(v(10.0, 10.0)),
            ],
            closed: true,
        };
        // A turn and one scale: the fillet radius scales and stays a fillet.
        let Some(Shape::Polyline {
            points,
            closed: true,
        }) = transform_shape(
            &poly,
            &DAffine2::from_scale_angle_translation(v(3.0, 3.0), 0.3, v(1.0, 1.0)),
        )
        else {
            panic!()
        };
        assert!(
            matches!(points[1].kind, VertexKind::Arc { radius } if (radius - 6.0).abs() < 1e-9)
        );
        // An asymmetric scale flattens the fillet into corners.
        let Some(Shape::Polyline { points, .. }) =
            transform_shape(&poly, &DAffine2::from_scale(v(2.0, 1.0)))
        else {
            panic!()
        };
        assert!(points.len() > 3 && points.iter().all(|p| p.kind == VertexKind::Corner));
        // Straight vertices only move.
        let line = Shape::Polyline {
            points: vec![Vertex::corner(v(0.0, 0.0)), Vertex::corner(v(1.0, 1.0))],
            closed: false,
        };
        let Some(Shape::Polyline { points, .. }) =
            transform_shape(&line, &DAffine2::from_scale(v(2.0, 1.0)))
        else {
            panic!()
        };
        assert_eq!(points[1].p, v(2.0, 1.0));
        // A nested symbol is not a plain shape.
        assert!(
            transform_shape(
                &Shape::Symbol {
                    symbol: ResourceId::from_num(1),
                    at: DVec2::ZERO,
                    rotation: 0.0,
                    scale: 1.0,
                    scale_y: None
                },
                &DAffine2::IDENTITY
            )
            .is_none()
        );
    }

    #[test]
    fn rects_circles_and_bounds_of_groups() {
        let rect = Shape::Rect {
            min: v(0.0, 0.0),
            max: v(10.0, 10.0),
        };
        assert!(
            hit_plain(&rect, false, v(0.2, 5.0), 0.5)
                && !hit_plain(&rect, false, v(5.0, 5.0), 0.5)
                && hit_plain(&rect, true, v(5.0, 5.0), 0.5)
        );
        let mut ed = Editor::default();
        let g = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: v(5.0, 5.0),
                    radius: 1.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let gid = ObjectId(
            ed.execute(&Command::Group {
                ids: vec![g],
                name: None,
            })
            .unwrap()
            .created[0],
        );
        let (lo, hi) = ed.doc().object_bounds(ed.doc().object(gid).unwrap());
        assert!(close(lo, v(4.0, 4.0)) && close(hi, v(6.0, 6.0)));
        // A group with no members has no bounds.
        let empty = Object::new(
            ObjectId::from_num(77),
            crate::model::FIRST_LAYER,
            crate::model::NONE_CLASS,
            Shape::Group {
                name: String::new(),
            },
        );
        assert!(ed.doc().object_bounds(&empty).0.x.is_infinite());
        assert_eq!(
            shape_polylines(&Shape::LandscapeArea {
                boundary: vec![
                    Vertex::corner(v(0.0, 0.0)),
                    Vertex::corner(v(1.0, 0.0)),
                    Vertex::corner(v(1.0, 1.0))
                ],
                name: String::new()
            })
            .len(),
            1
        );
        assert_eq!(
            shape_polylines(&Shape::Polyline {
                points: vec![Vertex::corner(v(0.0, 0.0)), Vertex::corner(v(1.0, 0.0))],
                closed: false
            })
            .len(),
            1
        );
    }

    #[test]
    fn a_loop_in_a_damaged_file_stops_at_the_depth_limit() {
        let mut doc = Document::default();
        let me = ResourceId::from_num(5);
        let def = SymbolDef {
            items: vec![
                SymbolItem {
                    shape: Shape::Circle {
                        center: DVec2::ZERO,
                        radius: 1.0,
                    },
                    attrs: Attributes::default(),
                    class: None,
                },
                SymbolItem {
                    shape: Shape::Symbol {
                        symbol: me,
                        at: v(1.0, 0.0),
                        rotation: 0.0,
                        scale: 1.0,
                        scale_y: None,
                    },
                    attrs: Attributes::default(),
                    class: None,
                },
            ],
            ..Default::default()
        };
        doc.resources.push(crate::resource::Resource {
            id: me,
            name: "Loop".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
        });
        let (out, truncated) = expand_report(&doc.resources, me, &DAffine2::IDENTITY);
        assert_eq!(out.len(), MAX_DEPTH + 1);
        assert!(truncated);
    }

    #[test]
    fn a_deep_tree_of_nested_instances_is_cut_off_with_a_flag() {
        // Four levels of 300 instances each over a leaf with only a 3D component: 300^4 visits without a budget.
        let mut doc = Document::default();
        let leaf = SymbolDef {
            solids: vec![Solid3d::Cylinder {
                center: DVec2::ZERO,
                radius: 1.0,
                base: 0.0,
                height: 1.0,
                color: None,
            }],
            ..Default::default()
        };
        doc.resources.push(crate::resource::Resource {
            id: ResourceId::from_num(10),
            name: "L0".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(leaf))),
        });
        for level in 1..=4u64 {
            let items = (0..300)
                .map(|k| SymbolItem {
                    shape: Shape::Symbol {
                        symbol: ResourceId(crate::uid::Uid::from_num(9 + level)),
                        at: v(k as f64, 0.0),
                        rotation: 0.0,
                        scale: 1.0,
                        scale_y: None,
                    },
                    attrs: Attributes::default(),
                    class: None,
                })
                .collect();
            let def = SymbolDef {
                items,
                ..Default::default()
            };
            doc.resources.push(crate::resource::Resource {
                id: ResourceId(crate::uid::Uid::from_num(10 + level)),
                name: format!("L{level}"),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
            });
        }
        let t = std::time::Instant::now();
        let (out, truncated) = expand_report(
            &doc.resources,
            ResourceId::from_num(14),
            &DAffine2::IDENTITY,
        );
        assert!(truncated && out.is_empty());
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
        let obj = Object::new(
            ObjectId::from_num(50),
            crate::model::FIRST_LAYER,
            crate::model::NONE_CLASS,
            Shape::Symbol {
                symbol: ResourceId::from_num(14),
                at: DVec2::ZERO,
                rotation: 0.0,
                scale: 1.0,
                scale_y: None,
            },
        );
        assert!(instance_objects(&doc, &obj).is_empty());
    }

    #[test]
    fn marks_get_no_pattern_and_symbols_in_paper_units_keep_their_size() {
        let (mut ed, sym) = doc_with_symbol();
        let owner = Object::new(
            ObjectId::from_num(60),
            crate::model::FIRST_LAYER,
            crate::model::NONE_CLASS,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        let m = mark_object(
            &owner,
            Some(crate::model::Color::BLACK),
            None,
            sym,
            v(1.0, 2.0),
            0.5,
            3.0,
        );
        assert!(m.data.contains_key(NO_PATTERN_KEY));
        let mut with_pattern = m.clone();
        with_pattern.attrs.fill_pattern = Some(ResourceId::from_num(1));
        assert!(
            crate::pattern::decorate(ed.doc(), &with_pattern, &with_pattern.attrs, 1.0).is_none()
        );
        // The parts of a mark carry the rule on, and lose a pattern that the definition gives them.
        let parts = instance_objects(ed.doc(), &m);
        assert!(parts.iter().all(|p| p.data.contains_key(NO_PATTERN_KEY)
            && p.attrs.fill_pattern.is_none()
            && p.attrs.line_type.is_none()));
        // A paper symbol: 10 x 20 mm on the sheet, 1000 x 2000 mm at 1:100.
        let mut def = ed.doc().resources.symbol(sym).unwrap().clone();
        def.units = crate::graphics::PatternUnits::Paper;
        ed.execute(&Command::ModifyResource {
            id: sym,
            name: None,
            body: Some(ResourceBody::Graphic(Box::new(GraphicResource::Symbol(
                def,
            )))),
        })
        .unwrap();
        let id = place(&mut ed, sym, v(0.0, 0.0), 0.0, 1.0, None);
        let (lo, hi) = ed.doc().object_bounds(ed.doc().object(id).unwrap());
        assert!(close(hi - lo, v(1000.0, 2000.0)), "{lo} {hi}");
        let o = ed.doc().object(id).unwrap();
        let at_50 = instance_objects_at(ed.doc(), o, 50.0);
        let b = at_50.iter().map(|p| p.shape.bounds()).fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            union,
        );
        assert!(close(b.1 - b.0, v(500.0, 1000.0)));
        assert_eq!(layer_paper_scale(ed.doc(), o), 100.0);
    }
}
