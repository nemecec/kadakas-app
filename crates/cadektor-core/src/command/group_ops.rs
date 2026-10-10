//! Commands for groups, copies, rotation and symbols. They run inside `Tx` like all others.

use std::collections::HashMap;

use super::*;
use crate::graphics::{GraphicResource, SymbolDef, SymbolItem, allowed_in_symbol};
use crate::symbols::{EDIT_GROUP_KEY, EDITING_KEY};

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

/// Longest name of a group.
const MAX_NAME: usize = 200;

/// `shape` turned about `center` by `angle` radians, counterclockwise.
/// Shapes with data of their own that would have to turn too (hardscapes, landscape areas, site
/// models) and paper space shapes cannot be turned.
pub fn rotate_shape(shape: &Shape, center: DVec2, angle: f64) -> Result<Shape, String> {
    let r = DVec2::from_angle(angle);
    let p = |q: DVec2| center + r.rotate(q - center);
    let corners =
        |lo: DVec2, hi: DVec2| vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    Ok(match shape {
        Shape::Line { a, b } => Shape::Line { a: p(*a), b: p(*b) },
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            Shape::Polyline {
                points: corners(lo, hi)
                    .into_iter()
                    .map(|q| Vertex::corner(p(q)))
                    .collect(),
                closed: true,
            }
        }
        Shape::Circle { center: c, radius } => Shape::Circle {
            center: p(*c),
            radius: *radius,
        },
        Shape::Arc {
            center: c,
            radius,
            start,
            end,
        } => {
            let sweep = arc_sweep(*start, *end);
            let ns = start + angle;
            Shape::Arc {
                center: p(*c),
                radius: *radius,
                start: ns,
                end: if sweep >= std::f64::consts::TAU {
                    ns + std::f64::consts::TAU
                } else {
                    end + angle
                },
            }
        }
        Shape::Polyline { points, closed } => Shape::Polyline {
            points: points
                .iter()
                .map(|v| Vertex {
                    p: p(v.p),
                    kind: v.kind,
                })
                .collect(),
            closed: *closed,
        },
        Shape::Text { at, text, height } => Shape::Text {
            at: p(*at),
            text: text.clone(),
            height: *height,
        },
        Shape::Plant {
            at,
            species,
            spread,
            height,
        } => Shape::Plant {
            at: p(*at),
            species: species.clone(),
            spread: *spread,
            height: *height,
        },
        Shape::Locus3d { p: q } => {
            let m = p(q.truncate());
            Shape::Locus3d {
                p: glam::DVec3::new(m.x, m.y, q.z),
            }
        }
        Shape::Mesh3d(m) => {
            let mut m = (**m).clone();
            for q in &mut m.vertices {
                let t = p(q.truncate());
                q.x = t.x;
                q.y = t.y;
            }
            Shape::Mesh3d(Box::new(m))
        }
        Shape::Polyline3d { points, closed } => Shape::Polyline3d {
            points: points
                .iter()
                .map(|q| {
                    let m = p(q.truncate());
                    glam::DVec3::new(m.x, m.y, q.z)
                })
                .collect(),
            closed: *closed,
        },
        Shape::Symbol {
            symbol,
            at,
            rotation,
            scale,
            scale_y,
        } => Shape::Symbol {
            symbol: *symbol,
            at: p(*at),
            rotation: rotation + angle,
            scale: *scale,
            scale_y: *scale_y,
        },
        Shape::Group { name } => Shape::Group { name: name.clone() },
        Shape::Irrigation(i) => {
            let mut i = i.clone();
            i.rotate(center, angle);
            Shape::Irrigation(i)
        }
        Shape::ExistingTree {
            at,
            species,
            dbh,
            spread,
            height,
            condition,
            action,
            rpz_factor,
            rpz_cap,
        } => Shape::ExistingTree {
            at: p(*at),
            species: species.clone(),
            dbh: *dbh,
            spread: *spread,
            height: *height,
            condition: *condition,
            action: *action,
            rpz_factor: *rpz_factor,
            rpz_cap: *rpz_cap,
        },
        Shape::PlantTag {
            at,
            anchor,
            style,
            height,
        } => Shape::PlantTag {
            at: p(*at),
            anchor: p(*anchor),
            style: *style,
            height: *height,
        },
        Shape::Massing(m) => {
            let mut turned = (**m).clone();
            for v in &mut turned.footprint {
                v.p = p(v.p);
            }
            turned.ridge_angle = turned.ridge_angle.map(|a| a + angle);
            Shape::Massing(Box::new(turned))
        }
        other => return Err(format!("a {} cannot be turned", other.kind())),
    })
}

