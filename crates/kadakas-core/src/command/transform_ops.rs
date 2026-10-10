//! Commands that scale, mirror, turn, move and array objects (see `crate::edit2d`).

use std::collections::HashMap;

use super::group_ops::rotate_shape;
use super::*;
use crate::edit2d::Xf;
use crate::edit2d::array::placements;
use crate::edit2d::xform::map_shape;
use crate::symbols::{EDIT_GROUP_KEY, EDITING_KEY};

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

/// Refuses a command that would create more than `MAX_CREATED` objects: `copies` times the
/// objects with their group members.
fn check_created(copies: usize, objects: usize) -> Result<(), CommandError> {
    if copies.saturating_mul(objects) > crate::edit2d::array::MAX_CREATED {
        return Err(invalid(format!(
            "this would create {} objects. One command creates at most {}",
            copies.saturating_mul(objects),
            crate::edit2d::array::MAX_CREATED
        )));
    }
    Ok(())
}

/// Applies the transform to the shape of one object, and to the data that goes with the shape.
/// A plant keeps its own turn in its data.
pub fn transform_object(o: &mut Object, xf: &Xf) -> Result<(), String> {
    if xf.is_translation() {
        o.shape.translate(xf.t);
        crate::hardscape::translate_object_data(o, xf.t);
        return Ok(());
    }
    let is_plant = matches!(o.shape, Shape::Plant { .. });
    if let Some((center, angle)) = xf.as_rotation() {
        o.shape = rotate_shape(&o.shape, center, angle)?;
    } else {
        o.shape = map_shape(&o.shape, xf)?;
    }
    if is_plant && xf.similarity().is_some() {
        let old = crate::plants::link::rotation(o);
        let new = if xf.is_reflection() {
            xf.angle() - old
        } else {
            old + xf.angle()
        };
        o.data
            .insert(crate::plants::link::ROTATION_KEY.into(), new.into());
    }
    Ok(())
}

