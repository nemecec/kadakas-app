//! Commands for resources and plants. They are part of `Command`, and run inside `Tx` like all others.

use super::*;
use crate::landscape::style::MixEntry;
use crate::plants::link::{self, PlantLink};
use crate::plants::style::PlantStyle;
use crate::plants::variation::MAX_POINTS;
use crate::resource::Resource;

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    /// A plant tag must refer to a plant style that exists.
    pub(super) fn check_shape_refs(&self, shape: &Shape) -> Result<(), CommandError> {
        match shape {
            Shape::PlantTag { style, .. } if self.doc.resources.plant_style(*style).is_none() => {
                Err(CommandError::NoResource(*style))
            }
            Shape::Annotation(a) => a.check_refs(self.doc).map_err(invalid),
            _ => self.check_symbol_shape(shape),
        }
    }

    /// A landscape area style may only use plant styles that exist.
    fn check_body_refs(&self, body: &ResourceBody) -> Result<(), CommandError> {
        // An annotation resource that could not be read (a value this version does not know) is
        // kept when a file is loaded. A new one must be valid.
        if let ResourceBody::Unknown(v) = body
            && let Some(Err(e)) = crate::annot::style::AnnotResource::from_json(v)
        {
            return Err(invalid(format!(
                "invalid {}: {e}",
                v["type"].as_str().unwrap_or("resource")
            )));
        }
        if let ResourceBody::PlantStyle(s) = body
            && let Some(img) = s.image
        {
            let is_texture = matches!(
                self.doc.resources.get(img).map(|r| &r.body),
                Some(ResourceBody::Graphic(g)) if matches!(**g, crate::graphics::GraphicResource::Texture(_))
            );
            let is_picture = self.doc.resources.image(img).is_some();
            if !is_texture && !is_picture {
                return Err(match self.doc.resources.get(img) {
                    Some(_) => invalid(format!("resource {img} is not an image or a texture")),
                    None => CommandError::NoResource(img),
                });
            }
        }
        if let ResourceBody::LandscapeAreaStyle(s) = body {
            for e in &s.mix {
                if self.doc.resources.plant_style(e.plant).is_none() {
                    return Err(match self.doc.resources.get(e.plant) {
                        Some(_) => invalid(format!("resource {} is not a plant style", e.plant)),
                        None => CommandError::NoResource(e.plant),
                    });
                }
            }
        }
        Ok(())
    }

    fn check_resource_name(
        &self,
        name: &str,
        body: &ResourceBody,
        except: Option<ResourceId>,
    ) -> Result<(), CommandError> {
        if name.trim().is_empty() {
            return Err(invalid("a resource needs a name"));
        }
        if matches!(body, ResourceBody::RecordFormat(_)) {
            crate::record::check_name("record", name).map_err(invalid)?;
            if crate::record::is_reserved_record_name(name)
                || name.trim().eq_ignore_ascii_case(crate::record::MARKER)
            {
                return Err(invalid(format!(
                    "'{}' is reserved. Choose another record name",
                    name.trim()
                )));
            }
            // Plain data under this key would be taken over by the record.
            let marker = except.map(|e| serde_json::Value::from(e.0));
            let taken = self.doc.objects.values().any(|o| {
                o.data.iter().any(|(k, v)| {
                    k.eq_ignore_ascii_case(name.trim())
                        && !(marker.is_some() && v.get(crate::record::MARKER) == marker.as_ref())
                })
            });
            if taken {
                return Err(invalid(format!(
                    "objects already have data under the key '{}'. Choose another record name",
                    name.trim()
                )));
            }
        }
        let clash = self.doc.resources.iter().any(|r| {
            Some(r.id) != except
                && r.body.type_name() == body.type_name()
                && r.name.eq_ignore_ascii_case(name.trim())
        });
        if clash {
            Err(invalid(format!(
                "a {} named '{}' exists already",
                body.type_name(),
                name.trim()
            )))
        } else {
            Ok(())
        }
    }

    /// Objects that use the resource: plants linked to the style, and plant tags.
    fn users_of(&self, id: ResourceId) -> Vec<ObjectId> {
        if let Some(r) = self.doc.resources.get(id)
            && matches!(r.body, ResourceBody::RecordFormat(_))
        {
            return self.record_users(id);
        }
        if self
            .doc
            .resources
            .get(id)
            .is_some_and(|r| matches!(r.body, ResourceBody::Worksheet(_)))
        {
            return crate::worksheet_table::users_of(self.doc, id);
        }
        self.doc
            .objects
            .values()
            .filter(|o| {
                link::link(o).is_some_and(|l| l.style == id)
                    || matches!(o.shape, Shape::PlantTag { style, .. } if style == id)
                    || matches!(&o.shape, Shape::Annotation(a) if a.uses_resource(id))
                    || crate::hardscape::style_id(o) == Some(id)
                    || crate::landscape::style_id(o) == Some(id)
            })
            .map(|o| o.id)
            .collect()
    }

    /// Brings the copied species, spread and height of linked plants up to date.
    fn resync(&mut self, ids: &[ObjectId]) {
        for &id in ids {
            self.touch(id);
            let mut o = self.doc.object(id).expect("exists").clone();
            link::sync_shape(self.doc, &mut o);
            crate::hardscape::data::sync_shape(self.doc, &mut o);
            *self.doc.object_mut(id).expect("exists") = o;
        }
    }

    pub(super) fn run_plant(
        &mut self,
        cmd: &Command,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        match cmd {
            Command::AddResource { name, body } => {
                body.validate().map_err(invalid)?;
                body.verify().map_err(invalid)?;
                self.check_body_refs(body)?;
                self.check_graphic_refs(None, body)?;
                self.check_resource_name(name, body, None)?;
                let id = ResourceId(self.alloc()?);
                self.doc.resources.push(Resource {
                    id,
                    name: name.trim().to_string(),
                    body: body.clone(),
                });
                out.created.push(id.0);
            }
            Command::ModifyResource { id, name, body } => {
                let current = self
                    .doc
                    .resources
                    .get(*id)
                    .ok_or(CommandError::NoResource(*id))?;
                let old_name = current.name.clone();
                let new_name = name.clone().unwrap_or_else(|| current.name.clone());
                let new_body = body.clone().unwrap_or_else(|| current.body.clone());
                if new_body.type_name() != current.body.type_name() {
                    return Err(invalid("the type of a resource cannot change"));
                }
                new_body.validate().map_err(invalid)?;
                // A new picture file is read once; the same file (the same shared text) is not.
                if new_body != current.body {
                    new_body.verify().map_err(invalid)?;
                }
                self.check_body_refs(&new_body)?;
                self.check_graphic_refs(Some(*id), &new_body)?;
                self.check_resource_name(&new_name, &new_body, Some(*id))?;
                let r = self.doc.resources.get_mut(*id).expect("checked");
                r.name = new_name.trim().to_string();
                r.body = new_body;
                self.conform_records(*id, &old_name);
                let users = self.users_of(*id);
                self.resync(&users);
            }
            Command::DeleteResource { id, reassign } => {
                if matches!(
                    self.doc.resources.get(*id).map(|r| &r.body),
                    Some(ResourceBody::Image(_))
                ) {
                    return self.delete_image(*id, *reassign);
                }
                if self.graphic_delete(*id, *reassign)? {
                    return Ok(());
                }
                if self.present_delete(*id)? {
                    return Ok(());
                }
                let current = self
                    .doc
                    .resources
                    .get(*id)
                    .ok_or(CommandError::NoResource(*id))?;
                let users = self.users_of(*id);
                let mixes = self.doc.resources.mixes_using(*id);
                if let Some(to) = reassign {
                    let target = self
                        .doc
                        .resources
                        .get(*to)
                        .ok_or(CommandError::NoResource(*to))?;
                    if to == id {
                        return Err(invalid("cannot reassign a resource to itself"));
                    }
                    if target.body.type_name() != current.body.type_name() {
                        return Err(invalid("reassign to a resource of the same type"));
                    }
                    if matches!(current.body, ResourceBody::RecordFormat(_)) {
                        return Err(invalid(
                            "records cannot be reassigned. Detach the record first",
                        ));
                    }
                    for &uid in &users {
                        self.touch(uid);
                        let o = self.doc.object_mut(uid).expect("exists");
                        if let Shape::PlantTag { style, .. } = &mut o.shape {
                            *style = *to;
                        }
                        if let Shape::Annotation(a) = &mut o.shape {
                            a.replace_resource(*id, *to);
                        }
                        if let Some(mut l) = link::link(o) {
                            l.style = *to;
                            link::set_link(o, Some(&l));
                        }
                        if let Some(mut d) =
                            crate::hardscape::data(o).filter(|d| d.style == Some(*id))
                        {
                            d.style = Some(*to);
                            crate::hardscape::data::set_data(o, &d);
                        }
                        if let Some(mut d) =
                            crate::landscape::data(o).filter(|d| d.style == Some(*id))
                        {
                            d.style = Some(*to);
                            crate::landscape::set_data(o, &d);
                        }
                    }
                    // Landscape area styles that use a plant style take the other plant style.
                    for sid in &mixes {
                        if let Some(Resource {
                            body: ResourceBody::LandscapeAreaStyle(s),
                            ..
                        }) = self.doc.resources.get_mut(*sid)
                        {
                            let Some(i) = s.mix.iter().position(|e| e.plant == *id) else {
                                continue;
                            };
                            let old = s.mix.remove(i);
                            match s.mix.iter_mut().find(|e| e.plant == *to) {
                                Some(e) => e.percent = (e.percent + old.percent).min(100.0),
                                None => s.mix.insert(i, MixEntry { plant: *to, ..old }),
                            }
                        }
                    }
                } else if !users.is_empty() || !mixes.is_empty() {
                    return Err(CommandError::ResourceInUse(*id, users.len() + mixes.len()));
                }
                self.doc.resources.remove(*id);
                self.resync(&users);
            }
            Command::AddPlants {
                style,
                overrides,
                positions,
                layer,
                class,
                variation,
            } => {
                let layer = layer.unwrap_or(self.doc.active_layer);
                let class = class.unwrap_or(self.doc.active_class);
                self.check_layer(layer)?;
                self.check_sheet_rules(
                    layer,
                    &Shape::Plant {
                        at: DVec2::ZERO,
                        species: String::new(),
                        spread: 1.0,
                        height: 1.0,
                    },
                    None,
                )?;
                self.check_class(class)?;
                if positions.is_empty() {
                    return Err(invalid("give at least one position"));
                }
                if positions.len() > MAX_POINTS {
                    return Err(invalid(format!(
                        "too many plants. The limit is {MAX_POINTS}"
                    )));
                }
                if let Some(v) = variation {
                    v.validate().map_err(invalid)?;
                }
                let (base, name) = match style {
                    Some(sid) => {
                        let r = self
                            .doc
                            .resources
                            .get(*sid)
                            .ok_or(CommandError::NoResource(*sid))?;
                        (
                            self.doc
                                .resources
                                .plant_style(*sid)
                                .ok_or_else(|| invalid("the resource is not a plant style"))?
                                .clone(),
                            r.name.clone(),
                        )
                    }
                    None => (PlantStyle::default(), "Plant".to_string()),
                };
                if style.is_some() {
                    link::check_overrides(&base, overrides).map_err(invalid)?;
                }
                let eff = link::merge(&base, overrides).map_err(invalid)?;
                let species = eff.species(&name);
                let factors = variation
                    .filter(|v| v.is_active())
                    .map(|v| v.factors(positions.len()));
                for (i, at) in positions.iter().enumerate() {
                    let (scale, angle) = factors.as_ref().map_or((1.0, 0.0), |f| f[i]);
                    // Size variation changes spread and height of this plant, unless the style locks them.
                    let mut own = overrides.clone();
                    let mut size = (eff.spread, eff.height);
                    if scale != 1.0 {
                        if !(style.is_some() && base.is_locked("spread")) {
                            size.0 = eff.spread * scale;
                        }
                        if !(style.is_some() && base.is_locked("height")) {
                            size.1 = eff.height * scale;
                        }
                        if style.is_some() {
                            own.insert("spread".into(), size.0.into());
                            own.insert("height".into(), size.1.into());
                        }
                    }
                    let shape = Shape::Plant {
                        at: *at,
                        species: species.clone(),
                        spread: size.0,
                        height: size.1,
                    };
                    validate_shape(&shape)?;
                    let id = ObjectId(self.alloc()?);
                    self.added.push(id);
                    self.added_set.insert(id);
                    let mut o = Object::new(id, layer, class, shape);
                    if let Some(sid) = style {
                        // Locked sizes stay with the style: drop their override again.
                        for f in ["spread", "height"] {
                            if base.is_locked(f) {
                                own.remove(f);
                            }
                        }
                        link::set_link(
                            &mut o,
                            Some(&PlantLink {
                                style: *sid,
                                overrides: own,
                            }),
                        );
                    }
                    if angle != 0.0 {
                        o.data.insert(link::ROTATION_KEY.into(), angle.into());
                    }
                    self.doc.objects.insert(id, o);
                    out.created.push(id.0);
                }
            }
            Command::SetPlantStyle {
                ids,
                style,
                overrides,
                unlink,
            } => {
                if *unlink && (style.is_some() || overrides.is_some()) {
                    return Err(invalid(
                        "unlink cannot be combined with a style or overrides",
                    ));
                }
                if !*unlink && style.is_none() && overrides.is_none() {
                    return Err(invalid("give a style, overrides or unlink"));
                }
                if let Some(sid) = style {
                    self.doc
                        .resources
                        .plant_style(*sid)
                        .ok_or(CommandError::NoResource(*sid))?;
                }
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                    if !matches!(
                        self.doc.object(id).expect("checked").shape,
                        Shape::Plant { .. }
                    ) {
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
                    let sid = style
                        .or(old.as_ref().map(|l| l.style))
                        .ok_or_else(|| invalid(format!("plant {id} has no style. Give one")))?;
                    let ov = match (overrides, &old) {
                        (Some(m), _) => m.clone(),
                        (None, Some(l)) if l.style == sid => l.overrides.clone(),
                        (None, _) => Default::default(),
                    };
                    let base = self.doc.resources.plant_style(sid).expect("checked");
                    // Overrides that are carried over drop the fields that the style locks now.
                    let mut ov = ov;
                    if overrides.is_none() {
                        ov.retain(|k, _| !base.is_locked(k));
                    }
                    link::check_overrides(base, &ov).map_err(invalid)?;
                    plan.push((
                        id,
                        Some(PlantLink {
                            style: sid,
                            overrides: ov,
                        }),
                    ));
                }
                for (id, l) in plan {
                    self.touch(id);
                    let mut o = self.doc.object(id).expect("checked").clone();
                    link::set_link(&mut o, l.as_ref());
                    link::sync_shape(self.doc, &mut o);
                    *self.doc.object_mut(id).expect("checked") = o;
                }
            }
            Command::SetPlantPicture { style, picture } => {
                let mut st = self
                    .doc
                    .resources
                    .plant_style(*style)
                    .ok_or(CommandError::NoResource(*style))?
                    .clone();
                let old = st.image;
                let new = match picture {
                    None => None,
                    Some(p) => Some(self.picture_resource(p, out)?),
                };
                st.image = new;
                let r = self.doc.resources.get_mut(*style).expect("checked");
                r.body = ResourceBody::plant_style(st);
                let users = self.users_of(*style);
                self.resync(&users);
                // A picture that no style uses any more goes with the style that had it.
                if let Some(old) = old.filter(|o| Some(*o) != new)
                    && self.doc.resources.image(old).is_some()
                    && self.image_users(old).is_empty()
                {
                    self.doc.resources.remove(old);
                }
            }
            _ => unreachable!("run_plant is called for plant commands only"),
        }
        Ok(())
    }

    /// The plant styles that use a picture resource.
    fn image_users(&self, id: ResourceId) -> Vec<ResourceId> {
        self.doc
            .resources
            .plant_styles()
            .filter(|(_, _, s)| s.image == Some(id))
            .map(|(sid, _, _)| sid)
            .collect()
    }

    /// The resource of a picture: the one with the same hash if the document has it, else a new one.
    fn picture_resource(
        &mut self,
        p: &crate::picture::ImageResource,
        out: &mut Outcome,
    ) -> Result<ResourceId, CommandError> {
        p.validate().map_err(invalid)?;
        if let Some(id) = self.doc.resources.find_image(&p.sha256) {
            return Ok(id);
        }
        // A new file: read it once and check it against its fields.
        p.verify_data().map_err(invalid)?;
        let taken = |doc: &crate::model::Document, n: &str| {
            doc.resources
                .iter()
                .any(|r| r.body.type_name() == "image" && r.name.eq_ignore_ascii_case(n))
        };
        let mut name = p.default_name();
        if taken(self.doc, &name) {
            name = format!("Image {}", &p.sha256[..16]);
            let mut n = 2;
            while taken(self.doc, &name) {
                name = format!("Image {} ({n})", &p.sha256[..16]);
                n += 1;
            }
        }
        let id = ResourceId(self.alloc()?);
        self.doc.resources.push(Resource {
            id,
            name,
            body: ResourceBody::Image(Box::new(p.clone())),
        });
        out.created.push(id.0);
        Ok(id)
    }

    /// Deletes a picture resource. The plant styles that use it take `reassign` (another picture) or lose their picture.
    fn delete_image(
        &mut self,
        id: ResourceId,
        reassign: Option<ResourceId>,
    ) -> Result<(), CommandError> {
        if let Some(to) = reassign {
            if to == id {
                return Err(invalid("cannot reassign a resource to itself"));
            }
            if self.doc.resources.image(to).is_none() {
                return Err(match self.doc.resources.get(to) {
                    Some(_) => invalid("reassign to a resource of the same type"),
                    None => CommandError::NoResource(to),
                });
            }
        }
        for sid in self.image_users(id) {
            if let Some(Resource {
                body: ResourceBody::PlantStyle(s),
                ..
            }) = self.doc.resources.get_mut(sid)
            {
                s.image = reassign;
            }
            let users = self.users_of(sid);
            self.resync(&users);
        }
        self.doc.resources.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plants::style::{PlantGraphic, PlantStyle};
    use serde_json::json;

    fn style(name: &str, spread: f64) -> Command {
        Command::AddResource {
            name: name.into(),
            body: ResourceBody::plant_style(PlantStyle {
                spread,
                id_code: "X".into(),
                ..PlantStyle::default()
            }),
        }
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

    fn place(
        ed: &mut Editor,
        style: Option<ResourceId>,
        overrides: Value,
        n: usize,
    ) -> Vec<ObjectId> {
        let overrides = overrides.as_object().cloned().unwrap_or_default();
        let out = ed
            .execute(&Command::AddPlants {
                style,
                overrides,
                positions: pts(n),
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        out.created.into_iter().map(ObjectId).collect()
    }

    fn plant_fields(ed: &Editor, id: ObjectId) -> (String, f64, f64) {
        match &ed.doc().object(id).unwrap().shape {
            Shape::Plant {
                species,
                spread,
                height,
                ..
            } => (species.clone(), *spread, *height),
            _ => panic!("not a plant"),
        }
    }

    #[test]
    fn add_resource_checks_names_and_numbers() {
        let mut ed = Editor::default();
        let id = add_style(&mut ed, " Oak ", 4000.0);
        assert_eq!(ed.doc().resources.get(id).unwrap().name, "Oak");
        assert!(
            matches!(ed.execute(&style("OAK", 1000.0)), Err(CommandError::Invalid(m)) if m.contains("exists"))
        );
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
        ed.execute(&Command::ModifyResource {
            id,
            name: Some("Oak tree".into()),
            body: None,
        })
        .unwrap();
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
        let new = PlantStyle {
            spread: 6000.0,
            height: 9000.0,
            botanical_name: "Newus".into(),
            ..PlantStyle::default()
        };
        let mut ed2 = Editor::new(ed.doc().clone());
        ed.execute(&Command::ModifyResource {
            id: a,
            name: None,
            body: Some(ResourceBody::plant_style(new)),
        })
        .unwrap();
        assert_eq!(plant_fields(&ed, on_a[0]), ("Newus".into(), 6000.0, 9000.0));
        assert_eq!(plant_fields(&ed, on_a[1]).1, 6000.0);
        // The override stays.
        assert_eq!(
            plant_fields(&ed, on_a_override[0]),
            ("Newus".into(), 1500.0, 9000.0)
        );
        assert_eq!(plant_fields(&ed, on_b[0]).1, 2000.0);
        // One undo step restores the plants and the style.
        let snapshot = ed2.doc().clone();
        ed.undo();
        same(ed.doc(), &snapshot);
        // Renaming only: species of a style without botanical name follows the style name.
        ed2.execute(&Command::ModifyResource {
            id: b,
            name: Some("B2".into()),
            body: None,
        })
        .unwrap();
        assert_eq!(plant_fields(&ed2, on_b[0]).0, "B2");
    }

    #[test]
    fn modify_resource_errors() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        add_style(&mut ed, "B", 4000.0);
        let m = |id, name: Option<&str>, body| Command::ModifyResource {
            id,
            name: name.map(String::from),
            body,
        };
        assert!(matches!(
            ed.execute(&m(ResourceId::from_num(99), None, None)),
            Err(CommandError::NoResource(_))
        ));
        assert!(ed.execute(&m(a, Some("b"), None)).is_err());
        assert!(ed.execute(&m(a, Some(" "), None)).is_err());
        let bad = ResourceBody::plant_style(PlantStyle {
            height: 0.0,
            ..PlantStyle::default()
        });
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
                shape: Shape::PlantTag {
                    at: DVec2::ZERO,
                    anchor: DVec2::ZERO,
                    style: a,
                    height: 400.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let before = ed.doc().clone();
        assert_eq!(
            ed.execute(&Command::DeleteResource {
                id: a,
                reassign: None
            }),
            Err(CommandError::ResourceInUse(a, 3))
        );
        same(ed.doc(), &before);
        assert_eq!(
            ed.execute(&Command::DeleteResource {
                id: a,
                reassign: Some(a)
            })
            .unwrap_err(),
            invalid("cannot reassign a resource to itself")
        );
        assert_eq!(
            ed.execute(&Command::DeleteResource {
                id: a,
                reassign: Some(ResourceId::from_num(99))
            }),
            Err(CommandError::NoResource(ResourceId::from_num(99)))
        );
        ed.execute(&Command::DeleteResource {
            id: a,
            reassign: Some(b),
        })
        .unwrap();
        assert!(ed.doc().resources.get(a).is_none());
        assert_eq!(
            link::link(ed.doc().object(plants[0]).unwrap())
                .unwrap()
                .style,
            b
        );
        assert_eq!(plant_fields(&ed, plants[1]), ("B".into(), 1000.0, 777.0));
        assert!(
            matches!(ed.doc().object(tag).unwrap().shape, Shape::PlantTag { style, .. } if style == b)
        );
        ed.undo();
        same(ed.doc(), &before);
        // An unused style is deleted without reassigning.
        let c = add_style(&mut ed, "C", 1000.0);
        ed.execute(&Command::DeleteResource {
            id: c,
            reassign: None,
        })
        .unwrap();
        assert_eq!(
            ed.execute(&Command::DeleteResource {
                id: c,
                reassign: None
            }),
            Err(CommandError::NoResource(c))
        );
    }

    #[test]
    fn add_plants_makes_one_undo_step() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let before = ed.doc().clone();
        let ids = place(&mut ed, Some(a), json!({"price": 5.0}), 25);
        assert_eq!(ids.len(), 25);
        assert_eq!(
            link::link(ed.doc().object(ids[3]).unwrap())
                .unwrap()
                .overrides["price"],
            5.0
        );
        ed.undo();
        same(ed.doc(), &before);
        ed.redo();
        assert_eq!(ed.doc().objects.len(), 25);
    }

    #[test]
    fn add_plants_without_a_style_is_by_instance() {
        let mut ed = Editor::default();
        let ids = place(
            &mut ed,
            None,
            json!({"botanical_name": "Oak", "spread": 800.0}),
            1,
        );
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
            variation: None,
        };
        assert!(ed.execute(&cmd(Some(a), json!({}), vec![], None)).is_err());
        assert!(
            ed.execute(&cmd(
                Some(ResourceId::from_num(99)),
                json!({}),
                pts(1),
                None
            ))
            .is_err()
        );
        assert!(
            ed.execute(&cmd(Some(a), json!({"bogus": 1}), pts(1), None))
                .is_err()
        );
        assert!(
            ed.execute(&cmd(Some(a), json!({"spread": -1.0}), pts(1), None))
                .is_err()
        );
        assert!(
            ed.execute(&cmd(
                Some(a),
                json!({}),
                vec![DVec2::new(f64::NAN, 0.0)],
                None
            ))
            .is_err()
        );
        assert!(
            ed.execute(&cmd(Some(a), json!({}), pts(MAX_POINTS + 1), None))
                .is_err()
        );
        assert!(
            ed.execute(&cmd(
                Some(a),
                json!({}),
                pts(1),
                Some(LayerId::from_num(99))
            ))
            .is_err()
        );
        assert!(ed.doc().objects.is_empty());
        // Locked layer.
        let l = ed
            .execute(&Command::AddLayer {
                name: "L".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::ModifyLayer {
            id: LayerId(l),
            name: None,
            visible: None,
            locked: Some(true),
            elevation: None,
            scale: None,
        })
        .unwrap();
        assert!(
            ed.execute(&cmd(Some(a), json!({}), pts(1), Some(LayerId(l))))
                .is_err()
        );
    }

    #[test]
    fn set_plant_style_links_overrides_and_unlinks() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let b = add_style(&mut ed, "B", 2000.0);
        let ids = place(
            &mut ed,
            None,
            json!({"botanical_name": "Own", "spread": 900.0}),
            2,
        );
        let set =
            |ids: &[ObjectId], style, overrides: Option<Value>, unlink| Command::SetPlantStyle {
                ids: ids.to_vec(),
                style,
                overrides: overrides.map(|o| o.as_object().cloned().unwrap()),
                unlink,
            };
        ed.execute(&set(&ids, Some(a), None, false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 4000.0);
        ed.execute(&set(
            &ids[..1],
            None,
            Some(json!({"spread": 3000.0})),
            false,
        ))
        .unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 3000.0);
        assert_eq!(plant_fields(&ed, ids[1]).1, 4000.0);
        // Same style again keeps the overrides. Another style clears them.
        ed.execute(&set(&ids[..1], Some(a), None, false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 3000.0);
        ed.execute(&set(&ids[..1], Some(b), None, false)).unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 2000.0);
        // Empty overrides remove them.
        ed.execute(&set(&ids[..1], None, Some(json!({"spread": 100.0})), false))
            .unwrap();
        ed.execute(&set(&ids[..1], None, Some(json!({})), false))
            .unwrap();
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
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::ZERO,
                    radius: 5.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let before = ed.doc().clone();
        let c =
            |ids: Vec<ObjectId>, style, overrides: Option<Value>, unlink| Command::SetPlantStyle {
                ids,
                style,
                overrides: overrides.map(|o| o.as_object().cloned().unwrap()),
                unlink,
            };
        assert!(ed.execute(&c(ids.clone(), None, None, false)).is_err());
        assert!(ed.execute(&c(ids.clone(), Some(a), None, true)).is_err());
        assert!(
            ed.execute(&c(ids.clone(), Some(ResourceId::from_num(99)), None, false))
                .is_err()
        );
        assert!(ed.execute(&c(vec![line], Some(a), None, false)).is_err());
        assert!(
            ed.execute(&c(vec![ObjectId::from_num(999)], Some(a), None, false))
                .is_err()
        );
        assert!(
            ed.execute(&c(ids.clone(), None, Some(json!({"nope": 1})), false))
                .is_err()
        );
        assert!(
            ed.execute(&c(ids.clone(), None, Some(json!({"spread": 0})), false))
                .is_err()
        );
        // A plant without a style and no style given.
        let free = place(&mut ed, None, json!({}), 1);
        let snapshot = ed.doc().clone();
        assert!(
            ed.execute(&c(
                vec![ids[0], free[0]],
                None,
                Some(json!({"price": 1.0})),
                false
            ))
            .is_err()
        );
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
        ed.execute(&Command::ModifyObject {
            id: ids[0],
            shape: Some(shape),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        let l = link::link(ed.doc().object(ids[0]).unwrap()).unwrap();
        assert_eq!(l.overrides["spread"], 1234.0);
        assert!(!l.overrides.contains_key("height"));
        // A later style change keeps the override.
        let new = PlantStyle {
            spread: 9000.0,
            height: 100.0,
            ..PlantStyle::default()
        };
        ed.execute(&Command::ModifyResource {
            id: a,
            name: None,
            body: Some(ResourceBody::plant_style(new)),
        })
        .unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 1234.0);
        assert_eq!(plant_fields(&ed, ids[0]).2, 100.0);
    }

    #[test]
    fn set_data_cannot_touch_the_plant_link() {
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let ids = place(&mut ed, Some(a), json!({}), 1);
        let set = |k: &str| Command::SetData {
            id: ids[0],
            set: [(k.to_string(), json!({"style": 999}))].into(),
            remove: vec![],
        };
        assert!(
            matches!(ed.execute(&set("plant")), Err(CommandError::Invalid(m)) if m.contains("reserved"))
        );
        let rm = Command::SetData {
            id: ids[0],
            set: Default::default(),
            remove: vec!["plant".into()],
        };
        assert!(matches!(ed.execute(&rm), Err(CommandError::Invalid(_))));
        assert_eq!(
            link::link(ed.doc().object(ids[0]).unwrap()).unwrap().style,
            a
        );
        // Other keys still work.
        assert!(ed.execute(&set("note")).is_ok());
    }

    fn locked_style() -> (Editor, ResourceId) {
        let mut ed = Editor::default();
        let body = ResourceBody::plant_style(PlantStyle {
            locked: vec!["price".into(), "spread".into()],
            spread: 3000.0,
            price: 10.0,
            ..PlantStyle::default()
        });
        let id = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Locked".into(),
                body,
            })
            .unwrap()
            .created[0],
        );
        (ed, id)
    }

    #[test]
    fn locked_fields_cannot_be_overridden_by_any_command() {
        let (mut ed, id) = locked_style();
        let add = |o: Value| Command::AddPlants {
            style: Some(id),
            overrides: o.as_object().cloned().unwrap(),
            positions: pts(1),
            layer: None,
            class: None,
            variation: None,
        };
        assert!(
            matches!(ed.execute(&add(json!({"price": 1.0}))), Err(CommandError::Invalid(m)) if m.contains("locked"))
        );
        assert!(ed.execute(&add(json!({"spread": 1.0}))).is_err());
        assert!(ed.doc().objects.is_empty());
        let ids = place(&mut ed, Some(id), json!({"height": 123.0}), 1);
        let set = |o: Value| Command::SetPlantStyle {
            ids: ids.clone(),
            style: None,
            overrides: Some(o.as_object().cloned().unwrap()),
            unlink: false,
        };
        assert!(ed.execute(&set(json!({"price": 2.0}))).is_err());
        assert!(ed.execute(&set(json!({"height": 5.0}))).is_ok());
        // Locking a field later makes old overrides inert, and the plant follows the style.
        let mut body = ed.doc().resources.plant_style(id).unwrap().clone();
        body.locked.push("height".into());
        ed.execute(&Command::ModifyResource {
            id,
            name: None,
            body: Some(ResourceBody::plant_style(body)),
        })
        .unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).2, 4000.0);
        // A direct edit of a locked field is undone by the style.
        let mut shape = ed.doc().object(ids[0]).unwrap().shape.clone();
        if let Shape::Plant { spread, .. } = &mut shape {
            *spread = 99.0;
        }
        ed.execute(&Command::ModifyObject {
            id: ids[0],
            shape: Some(shape),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        assert_eq!(plant_fields(&ed, ids[0]).1, 3000.0);
        // An unknown lock name is refused when the style is saved.
        let mut bad = ed.doc().resources.plant_style(id).unwrap().clone();
        bad.locked.push("nope".into());
        assert!(
            ed.execute(&Command::ModifyResource {
                id,
                name: None,
                body: Some(ResourceBody::plant_style(bad))
            })
            .is_err()
        );
    }

    fn vary(
        ed: &mut Editor,
        style: Option<ResourceId>,
        v: crate::plants::variation::Variation,
        n: usize,
    ) -> Vec<ObjectId> {
        let cmd = Command::AddPlants {
            style,
            overrides: Default::default(),
            positions: pts(n),
            layer: None,
            class: None,
            variation: Some(v),
        };
        ed.execute(&cmd)
            .unwrap()
            .created
            .into_iter()
            .map(ObjectId)
            .collect()
    }

    #[test]
    fn variation_changes_size_and_rotation_deterministically() {
        use crate::plants::variation::Variation;
        let mut ed = Editor::default();
        let id = add_style(&mut ed, "A", 4000.0);
        let v = Variation {
            seed: 7,
            size: 0.3,
            rotation: true,
        };
        let ids = vary(&mut ed, Some(id), v, 20);
        let sizes: Vec<f64> = ids.iter().map(|i| plant_fields(&ed, *i).1).collect();
        assert!(
            sizes.iter().all(|s| (2800.0..=5200.0).contains(s))
                && sizes.windows(2).any(|w| w[0] != w[1])
        );
        let angles: Vec<f64> = ids
            .iter()
            .map(|i| link::rotation(ed.doc().object(*i).unwrap()))
            .collect();
        assert!(
            angles.iter().any(|a| *a > 0.0)
                && angles
                    .iter()
                    .all(|a| (0.0..std::f64::consts::TAU).contains(a))
        );
        // The same seed gives the same plants. A heights follow the same factor.
        let again = vary(&mut ed, Some(id), v, 20);
        assert_eq!(
            again
                .iter()
                .map(|i| plant_fields(&ed, *i))
                .collect::<Vec<_>>(),
            ids.iter()
                .map(|i| plant_fields(&ed, *i))
                .collect::<Vec<_>>()
        );
        let (s0, h0) = (plant_fields(&ed, ids[0]).1, plant_fields(&ed, ids[0]).2);
        assert!((s0 / 4000.0 - h0 / 4000.0).abs() < 1e-9);
        // The overrides explain the size of a styled plant, and the schedule still adds up.
        assert!(
            link::link(ed.doc().object(ids[0]).unwrap())
                .unwrap()
                .overrides
                .contains_key("spread")
        );
        // Without a style the size is in the shape only.
        let free = vary(&mut ed, None, Variation { size: 0.5, ..v }, 5);
        assert!(
            ed.doc()
                .object(free[0])
                .unwrap()
                .data
                .contains_key(link::ROTATION_KEY)
        );
        assert!(!ed.doc().object(free[0]).unwrap().data.contains_key("plant"));
        // Undo removes all of it in one step.
        ed.undo();
        ed.undo();
        ed.undo();
        assert!(ed.doc().objects.is_empty());
    }

    #[test]
    fn variation_respects_locked_sizes_and_checks_its_values() {
        use crate::plants::variation::Variation;
        let (mut ed, id) = locked_style();
        let ids = vary(
            &mut ed,
            Some(id),
            Variation {
                seed: 3,
                size: 0.4,
                rotation: false,
            },
            10,
        );
        assert!(
            ids.iter().all(|i| plant_fields(&ed, *i).1 == 3000.0),
            "spread is locked"
        );
        assert!(
            ids.iter().any(|i| plant_fields(&ed, *i).2 != 4000.0),
            "height varies"
        );
        assert!(ids.iter().all(|i| {
            !ed.doc()
                .object(*i)
                .unwrap()
                .data
                .contains_key(link::ROTATION_KEY)
        }));
        let bad = Command::AddPlants {
            style: None,
            overrides: Default::default(),
            positions: pts(1),
            layer: None,
            class: None,
            variation: Some(Variation {
                size: 2.0,
                ..Default::default()
            }),
        };
        assert!(ed.execute(&bad).is_err());
        // An inactive variation changes nothing.
        let same = vary(&mut ed, Some(id), Variation::default(), 2);
        assert_eq!(
            plant_fields(&ed, same[0]),
            (
                "Plant".to_string().replace("Plant", "Locked"),
                3000.0,
                4000.0
            )
        );
    }

    #[test]
    fn plant_tags_need_an_existing_style() {
        let mut ed = Editor::default();
        let tag = |style| Command::AddObject {
            shape: Shape::PlantTag {
                at: DVec2::ZERO,
                anchor: DVec2::new(1.0, 1.0),
                style,
                height: 400.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        assert_eq!(
            ed.execute(&tag(ResourceId::from_num(5))),
            Err(CommandError::NoResource(ResourceId::from_num(5)))
        );
        let a = add_style(&mut ed, "A", 4000.0);
        let id = ObjectId(ed.execute(&tag(a)).unwrap().created[0]);
        let moved = Shape::PlantTag {
            at: DVec2::ZERO,
            anchor: DVec2::ZERO,
            style: ResourceId::from_num(77),
            height: 400.0,
        };
        assert!(
            ed.execute(&Command::ModifyObject {
                id,
                shape: Some(moved),
                layer: None,
                class: None,
                attrs: None
            })
            .is_err()
        );
        let bad_height = Shape::PlantTag {
            at: DVec2::ZERO,
            anchor: DVec2::ZERO,
            style: a,
            height: 0.0,
        };
        assert!(
            ed.execute(&Command::ModifyObject {
                id,
                shape: Some(bad_height),
                layer: None,
                class: None,
                attrs: None
            })
            .is_err()
        );
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
                rpz_cap: None,
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
        let ids = place(
            &mut ed,
            Some(a),
            json!({"graphic": "star", "id_code": "ZZ"}),
            1,
        );
        let e = link::effective(ed.doc(), ed.doc().object(ids[0]).unwrap()).unwrap();
        assert_eq!(
            (e.style.graphic, e.style.id_code.as_str()),
            (PlantGraphic::Star, "ZZ")
        );
    }

    fn set_picture(
        ed: &mut Editor,
        style: ResourceId,
        p: Option<crate::picture::ImageResource>,
    ) -> Result<Outcome, CommandError> {
        ed.execute(&Command::SetPlantPicture {
            style,
            picture: p.map(Box::new),
        })
    }

    #[test]
    fn a_picture_is_stored_once_shared_and_removed_with_its_last_style() {
        use crate::picture::tests::sample;
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let b = add_style(&mut ed, "B", 4000.0);
        let before = ed.doc().resources.len();
        let out = set_picture(&mut ed, a, Some(sample(1))).unwrap();
        let img = ResourceId(out.created[0]);
        assert_eq!(ed.doc().resources.plant_style(a).unwrap().image, Some(img));
        assert_eq!(ed.doc().resources.len(), before + 1);
        // The same file for another style makes no new resource.
        let out = set_picture(&mut ed, b, Some(sample(1))).unwrap();
        assert!(out.created.is_empty());
        assert_eq!(ed.doc().resources.plant_style(b).unwrap().image, Some(img));
        assert_eq!(ed.doc().resources.find_image(&sample(1).sha256), Some(img));
        // Clearing one keeps the picture for the other, clearing the last removes it.
        set_picture(&mut ed, a, None).unwrap();
        assert!(ed.doc().resources.image(img).is_some());
        set_picture(&mut ed, b, None).unwrap();
        assert!(ed.doc().resources.image(img).is_none());
        assert_eq!(ed.doc().resources.len(), before);
        // Replacing a picture removes the old one. One undo step brings both back.
        set_picture(&mut ed, a, Some(sample(1))).unwrap();
        let depth = ed.undo_depth();
        set_picture(&mut ed, a, Some(sample(2))).unwrap();
        assert_eq!(ed.undo_depth(), depth + 1);
        assert!(ed.doc().resources.find_image(&sample(1).sha256).is_none());
        assert!(ed.undo());
        assert!(ed.doc().resources.find_image(&sample(1).sha256).is_some());
        assert!(ed.doc().resources.find_image(&sample(2).sha256).is_none());
    }

    #[test]
    fn picture_errors_change_nothing() {
        use crate::picture::tests::sample;
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let mut bad = sample(1);
        bad.sha256 = "0".repeat(64);
        assert!(matches!(
            set_picture(&mut ed, a, Some(bad)),
            Err(CommandError::Invalid(m)) if m.contains("hash")
        ));
        assert!(matches!(
            set_picture(&mut ed, ResourceId(crate::uid::Uid::from_u128(9)), None),
            Err(CommandError::NoResource(_))
        ));
        assert!(ed.doc().resources.plant_style(a).unwrap().image.is_none());
    }

    #[test]
    fn deleting_a_picture_clears_or_reassigns_the_styles() {
        use crate::picture::tests::sample;
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let one = ResourceId(set_picture(&mut ed, a, Some(sample(1))).unwrap().created[0]);
        let two = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Other".into(),
                body: ResourceBody::Image(Box::new(sample(2))),
            })
            .unwrap()
            .created[0],
        );
        for bad in [one, ResourceId(crate::uid::Uid::from_u128(9)), a] {
            assert!(
                ed.execute(&Command::DeleteResource {
                    id: one,
                    reassign: Some(bad)
                })
                .is_err()
            );
        }
        ed.execute(&Command::DeleteResource {
            id: one,
            reassign: Some(two),
        })
        .unwrap();
        assert_eq!(ed.doc().resources.plant_style(a).unwrap().image, Some(two));
        ed.execute(&Command::DeleteResource {
            id: two,
            reassign: None,
        })
        .unwrap();
        assert!(ed.doc().resources.plant_style(a).unwrap().image.is_none());
        // A style may refer to a picture resource through `image`, but not to a style.
        let three = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Third".into(),
                body: ResourceBody::Image(Box::new(sample(3))),
            })
            .unwrap()
            .created[0],
        );
        let mut st = ed.doc().resources.plant_style(a).unwrap().clone();
        st.image = Some(three);
        let modify = |st: PlantStyle| Command::ModifyResource {
            id: a,
            name: None,
            body: Some(ResourceBody::plant_style(st)),
        };
        assert!(ed.execute(&modify(st.clone())).is_ok());
        st.image = Some(a);
        assert!(matches!(
            ed.execute(&modify(st)),
            Err(CommandError::Invalid(m)) if m.contains("not an image")
        ));
    }

    #[test]
    fn pictures_obey_the_limits_of_the_editor_by_every_way_in() {
        use crate::picture::tests::sample;
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let size = sample(1).byte_len();
        ed.set_picture_limits(size - 1, u64::MAX);
        let set = Command::SetPlantPicture {
            style: a,
            picture: Some(Box::new(sample(1))),
        };
        let add = Command::AddResource {
            name: "I".into(),
            body: ResourceBody::Image(Box::new(sample(2))),
        };
        let modify = Command::ModifyResource {
            id: a,
            name: None,
            body: Some(ResourceBody::Image(Box::new(sample(3)))),
        };
        for c in [set.clone(), add.clone(), modify] {
            assert!(
                matches!(ed.execute(&c), Err(CommandError::Invalid(m)) if m.contains("larger"))
            );
            let batch = Command::Batch { commands: vec![c] };
            assert!(ed.execute(&batch).is_err());
        }
        ed.set_picture_limits(usize::MAX, 3);
        assert!(matches!(ed.execute(&set), Err(CommandError::Invalid(m)) if m.contains("pixels")));
        // The limits never go above the limits of the format, and a picture within them is fine.
        ed.set_picture_limits(usize::MAX, u64::MAX);
        assert!(ed.execute(&set).is_ok());
        assert!(ed.execute(&add).is_ok());
    }

    #[test]
    fn a_picture_whose_data_does_not_match_its_fields_is_refused() {
        use crate::picture::tests::sample;
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let mut bad = sample(1);
        bad.sha256 = "0".repeat(64);
        let add = Command::AddResource {
            name: "I".into(),
            body: ResourceBody::Image(Box::new(bad.clone())),
        };
        assert!(matches!(ed.execute(&add), Err(CommandError::Invalid(m)) if m.contains("hash")));
        let set = Command::SetPlantPicture {
            style: a,
            picture: Some(Box::new(bad)),
        };
        assert!(ed.execute(&set).is_err());
        let mut wrong_size = sample(2);
        wrong_size.width = 7;
        let set = Command::SetPlantPicture {
            style: a,
            picture: Some(Box::new(wrong_size)),
        };
        assert!(matches!(ed.execute(&set), Err(CommandError::Invalid(m)) if m.contains("size")));
        // A good picture is added, and a change of its name does not read the file again.
        let good = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Good".into(),
                body: ResourceBody::Image(Box::new(sample(3))),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::ModifyResource {
            id: good,
            name: Some("Renamed".into()),
            body: None,
        })
        .unwrap();
        let body = ed.doc().resources.get(good).unwrap().body.clone();
        ed.execute(&Command::ModifyResource {
            id: good,
            name: None,
            body: Some(body),
        })
        .ok();
    }

    #[test]
    fn the_history_shares_the_picture_text_and_counts_it_as_a_small_cost() {
        use crate::picture::tests::sample;
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        ed.execute(&Command::SetPlantPicture {
            style: a,
            picture: Some(Box::new(sample(1))),
        })
        .unwrap();
        let img = ed.doc().resources.find_image(&sample(1).sha256).unwrap();
        let before = ed.doc().resources.image(img).unwrap().data.clone();
        // Edits of other resources keep the same text in the document, in the undo steps and in the log.
        for n in 0..5 {
            ed.execute(&Command::ModifyResource {
                id: a,
                name: Some(format!("Style {n}")),
                body: None,
            })
            .unwrap();
        }
        let after = &ed.doc().resources.image(img).unwrap().data;
        assert!(std::sync::Arc::ptr_eq(before.as_arc(), after.as_arc()));
        assert!(ed.undo());
        let back = &ed.doc().resources.image(img).unwrap().data;
        assert!(std::sync::Arc::ptr_eq(before.as_arc(), back.as_arc()));
        // A big picture adds a fixed cost to the history, not its text.
        let big = {
            let mut p = sample(9);
            p.data = "A".repeat(4 * 1024 * 1024).into();
            p
        };
        let mut doc = ed.doc().clone();
        let big_id = ResourceId(doc.alloc_id());
        doc.resources.push(Resource {
            id: big_id,
            name: "Big".into(),
            body: ResourceBody::Image(Box::new(big)),
        });
        let mut ed2 = Editor::new(doc);
        ed2.set_history_limit(64 * 1024);
        for n in 0..4 {
            ed2.execute(&Command::ModifyResource {
                id: a,
                name: Some(format!("S{n}")),
                body: None,
            })
            .unwrap();
        }
        assert!(ed2.undo_depth() >= 4, "{}", ed2.undo_depth());
    }

    #[test]
    fn a_new_picture_name_never_clashes_and_a_deleted_picture_resyncs_its_styles() {
        use crate::picture::tests::sample;
        let mut ed = Editor::default();
        let a = add_style(&mut ed, "A", 4000.0);
        let p = sample(1);
        // A resource that already has the default name, with other content.
        ed.execute(&Command::AddResource {
            name: p.default_name(),
            body: ResourceBody::Image(Box::new(sample(2))),
        })
        .unwrap();
        ed.execute(&Command::AddResource {
            name: format!("Image {}", &p.sha256[..16]),
            body: ResourceBody::Image(Box::new(sample(3))),
        })
        .unwrap();
        let out = set_picture(&mut ed, a, Some(p)).unwrap();
        let name = &ed
            .doc()
            .resources
            .get(ResourceId(out.created[0]))
            .unwrap()
            .name;
        assert!(name.ends_with("(2)"), "{name}");
        ed.doc().clone().validate().unwrap();
        ed.execute(&Command::DeleteResource {
            id: ResourceId(out.created[0]),
            reassign: None,
        })
        .unwrap();
        assert!(ed.doc().resources.plant_style(a).unwrap().image.is_none());
    }
}
