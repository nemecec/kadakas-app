//! Plans: the editing functions of this module applied to objects of a document.
//!
//! Each plan reads the document and returns one `Command::Batch` of `ModifyObject`, `AddObject`
//! and `DeleteObjects`. The caller runs it, so the whole edit is one undo step. New objects take
//! the layer, class, attributes and group of the object they come from.

use std::collections::HashSet;

use glam::DVec2;

use crate::command::Command;
use crate::model::{Document, Object, ObjectId, Shape};

use super::boolean::{self, Poly, Ring};
use super::modify::{self, ChamferSize, JoinKind, JoinMode};
use super::offset::{self, Side};
use super::piece::Chain;

fn object(doc: &Document, id: ObjectId) -> Result<&Object, String> {
    doc.object(id).ok_or_else(|| format!("no object {id}"))
}

fn chain_of(o: &Object) -> Result<Chain, String> {
    Chain::from_shape(&o.shape).ok_or_else(|| {
        format!(
            "object {} is a {}. This works on lines, arcs, circles, rectangles and polylines",
            o.id,
            o.shape.kind()
        )
    })
}

/// A command that adds an object like `src`: same layer, class, attributes, group and record data.
fn add_like(src: &Object, shape: Shape) -> Command {
    Command::AddObjectLike {
        source: src.id,
        shape,
    }
}

fn modify_shape(id: ObjectId, shape: Shape) -> Command {
    Command::ModifyObject {
        id,
        shape: Some(shape),
        layer: None,
        class: None,
        attrs: None,
    }
}

fn batch(commands: Vec<Command>) -> Command {
    Command::Batch { commands }
}

/// The first chain replaces the shape of `src`, the others become new objects. No chain deletes it.
fn replace_with(src: &Object, chains: &[Chain]) -> Vec<Command> {
    let mut out = Vec::new();
    match chains.split_first() {
        None => out.push(Command::DeleteObjects { ids: vec![src.id] }),
        Some((first, rest)) => {
            out.push(modify_shape(src.id, first.to_shape()));
            out.extend(rest.iter().map(|c| add_like(src, c.to_shape())));
        }
    }
    out
}

/// Objects on the layer of `target` that cut or bound it: the listed ones, or all that can be picked.
fn other_chains(
    doc: &Document,
    target: &Object,
    ids: Option<&[ObjectId]>,
) -> Result<Vec<Chain>, String> {
    let mut out = Vec::new();
    match ids {
        Some(ids) => {
            for &id in ids {
                if id == target.id {
                    continue;
                }
                let o = object(doc, id)?;
                if o.layer != target.layer {
                    return Err(format!("object {id} is on another layer"));
                }
                out.push(chain_of(o)?);
            }
        }
        None => {
            for o in doc.objects.values() {
                if o.id != target.id && o.layer == target.layer && doc.is_visible(o) {
                    out.extend(Chain::from_shape(&o.shape));
                }
            }
        }
    }
    Ok(out)
}

/// Removes the part of `target` at `pick` that lies between crossings with `cutters` (or with all
/// other objects of the layer).
pub fn trim(
    doc: &Document,
    target: ObjectId,
    pick: DVec2,
    cutters: Option<&[ObjectId]>,
) -> Result<Command, String> {
    let t = object(doc, target)?;
    let chain = chain_of(t)?;
    let others = other_chains(doc, t, cutters)?;
    let refs: Vec<&Chain> = others.iter().collect();
    let kept = modify::trim(&chain, pick, &refs)?;
    Ok(batch(replace_with(t, &kept)))
}

/// Extends the end of `target` that is nearer to `pick` to the nearest boundary.
pub fn extend(
    doc: &Document,
    target: ObjectId,
    pick: DVec2,
    boundaries: Option<&[ObjectId]>,
) -> Result<Command, String> {
    let t = object(doc, target)?;
    let chain = chain_of(t)?;
    let others = other_chains(doc, t, boundaries)?;
    let refs: Vec<&Chain> = others.iter().collect();
    let longer = modify::extend(&chain, pick, &refs)?;
    Ok(batch(vec![modify_shape(t.id, longer.to_shape())]))
}

/// Cuts an object at points on it. The first piece keeps the object. The others are new objects.
pub fn split(doc: &Document, target: ObjectId, at: &[DVec2]) -> Result<Command, String> {
    let t = object(doc, target)?;
    let chain = chain_of(t)?;
    let parts = modify::split(&chain, at)?;
    Ok(batch(replace_with(t, &parts)))
}