impl Tx<'_> {
    /// Copies the objects (and the members of groups among them) and puts `xf` on the copies.
    /// Returns the IDs of the copies of `ids`.
    fn copy_transformed(
        &mut self,
        ids: &[ObjectId],
        xf: &Xf,
    ) -> Result<Vec<ObjectId>, CommandError> {
        let all = self.doc.with_members(ids);
        let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
        for &old in &all {
            map.insert(old, ObjectId(self.alloc()?));
        }
        for &old in &all {
            let mut o = self.doc.object(old).expect("exists").clone();
            o.id = map[&old];
            o.parent = o.parent.map(|p| map.get(&p).copied().unwrap_or(p));
            transform_object(&mut o, xf).map_err(invalid)?;
            o.data.remove(EDITING_KEY);
            o.data.remove(EDIT_GROUP_KEY);
            self.check_sheet_rules(o.layer, &o.shape, None)?;
            self.add_new(o);
        }
        self.present_copy_slots(&map);
        Ok(ids.iter().map(|id| map[id]).collect())
    }

    pub(super) fn run_transform(
        &mut self,
        cmd: &Command,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        match cmd {
            Command::TransformObjects {
                ids,
                transform,
                copy,
            } => {
                let xf = transform.xf().map_err(invalid)?;
                if *copy {
                    let ids = unique(ids);
                    if ids.is_empty() {
                        return Err(invalid("give at least one object"));
                    }
                    for &id in &ids {
                        self.editable(id)?;
                    }
                    let all = self.doc.with_members(&ids);
                    for &id in &all {
                        let layer = self.doc.object(id).expect("exists").layer;
                        self.check_layer(layer)?;
                    }
                    check_created(1, all.len())?;
                    let copies = self.copy_transformed(&ids, &xf)?;
                    out.created.extend(copies.iter().map(|c| c.0));
                    return Ok(());
                }
                let leaves = self.editable_leaves(ids)?;
                if leaves.is_empty() {
                    return Err(invalid("give at least one object"));
                }
                for id in leaves {
                    self.touch(id);
                    let o = self.doc.object_mut(id).expect("checked");
                    transform_object(o, &xf).map_err(invalid)?;
                }
            }
            Command::ArrayObjects { ids, array } => {
                let ids = unique(ids);
                if ids.is_empty() {
                    return Err(invalid("give at least one object"));
                }
                for &id in &ids {
                    self.editable(id)?;
                }
                let all = self.doc.with_members(&ids);
                for &id in &all {
                    let layer = self.doc.object(id).expect("exists").layer;
                    self.check_layer(layer)?;
                }
                let spec = array.to_spec().map_err(invalid)?;
                // The limit is checked before any work starts.
                check_created(spec.copies(), all.len())?;
                // The reference point: the center of the bounds of the objects.
                let (mut lo, mut hi) = (
                    glam::DVec2::splat(f64::INFINITY),
                    glam::DVec2::splat(f64::NEG_INFINITY),
                );
                for &id in &ids {
                    let (a, b) = self.doc.object_bounds(self.doc.object(id).expect("exists"));
                    if a.is_finite() && b.is_finite() {
                        lo = lo.min(a);
                        hi = hi.max(b);
                    }
                }
                let origin = if lo.is_finite() {
                    (lo + hi) / 2.0
                } else {
                    glam::DVec2::ZERO
                };
                let mut xfs = placements(&spec, origin).map_err(invalid)?;
                // The first place of a rectangular array is the original.
                if array.retain && matches!(spec.layout, crate::edit2d::Layout::Rectangular { .. })
                {
                    xfs.remove(0);
                }
                for xf in &xfs {
                    let copies = self.copy_transformed(&ids, xf)?;
                    out.created.extend(copies.iter().map(|c| c.0));
                }
                if !array.retain {
                    self.run(&Command::DeleteObjects { ids }, out)?;
                }
            }
            Command::AddObjectLike { source, shape } => {
                let src = self
                    .doc
                    .object(*source)
                    .ok_or(CommandError::NoObject(*source))?;
                let (layer, class, parent, attrs, mut data) = (
                    src.layer,
                    src.class,
                    src.parent,
                    src.attrs.clone(),
                    src.data.clone(),
                );
                self.check_layer(layer)?;
                validate_shape(shape)?;
                self.check_shape_refs(shape)?;
                self.check_sheet_rules(layer, shape, None)?;
                data.remove(EDITING_KEY);
                data.remove(EDIT_GROUP_KEY);
                let id = ObjectId(self.alloc()?);
                let mut o = Object::new(id, layer, class, shape.clone());
                o.attrs = attrs;
                o.parent = parent;
                o.data = data;
                self.add_new(o);
                out.created.push(id.0);
            }
            _ => unreachable!("run_transform gets only transform commands"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit2d::{ArrayParams, Transform};
    use glam::DVec2;

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

    fn circle(ed: &mut Editor) -> ObjectId {
        add(
            ed,
            Shape::Circle {
                center: v(10.0, 0.0),
                radius: 2.0,
            },
        )
    }

    fn array(json: serde_json::Value) -> ArrayParams {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn scale_mirror_in_place_and_undo() {
        let mut ed = Editor::default();
        let c = circle(&mut ed);
        let before = ed.doc().clone();
        ed.execute(&Command::TransformObjects {
            ids: vec![c],
            transform: Transform::Scale {
                center: v(0.0, 0.0),
                sx: 2.0,
                sy: None,
            },
            copy: false,
        })
        .unwrap();
        assert_eq!(
            ed.doc().object(c).unwrap().shape,
            Shape::Circle {
                center: v(20.0, 0.0),
                radius: 4.0
            }
        );
        ed.execute(&Command::TransformObjects {
            ids: vec![c],
            transform: Transform::Mirror {
                a: v(0.0, 0.0),
                b: v(0.0, 1.0),
            },
            copy: false,
        })
        .unwrap();
        let Shape::Circle { center, .. } = ed.doc().object(c).unwrap().shape else {
            panic!()
        };
        assert!(center.distance(v(-20.0, 0.0)) < 1e-9);
        ed.undo();
        ed.undo();
        assert_eq!(ed.doc().objects, before.objects);
    }

    #[test]
    fn copies_keep_the_original() {
        let mut ed = Editor::default();
        let c = circle(&mut ed);
        let out = ed
            .execute(&Command::TransformObjects {
                ids: vec![c],
                transform: Transform::Mirror {
                    a: v(0.0, 0.0),
                    b: v(0.0, 1.0),
                },
                copy: true,
            })
            .unwrap();
        assert_eq!(out.created.len(), 1);
        assert_eq!(ed.doc().objects.len(), 2);
        assert!(matches!(
            ed.doc().object(ObjectId(out.created[0])).unwrap().shape,
            Shape::Circle { center, .. } if center.distance(v(-10.0, 0.0)) < 1e-9
        ));
        // Errors: nothing, a non-uniform scale of a circle, zero factor.
        assert!(
            ed.execute(&Command::TransformObjects {
                ids: vec![],
                transform: Transform::Move { delta: v(1.0, 0.0) },
                copy: true
            })
            .is_err()
        );
        assert!(
            ed.execute(&Command::TransformObjects {
                ids: vec![c],
                transform: Transform::Scale {
                    center: v(0.0, 0.0),
                    sx: 2.0,
                    sy: Some(3.0)
                },
                copy: false
            })
            .is_err()
        );
        assert!(
            ed.execute(&Command::TransformObjects {
                ids: vec![c],
                transform: Transform::Scale {
                    center: v(0.0, 0.0),
                    sx: 0.0,
                    sy: None
                },
                copy: false
            })
            .is_err()
        );
        assert_eq!(ed.doc().objects.len(), 2);
    }

    #[test]
    fn linear_array_is_one_undo_step() {
        let mut ed = Editor::default();
        let c = circle(&mut ed);
        let before = ed.doc().clone();
        let out = ed
            .execute(&Command::ArrayObjects {
                ids: vec![c],
                array: array(serde_json::json!({"shape": "linear", "count": 3, "offset": [5, 0]})),
            })
            .unwrap();
        assert_eq!(out.created.len(), 3);
        assert_eq!(ed.doc().objects.len(), 4);
        let Shape::Circle { center, .. } = ed.doc().object(ObjectId(out.created[2])).unwrap().shape
        else {
            panic!()
        };
        assert_eq!(center, v(25.0, 0.0));
        ed.undo();
        assert_eq!(ed.doc().objects, before.objects);
    }

    #[test]
    fn rectangular_array_and_not_retaining() {
        let mut ed = Editor::default();
        let c = circle(&mut ed);
        let json = serde_json::json!({"shape": "rectangular", "columns": 3, "rows": 2, "column_spacing": 10, "row_spacing": 10});
        let out = ed
            .execute(&Command::ArrayObjects {
                ids: vec![c],
                array: array(json.clone()),
            })
            .unwrap();
        // Six places: the original and five copies.
        assert_eq!(out.created.len(), 5);
        assert_eq!(ed.doc().objects.len(), 6);
        let mut ed = Editor::default();
        let c = circle(&mut ed);
        let mut j = json;
        j["retain"] = false.into();
        let out = ed
            .execute(&Command::ArrayObjects {
                ids: vec![c],
                array: array(j),
            })
            .unwrap();
        assert_eq!(out.created.len(), 6);
        assert_eq!(ed.doc().objects.len(), 6);
        assert!(ed.doc().object(c).is_none());
    }

    #[test]
    fn circular_array_turns_the_copies() {
        let mut ed = Editor::default();
        let p = add(
            &mut ed,
            Shape::Polyline {
                points: crate::curve::corners(&[v(10.0, -1.0), v(12.0, -1.0), v(12.0, 1.0)]),
                closed: true,
            },
        );
        let out = ed
            .execute(&Command::ArrayObjects {
                ids: vec![p],
                array: array(
                    serde_json::json!({"shape": "circular", "count": 3, "center": [0, 0]}),
                ),
            })
            .unwrap();
        assert_eq!(out.created.len(), 3);
        // The areas stay the same: the copies are turned, not changed.
        let a0 = ed.doc().object(p).unwrap().shape.area().unwrap();
        for id in out.created {
            let a = ed.doc().object(ObjectId(id)).unwrap().shape.area().unwrap();
            assert!((a - a0).abs() < 1e-9);
        }
        // Hardscapes cannot be turned, but they can be placed round a circle without turning.
        let h = add(
            &mut ed,
            Shape::Hardscape {
                boundary: crate::curve::corners(&[v(10.0, 0.0), v(12.0, 0.0), v(12.0, 2.0)]),
                material: "x".into(),
                thickness: None,
                elevation: None,
            },
        );
        let turn = ed.execute(&Command::ArrayObjects {
            ids: vec![h],
            array: array(serde_json::json!({"shape": "circular", "count": 2, "center": [0, 0]})),
        });
        assert!(turn.is_err());
        let place = ed.execute(&Command::ArrayObjects { ids: vec![h], array: array(serde_json::json!({"shape": "circular", "count": 2, "center": [0, 0], "rotate_objects": false})) });
        assert!(place.is_ok(), "{place:?}");
    }

    #[test]
    fn groups_are_copied_as_a_unit_and_errors() {
        let mut ed = Editor::default();
        let a = circle(&mut ed);
        let b = circle(&mut ed);
        let g = ObjectId(
            ed.execute(&Command::Group {
                ids: vec![a, b],
                name: None,
            })
            .unwrap()
            .created[0],
        );
        let out = ed
            .execute(&Command::ArrayObjects {
                ids: vec![g],
                array: array(serde_json::json!({"shape": "linear", "count": 2, "offset": [0, 10]})),
            })
            .unwrap();
        assert_eq!(out.created.len(), 2);
        // Group + 2 members, three times.
        assert_eq!(ed.doc().objects.len(), 9);
        assert!(
            ed.execute(&Command::ArrayObjects {
                ids: vec![],
                array: array(serde_json::json!({"shape": "linear", "count": 2, "offset": [0, 10]}))
            })
            .is_err()
        );
        assert!(
            ed.execute(&Command::ArrayObjects {
                ids: vec![g],
                array: array(serde_json::json!({"shape": "linear", "count": 0, "offset": [0, 10]}))
            })
            .is_err()
        );
        assert!(
            ed.execute(&Command::ArrayObjects {
                ids: vec![ObjectId::from_num(999)],
                array: array(serde_json::json!({"shape": "linear", "count": 1, "offset": [0, 10]}))
            })
            .is_err()
        );
    }

    #[test]
    fn plants_keep_their_turn() {
        let mut ed = Editor::default();
        let p = add(
            &mut ed,
            Shape::Plant {
                at: v(5.0, 0.0),
                species: "x".into(),
                spread: 2.0,
                height: 1.0,
            },
        );
        ed.execute(&Command::TransformObjects {
            ids: vec![p],
            transform: Transform::Rotate {
                center: v(0.0, 0.0),
                angle: 1.0,
            },
            copy: false,
        })
        .unwrap();
        let r = crate::plants::link::rotation(ed.doc().object(p).unwrap());
        assert!((r - 1.0).abs() < 1e-12);
        ed.execute(&Command::TransformObjects {
            ids: vec![p],
            transform: Transform::Scale {
                center: v(0.0, 0.0),
                sx: 2.0,
                sy: None,
            },
            copy: false,
        })
        .unwrap();
        let Shape::Plant { spread, .. } = ed.doc().object(p).unwrap().shape else {
            panic!()
        };
        assert_eq!(spread, 4.0);
    }

    #[test]
    fn the_number_of_created_objects_is_limited_before_any_work() {
        let mut ed = Editor::default();
        let ids: Vec<ObjectId> = (0..11).map(|_| circle(&mut ed)).collect();
        let before = ed.doc().objects.len();
        // 10 000 copies of 11 objects is 110 000 objects.
        let r = ed.execute(&Command::ArrayObjects {
            ids: ids.clone(),
            array: array(serde_json::json!({"shape": "linear", "count": 10000, "offset": [1, 0]})),
        });
        let Err(CommandError::Invalid(m)) = r else {
            panic!("{r:?}")
        };
        assert!(m.contains("110000") && m.contains("100000"), "{m}");
        assert_eq!(ed.doc().objects.len(), before);
        // A small array is fine.
        let r = ed.execute(&Command::ArrayObjects {
            ids: ids[..10].to_vec(),
            array: array(serde_json::json!({"shape": "linear", "count": 50, "offset": [1, 0]})),
        });
        assert!(r.is_ok(), "{r:?}");
        ed.undo();
        assert_eq!(ed.doc().objects.len(), before);
    }
}
