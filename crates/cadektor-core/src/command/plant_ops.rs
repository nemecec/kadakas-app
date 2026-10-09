//! Commands for resources and plants. They are part of `Command`, and run inside `Tx` like all others.

use super::*;
use crate::plants::link::{self, PlantLink};
use crate::plants::placement::MAX_POINTS;
use crate::plants::style::PlantStyle;
use crate::resource::Resource;

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    /// A plant tag must refer to a plant style that exists.
    pub(super) fn check_shape_refs(&self, shape: &Shape) -> Result<(), CommandError> {
        match shape {
            Shape::PlantTag { style, .. } if self.doc.resources.plant_style(*style).is_none() => Err(CommandError::NoResource(*style)),
            _ => Ok(()),
        }
    }

    fn check_resource_name(&self, name: &str, body: &ResourceBody, except: Option<ResourceId>) -> Result<(), CommandError> {
        if name.trim().is_empty() {
            return Err(invalid("a resource needs a name"));
        }
        let clash = self
            .doc
            .resources
            .iter()
            .any(|r| Some(r.id) != except && r.body.type_name() == body.type_name() && r.name.eq_ignore_ascii_case(name.trim()));
        if clash { Err(invalid(format!("a {} named '{}' exists already", body.type_name(), name.trim()))) } else { Ok(()) }
    }

    /// Objects that use the resource: plants linked to the style, and plant tags.
    fn users_of(&self, id: ResourceId) -> Vec<ObjectId> {
        self.doc
            .objects
            .values()
            .filter(|o| link::link(o).is_some_and(|l| l.style == id) || matches!(o.shape, Shape::PlantTag { style, .. } if style == id))
            .map(|o| o.id)
            .collect()
    }

    /// Brings the copied species, spread and height of linked plants up to date.
    fn resync(&mut self, ids: &[ObjectId]) {
        for &id in ids {
            self.touch(id);
            let mut o = self.doc.object(id).expect("exists").clone();
            link::sync_shape(self.doc, &mut o);
            *self.doc.object_mut(id).expect("exists") = o;
        }
    }

    pub(super) fn run_plant(&mut self, cmd: &Command, out: &mut Outcome) -> Result<(), CommandError> {
        match cmd {
            Command::AddResource { name, body } => {
                body.validate().map_err(invalid)?;
                self.check_resource_name(name, body, None)?;
                let id = ResourceId(self.alloc()?);
                self.doc.resources.push(Resource { id, name: name.trim().to_string(), body: body.clone() });
                out.created.push(id.0);
            }
            Command::ModifyResource { id, name, body } => {
                let current = self.doc.resources.get(*id).ok_or(CommandError::NoResource(*id))?;
                let new_name = name.clone().unwrap_or_else(|| current.name.clone());
                let new_body = body.clone().unwrap_or_else(|| current.body.clone());
                if new_body.type_name() != current.body.type_name() {
                    return Err(invalid("the type of a resource cannot change"));
                }
                new_body.validate().map_err(invalid)?;
                self.check_resource_name(&new_name, &new_body, Some(*id))?;
                let r = self.doc.resources.get_mut(*id).expect("checked");
                r.name = new_name.trim().to_string();
                r.body = new_body;
                let users = self.users_of(*id);
                self.resync(&users);
            }
            Command::DeleteResource { id, reassign } => {
                let current = self.doc.resources.get(*id).ok_or(CommandError::NoResource(*id))?;
                let users = self.users_of(*id);
                if let Some(to) = reassign {
                    let target = self.doc.resources.get(*to).ok_or(CommandError::NoResource(*to))?;
                    if to == id {
                        return Err(invalid("cannot reassign a resource to itself"));
                    }
                    if target.body.type_name() != current.body.type_name() {
                        return Err(invalid("reassign to a resource of the same type"));
                    }
                    for &uid in &users {
                        self.touch(uid);
                        let o = self.doc.object_mut(uid).expect("exists");
                        if let Shape::PlantTag { style, .. } = &mut o.shape {
                            *style = *to;
                        }
                        if let Some(mut l) = link::link(o) {
                            l.style = *to;
                            link::set_link(o, Some(&l));
                        }
                    }
                } else if !users.is_empty() {
                    return Err(CommandError::ResourceInUse(*id, users.len()));
                }
                self.doc.resources.remove(*id);
                self.resync(&users);
            }
            Command::AddPlants { style, overrides, positions, layer, class } => {
                let layer = layer.unwrap_or(self.doc.active_layer);
                let class = class.unwrap_or(self.doc.active_class);
                self.check_layer(layer)?;
                self.check_class(class)?;
                if positions.is_empty() {
                    return Err(invalid("give at least one position"));
                }
                if positions.len() > MAX_POINTS {
                    return Err(invalid(format!("too many plants. The limit is {MAX_POINTS}")));
                }
                let (base, name) = match style {
                    Some(sid) => {
                        let r = self.doc.resources.get(*sid).ok_or(CommandError::NoResource(*sid))?;
                        (self.doc.resources.plant_style(*sid).ok_or_else(|| invalid("the resource is not a plant style"))?.clone(), r.name.clone())
                    }
                    None => (PlantStyle::default(), "Plant".to_string()),
                };
                let eff = link::merge(&base, overrides).map_err(invalid)?;
                let species = eff.species(&name);
                for at in positions {
                    let shape = Shape::Plant { at: *at, species: species.clone(), spread: eff.spread, height: eff.height };
                    validate_shape(&shape)?;
                    let id = ObjectId(self.alloc()?);
                    self.added.push(id);
                    self.added_set.insert(id);
                    let mut o = Object::new(id, layer, class, shape);
                    if let Some(sid) = style {
                        link::set_link(&mut o, Some(&PlantLink { style: *sid, overrides: overrides.clone() }));
                    }
                    self.doc.objects.insert(id, o);
                    out.created.push(id.0);
                }
            }
            Command::SetPlantStyle { ids, style, overrides, unlink } => {
                if *unlink && (style.is_some() || overrides.is_some()) {
                    return Err(invalid("unlink cannot be combined with a style or overrides"));
                }
                if !*unlink && style.is_none() && overrides.is_none() {
                    return Err(invalid("give a style, overrides or unlink"));
                }
                if let Some(sid) = style {
                    self.doc.resources.plant_style(*sid).ok_or(CommandError::NoResource(*sid))?;
                }
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                    if !matches!(self.doc.object(id).expect("checked").shape, Shape::Plant { .. }) {
                        return Err(invalid(format!("object {id} is not a plant")));
                    }
                }
                // Check everything before the first change, so a failure changes nothing.
                let mut plan = Vec::new();
                for &id in &ids {
                    let o = self.doc.object(id).expect("checked");
                    if *unlink {
                        plan.push((id, None));
                        continue;
                    }
                    let old = link::link(o);
                    let sid = style.or(old.as_ref().map(|l| l.style)).ok_or_else(|| invalid(format!("plant {id} has no style. Give one")))?;
                    let ov = match (overrides, &old) {
                        (Some(m), _) => m.clone(),
                        (None, Some(l)) if l.style == sid => l.overrides.clone(),
                        (None, _) => Default::default(),
                    };
                    let base = self.doc.resources.plant_style(sid).expect("checked");
                    link::merge(base, &ov).map_err(invalid)?;
                    plan.push((id, Some(PlantLink { style: sid, overrides: ov })));
                }
                for (id, l) in plan {
                    self.touch(id);
                    let mut o = self.doc.object(id).expect("checked").clone();
                    link::set_link(&mut o, l.as_ref());
                    link::sync_shape(self.doc, &mut o);
                    *self.doc.object_mut(id).expect("checked") = o;
                }
            }
            _ => unreachable!("run_plant is called for plant commands only"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plants::style::{PlantGraphic, PlantStyle};
    use serde_json::json;

    fn style(name: &str, spread: f64) -> Command {
        Command::AddResource { name: name.into(), body: ResourceBody::plant_style(PlantStyle { spread, id_code: "X".into(), ..PlantStyle::default() }) }
    }

    fn add_style(ed: &mut Editor, name: &str, spread: f64) -> ResourceId {
        ResourceId(ed.execute(&style(name, spread)).unwrap().created[0])
    }

    /// Undo does not give back used IDs, so compare the content only.
    fn same(a: &Document, b: &Document) {
        assert_eq!(a.objects, b.objects);
        assert_eq!(a.resources, b.resources);
    }

    fn pts(n: usize) -> Vec<DVec2> {
        (0..n).map(|i| DVec2::new(i as f64 * 1000.0, 0.0)).collect()
    }

    fn place(ed: &mut Editor, style: Option<ResourceId>, overrides: Value, n: usize) -> Vec<ObjectId> {
        let overrides = overrides.as_object().cloned().unwrap_or_default();
        let out = ed.execute(&Command::AddPlants { style, overrides, positions: pts(n), layer: None, class: None }).unwrap();
        out.created.into_iter().map(ObjectId).collect()
    }

    fn plant_fields(ed: &Editor, id: ObjectId) -> (String, f64, f64) {
        match &ed.doc().object(id).unwrap().shape {
            Shape::Plant { species, spread, height, .. } => (species.clone(), *spread, *height),
            _ => panic!("not a plant"),
        }
    }

    #[test]
    fn add_resource_checks_names_and_numbers() {
        let mut ed = Editor::default();
        let id = add_style(&mut ed, " Oak ", 4000.0);
        assert_eq!(ed.doc().resources.get(id).unwrap().name, "Oak");
        assert!(matches!(ed.execute(&style("OAK", 1000.0)), Err(CommandError::Invalid(m)) if m.contains("exists")));
        assert!(ed.execute(&style("  ", 1000.0)).is_err());
        assert!(ed.execute(&style("Bad", f64::NAN)).is_err());
        assert!(ed.execute(&style("Bad", -1.0)).is_err());
        assert_eq!(ed.doc().resources.len(), 1);
    }

    #[test]
    fn resource_commands_undo_and_redo() {
        let mut ed = Editor::default();
        let before = ed.doc().clone();
        let id = add_style(&mut ed, "Oak", 4000.0);
        let after_add = ed.doc().clone();
        ed.execute(&Command::ModifyResource { id, name: Some("Oak tree".into()), body: None }).unwrap();
        assert_eq!(ed.doc().resources.get(id).unwrap().name, "Oak tree");
        ed.undo();
        same(ed.doc(), &after_add);
        ed.undo();
        same(ed.doc(), &before);
        ed.redo();
        same(ed.doc(), &after_add);
    }

    #[test]
    fn modify_resource_updates_the_plants_that_use_it() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let b = add_style(&mut ed, "B", 2000.0);
        let on_a = place(&mut ed, Some(a), json!({}), 2);
        let on_a_override = place(&mut ed, Some(a), json!({"spread": 1500.0}), 1);
        let on_b = place(&mut ed, Some(b), json!({}), 1);
        let new = PlantStyle { spread: 6000.0, height: 9000.0, botanical_name: "Newus".into(), ..PlantStyle::default() };
        let mut ed2 = Editor::new(ed.doc().clone());
        ed.execute(&Command::ModifyResource { id: a, name: None, body: Some(ResourceBody::plant_style(new)) }).unwrap();
        assert_eq!(plant_fields(&ed, on_a[0]), ("Newus".into(), 6000.0, 9000.0));
        assert_eq!(plant_fields(&ed, on_a[1]).1, 6000.0);
        // The override stays.
        assert_eq!(plant_fields(&ed, on_a_override[0]), ("Newus".into(), 1500.0, 9000.0));
        assert_eq!(plant_fields(&ed, on_b[0]).1, 2000.0);
        // One undo step restores the plants and the style.
        let snapshot = ed2.doc().clone();
        ed.undo();
        same(ed.doc(), &snapshot);
        // Renaming only: species of a style without botanical name follows the style name.
        ed2.execute(&Command::ModifyResource { id: b, name: Some("B2".into()), body: None }).unwrap();
        assert_eq!(plant_fields(&ed2, on_b[0]).0, "B2");
    }

    #[test]
    fn modify_resource_errors() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        add_style(&mut ed, "B", 4000.0);
        let m = |id, name: Option<&str>, body| Command::ModifyResource { id, name: name.map(String::from), body };
        assert!(matches!(ed.execute(&m(ResourceId(99), None, None)), Err(CommandError::NoResource(_))));
        assert!(ed.execute(&m(a, Some("b"), None)).is_err());
        assert!(ed.execute(&m(a, Some(" "), None)).is_err());
        let bad = ResourceBody::plant_style(PlantStyle { height: 0.0, ..PlantStyle::default() });
        assert!(ed.execute(&m(a, None, Some(bad))).is_err());
        // Renaming to its own name in another case is fine.
        assert!(ed.execute(&m(a, Some("a"), None)).is_ok());
    }

    #[test]
    fn delete_refuses_a_style_in_use_and_reassign_moves_the_users() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let b = add_style(&mut ed, "B", 1000.0);
        let plants = place(&mut ed, Some(a), json!({"height": 777.0}), 2);
        let tag = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::PlantTag { at: DVec2::ZERO, anchor: DVec2::ZERO, style: a, height: 400.0 },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let before = ed.doc().clone();
        assert_eq!(ed.execute(&Command::DeleteResource { id: a, reassign: None }), Err(CommandError::ResourceInUse(a, 3)));
        same(ed.doc(), &before);
        assert_eq!(ed.execute(&Command::DeleteResource { id: a, reassign: Some(a) }).unwrap_err(), invalid("cannot reassign a resource to itself"));
        assert_eq!(ed.execute(&Command::DeleteResource { id: a, reassign: Some(ResourceId(99)) }), Err(CommandError::NoResource(ResourceId(99))));
        ed.execute(&Command::DeleteResource { id: a, reassign: Some(b) }).unwrap();
        assert!(ed.doc().resources.get(a).is_none());
        assert_eq!(link::link(ed.doc().object(plants[0]).unwrap()).unwrap().style, b);
        assert_eq!(plant_fields(&ed, plants[1]), ("B".into(), 1000.0, 777.0));
        assert!(matches!(ed.doc().object(tag).unwrap().shape, Shape::PlantTag { style, .. } if style == b));
        ed.undo();
        same(ed.doc(), &before);
        // An unused style is deleted without reassigning.
        let c = add_style(&mut ed, "C", 1000.0);
        ed.execute(&Command::DeleteResource { id: c, reassign: None }).unwrap();
        assert_eq!(ed.execute(&Command::DeleteResource { id: c, reassign: None }), Err(CommandError::NoResource(c)));
    }

    #[test]
    fn add_plants_makes_one_undo_step() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let before = ed.doc().clone();
        let ids = place(&mut ed, Some(a), json!({"price": 5.0}), 25);
        assert_eq!(ids.len(), 25);
        assert_eq!(link::link(ed.doc().object(ids[3]).unwrap()).unwrap().overrides["price"], 5.0);
        ed.undo();
        same(ed.doc(), &before);
        ed.redo();
        assert_eq!(ed.doc().objects.len(), 25);
    }

    #[test]
    fn add_plants_without_a_style_is_by_instance() {
        let mut ed = Editor::default();
        let ids = place(&mut ed, None, json!({"botanical_name": "Oak", "spread": 800.0}), 1);
        assert_eq!(plant_fields(&ed, ids[0]), ("Oak".into(), 800.0, 4000.0));
        assert!(ed.doc().object(ids[0]).unwrap().data.is_empty());
        let ids = place(&mut ed, None, json!({}), 1);
        assert_eq!(plant_fields(&ed, ids[0]).0, "Plant");
    }

    #[test]
    fn add_plants_errors() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let cmd = |style, overrides: Value, positions, layer: Option<LayerId>| Command::AddPlants {
            style,
            overrides: overrides.as_object().cloned().unwrap(),
            positions,
            layer,
            class: None,
        };
        assert!(ed.execute(&cmd(Some(a), json!({}), vec![], None)).is_err());
        assert!(ed.execute(&cmd(Some(ResourceId(99)), json!({}), pts(1), None)).is_err());
        assert!(ed.execute(&cmd(Some(a), json!({"bogus": 1}), pts(1), None)).is_err());
        assert!(ed.execute(&cmd(Some(a), json!({"spread": -1.0}), pts(1), None)).is_err());
        assert!(ed.execute(&cmd(Some(a), json!({}), vec![DVec2::new(f64::NAN, 0.0)], None)).is_err());
        assert!(ed.execute(&cmd(Some(a), json!({}), pts(MAX_POINTS + 1), None)).is_err());
        assert!(ed.execute(&cmd(Some(a), json!({}), pts(1), Some(LayerId(99)))).is_err());
        assert!(ed.doc().objects.is_empty());
        // Locked layer.
        let l = ed.execute(&Command::AddLayer { name: "L".into(), elevation: 0.0, scale: None, kind: None }).unwrap().created[0];
        ed.execute(&Command::ModifyLayer { id: LayerId(l), name: None, visible: None, locked: Some(true), elevation: None, scale: None }).unwrap();
        assert!(ed.execute(&cmd(Some(a), json!({}), pts(1), Some(LayerId(l)))).is_err());
    }

    #[test]
    fn set_plant_style_links_overrides_and_unlinks() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let b = add_style(&mut ed, "B", 2000.0);
        let ids = place(&mut ed, None, json!({"botanical_name": "Own", "spread": 900.0}), 2);
        let set = |ids: &[ObjectId], style, overrides: Option<Value>, unlink| Command::SetPlantStyle {
            ids: ids.to_vec(),
            style,
            overrides: overrides.map(|o| o.as_object().cloned().unwrap()),
            unlink,
        };
        ed.execute(&set(&ids, Some(a), None, false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 4000.0);
        ed.execute(&set(&ids[..1], None, Some(json!({"spread": 3000.0})), false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 3000.0);
        assert_eq!(plant_fields(&ed, ids[1]).1, 4000.0);
        // Same style again keeps the overrides. Another style clears them.
        ed.execute(&set(&ids[..1], Some(a), None, false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 3000.0);
        ed.execute(&set(&ids[..1], Some(b), None, false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 2000.0);
        // Empty overrides remove them.
        ed.execute(&set(&ids[..1], None, Some(json!({"spread": 100.0})), false)).unwrap();
        ed.execute(&set(&ids[..1], None, Some(json!({})), false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 2000.0);
        // Unlink keeps the values the plant has.
        ed.execute(&set(&ids, None, None, true)).unwrap();
        assert_eq!(link::link(ed.doc().object(ids[1]).unwrap()), None);
        assert_eq!(plant_fields(&ed, ids[1]).1, 4000.0);
    }

    #[test]
    fn set_plant_style_errors_change_nothing() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let ids = place(&mut ed, Some(a), json!({}), 2);
        let line = ObjectId(
            ed.execute(&Command::AddObject { shape: Shape::Circle { center: DVec2::ZERO, radius: 5.0 }, layer: None, class: None, attrs: Attributes::default() })
                .unwrap()
                .created[0],
        );
        let before = ed.doc().clone();
        let c = |ids: Vec<ObjectId>, style, overrides: Option<Value>, unlink| Command::SetPlantStyle {
            ids,
            style,
            overrides: overrides.map(|o| o.as_object().cloned().unwrap()),
            unlink,
        };
        assert!(ed.execute(&c(ids.clone(), None, None, false)).is_err());
        assert!(ed.execute(&c(ids.clone(), Some(a), None, true)).is_err());
        assert!(ed.execute(&c(ids.clone(), Some(ResourceId(99)), None, false)).is_err());
        assert!(ed.execute(&c(vec![line], Some(a), None, false)).is_err());
        assert!(ed.execute(&c(vec![ObjectId(999)], Some(a), None, false)).is_err());
        assert!(ed.execute(&c(ids.clone(), None, Some(json!({"nope": 1})), false)).is_err());
        assert!(ed.execute(&c(ids.clone(), None, Some(json!({"spread": 0})), false)).is_err());
        // A plant without a style and no style given.
        let free = place(&mut ed, None, json!({}), 1);
        let snapshot = ed.doc().clone();
        assert!(ed.execute(&c(vec![ids[0], free[0]], None, Some(json!({"price": 1.0})), false)).is_err());
        same(ed.doc(), &snapshot);
        let _ = before;
    }

    #[test]
    fn editing_the_shape_of_a_linked_plant_adds_overrides() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let ids = place(&mut ed, Some(a), json!({}), 1);
        let mut shape = ed.doc().object(ids[0]).unwrap().shape.clone();
        if let Shape::Plant { spread, at, .. } = &mut shape {
            *spread = 1234.0;
            *at = DVec2::new(5.0, 5.0);
        }
        ed.execute(&Command::ModifyObject { id: ids[0], shape: Some(shape), layer: None, class: None, attrs: None }).unwrap();
        let l = link::link(ed.doc().object(ids[0]).unwrap()).unwrap();
        assert_eq!(l.overrides["spread"], 1234.0);
        assert!(!l.overrides.contains_key("height"));
        // A later style change keeps the override.
        let new = PlantStyle { spread: 9000.0, height: 100.0, ..PlantStyle::default() };
        ed.execute(&Command::ModifyResource { id: a, name: None, body: Some(ResourceBody::plant_style(new)) }).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 1234.0);
        assert_eq!(plant_fields(&ed, ids[0]).2, 100.0);
    }

    #[test]
    fn plant_tags_need_an_existing_style() {
        let mut ed = Editor::default();
        let tag = |style| Command::AddObject {
            shape: Shape::PlantTag { at: DVec2::ZERO, anchor: DVec2::new(1.0, 1.0), style, height: 400.0 },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        assert_eq!(ed.execute(&tag(ResourceId(5))), Err(CommandError::NoResource(ResourceId(5))));
        let a = add_style(&mut ed, "A", 4000.0);
        let id = ObjectId(ed.execute(&tag(a)).unwrap().created[0]);
        let moved = Shape::PlantTag { at: DVec2::ZERO, anchor: DVec2::ZERO, style: ResourceId(77), height: 400.0 };
        assert!(ed.execute(&Command::ModifyObject { id, shape: Some(moved), layer: None, class: None, attrs: None }).is_err());
        let bad_height = Shape::PlantTag { at: DVec2::ZERO, anchor: DVec2::ZERO, style: a, height: 0.0 };
        assert!(ed.execute(&Command::ModifyObject { id, shape: Some(bad_height), layer: None, class: None, attrs: None }).is_err());
    }

    #[test]
    fn existing_tree_shape_is_validated() {
        let mut ed = Editor::default();
        let tree = |dbh: f64, spread: f64, factor: Option<f64>| Command::AddObject {
            shape: Shape::ExistingTree {
                at: DVec2::ZERO,
                species: "Oak".into(),
                dbh,
                spread,
                height: 10_000.0,
                condition: Default::default(),
                action: Default::default(),
                rpz_factor: factor,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        assert!(ed.execute(&tree(400.0, 8000.0, None)).is_ok());
        assert!(ed.execute(&tree(400.0, 8000.0, Some(10.0))).is_ok());
        assert!(ed.execute(&tree(0.0, 8000.0, None)).is_err());
        assert!(ed.execute(&tree(400.0, 0.0, None)).is_err());
        assert!(ed.execute(&tree(400.0, 8000.0, Some(-1.0))).is_err());
        assert!(ed.execute(&tree(400.0, 8000.0, Some(f64::NAN))).is_err());
    }

    #[test]
    fn graphic_and_other_fields_pass_through_overrides() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let ids = place(&mut ed, Some(a), json!({"graphic": "star", "id_code": "ZZ"}), 1);
        let e = link::effective(ed.doc(), ed.doc().object(ids[0]).unwrap()).unwrap();
        assert_eq!((e.style.graphic, e.style.id_code.as_str()), (PlantGraphic::Star, "ZZ"));
    }
}