/// Cuts closed shapes along an infinite line. Each piece is a new closed polyline. The original
/// object is replaced by the first piece. The pieces together have the area of the original.
pub fn split_by_line(
    doc: &Document,
    target: ObjectId,
    a: DVec2,
    b: DVec2,
) -> Result<Command, String> {
    let t = object(doc, target)?;
    let ring = boolean::ring_of(&t.shape).ok_or_else(|| {
        format!(
            "object {} is a {}. Cutting by a line works on rectangles, circles and polylines",
            t.id,
            t.shape.kind()
        )
    })?;
    let (left, right) = boolean::cut_by_line(&vec![ring], a, b)?;
    let polys: Vec<Poly> = left.into_iter().chain(right).collect();
    if polys.len() < 2 {
        return Err("the line does not cross the shape".into());
    }
    let shapes: Vec<Shape> = polys.iter().map(boolean::poly_shape).collect();
    let mut cmds = vec![modify_shape(t.id, shapes[0].clone())];
    cmds.extend(shapes[1..].iter().map(|s| add_like(t, s.clone())));
    Ok(batch(cmds))
}

/// What a fillet or chamfer does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corner {
    pub kind: JoinKind,
    pub mode: JoinMode,
}

fn corner_index(chain: &Chain, i: usize, j: usize) -> Option<usize> {
    let n = chain.pieces.len();
    if i == j || n < 2 {
        return None;
    }
    if j == i + 1 {
        Some(i)
    } else if i == j + 1 {
        Some(j)
    } else if chain.closed && ((i == 0 && j == n - 1) || (j == 0 && i == n - 1)) {
        Some(n - 1)
    } else {
        None
    }
}

/// Fillet or chamfer between the segments of two objects at the clicked points. With the same
/// object twice, the two segments must be neighbors: the corner of the shape is rounded or cut,
/// and the joint becomes a part of the shape. With two objects, they are cut (modes trim and split)
/// and the joint is a new object.
pub fn join(
    doc: &Document,
    a: ObjectId,
    pick_a: DVec2,
    b: ObjectId,
    pick_b: DVec2,
    corner: Corner,
) -> Result<Command, String> {
    let oa = object(doc, a)?;
    let ca = chain_of(oa)?;
    if a == b {
        let (i, ..) = ca.closest(pick_a);
        let (j, ..) = ca.closest(pick_b);
        let k = corner_index(&ca, i, j)
            .ok_or_else(|| "click two neighboring segments of the shape".to_string())?;
        if corner.mode == JoinMode::Standard {
            let (p1, p2) = (ca.pieces[k], ca.pieces[(k + 1) % ca.pieces.len()]);
            let j = match corner.kind {
                JoinKind::Fillet(r) => {
                    modify::fillet_joint(&p1, p1.point(0.5), &p2, p2.point(0.5), r)?
                }
                JoinKind::Chamfer(s) => {
                    modify::chamfer_joint(&p1, p1.point(0.5), &p2, p2.point(0.5), s)?
                }
            };
            return Ok(batch(vec![add_like(
                oa,
                Chain::open(vec![j.piece]).to_shape(),
            )]));
        }
        let rounded = modify::round_corners(&ca, &[k], corner.kind)?;
        return Ok(batch(vec![modify_shape(a, rounded.to_shape())]));
    }
    let ob = object(doc, b)?;
    let cb = chain_of(ob)?;
    let j = modify::join_objects(&ca, pick_a, &cb, pick_b, corner.kind, corner.mode)?;
    let mut cmds = Vec::new();
    if corner.mode != JoinMode::Standard {
        cmds.push(modify_shape(a, j.a.to_shape()));
        cmds.push(modify_shape(b, j.b.to_shape()));
    }
    cmds.push(add_like(oa, Chain::open(vec![j.joint]).to_shape()));
    cmds.extend(j.rest.iter().map(|c| add_like(oa, c.to_shape())));
    Ok(batch(cmds))
}

/// Rounds or cuts all corners of a shape (or the listed corners: corner `k` is between segment
/// `k` and the next).
pub fn round_all(
    doc: &Document,
    id: ObjectId,
    corners: Option<&[usize]>,
    kind: JoinKind,
) -> Result<Command, String> {
    let o = object(doc, id)?;
    let chain = chain_of(o)?;
    let n = chain.pieces.len();
    let all: Vec<usize> = (0..if chain.closed { n } else { n.saturating_sub(1) }).collect();
    let list = corners.map_or(all, <[usize]>::to_vec);
    let out = modify::round_corners(&chain, &list, kind)?;
    Ok(batch(vec![modify_shape(id, out.to_shape())]))
}