impl Tx<'_> {
    /// The objects and the members of the groups among them. A group that loses its last member
    /// goes too. In z-order.
    pub(super) fn with_group_members(&self, ids: &[ObjectId]) -> Vec<ObjectId> {
        let mut set: HashSet<ObjectId> = self.doc.with_members(ids).into_iter().collect();
        loop {
            let mut grew = false;
            for id in set.clone() {
                let Some(parent) = self.doc.object(id).and_then(|o| o.parent) else {
                    continue;
                };
                if !set.contains(&parent)
                    && !self.is_edit_group(parent)
                    && self.doc.children(parent).iter().all(|c| set.contains(c))
                {
                    set.insert(parent);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        self.doc
            .objects
            .keys()
            .copied()
            .filter(|id| set.contains(id))
            .collect()
    }

    /// True if the group holds the components of a symbol that is being edited. It stays when it is empty.
    fn is_edit_group(&self, id: ObjectId) -> bool {
        self.doc
            .object(id)
            .is_some_and(|g| g.data.contains_key(EDIT_GROUP_KEY))
    }

    /// When an edit group is deleted, the symbol instance it hid shows again.
    pub(super) fn release_edited(&mut self, ids: &[ObjectId]) {
        for &id in ids {
            let instance = self
                .doc
                .object(id)
                .and_then(|g| g.data.get(EDIT_GROUP_KEY))
                .and_then(|v| v.get("instance"))
                .and_then(Value::as_u64)
                .map(ObjectId);
            if let Some(i) = instance.filter(|i| {
                self.doc
                    .object(*i)
                    .is_some_and(|o| o.data.contains_key(EDITING_KEY))
            }) {
                self.touch(i);
                self.doc
                    .object_mut(i)
                    .expect("exists")
                    .data
                    .remove(EDITING_KEY);
            }
        }
    }

    /// The given objects, with each group replaced by its members at any depth. All must be editable.
    fn editable_leaves(&self, ids: &[ObjectId]) -> Result<Vec<ObjectId>, CommandError> {
        let ids = unique(ids);
        for &id in &ids {
            self.editable(id)?;
        }
        let ids = if ids.iter().any(|i| self.doc.is_group(*i)) {
            self.doc.leaves(&ids)
        } else {
            ids
        };
        for &id in &ids {
            self.editable(id)?;
        }
        Ok(ids)
    }

    /// Objects of the same kind of layer rules: adds a new object made by this command.
    fn add_new(&mut self, o: Object) {
        let id = o.id;
        self.added.push(id);
        self.added_set.insert(id);
        self.doc.objects.insert(id, o);
    }

    pub(super) fn run_group(
        &mut self,
        cmd: &Command,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        match cmd {
            Command::Group { ids, name } => {
                let ids = unique(ids);
                let Some(&first) = ids.first() else {
                    return Err(invalid("give at least one object to group"));
                };
                for &id in &ids {
                    self.editable(id)?;
                }
                let name = name.clone().unwrap_or_default();
                if name.chars().count() > MAX_NAME {
                    return Err(invalid(format!(
                        "a group name has at most {MAX_NAME} characters"
                    )));
                }
                let f = self.doc.object(first).expect("checked");
                let (layer, class, parent) = (f.layer, f.class, f.parent);
                for &id in &ids {
                    let o = self.doc.object(id).expect("checked");
                    if o.layer != layer {
                        return Err(invalid("the objects of a group must be on one layer"));
                    }
                    if o.parent != parent {
                        return Err(invalid(
                            "the objects are in different groups. Group objects that are in the same group",
                        ));
                    }
                }
                let gid = ObjectId(self.alloc()?);
                let mut g = Object::new(gid, layer, class, Shape::Group { name });
                g.parent = parent;
                self.add_new(g);
                for id in ids {
                    self.touch(id);
                    self.doc.object_mut(id).expect("checked").parent = Some(gid);
                }
                out.created.push(gid.0);
            }
            Command::Ungroup { ids } => {
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                    if !self.doc.is_group(id) {
                        return Err(invalid(format!("object {id} is not a group")));
                    }
                }
                for &g in &ids {
                    let parent = self.doc.object(g).expect("checked").parent;
                    for c in self.doc.children(g) {
                        self.touch(c);
                        self.doc.object_mut(c).expect("exists").parent = parent;
                    }
                }
                self.remove_objects(ids);
            }
            Command::RotateObjects { ids, center, angle } => {
                if !center.is_finite() || !angle.is_finite() {
                    return Err(invalid(super::NOT_FINITE));
                }
                for id in self.editable_leaves(ids)? {
                    self.touch(id);
                    let o = self.doc.object_mut(id).expect("checked");
                    o.shape =
                        rotate_shape(&o.shape, *center, *angle).map_err(CommandError::Invalid)?;
                    if matches!(o.shape, Shape::Plant { .. }) {
                        let r = crate::plants::link::rotation(o) + angle;
                        o.data
                            .insert(crate::plants::link::ROTATION_KEY.into(), r.into());
                    }
                }
            }
            Command::CopyObjects { ids, delta } => {
                if !delta.is_finite() {
                    return Err(invalid(super::NOT_FINITE));
                }
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                }
                let all = self.doc.with_members(&ids);
                for &id in &all {
                    let layer = self.doc.object(id).expect("exists").layer;
                    self.check_layer(layer)?;
                }
                let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
                for &old in &all {
                    map.insert(old, ObjectId(self.alloc()?));
                }
                for &old in &all {
                    let mut o = self.doc.object(old).expect("exists").clone();
                    o.id = map[&old];
                    o.parent = o.parent.map(|p| map.get(&p).copied().unwrap_or(p));
                    o.shape.translate(*delta);
                    crate::hardscape::translate_object_data(&mut o, *delta);
                    o.data.remove(EDITING_KEY);
                    o.data.remove(EDIT_GROUP_KEY);
                    self.check_sheet_rules(o.layer, &o.shape, None)?;
                    self.add_new(o);
                }
                self.present_copy_slots(&map);
                out.created.extend(ids.iter().map(|id| map[id].0));
                out.created.extend(
                    all.iter()
                        .filter(|id| !ids.contains(id))
                        .map(|id| map[id].0),
                );
            }
            Command::AddObjectIn {
                parent,
                shape,
                class,
                attrs,
            } => {
                self.editable(*parent)?;
                let g = self.doc.object(*parent).expect("checked");
                if !matches!(g.shape, Shape::Group { .. }) {
                    return Err(invalid(format!("object {parent} is not a group")));
                }
                let layer = g.layer;
                self.run(
                    &Command::AddObject {
                        shape: shape.clone(),
                        layer: Some(layer),
                        class: *class,
                        attrs: attrs.clone(),
                    },
                    out,
                )?;
                let id = ObjectId(*out.created.last().expect("added"));
                self.doc.object_mut(id).expect("added").parent = Some(*parent);
            }
            Command::SetParent { ids, parent } => {
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                }
                if ids.is_empty() {
                    return Err(invalid("give at least one object"));
                }
                if let Some(p) = parent {
                    self.editable(*p)?;
                    if !self.doc.is_group(*p) {
                        return Err(invalid(format!("object {p} is not a group")));
                    }
                    if self.doc.with_members(&ids).contains(p) {
                        return Err(invalid("a group cannot go into itself"));
                    }
                    let layer = self.doc.object(*p).expect("checked").layer;
                    if ids
                        .iter()
                        .any(|i| self.doc.object(*i).expect("checked").layer != layer)
                    {
                        return Err(invalid("the objects must be on the layer of the group"));
                    }
                }
                let old: Vec<ObjectId> = ids
                    .iter()
                    .filter_map(|i| self.doc.object(*i).and_then(|o| o.parent))
                    .collect();
                for &id in &ids {
                    self.touch(id);
                    self.doc.object_mut(id).expect("checked").parent = *parent;
                }
                // A group that lost its last member goes, up the tree. An edit group stays.
                let mut empty: Vec<ObjectId> = Vec::new();
                let mut todo = old;
                while let Some(g) = todo.pop() {
                    if !empty.contains(&g)
                        && !self.is_edit_group(g)
                        && self.doc.children(g).is_empty()
                    {
                        empty.push(g);
                        todo.extend(self.doc.object(g).and_then(|o| o.parent));
                    }
                }
                if !empty.is_empty() {
                    for g in &empty {
                        self.editable(*g)?;
                    }
                    self.remove_objects(empty);
                }
            }
            Command::CreateSymbol {
                name,
                ids,
                insertion,
                replace,
            } => self.create_symbol(name, ids, *insertion, *replace, out)?,
            Command::ConvertSymbolToGroup { ids } => {
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                    if !matches!(
                        self.doc.object(id).expect("checked").shape,
                        Shape::Symbol { .. }
                    ) {
                        return Err(invalid(format!("object {id} is not a symbol instance")));
                    }
                }
                for id in ids {
                    let inst = self.doc.object(id).expect("checked").clone();
                    let parts = crate::symbols::instance_objects(self.doc, &inst);
                    if parts.is_empty() {
                        return Err(invalid(format!(
                            "object {id} has no 2D components to turn into a group"
                        )));
                    }
                    let Shape::Symbol { symbol, .. } = inst.shape else {
                        unreachable!("checked")
                    };
                    let name = self
                        .doc
                        .resources
                        .get(symbol)
                        .map(|r| r.name.clone())
                        .unwrap_or_default();
                    let gid = ObjectId(self.alloc()?);
                    let mut g = Object::new(gid, inst.layer, inst.class, Shape::Group { name });
                    g.parent = inst.parent;
                    self.add_new(g);
                    for part in parts {
                        validate_shape(&part.shape)?;
                        self.check_sheet_rules(inst.layer, &part.shape, None)?;
                        let mut o = Object::new(
                            ObjectId(self.alloc()?),
                            inst.layer,
                            part.class,
                            part.shape,
                        );
                        o.attrs = part.attrs;
                        o.parent = Some(gid);
                        self.add_new(o);
                    }
                    self.remove_objects(vec![id]);
                    out.created.push(gid.0);
                }
            }
            Command::EditSymbol { id } => self.edit_symbol(*id, out)?,
            Command::ExitSymbolEdit { group } => self.exit_symbol_edit(*group)?,
            _ => unreachable!("run routes only group commands here"),
        }
        Ok(())
    }

    /// Makes a symbol definition from objects (groups count as their members).
    fn create_symbol(
        &mut self,
        name: &str,
        ids: &[ObjectId],
        insertion: Option<DVec2>,
        replace: bool,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        let ids = unique(ids);
        if ids.is_empty() {
            return Err(invalid("give the objects that make the symbol"));
        }
        for &id in &ids {
            self.editable(id)?;
        }
        let members = self.doc.leaves(&ids);
        if members.is_empty() {
            return Err(invalid("a symbol needs at least one object"));
        }
        let first = self.doc.object(members[0]).expect("exists");
        let (layer, parent) = (first.layer, first.parent);
        let mut bounds = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for &m in &members {
            let o = self.doc.object(m).expect("exists");
            if !allowed_in_symbol(&o.shape) {
                return Err(invalid(format!(
                    "a {} cannot be part of a symbol. Use lines, rects, circles, arcs, polylines, text and symbols",
                    o.shape.kind()
                )));
            }
            let b = self.doc.object_bounds(o);
            bounds = (bounds.0.min(b.0), bounds.1.max(b.1));
        }
        let at = insertion.unwrap_or((bounds.0 + bounds.1) / 2.0);
        if !at.is_finite() {
            return Err(invalid(super::NOT_FINITE));
        }
        let items = members
            .iter()
            .map(|&m| {
                let o = self.doc.object(m).expect("exists");
                let mut shape = o.shape.clone();
                shape.translate(-at);
                SymbolItem {
                    shape,
                    attrs: o.attrs.clone(),
                    class: (o.class != NONE_CLASS).then_some(o.class),
                }
            })
            .collect();
        let def = SymbolDef {
            insertion: DVec2::ZERO,
            items,
            ..Default::default()
        };
        self.run(
            &Command::AddResource {
                name: name.to_string(),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
            },
            out,
        )?;
        let symbol = ResourceId(*out.created.last().expect("added"));
        if replace {
            self.run(&Command::DeleteObjects { ids: ids.clone() }, out)?;
            let shape = Shape::Symbol {
                symbol,
                at,
                rotation: 0.0,
                scale: 1.0,
                scale_y: None,
            };
            self.run(
                &Command::AddObject {
                    shape,
                    layer: Some(layer),
                    class: Some(NONE_CLASS),
                    attrs: Attributes::default(),
                },
                out,
            )?;
            let inst = ObjectId(*out.created.last().expect("added"));
            self.doc.object_mut(inst).expect("added").parent =
                parent.filter(|p| self.doc.object(*p).is_some());
        }
        Ok(())
    }

    /// Edit a symbol in place: its 2D components become a group of objects at the instance.
    fn edit_symbol(&mut self, id: ObjectId, out: &mut Outcome) -> Result<(), CommandError> {
        self.editable(id)?;
        let inst = self.doc.object(id).expect("checked").clone();
        let Shape::Symbol { symbol, at, .. } = inst.shape else {
            return Err(invalid(format!("object {id} is not a symbol instance")));
        };
        if crate::symbols::is_being_edited(&inst) {
            return Err(invalid("this symbol is being edited already"));
        }
        let (name, def) = match self.doc.resources.get(symbol) {
            Some(r) => match &r.body {
                ResourceBody::Graphic(g) => match g.as_ref() {
                    GraphicResource::Symbol(d) => (r.name.clone(), d.clone()),
                    _ => return Err(invalid("the resource is not a symbol")),
                },
                _ => return Err(invalid("the resource is not a symbol")),
            },
            None => return Err(CommandError::NoResource(symbol)),
        };
        if self.doc.objects.values().any(|o| {
            o.data
                .get(EDIT_GROUP_KEY)
                .and_then(|v| v.get("symbol"))
                .and_then(Value::as_u64)
                == Some(symbol.0)
        }) {
            return Err(invalid("this symbol is being edited already"));
        }
        // The components keep their definition coordinates, moved so that the insertion point sits at the instance.
        let offset = at - def.insertion;
        let gid = ObjectId(self.alloc()?);
        let mut g = Object::new(
            gid,
            inst.layer,
            inst.class,
            Shape::Group {
                name: format!("Edit {name}"),
            },
        );
        g.parent = inst.parent;
        g.data.insert(EDIT_GROUP_KEY.into(), serde_json::json!({"symbol": symbol.0, "instance": id.0, "offset": [offset.x, offset.y]}));
        self.add_new(g);
        for it in &def.items {
            let mut shape = it.shape.clone();
            shape.translate(offset);
            validate_shape(&shape)?;
            self.check_sheet_rules(inst.layer, &shape, None)?;
            let mut o = Object::new(
                ObjectId(self.alloc()?),
                inst.layer,
                it.class.unwrap_or(inst.class),
                shape,
            );
            o.attrs = it.attrs.clone();
            o.parent = Some(gid);
            self.add_new(o);
        }
        self.touch(id);
        self.doc
            .object_mut(id)
            .expect("checked")
            .data
            .insert(EDITING_KEY.into(), true.into());
        out.created.push(gid.0);
        Ok(())
    }

    /// Ends editing in place: the objects of the edit group become the 2D components of the
    /// definition, and the group goes. All instances show the new definition.
    fn exit_symbol_edit(&mut self, group: ObjectId) -> Result<(), CommandError> {
        self.editable(group)?;
        let g = self.doc.object(group).expect("checked");
        let info =
            g.data.get(EDIT_GROUP_KEY).cloned().ok_or_else(|| {
                invalid(format!("object {group} is not the group of a symbol edit"))
            })?;
        let symbol = ResourceId(
            info.get("symbol")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("the edit group is damaged"))?,
        );
        let instance = info.get("instance").and_then(Value::as_u64).map(ObjectId);
        let off = info
            .get("offset")
            .and_then(Value::as_array)
            .map(|a| {
                DVec2::new(
                    a.first().and_then(Value::as_f64).unwrap_or(0.0),
                    a.get(1).and_then(Value::as_f64).unwrap_or(0.0),
                )
            })
            .unwrap_or(DVec2::ZERO);
        let inst_class = instance.and_then(|i| self.doc.object(i)).map(|o| o.class);
        let old = self
            .doc
            .resources
            .symbol(symbol)
            .ok_or(CommandError::NoResource(symbol))?
            .clone();
        let members = self.doc.leaves(&[group]);
        let mut items = Vec::with_capacity(members.len());
        for &m in &members {
            self.editable(m)?;
            let o = self.doc.object(m).expect("exists");
            if !allowed_in_symbol(&o.shape) {
                return Err(invalid(format!(
                    "a {} cannot be part of a symbol",
                    o.shape.kind()
                )));
            }
            let mut shape = o.shape.clone();
            shape.translate(-off);
            let class = (Some(o.class) != inst_class && o.class != NONE_CLASS).then_some(o.class);
            items.push(SymbolItem {
                shape,
                attrs: o.attrs.clone(),
                class,
            });
        }
        let def = SymbolDef { items, ..old };
        let body = ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def)));
        body.validate().map_err(invalid)?;
        self.check_graphic_refs(Some(symbol), &body)?;
        self.doc.resources.get_mut(symbol).expect("checked").body = body;
        let mut all = self.doc.with_members(&[group]);
        all.retain(|id| self.doc.object(*id).is_some());
        self.remove_objects(all);
        if let Some(i) = instance.filter(|i| self.doc.object(*i).is_some()) {
            self.touch(i);
            self.doc
                .object_mut(i)
                .expect("exists")
                .data
                .remove(EDITING_KEY);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

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

    fn circle(ed: &mut Editor, x: f64) -> ObjectId {
        add(
            ed,
            Shape::Circle {
                center: v(x, 0.0),
                radius: 1.0,
            },
        )
    }

    fn group(ed: &mut Editor, ids: &[ObjectId]) -> ObjectId {
        ObjectId(
            ed.execute(&Command::Group {
                ids: ids.to_vec(),
                name: Some("g".into()),
            })
            .unwrap()
            .created[0],
        )
    }

    /// The document without its ID counter, which undo does not take back.
    fn norm(d: &Document) -> Document {
        let mut c = d.clone();
        c.set_next_id(0);
        c
    }

    fn center(ed: &Editor, id: ObjectId) -> DVec2 {
        let Shape::Circle { center, .. } = ed.doc().object(id).unwrap().shape else {
            panic!()
        };
        center
    }

    #[test]
    fn grouping_and_ungrouping_undo() {
        let mut ed = Editor::default();
        let (a, b, c) = (
            circle(&mut ed, 0.0),
            circle(&mut ed, 5.0),
            circle(&mut ed, 10.0),
        );
        let before = ed.doc().clone();
        let g = group(&mut ed, &[a, b]);
        assert_eq!(ed.doc().object(a).unwrap().parent, Some(g));
        assert_eq!(ed.doc().object(c).unwrap().parent, None);
        assert!(matches!(&ed.doc().object(g).unwrap().shape, Shape::Group { name } if name == "g"));
        assert!(ed.undo());
        assert_eq!(norm(ed.doc()), norm(&before));
        assert!(ed.redo());
        ed.execute(&Command::Ungroup { ids: vec![g] }).unwrap();
        assert!(ed.doc().object(g).is_none());
        assert_eq!(ed.doc().object(a).unwrap().parent, None);
        assert!(ed.undo());
        assert_eq!(ed.doc().object(a).unwrap().parent, Some(g));
    }

    #[test]
    fn nested_groups_undo_and_ungroup_one_level() {
        let mut ed = Editor::default();
        let (a, b, c) = (
            circle(&mut ed, 0.0),
            circle(&mut ed, 5.0),
            circle(&mut ed, 10.0),
        );
        let start = ed.doc().clone();
        let inner = group(&mut ed, &[a, b]);
        let after_inner = ed.doc().clone();
        let outer = group(&mut ed, &[inner, c]);
        assert_eq!(ed.doc().object(inner).unwrap().parent, Some(outer));
        assert_eq!(ed.doc().top_level(a), outer);
        // Move the outer group: all three circles move.
        ed.execute(&Command::MoveObjects {
            ids: vec![outer],
            delta: v(0.0, 7.0),
        })
        .unwrap();
        assert_eq!(center(&ed, a), v(0.0, 7.0));
        assert_eq!(center(&ed, c), v(10.0, 7.0));
        // Ungroup the outer one level: the inner group stays.
        ed.execute(&Command::Ungroup { ids: vec![outer] }).unwrap();
        assert_eq!(ed.doc().object(inner).unwrap().parent, None);
        assert_eq!(ed.doc().object(a).unwrap().parent, Some(inner));
        // Undo everything step by step.
        assert!(ed.undo() && ed.undo() && ed.undo());
        assert_eq!(norm(ed.doc()), norm(&after_inner));
        assert!(ed.undo());
        assert_eq!(norm(ed.doc()), norm(&start));
        while ed.redo() {}
        assert_eq!(ed.doc().object(a).unwrap().parent, Some(inner));
    }

    #[test]
    fn group_rules() {
        let mut ed = Editor::default();
        let (a, b) = (circle(&mut ed, 0.0), circle(&mut ed, 5.0));
        let err = |ed: &mut Editor, ids: Vec<ObjectId>| {
            ed.execute(&Command::Group { ids, name: None }).unwrap_err()
        };
        assert!(
            matches!(err(&mut ed, vec![]), CommandError::Invalid(m) if m.contains("at least one"))
        );
        assert_eq!(
            err(&mut ed, vec![ObjectId(999)]),
            CommandError::NoObject(ObjectId(999))
        );
        let layer = LayerId(
            ed.execute(&Command::AddLayer {
                name: "L2".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0],
        );
        let other = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: v(1.0, 1.0),
                    radius: 1.0,
                },
                layer: Some(layer),
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        assert!(
            matches!(err(&mut ed, vec![a, other]), CommandError::Invalid(m) if m.contains("one layer"))
        );
        let g = group(&mut ed, &[a]);
        assert!(matches!(
            err(&mut ed, vec![g, b, a]),
            CommandError::Invalid(_)
        ));
        assert!(
            matches!(err(&mut ed, vec![b, a]), CommandError::Invalid(m) if m.contains("different groups"))
        );
        let long = "x".repeat(MAX_NAME + 1);
        assert!(
            matches!(ed.execute(&Command::Group { ids: vec![b], name: Some(long) }), Err(CommandError::Invalid(m)) if m.contains("at most"))
        );
        assert!(
            matches!(ed.execute(&Command::Ungroup { ids: vec![b] }), Err(CommandError::Invalid(m)) if m.contains("not a group"))
        );
        // A locked layer refuses.
        ed.execute(&Command::ModifyLayer {
            id: layer,
            name: None,
            visible: None,
            locked: Some(true),
            elevation: None,
            scale: None,
        })
        .unwrap();
        assert_eq!(err(&mut ed, vec![other]), CommandError::LayerLocked(layer));
    }

    #[test]
    fn deleting_a_group_deletes_its_members_and_empty_groups() {
        let mut ed = Editor::default();
        let (a, b, c) = (
            circle(&mut ed, 0.0),
            circle(&mut ed, 5.0),
            circle(&mut ed, 10.0),
        );
        let inner = group(&mut ed, &[a, b]);
        let outer = group(&mut ed, &[inner, c]);
        let full = ed.doc().clone();
        ed.execute(&Command::DeleteObjects { ids: vec![outer] })
            .unwrap();
        assert!(ed.doc().objects.is_empty());
        assert!(ed.undo());
        assert_eq!(norm(ed.doc()), norm(&full));
        // Deleting every member removes the group that held them, up the tree.
        ed.execute(&Command::DeleteObjects { ids: vec![a, b] })
            .unwrap();
        assert!(ed.doc().object(inner).is_none());
        assert_eq!(ed.doc().objects.len(), 2, "c and the outer group remain");
        ed.execute(&Command::DeleteObjects { ids: vec![c] })
            .unwrap();
        assert!(ed.doc().objects.is_empty());
        while ed.undo() {}
        assert_eq!(norm(ed.doc()), norm(&Document::default()));
    }

    #[test]
    fn moving_and_copying_a_group_as_a_unit() {
        let mut ed = Editor::default();
        let (a, b) = (circle(&mut ed, 0.0), circle(&mut ed, 5.0));
        let g = group(&mut ed, &[a, b]);
        let out = ed
            .execute(&Command::CopyObjects {
                ids: vec![g],
                delta: v(100.0, 0.0),
            })
            .unwrap();
        let g2 = ObjectId(out.created[0]);
        assert_eq!(out.created.len(), 3, "the group, then its members");
        assert!(ed.doc().is_group(g2) && g2 != g);
        let kids = ed.doc().children(g2);
        assert_eq!(kids.len(), 2);
        assert_eq!(center(&ed, kids[0]), v(100.0, 0.0));
        assert_eq!(center(&ed, kids[1]), v(105.0, 0.0));
        // The originals did not move and still belong to the first group.
        assert_eq!(center(&ed, a), v(0.0, 0.0));
        assert_eq!(ed.doc().object(a).unwrap().parent, Some(g));
        assert!(ed.undo());
        assert_eq!(ed.doc().objects.len(), 3);
        // A single member copied stays in its group.
        let out = ed
            .execute(&Command::CopyObjects {
                ids: vec![a],
                delta: v(0.0, 50.0),
            })
            .unwrap();
        assert_eq!(
            ed.doc().object(ObjectId(out.created[0])).unwrap().parent,
            Some(g)
        );
        assert!(
            ed.execute(&Command::CopyObjects {
                ids: vec![a],
                delta: v(f64::NAN, 0.0)
            })
            .is_err()
        );
    }

    #[test]
    fn rotating_a_group_as_a_unit() {
        let mut ed = Editor::default();
        let (a, b) = (circle(&mut ed, 10.0), circle(&mut ed, 20.0));
        let rect = add(
            &mut ed,
            Shape::Rect {
                min: v(0.0, 0.0),
                max: v(2.0, 1.0),
            },
        );
        let g = group(&mut ed, &[a, b, rect]);
        let before = ed.doc().clone();
        ed.execute(&Command::RotateObjects {
            ids: vec![g],
            center: DVec2::ZERO,
            angle: FRAC_PI_2,
        })
        .unwrap();
        assert!(center(&ed, a).distance(v(0.0, 10.0)) < 1e-9);
        assert!(center(&ed, b).distance(v(0.0, 20.0)) < 1e-9);
        let Shape::Polyline {
            points,
            closed: true,
        } = &ed.doc().object(rect).unwrap().shape
        else {
            panic!()
        };
        assert!(points[2].p.distance(v(-1.0, 2.0)) < 1e-9);
        assert!(ed.undo());
        assert_eq!(norm(ed.doc()), norm(&before));
        assert!(
            ed.execute(&Command::RotateObjects {
                ids: vec![g],
                center: DVec2::ZERO,
                angle: f64::NAN
            })
            .is_err()
        );
        // Paper space and area shapes cannot be turned.
        let area = add(
            &mut ed,
            Shape::LandscapeArea {
                boundary: vec![
                    Vertex::corner(v(0.0, 0.0)),
                    Vertex::corner(v(5.0, 0.0)),
                    Vertex::corner(v(5.0, 5.0)),
                ],
                name: "a".into(),
            },
        );
        assert!(
            matches!(ed.execute(&Command::RotateObjects { ids: vec![area], center: DVec2::ZERO, angle: 1.0 }), Err(CommandError::Invalid(m)) if m.contains("cannot be turned"))
        );
    }

    #[test]
    fn every_basic_shape_turns() {
        let c = v(1.0, 1.0);
        let shapes = vec![
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(2.0, 0.0),
            },
            Shape::Circle {
                center: v(3.0, 1.0),
                radius: 1.0,
            },
            Shape::Arc {
                center: v(3.0, 1.0),
                radius: 1.0,
                start: 0.0,
                end: 1.0,
            },
            Shape::Arc {
                center: v(3.0, 1.0),
                radius: 1.0,
                start: 0.5,
                end: 0.5 + std::f64::consts::TAU,
            },
            Shape::Text {
                at: v(0.0, 0.0),
                text: "t".into(),
                height: 1.0,
            },
            Shape::Plant {
                at: v(0.0, 0.0),
                species: "p".into(),
                spread: 1.0,
                height: 1.0,
            },
            Shape::Locus3d {
                p: glam::DVec3::new(0.0, 0.0, 5.0),
            },
            Shape::Polyline3d {
                points: vec![
                    glam::DVec3::new(0.0, 0.0, 1.0),
                    glam::DVec3::new(1.0, 0.0, 2.0),
                ],
                closed: false,
            },
            Shape::Symbol {
                symbol: ResourceId(1),
                at: v(0.0, 0.0),
                rotation: 0.25,
                scale: 1.0,
                scale_y: Some(2.0),
            },
            Shape::Group { name: "g".into() },
            Shape::PlantTag {
                at: v(0.0, 0.0),
                anchor: v(1.0, 1.0),
                style: ResourceId(1),
                height: 1.0,
            },
            Shape::ExistingTree {
                at: v(0.0, 0.0),
                species: String::new(),
                dbh: 1.0,
                spread: 1.0,
                height: 1.0,
                condition: Default::default(),
                action: Default::default(),
                rpz_factor: None,
                rpz_cap: None,
            },
        ];
        for s in shapes {
            let r = rotate_shape(&s, c, 1.0).unwrap();
            assert_eq!(r.kind(), s.kind());
            // Turning back gives the shape again.
            let back = rotate_shape(&r, c, -1.0).unwrap();
            let (a, b) = (s.bounds(), back.bounds());
            if a.0.is_finite() {
                assert!(
                    a.0.distance(b.0) < 1e-9 && a.1.distance(b.1) < 1e-9,
                    "{}",
                    s.kind()
                );
            }
        }
        let tag = Shape::PlantTag {
            at: v(0.0, 0.0),
            anchor: v(1.0, 1.0),
            style: ResourceId(1),
            height: 1.0,
        };
        assert_eq!(rotate_shape(&tag, c, 0.0).unwrap(), tag);
        assert!(
            rotate_shape(
                &Shape::Hardscape {
                    boundary: vec![],
                    material: String::new(),
                    thickness: None,
                    elevation: None
                },
                c,
                1.0
            )
            .is_err()
        );
        // Turning a plant adds to its rotation.
        let mut ed = Editor::default();
        let p = add(
            &mut ed,
            Shape::Plant {
                at: v(1.0, 0.0),
                species: "x".into(),
                spread: 1.0,
                height: 1.0,
            },
        );
        ed.execute(&Command::RotateObjects {
            ids: vec![p],
            center: DVec2::ZERO,
            angle: 0.5,
        })
        .unwrap();
        ed.execute(&Command::RotateObjects {
            ids: vec![p],
            center: DVec2::ZERO,
            angle: 0.25,
        })
        .unwrap();
        assert!((crate::plants::link::rotation(ed.doc().object(p).unwrap()) - 0.75).abs() < 1e-12);
    }

    fn symbol_from(ed: &mut Editor, ids: &[ObjectId], replace: bool) -> Vec<u64> {
        ed.execute(&Command::CreateSymbol {
            name: "Tree".into(),
            ids: ids.to_vec(),
            insertion: None,
            replace,
        })
        .unwrap()
        .created
    }

    #[test]
    fn creating_a_symbol_from_objects() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Circle {
                center: v(100.0, 100.0),
                radius: 10.0,
            },
        );
        let b = add(
            &mut ed,
            Shape::Line {
                a: v(90.0, 100.0),
                b: v(110.0, 100.0),
            },
        );
        let g = group(&mut ed, &[a, b]);
        let before = ed.doc().object_bounds(ed.doc().object(g).unwrap());
        let created = symbol_from(&mut ed, &[g], true);
        assert_eq!(created.len(), 2);
        let (sym, inst) = (ResourceId(created[0]), ObjectId(created[1]));
        let def = ed.doc().resources.symbol(sym).unwrap();
        assert_eq!(def.items.len(), 2);
        assert_eq!(def.insertion, DVec2::ZERO);
        // The originals are gone and the instance looks the same.
        assert!(ed.doc().object(a).is_none() && ed.doc().object(g).is_none());
        assert_eq!(ed.doc().objects.len(), 1);
        let after = ed.doc().object_bounds(ed.doc().object(inst).unwrap());
        assert!(
            before.0.distance(after.0) < 1e-9 && before.1.distance(after.1) < 1e-9,
            "{before:?} {after:?}"
        );
        assert!(ed.undo());
        assert!(ed.doc().resources.is_empty() && ed.doc().object(a).is_some());
        // Without replace the objects stay.
        let created = ed
            .execute(&Command::CreateSymbol {
                name: "Tree".into(),
                ids: vec![a],
                insertion: Some(v(100.0, 100.0)),
                replace: false,
            })
            .unwrap()
            .created;
        assert_eq!(created.len(), 1);
        assert!(ed.doc().object(a).is_some());
        // Errors.
        let bad = |ed: &mut Editor, ids: Vec<ObjectId>| {
            ed.execute(&Command::CreateSymbol {
                name: "S".into(),
                ids,
                insertion: None,
                replace: false,
            })
            .unwrap_err()
        };
        assert!(matches!(bad(&mut ed, vec![]), CommandError::Invalid(_)));
        let tag = add(
            &mut ed,
            Shape::Locus3d {
                p: glam::DVec3::new(0.0, 0.0, 1.0),
            },
        );
        assert!(
            matches!(bad(&mut ed, vec![tag]), CommandError::Invalid(m) if m.contains("cannot be part"))
        );
        let empty = group(&mut ed, &[tag]);
        ed.execute(&Command::DeleteObjects { ids: vec![tag] })
            .unwrap();
        assert!(ed.doc().object(empty).is_none());
        // A name that is taken.
        assert!(
            matches!(ed.execute(&Command::CreateSymbol { name: "tree".into(), ids: vec![a], insertion: None, replace: false }), Err(CommandError::Invalid(m)) if m.contains("exists already"))
        );
        // Keeps the class of components.
        let class = ClassId(
            ed.execute(&Command::AddClass {
                name: "Trees".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let c = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: v(0.0, 0.0),
                    radius: 3.0,
                },
                layer: None,
                class: Some(class),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let created = ed
            .execute(&Command::CreateSymbol {
                name: "Classy".into(),
                ids: vec![c],
                insertion: None,
                replace: true,
            })
            .unwrap()
            .created;
        assert_eq!(
            ed.doc()
                .resources
                .symbol(ResourceId(created[0]))
                .unwrap()
                .items[0]
                .class,
            Some(class)
        );
        let parts = crate::symbols::instance_objects(
            ed.doc(),
            ed.doc().object(ObjectId(created[1])).unwrap(),
        );
        assert_eq!(parts[0].class, class);
    }

    #[test]
    fn a_symbol_converts_to_a_group_and_edits_in_place() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Rect {
                min: v(0.0, 0.0),
                max: v(10.0, 10.0),
            },
        );
        let created = symbol_from(&mut ed, &[a], true);
        let (sym, inst) = (ResourceId(created[0]), ObjectId(created[1]));
        // A second instance, moved and scaled.
        let inst2 = add(
            &mut ed,
            Shape::Symbol {
                symbol: sym,
                at: v(100.0, 0.0),
                rotation: 0.0,
                scale: 2.0,
                scale_y: None,
            },
        );
        // Edit in place: the instance hides, the group appears with the component.
        let before_edit = ed.doc().clone();
        let out = ed.execute(&Command::EditSymbol { id: inst }).unwrap();
        let g = ObjectId(out.created[0]);
        assert!(!ed.doc().is_visible(ed.doc().object(inst).unwrap()));
        assert!(ed.doc().is_visible(ed.doc().object(inst2).unwrap()));
        let kids = ed.doc().children(g);
        assert_eq!(kids.len(), 1);
        assert!(matches!(
            ed.doc().object(kids[0]).unwrap().shape,
            Shape::Rect { .. }
        ));
        assert!(
            matches!(ed.execute(&Command::EditSymbol { id: inst2 }), Err(CommandError::Invalid(m)) if m.contains("being edited"))
        );
        // Change the component: add a circle, and make the rect bigger.
        ed.execute(&Command::ModifyObject {
            id: kids[0],
            shape: Some(Shape::Rect {
                min: v(-5.0, -5.0),
                max: v(15.0, 15.0),
            }),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        // A new object outside the edit group is not part of the definition.
        ed.execute(&Command::AddObject {
            shape: Shape::Circle {
                center: v(5.0, 5.0),
                radius: 2.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        ed.execute(&Command::ExitSymbolEdit { group: g }).unwrap();
        assert!(ed.doc().object(g).is_none());
        assert!(ed.doc().is_visible(ed.doc().object(inst).unwrap()));
        let def = ed.doc().resources.symbol(sym).unwrap();
        assert_eq!(def.items.len(), 1);
        // Both instances show the new size. The second one is scaled by 2: 20 x 20 becomes 40 x 40.
        let (lo, hi) = ed.doc().object_bounds(ed.doc().object(inst2).unwrap());
        assert!((hi - lo).distance(v(40.0, 40.0)) < 1e-9, "{lo} {hi}");
        let (lo, hi) = ed.doc().object_bounds(ed.doc().object(inst).unwrap());
        assert!((hi - lo).distance(v(20.0, 20.0)) < 1e-9);
        // One undo reverts the exit, another the edit.
        assert!(ed.undo());
        assert!(ed.doc().object(g).is_some());
        while ed.undo() {
            if norm(ed.doc()) == norm(&before_edit) {
                break;
            }
        }
        assert_eq!(norm(ed.doc()), norm(&before_edit));
        // Exit needs an edit group.
        assert!(
            matches!(ed.execute(&Command::ExitSymbolEdit { group: inst2 }), Err(CommandError::Invalid(m)) if m.contains("not the group"))
        );
        assert!(matches!(
            ed.execute(&Command::EditSymbol { id: a }),
            Err(CommandError::NoObject(_))
        ));

        // Convert to group: plain objects, the instance goes.
        let out = ed
            .execute(&Command::ConvertSymbolToGroup { ids: vec![inst2] })
            .unwrap();
        let g2 = ObjectId(out.created[0]);
        assert!(ed.doc().object(inst2).is_none());
        let kids = ed.doc().children(g2);
        assert_eq!(kids.len(), 1);
        let Shape::Rect { min, max } = ed.doc().object(kids[0]).unwrap().shape else {
            panic!()
        };
        assert!((max - min).distance(v(20.0, 20.0)) < 1e-9);
        assert!(
            matches!(&ed.doc().object(g2).unwrap().shape, Shape::Group { name } if name == "Tree")
        );
        assert!(ed.undo());
        assert!(ed.doc().object(inst2).is_some());
        assert!(matches!(
            ed.execute(&Command::ConvertSymbolToGroup { ids: vec![a] }),
            Err(CommandError::NoObject(_))
        ));
        let plain = add(
            &mut ed,
            Shape::Circle {
                center: v(0.0, 0.0),
                radius: 1.0,
            },
        );
        assert!(
            matches!(ed.execute(&Command::ConvertSymbolToGroup { ids: vec![plain] }), Err(CommandError::Invalid(m)) if m.contains("not a symbol"))
        );
    }

    #[test]
    fn exiting_the_edit_cleans_up_when_the_instance_is_gone_and_checks_kinds() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Circle {
                center: v(0.0, 0.0),
                radius: 5.0,
            },
        );
        let created = symbol_from(&mut ed, &[a], true);
        let inst = ObjectId(created[1]);
        let g = ObjectId(
            ed.execute(&Command::EditSymbol { id: inst })
                .unwrap()
                .created[0],
        );
        // A hardscape in the edit group cannot be part of a symbol.
        let kid = ed.doc().children(g)[0];
        ed.execute(&Command::ModifyObject {
            id: kid,
            shape: Some(Shape::Circle {
                center: v(0.0, 0.0),
                radius: 9.0,
            }),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        // Delete the hidden instance, then exit: the definition still changes.
        ed.execute(&Command::DeleteObjects { ids: vec![inst] })
            .unwrap();
        ed.execute(&Command::ExitSymbolEdit { group: g }).unwrap();
        let def = ed.doc().resources.symbol(ResourceId(created[0])).unwrap();
        assert!(matches!(def.items[0].shape, Shape::Circle { radius, .. } if radius == 9.0));
        assert!(ed.doc().objects.is_empty());
    }

    #[test]
    fn deleting_every_component_keeps_the_edit_group_and_exit_shows_the_instance() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Circle {
                center: v(0.0, 0.0),
                radius: 5.0,
            },
        );
        let created = symbol_from(&mut ed, &[a], true);
        let (sym, inst) = (ResourceId(created[0]), ObjectId(created[1]));
        let g = ObjectId(
            ed.execute(&Command::EditSymbol { id: inst })
                .unwrap()
                .created[0],
        );
        let kids = ed.doc().children(g);
        ed.execute(&Command::DeleteObjects { ids: kids }).unwrap();
        assert!(
            ed.doc().object(g).is_some(),
            "the edit group stays when it is empty"
        );
        assert!(!ed.doc().is_visible(ed.doc().object(inst).unwrap()));
        ed.execute(&Command::ExitSymbolEdit { group: g }).unwrap();
        assert!(ed.doc().resources.symbol(sym).unwrap().items.is_empty());
        assert!(ed.doc().is_visible(ed.doc().object(inst).unwrap()));
        // Deleting the edit group itself shows the instance again.
        let g = ObjectId(
            ed.execute(&Command::EditSymbol { id: inst })
                .unwrap()
                .created[0],
        );
        ed.execute(&Command::DeleteObjects { ids: vec![g] })
            .unwrap();
        assert!(ed.doc().is_visible(ed.doc().object(inst).unwrap()));
        // A file saved in the middle of an edit, with the group gone, repairs on load.
        let mut doc = ed.doc().clone();
        doc.object_mut(inst)
            .unwrap()
            .data
            .insert(EDITING_KEY.into(), true.into());
        doc.validate().unwrap();
        assert!(!doc.object(inst).unwrap().data.contains_key(EDITING_KEY));
    }

    #[test]
    fn new_objects_join_a_group_and_move_in_and_out() {
        let mut ed = Editor::default();
        let (a, b) = (circle(&mut ed, 0.0), circle(&mut ed, 5.0));
        let g = group(&mut ed, &[a]);
        let out = ed
            .execute(&Command::AddObjectIn {
                parent: g,
                shape: Shape::Circle {
                    center: v(9.0, 0.0),
                    radius: 1.0,
                },
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let n = ObjectId(out.created[0]);
        assert_eq!(ed.doc().object(n).unwrap().parent, Some(g));
        assert!(ed.undo());
        assert!(ed.doc().object(n).is_none());
        assert!(
            matches!(ed.execute(&Command::AddObjectIn { parent: a, shape: Shape::Circle { center: v(9.0, 0.0), radius: 1.0 }, class: None, attrs: Attributes::default() }), Err(CommandError::Invalid(m)) if m.contains("not a group"))
        );
        // Move b into the group, then out again.
        ed.execute(&Command::SetParent {
            ids: vec![b],
            parent: Some(g),
        })
        .unwrap();
        assert_eq!(ed.doc().children(g), vec![a, b]);
        ed.execute(&Command::SetParent {
            ids: vec![a, b],
            parent: None,
        })
        .unwrap();
        assert!(ed.doc().object(g).is_none(), "an emptied group goes");
        assert_eq!(ed.doc().object(a).unwrap().parent, None);
        assert!(ed.undo());
        assert_eq!(ed.doc().object(a).unwrap().parent, Some(g));
        // Rules.
        assert!(matches!(
            ed.execute(&Command::SetParent {
                ids: vec![],
                parent: None
            }),
            Err(CommandError::Invalid(_))
        ));
        assert!(
            matches!(ed.execute(&Command::SetParent { ids: vec![g], parent: Some(g) }), Err(CommandError::Invalid(m)) if m.contains("into itself"))
        );
        assert!(
            matches!(ed.execute(&Command::SetParent { ids: vec![b], parent: Some(a) }), Err(CommandError::Invalid(m)) if m.contains("not a group"))
        );
        let layer = LayerId(
            ed.execute(&Command::AddLayer {
                name: "L2".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0],
        );
        let far = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: v(1.0, 1.0),
                    radius: 1.0,
                },
                layer: Some(layer),
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        assert!(
            matches!(ed.execute(&Command::SetParent { ids: vec![far], parent: Some(g) }), Err(CommandError::Invalid(m)) if m.contains("layer"))
        );
        // SetData cannot touch the keys of the symbol editor.
        for k in crate::symbols::RESERVED_KEYS {
            let mut set = std::collections::BTreeMap::new();
            set.insert(k.to_string(), serde_json::json!(1));
            assert!(
                matches!(ed.execute(&Command::SetData { id: b, set, remove: vec![] }), Err(CommandError::Invalid(m)) if m.contains("reserved")),
                "{k}"
            );
        }
    }
}