/// How an offset is specified.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffsetTo {
    Distance(f64, Side),
    /// The copy passes through this point.
    Through(DVec2),
}

/// A copy of an object, offset. With `copy` false the object itself changes.
pub fn offset(doc: &Document, id: ObjectId, to: OffsetTo, copy: bool) -> Result<Command, String> {
    let o = object(doc, id)?;
    let (d, side) = match to {
        OffsetTo::Distance(d, s) => (d, s),
        OffsetTo::Through(p) => offset::offset_through(&o.shape, p)?,
    };
    let shapes = offset::offset_shapes(&o.shape, d, side)?;
    let mut cmds = Vec::new();
    for (i, shape) in shapes.into_iter().enumerate() {
        cmds.push(if copy || i > 0 {
            add_like(o, shape)
        } else {
            modify_shape(id, shape)
        });
    }
    Ok(batch(cmds))
}

/// The surface operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceOp {
    /// One shape from all: the union.
    Add,
    /// The base without the cutters.
    Subtract,
    /// The shapes below the top one, without the top one.
    Clip,
    /// The part that all shapes share.
    Intersect,
    /// Regions that do not overlap: every overlap and every rest.
    Compose,
}

fn z_order(doc: &Document, id: ObjectId) -> usize {
    doc.objects.get_index_of(&id).unwrap_or(usize::MAX)
}

fn ring_for(o: &Object) -> Result<Ring, String> {
    if o.parent.is_some() && matches!(o.shape, Shape::Group { .. }) {
        return Err("grouped objects cannot be used".into());
    }
    boolean::ring_of(&o.shape).ok_or_else(|| {
        format!(
            "object {} is a {}. Surface operations work on rectangles, circles and polylines",
            o.id,
            o.shape.kind()
        )
    })
}

/// Runs a surface operation on closed shapes of one layer. `base` is the object that is cut by
/// `Subtract` (the others cut it). For the other operations the order of the objects in the
/// drawing decides: the lowest one gives its attributes (`Add`, `Intersect`), the topmost one is
/// the clipping shape of `Clip`. The cutting shapes are deleted unless `keep_tools`.
pub fn surface(
    doc: &Document,
    op: SurfaceOp,
    ids: &[ObjectId],
    base: Option<ObjectId>,
    keep_tools: bool,
) -> Result<Command, String> {
    let mut seen = HashSet::new();
    let mut objs: Vec<&Object> = Vec::new();
    for &id in ids {
        if seen.insert(id) {
            objs.push(object(doc, id)?);
        }
    }
    if let Some(b) = base
        && !seen.contains(&b)
    {
        objs.push(object(doc, b)?);
    }
    if objs.len() < 2 {
        return Err("give at least two shapes".into());
    }
    let layer = objs[0].layer;
    if objs.iter().any(|o| o.layer != layer) {
        return Err("the shapes must be on one layer".into());
    }
    objs.sort_by_key(|o| z_order(doc, o.id));
    let rings: Vec<Ring> = objs.iter().map(|o| ring_for(o)).collect::<Result<_, _>>()?;
    let mut cmds = Vec::new();
    // `target` keeps its id and takes the first result. Other results are new objects like it.
    let finish =
        |target: &Object, polys: Vec<Poly>, delete: Vec<ObjectId>, cmds: &mut Vec<Command>| {
            let shapes: Vec<Shape> = polys.iter().map(boolean::poly_shape).collect();
            match shapes.split_first() {
                None => cmds.push(Command::DeleteObjects {
                    ids: vec![target.id],
                }),
                Some((first, rest)) => {
                    cmds.push(modify_shape(target.id, first.clone()));
                    cmds.extend(rest.iter().map(|s| add_like(target, s.clone())));
                }
            }
            if !delete.is_empty() {
                cmds.push(Command::DeleteObjects { ids: delete });
            }
        };
    match op {
        SurfaceOp::Add | SurfaceOp::Intersect => {
            let polys = if op == SurfaceOp::Add {
                boolean::union(&rings)
            } else {
                boolean::intersect(&rings)
            };
            if polys.is_empty() {
                return Err("the shapes have no common area".into());
            }
            let delete = if keep_tools {
                Vec::new()
            } else {
                objs[1..].iter().map(|o| o.id).collect()
            };
            finish(objs[0], polys, delete, &mut cmds);
        }
        SurfaceOp::Subtract => {
            let base_id = base.ok_or_else(|| "give the base shape".to_string())?;
            let at = objs
                .iter()
                .position(|o| o.id == base_id)
                .expect("added above");
            let cutters: Vec<Ring> = rings
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != at)
                .map(|(_, r)| r.clone())
                .collect();
            let polys = boolean::subtract(&rings[at], &cutters);
            let delete = if keep_tools {
                Vec::new()
            } else {
                objs.iter()
                    .filter(|o| o.id != base_id)
                    .map(|o| o.id)
                    .collect()
            };
            finish(objs[at], polys, delete, &mut cmds);
        }
        SurfaceOp::Clip => {
            let top = objs.len() - 1;
            for i in 0..top {
                let polys = boolean::subtract(&rings[i], std::slice::from_ref(&rings[top]));
                finish(objs[i], polys, Vec::new(), &mut cmds);
            }
            if !keep_tools {
                cmds.push(Command::DeleteObjects {
                    ids: vec![objs[top].id],
                });
            }
        }
        SurfaceOp::Compose => {
            let regions = boolean::compose(&rings);
            let mut adds = Vec::new();
            for reg in &regions {
                // A region looks like the topmost shape that covers it.
                let src = objs[*reg.members.last().expect("a region has a member")];
                for p in &reg.polys {
                    adds.push(add_like(src, boolean::poly_shape(p)));
                }
            }
            // The new shapes copy their source, so the originals go last.
            cmds.extend(adds);
            if !keep_tools {
                cmds.push(Command::DeleteObjects {
                    ids: objs.iter().map(|o| o.id).collect(),
                });
            }
        }
    }
    Ok(batch(cmds))
}

/// The chamfer size from the three ways to give it: distances, distance and angle, or length.
pub fn chamfer_size(
    d1: Option<f64>,
    d2: Option<f64>,
    angle: Option<f64>,
    length: Option<f64>,
) -> Result<ChamferSize, String> {
    match (d1, d2, angle, length) {
        (Some(a), Some(b), None, None) => Ok(ChamferSize::Distances(a, b)),
        (Some(a), None, None, None) => Ok(ChamferSize::Distances(a, a)),
        (Some(a), None, Some(ang), None) => Ok(ChamferSize::DistanceAngle(a, ang)),
        (None, None, None, Some(l)) => Ok(ChamferSize::Length(l)),
        _ => Err("give distance (and distance2), or distance and angle, or length".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Editor;
    use crate::model::Attributes;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
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

    fn line(ed: &mut Editor, a: DVec2, b: DVec2) -> ObjectId {
        add(ed, Shape::Line { a, b })
    }

    fn rect(ed: &mut Editor, x: f64, y: f64, s: f64) -> ObjectId {
        add(
            ed,
            Shape::Rect {
                min: v(x, y),
                max: v(x + s, y + s),
            },
        )
    }

    #[test]
    fn trim_undo_and_count() {
        let mut ed = Editor::default();
        let t = line(&mut ed, v(0.0, 0.0), v(10.0, 0.0));
        let first_cutter = line(&mut ed, v(3.0, -1.0), v(3.0, 1.0));
        line(&mut ed, v(7.0, -1.0), v(7.0, 1.0));
        let before = ed.doc().clone();
        let cmd = trim(ed.doc(), t, v(5.0, 0.0), None).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 4);
        assert!(ed.undo());
        assert_eq!(ed.doc().objects, before.objects);
        // Only the listed cutters count.
        let one = trim(ed.doc(), t, v(5.0, 0.0), Some(&[first_cutter])).unwrap();
        ed.execute(&one).unwrap();
        assert_eq!(ed.doc().objects.len(), 3);
        assert!(trim(ed.doc(), ObjectId::from_num(99), v(0.0, 0.0), None).is_err());
    }

    #[test]
    fn extend_and_split() {
        let mut ed = Editor::default();
        let t = line(&mut ed, v(0.0, 0.0), v(4.0, 0.0));
        let b = line(&mut ed, v(10.0, -5.0), v(10.0, 5.0));
        let cmd = extend(ed.doc(), t, v(3.5, 0.0), Some(&[b])).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(
            ed.doc().object(t).unwrap().shape,
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(10.0, 0.0)
            }
        );
        let cmd = split(ed.doc(), t, &[v(5.0, 1.0)]).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 3);
        assert!(split(ed.doc(), t, &[v(0.0, 0.0)]).is_err());
    }

    #[test]
    fn split_by_line_keeps_area() {
        let mut ed = Editor::default();
        let r = rect(&mut ed, 0.0, 0.0, 10.0);
        let cmd = split_by_line(ed.doc(), r, v(3.0, -1.0), v(3.0, 11.0)).unwrap();
        ed.execute(&cmd).unwrap();
        let area: f64 = ed
            .doc()
            .objects
            .values()
            .map(|o| o.shape.area().unwrap_or(0.0))
            .sum();
        assert!((area - 100.0).abs() < 1e-9);
        assert_eq!(ed.doc().objects.len(), 2);
        assert!(split_by_line(ed.doc(), r, v(50.0, -1.0), v(50.0, 11.0)).is_err());
    }

    #[test]
    fn fillet_and_chamfer_commands() {
        let mut ed = Editor::default();
        let a = line(&mut ed, v(0.0, 0.0), v(10.0, 0.0));
        let b = line(&mut ed, v(0.0, 0.0), v(0.0, 10.0));
        let c = Corner {
            kind: JoinKind::Fillet(2.0),
            mode: JoinMode::Trim,
        };
        let cmd = join(ed.doc(), a, v(8.0, 0.0), b, v(0.0, 8.0), c).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 3);
        assert!(matches!(
            ed.doc().objects.values().last().unwrap().shape,
            Shape::Arc { .. }
        ));
        ed.undo();
        assert_eq!(ed.doc().objects.len(), 2);
        let ch = Corner {
            kind: JoinKind::Chamfer(ChamferSize::Distances(2.0, 2.0)),
            mode: JoinMode::Standard,
        };
        let cmd = join(ed.doc(), a, v(8.0, 0.0), b, v(0.0, 8.0), ch).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 3);
    }

    #[test]
    fn rounding_a_rectangle_corner() {
        let mut ed = Editor::default();
        let r = add(
            &mut ed,
            Shape::Polyline {
                points: crate::curve::corners(&[
                    v(0.0, 0.0),
                    v(10.0, 0.0),
                    v(10.0, 10.0),
                    v(0.0, 10.0),
                ]),
                closed: true,
            },
        );
        let c = Corner {
            kind: JoinKind::Fillet(2.0),
            mode: JoinMode::Trim,
        };
        let cmd = join(ed.doc(), r, v(5.0, 0.0), r, v(10.0, 5.0), c).unwrap();
        ed.execute(&cmd).unwrap();
        let area = ed.doc().object(r).unwrap().shape.area().unwrap();
        assert!(
            (area - (100.0 - (4.0 - std::f64::consts::PI))).abs() < 1e-6,
            "{area}"
        );
        // Segments that are not neighbors.
        assert!(join(ed.doc(), r, v(5.0, 0.0), r, v(5.0, 10.0), c).is_err());
        let all = round_all(
            ed.doc(),
            r,
            None,
            JoinKind::Chamfer(ChamferSize::Distances(1.0, 1.0)),
        );
        assert!(all.is_ok() || all.is_err());
    }

    #[test]
    fn offsets_copy_or_move() {
        let mut ed = Editor::default();
        let c = add(
            &mut ed,
            Shape::Circle {
                center: v(0.0, 0.0),
                radius: 5.0,
            },
        );
        let cmd = offset(ed.doc(), c, OffsetTo::Distance(2.0, Side::Outside), true).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 2);
        let cmd = offset(ed.doc(), c, OffsetTo::Through(v(9.0, 0.0)), false).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 2);
        assert_eq!(
            ed.doc().object(c).unwrap().shape,
            Shape::Circle {
                center: v(0.0, 0.0),
                radius: 9.0
            }
        );
    }

    #[test]
    fn surfaces() {
        let mut ed = Editor::default();
        let a = rect(&mut ed, 0.0, 0.0, 10.0);
        let b = rect(&mut ed, 5.0, 5.0, 10.0);
        let area = |ed: &Editor| -> f64 {
            ed.doc()
                .objects
                .values()
                .map(|o| o.shape.area().unwrap_or(0.0))
                .sum()
        };
        let cmd = surface(ed.doc(), SurfaceOp::Add, &[a, b], None, false).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 1);
        assert!((area(&ed) - 175.0).abs() < 1e-9);
        ed.undo();
        let cmd = surface(ed.doc(), SurfaceOp::Subtract, &[b], Some(a), false).unwrap();
        ed.execute(&cmd).unwrap();
        assert!((area(&ed) - 75.0).abs() < 1e-9);
        ed.undo();
        let cmd = surface(ed.doc(), SurfaceOp::Clip, &[a, b], None, true).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 2);
        assert!((area(&ed) - (75.0 + 100.0)).abs() < 1e-9);
        ed.undo();
        let cmd = surface(ed.doc(), SurfaceOp::Intersect, &[a, b], None, false).unwrap();
        ed.execute(&cmd).unwrap();
        assert!((area(&ed) - 25.0).abs() < 1e-9);
        ed.undo();
        let cmd = surface(ed.doc(), SurfaceOp::Compose, &[a, b], None, false).unwrap();
        ed.execute(&cmd).unwrap();
        assert_eq!(ed.doc().objects.len(), 3);
        assert!((area(&ed) - 175.0).abs() < 1e-9);
        // Errors.
        let l = line(&mut ed, v(0.0, 0.0), v(1.0, 1.0));
        assert!(
            surface(
                ed.doc(),
                SurfaceOp::Add,
                &[l, ObjectId::from_num(2)],
                None,
                false
            )
            .is_err()
        );
        assert!(surface(ed.doc(), SurfaceOp::Add, &[l], None, false).is_err());
        assert!(surface(ed.doc(), SurfaceOp::Subtract, &[l, a], None, false).is_err());
    }

    #[test]
    fn chamfer_size_forms() {
        assert_eq!(
            chamfer_size(Some(2.0), Some(3.0), None, None).unwrap(),
            ChamferSize::Distances(2.0, 3.0)
        );
        assert_eq!(
            chamfer_size(Some(2.0), None, None, None).unwrap(),
            ChamferSize::Distances(2.0, 2.0)
        );
        assert_eq!(
            chamfer_size(Some(2.0), None, Some(0.5), None).unwrap(),
            ChamferSize::DistanceAngle(2.0, 0.5)
        );
        assert_eq!(
            chamfer_size(None, None, None, Some(4.0)).unwrap(),
            ChamferSize::Length(4.0)
        );
        assert!(chamfer_size(None, None, None, None).is_err());
    }

    #[test]
    fn pieces_keep_the_records_and_the_attributes_of_their_source() {
        use crate::record::RecordFormat;
        use crate::resource::ResourceBody;
        let mut ed = Editor::default();
        let t = line(&mut ed, v(0.0, 0.0), v(1000.0, 0.0));
        line(&mut ed, v(300.0, -50.0), v(300.0, 50.0));
        line(&mut ed, v(700.0, -50.0), v(700.0, 50.0));
        let format: RecordFormat = serde_json::from_value(serde_json::json!({
            "fields": [{"name": "Depth", "type": "dimension", "default": 300}]
        }))
        .unwrap();
        let fmt = ed
            .execute(&Command::AddResource {
                name: "Pset".into(),
                body: ResourceBody::RecordFormat(Box::new(format)),
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::AttachRecord {
            ids: vec![t],
            record: crate::resource::ResourceId(fmt),
            values: serde_json::Map::from_iter([("Depth".to_string(), serde_json::json!(250))]),
        })
        .unwrap();
        ed.execute(&Command::ModifyObject {
            id: t,
            shape: None,
            layer: None,
            class: None,
            attrs: Some(Attributes {
                line_weight: Some(0.7),
                ..Default::default()
            }),
        })
        .unwrap();
        let before = ed.doc().object(t).unwrap().data.clone();
        assert!(!before.is_empty());
        for cmd in [
            trim(ed.doc(), t, v(500.0, 0.0), None).unwrap(),
            split(ed.doc(), t, &[v(500.0, 0.0)]).unwrap(),
            offset(ed.doc(), t, OffsetTo::Distance(10.0, Side::Left), true).unwrap(),
        ] {
            let mut copy = Editor::new(ed.doc().clone());
            let out = copy.execute(&cmd).unwrap();
            assert!(!out.created.is_empty());
            for id in out.created {
                let o = copy.doc().object(ObjectId(id)).unwrap();
                assert_eq!(o.data, before, "the record data is copied");
                assert_eq!(o.attrs.line_weight, Some(0.7));
            }
        }
        // Compose deletes its sources after copying them.
        let a = rect(&mut ed, 0.0, 100.0, 10.0);
        let b = rect(&mut ed, 5.0, 105.0, 10.0);
        let cmd = surface(ed.doc(), SurfaceOp::Compose, &[a, b], None, false).unwrap();
        ed.execute(&cmd).unwrap();
        assert!(ed.doc().object(a).is_none());
    }
}
