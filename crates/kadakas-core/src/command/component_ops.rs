//! Commands for groups and components of the 3D modeler (MOD-07). The rules are in
//! `crate::component`. Every command here is one undo step, like all others.

use std::collections::BTreeMap;

use glam::DAffine3;

use super::*;
use crate::component::{self, ComponentError, Made, Part};
use crate::graphics::{GraphicResource, SymbolDef};
use crate::surface::{Absorber, Selection, SurfaceModel};
use crate::symbols::{EDIT_GROUP_KEY, EDITING_KEY};

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

fn refuse(e: ComponentError) -> CommandError {
    CommandError::Invalid(e.to_string())
}

/// Refuses an explode that would weld more than [`component::MAX_EXPLODE_FACES`] faces into the
/// model of the level. The weld of many parts costs more than the number of faces (it was 5
/// minutes for 10,000 squares), so the person is told before the work starts.
fn check_explode_size(faces: usize) -> Result<(), CommandError> {
    if faces > component::MAX_EXPLODE_FACES {
        return Err(invalid(format!(
            "this explode would weld {} faces, the limit is {}. Explode the parts in smaller groups, or keep the component",
            component::grouped(faces),
            component::grouped(component::MAX_EXPLODE_FACES)
        )));
    }
    Ok(())
}

/// Refuses an explode that would weld more than [`component::MAX_EXPLODE_PARTS`] parts. The cost
/// of the weld follows the number of parts, not the number of faces.
fn check_explode_parts(parts: usize) -> Result<(), CommandError> {
    if parts > component::MAX_EXPLODE_PARTS {
        return Err(invalid(format!(
            "this explode would weld {} parts, the limit is {}. Explode the parts in smaller groups, or keep the component",
            component::grouped(parts),
            component::grouped(component::MAX_EXPLODE_PARTS)
        )));
    }
    Ok(())
}

/// Longest name of a group.
const MAX_NAME: usize = 200;

/// What `MakeGroup` and `MakeComponent` work on, checked.
struct Chosen {
    layer: LayerId,
    class: ClassId,
    parent: Option<ObjectId>,
    /// The surface model and the part of it that is chosen (none: no geometry is chosen).
    model: Option<(ObjectId, Selection)>,
    objects: Vec<ObjectId>,
}

impl Tx<'_> {
    /// A new group under `parent` that holds a tree `height` levels of groups high (the new
    /// group itself counts as one) must stay within [`component::MAX_GROUP_DEPTH`].
    pub(super) fn check_group_depth(
        &self,
        parent: Option<ObjectId>,
        height: usize,
    ) -> Result<(), CommandError> {
        let above = parent.map_or(0, |p| self.doc.ancestors(p).len() + 1);
        if above + height > component::MAX_GROUP_DEPTH {
            return Err(refuse(ComponentError::GroupsTooDeep {
                max: component::MAX_GROUP_DEPTH,
            }));
        }
        Ok(())
    }

    /// Checks the choice of `MakeGroup` and `MakeComponent`.
    fn choose(
        &self,
        model: Option<ObjectId>,
        faces: &[crate::surface::FaceId],
        edges: &[crate::surface::EdgeId],
        objects: &[ObjectId],
    ) -> Result<Chosen, CommandError> {
        let objects = unique(objects);
        let geometry = !faces.is_empty() || !edges.is_empty();
        if objects.is_empty() && !geometry {
            return Err(invalid("choose faces, edges or objects"));
        }
        if geometry && model.is_none() {
            return Err(invalid("give the surface model of the faces and edges"));
        }
        for &o in &objects {
            self.editable(o)?;
        }
        let mut sel = None;
        if let Some(m) = model {
            self.editable(m)?;
            let Some(Shape::SurfaceModel(sm)) = self.doc.object(m).map(|o| &o.shape) else {
                return Err(invalid(format!("object {m} is not a surface model")));
            };
            if objects.contains(&m) {
                return Err(invalid(
                    "the surface model is both a chosen object and the source of chosen faces and edges",
                ));
            }
            if geometry {
                sel = Some(
                    sm.select(faces, edges)
                        .map_err(|e| invalid(e.to_string()))?,
                );
            }
        }
        let reference = model.or_else(|| objects.first().copied()).expect("checked");
        let r = self.doc.object(reference).expect("checked");
        let (layer, class, parent) = (r.layer, r.class, r.parent);
        for &o in &objects {
            let x = self.doc.object(o).expect("checked");
            if x.layer != layer {
                return Err(invalid("the objects of a group must be on one layer"));
            }
            if x.parent != parent {
                return Err(invalid(
                    "the objects are in different groups. Choose objects that are in the same group",
                ));
            }
        }
        // A component that is open for edit stays where it is until it is closed.
        for id in self.doc.with_members(&objects) {
            let o = self.doc.object(id).expect("exists");
            if o.data.contains_key(EDIT_GROUP_KEY) || o.data.contains_key(EDITING_KEY) {
                return Err(invalid(
                    "a component that is open for edit cannot be grouped or changed. Close it first",
                ));
            }
        }
        Ok(Chosen {
            layer,
            class,
            parent,
            model: model.zip(sel),
            objects,
        })
    }

    /// The model object of the choice and its selection, if geometry is chosen.
    fn source(&self, ch: &Chosen) -> Option<(ObjectId, Selection, SurfaceModel)> {
        let (id, sel) = ch.model.as_ref()?;
        match &self.doc.object(*id)?.shape {
            Shape::SurfaceModel(m) => Some((*id, sel.clone(), (**m).clone())),
            _ => None,
        }
    }

    /// Writes a surface model object that this command changes.
    fn put_model(&mut self, id: ObjectId, m: SurfaceModel) -> Result<(), CommandError> {
        if let Some(p) = m.problem() {
            return Err(invalid(p));
        }
        self.touch(id);
        self.doc.object_mut(id).expect("checked").shape = Shape::SurfaceModel(Box::new(m));
        Ok(())
    }

    fn make_group(
        &mut self,
        model: Option<ObjectId>,
        faces: &[crate::surface::FaceId],
        edges: &[crate::surface::EdgeId],
        objects: &[ObjectId],
        name: &Option<String>,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        let ch = self.choose(model, faces, edges, objects)?;
        let name = name.clone().unwrap_or_default();
        if name.chars().count() > MAX_NAME {
            return Err(invalid(format!(
                "a group name has at most {MAX_NAME} characters"
            )));
        }
        let height = ch
            .objects
            .iter()
            .map(|o| component::group_height(self.doc, *o))
            .max()
            .unwrap_or(0);
        self.check_group_depth(ch.parent, 1 + height)?;
        let gid = ObjectId(self.alloc()?);
        let mut g = Object::new(gid, ch.layer, ch.class, Shape::Group { name });
        g.parent = ch.parent;
        self.add_new(g);
        out.created.push(gid.0);
        if let Some((mid, sel, m)) = self.source(&ch) {
            if m.is_whole(&sel) {
                self.touch(mid);
                self.doc.object_mut(mid).expect("checked").parent = Some(gid);
            } else {
                let part = m.extract(&sel);
                let mut rest = m;
                rest.remove(&sel).map_err(|e| invalid(e.to_string()))?;
                if let Some(p) = part.problem() {
                    return Err(invalid(p));
                }
                let src = self.doc.object(mid).expect("checked");
                let (layer, class, attrs) = (src.layer, src.class, src.attrs.clone());
                let mut o = Object::new(
                    ObjectId(self.alloc()?),
                    layer,
                    class,
                    Shape::SurfaceModel(Box::new(part)),
                );
                o.attrs = attrs;
                o.parent = Some(gid);
                out.created.push(o.id.0);
                self.add_new(o);
                self.put_model(mid, rest)?;
            }
        }
        for id in ch.objects {
            self.touch(id);
            self.doc.object_mut(id).expect("checked").parent = Some(gid);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn make_component(
        &mut self,
        name: &str,
        model: Option<ObjectId>,
        faces: &[crate::surface::FaceId],
        edges: &[crate::surface::EdgeId],
        objects: &[ObjectId],
        replace: bool,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        let ch = self.choose(model, faces, edges, objects)?;
        // The axes of the definition are the axes of the model.
        // A component made from objects only has its origin at the minimum corner of their
        // bounds, so the definition is small numbers and not drawing coordinates.
        let origin = match model.and_then(|m| self.doc.object(m)).map(|o| &o.shape) {
            Some(Shape::SurfaceModel(m)) => m.transform,
            _ => {
                let mut lo = glam::DVec3::splat(f64::INFINITY);
                for &o in &ch.objects {
                    if let Some((a, _)) = component::object_bounds3(self.doc, o) {
                        lo = lo.min(a);
                    }
                }
                if lo.is_finite() {
                    DAffine3::from_translation(lo)
                } else {
                    DAffine3::IDENTITY
                }
            }
        };
        if !component::is_regular(&origin) {
            return Err(invalid(
                "the surface model has a transform that cannot be inverted",
            ));
        }
        let to_def = if origin == DAffine3::IDENTITY {
            DAffine3::IDENTITY
        } else {
            origin.inverse()
        };
        let mut parts: Vec<Part> = Vec::new();
        let source = self.source(&ch);
        let mut remove_model: Option<(ObjectId, Option<SurfaceModel>)> = None;
        if let Some((mid, sel, m)) = &source {
            let mut part = m.extract(sel);
            part.transform = DAffine3::IDENTITY;
            let src = self.doc.object(*mid).expect("checked");
            parts.push(Part {
                shape: Shape::SurfaceModel(Box::new(part)),
                attrs: src.attrs.clone(),
                class: (src.class != NONE_CLASS).then_some(src.class),
                parent: None,
            });
            if replace {
                if m.is_whole(sel) {
                    remove_model = Some((*mid, None));
                } else {
                    let mut rest = m.clone();
                    rest.remove(sel).map_err(|e| invalid(e.to_string()))?;
                    remove_model = Some((*mid, Some(rest)));
                }
            }
        }
        let shift = parts.len() as u32;
        for mut p in
            component::parts_from_objects(self.doc, &ch.objects, &to_def).map_err(refuse)?
        {
            p.parent = p.parent.map(|q| q + shift);
            parts.push(p);
        }
        if parts.is_empty() {
            return Err(invalid("a component needs at least one part"));
        }
        // Nesting: the new definition is one level deeper than the deepest one that it uses.
        let deepest =
            component::max_definition_depth(&self.doc.resources, component::part_refs(&parts));
        if deepest + 1 > component::MAX_COMPONENT_DEPTH {
            return Err(refuse(ComponentError::ComponentsTooDeep {
                max: component::MAX_COMPONENT_DEPTH,
            }));
        }
        let def = SymbolDef {
            parts,
            ..SymbolDef::default()
        };
        self.run(
            &Command::AddResource {
                name: name.to_string(),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
            },
            out,
        )?;
        let symbol = ResourceId(*out.created.last().expect("added"));
        // The whole expansion must be within the limits.
        component::leaves(&self.doc.resources, symbol).map_err(refuse)?;
        if replace {
            if let Some((mid, rest)) = remove_model {
                match rest {
                    None => self.remove_objects(vec![mid]),
                    Some(rest) => self.put_model(mid, rest)?,
                }
            }
            let all = self.doc.with_members(&ch.objects);
            self.remove_objects(all);
            let shape = Shape::Component {
                symbol,
                transform: component::raw(&origin),
            };
            validate_shape(&shape)?;
            self.check_sheet_rules(ch.layer, &shape, None)?;
            let iid = ObjectId(self.alloc()?);
            let mut inst = Object::new(iid, ch.layer, ch.class, shape);
            inst.parent = ch.parent;
            self.add_new(inst);
            out.created.push(iid.0);
        }
        Ok(())
    }

    /// Makes objects from parts, all inside the group `top`, on `layer`. Returns their IDs in the
    /// order of the parts.
    fn spawn(
        &mut self,
        made: Vec<Made>,
        top: ObjectId,
        layer: LayerId,
        class: ClassId,
        top_stays: bool,
    ) -> Result<Vec<ObjectId>, CommandError> {
        // The groups of the parts, with `top`, must stay within the depth limit. When `top` is
        // exploded at once (`top_stays` off) it does not count.
        let mut levels: Vec<usize> = Vec::with_capacity(made.len());
        for m in &made {
            let base = m.parent.map_or(0, |q| levels[q]);
            levels.push(base + usize::from(matches!(m.shape, Shape::Group { .. })));
        }
        let height = levels.iter().copied().max().unwrap_or(0);
        let top_level = self.doc.ancestors(top).len() + usize::from(top_stays);
        if top_level + height > component::MAX_GROUP_DEPTH {
            return Err(refuse(ComponentError::GroupsTooDeep {
                max: component::MAX_GROUP_DEPTH,
            }));
        }
        let ids = made
            .iter()
            .map(|_| self.alloc().map(ObjectId))
            .collect::<Result<Vec<_>, _>>()?;
        for (i, m) in made.into_iter().enumerate() {
            validate_shape(&m.shape)?;
            self.check_shape_refs(&m.shape)?;
            self.check_sheet_rules(layer, &m.shape, None)?;
            let mut o = Object::new(ids[i], layer, m.class.unwrap_or(class), m.shape);
            o.attrs = m.attrs;
            o.parent = Some(m.parent.map_or(top, |q| ids[q]));
            self.add_new(o);
        }
        Ok(ids)
    }

    /// The instance and its definition for the commands that open, explode or copy one.
    fn instance(
        &self,
        id: ObjectId,
    ) -> Result<(Object, ResourceId, DAffine3, Vec<Part>, String), CommandError> {
        self.editable(id)?;
        let inst = self.doc.object(id).expect("checked").clone();
        let Some((symbol, xf)) = component::placement(&inst.shape) else {
            return Err(invalid(format!("object {id} is not a component instance")));
        };
        if crate::symbols::is_being_edited(&inst) {
            return Err(invalid("this component is being edited. Close it first"));
        }
        let r = self
            .doc
            .resources
            .get(symbol)
            .ok_or(CommandError::NoResource(symbol))?;
        let def = self
            .doc
            .resources
            .symbol(symbol)
            .ok_or_else(|| invalid(format!("resource {symbol} is not a component")))?;
        Ok((inst, symbol, xf, def.parts.clone(), r.name.clone()))
    }

    fn explode(&mut self, ids: &[ObjectId]) -> Result<(), CommandError> {
        let ids = unique(ids);
        if ids.is_empty() {
            return Err(invalid("give at least one group or component"));
        }
        for &id in &ids {
            self.editable(id)?;
            let o = self.doc.object(id).expect("checked");
            if !matches!(o.shape, Shape::Group { .. } | Shape::Component { .. }) {
                return Err(invalid(format!(
                    "object {id} is not a group or a component instance"
                )));
            }
            if o.data.contains_key(EDIT_GROUP_KEY) || o.data.contains_key(EDITING_KEY) {
                return Err(invalid(
                    "a component that is open for edit cannot be exploded. Close it first",
                ));
            }
        }
        self.check_explode_total(&ids)?;
        for id in ids {
            if self.doc.is_group(id) {
                self.explode_group(id)?;
            } else {
                self.explode_instance(id)?;
            }
        }
        Ok(())
    }

    /// Before any work: the faces that the whole command welds, over all its groups and
    /// instances, and the faces that the models of the levels already hold, must be within
    /// [`component::MAX_EXPLODE_FACES`] and the face limit of a surface model. Only the surface
    /// models at the top of a group or of a definition are welded. Groups and instances inside
    /// move up as they are, so a nested or mirrored instance adds no face. The command fails
    /// here, at once, and the document stays as it was.
    fn check_explode_total(&self, ids: &[ObjectId]) -> Result<(), CommandError> {
        let faces_of = |shape: &Shape| match shape {
            Shape::SurfaceModel(m) => m.face_count(),
            _ => 0,
        };
        let parts_of = |shape: &Shape| usize::from(matches!(shape, Shape::SurfaceModel(_)));
        let mut new_faces = 0usize;
        let mut new_parts = 0usize;
        // The model that each level welds into, counted once for the level.
        let mut present: BTreeMap<(Option<ObjectId>, LayerId), usize> = BTreeMap::new();
        for &id in ids {
            let o = self.doc.object(id).expect("checked");
            let (f, p) = match &o.shape {
                Shape::Group { .. } => self
                    .doc
                    .children(id)
                    .into_iter()
                    .filter_map(|k| self.doc.object(k))
                    .fold((0, 0), |(f, p), k| {
                        (f + faces_of(&k.shape), p + parts_of(&k.shape))
                    }),
                Shape::Component { symbol, .. } => {
                    self.doc.resources.symbol(*symbol).map_or((0, 0), |d| {
                        d.parts
                            .iter()
                            .filter(|p| p.parent.is_none())
                            .fold((0, 0), |(f, n), p| {
                                (f + faces_of(&p.shape), n + parts_of(&p.shape))
                            })
                    })
                }
                _ => (0, 0),
            };
            new_faces += f;
            new_parts += p;
            present.entry((o.parent, o.layer)).or_insert_with(|| {
                component::raw_model(self.doc, o.parent, o.layer)
                    .and_then(|t| self.doc.object(t))
                    .map_or(0, |t| faces_of(&t.shape))
            });
        }
        check_explode_parts(new_parts)?;
        check_explode_size(new_faces)?;
        let total = new_faces + present.values().sum::<usize>();
        let max = crate::surface::MAX_FACES * crate::surface::limit_scale();
        if total > max {
            return Err(invalid(format!(
                "this explode would make a surface model of {} faces, the limit is {}. Explode fewer groups at once",
                component::grouped(total),
                component::grouped(max)
            )));
        }
        Ok(())
    }

    /// Dissolves a group: its surface models merge into the raw model of the level, the rest
    /// moves up.
    fn explode_group(&mut self, gid: ObjectId) -> Result<(), CommandError> {
        let g = self.doc.object(gid).expect("checked").clone();
        let level = g.parent;
        let kids = self.doc.children(gid);
        // The raw model of the level that is not a part of the group.
        // The models of the group are taken into it one after another, with the indexes of the
        // target kept between them (`Absorber`), so the cost grows with the parts and not with
        // the square of their number.
        let mut target: Option<(ObjectId, Absorber)> =
            component::raw_model(self.doc, level, g.layer).and_then(|t| {
                match &self.doc.object(t)?.shape {
                    Shape::SurfaceModel(m) => Some((t, Absorber::new((**m).clone()))),
                    _ => None,
                }
            });
        if let Some((t, _)) = &target {
            self.editable(*t)?;
        }
        let mut dirty = false;
        let mut gone: Vec<ObjectId> = Vec::new();
        for kid in kids {
            self.editable(kid)?;
            let shape = self.doc.object(kid).expect("exists").shape.clone();
            // A model with a look of its own (class, fill, line type) is not merged into one
            // with another look: merging would change its colour silently. It stays a separate
            // model of the level.
            let same_look = target.as_ref().is_none_or(|(t, _)| {
                let (a, b) = (self.doc.object(*t), self.doc.object(kid));
                matches!((a, b), (Some(a), Some(b)) if a.attrs == b.attrs && a.class == b.class)
            });
            // Only a model with no look of its own becomes the raw model of the level.
            let kid_is_plain = self.doc.object(kid).is_some_and(|k| {
                k.attrs == Attributes::default()
                    && (k.class == g.class || component::is_plain_model(self.doc, kid, level))
            });
            match (shape, &mut target) {
                (Shape::SurfaceModel(s), Some((_, t))) if same_look => {
                    let tt = t.model().transform;
                    let rel = (s.transform != tt).then(|| tt.inverse() * s.transform);
                    if rel.is_some_and(|r| !component::is_regular(&r)) {
                        return Err(invalid(
                            "a surface model has a transform that cannot be inverted",
                        ));
                    }
                    t.absorb(&s, rel).map_err(|e| invalid(e.to_string()))?;
                    dirty = true;
                    gone.push(kid);
                }
                (Shape::SurfaceModel(s), None) if kid_is_plain => {
                    // The first model of the group becomes the model of the level.
                    self.touch(kid);
                    self.doc.object_mut(kid).expect("exists").parent = level;
                    target = Some((kid, Absorber::new(*s)));
                }
                _ => {
                    self.touch(kid);
                    self.doc.object_mut(kid).expect("exists").parent = level;
                }
            }
        }
        if dirty && let Some((t, m)) = target {
            self.put_model(t, m.finish())?;
        }
        gone.push(gid);
        self.remove_objects(gone);
        Ok(())
    }

    fn explode_instance(&mut self, id: ObjectId) -> Result<(), CommandError> {
        let (inst, symbol, xf, parts, name) = self.instance(id)?;
        if !component::is_regular(&xf) {
            return Err(invalid(
                "the instance has a transform that cannot be inverted",
            ));
        }
        let gid = ObjectId(self.alloc()?);
        let mut g = Object::new(gid, inst.layer, inst.class, Shape::Group { name });
        g.parent = inst.parent;
        self.add_new(g);
        self.spawn(
            component::materialize(&parts, &xf),
            gid,
            inst.layer,
            inst.class,
            false,
        )?;
        self.remove_objects(vec![id]);
        self.explode_group(gid)?;
        // The last user takes the definition with it.
        if crate::library::usage(self.doc, symbol).is_unused() {
            self.doc.resources.remove(symbol);
        }
        Ok(())
    }

    fn make_unique(&mut self, ids: &[ObjectId], out: &mut Outcome) -> Result<(), CommandError> {
        let ids = unique(ids);
        if ids.is_empty() {
            return Err(invalid("give at least one component instance"));
        }
        let mut by_symbol: BTreeMap<ResourceId, usize> = BTreeMap::new();
        for &id in &ids {
            let (_, symbol, ..) = self.instance(id)?;
            *by_symbol.entry(symbol).or_default() += 1;
        }
        for (symbol, n) in &by_symbol {
            if crate::library::usage(self.doc, *symbol).total() <= *n {
                let name = self
                    .doc
                    .resources
                    .get(*symbol)
                    .map_or("", |r| r.name.as_str());
                return Err(invalid(format!(
                    "component '{name}' is already unique: no other instance or component uses it"
                )));
            }
        }
        let mut copies: BTreeMap<ResourceId, ResourceId> = BTreeMap::new();
        for &id in &ids {
            let (_, symbol, ..) = self.instance(id)?;
            let copy = match copies.get(&symbol) {
                Some(c) => *c,
                None => {
                    let old = self.doc.resources.get(symbol).expect("checked");
                    let base = old
                        .name
                        .rsplit_once('#')
                        .map_or(old.name.as_str(), |(b, n)| {
                            if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
                                b
                            } else {
                                old.name.as_str()
                            }
                        });
                    let base = base.to_string();
                    let body = old.body.clone();
                    let taken = |n: &str| {
                        self.doc.resources.iter().any(|r| {
                            r.name.eq_ignore_ascii_case(n) && r.body.type_name() == "symbol"
                        })
                    };
                    let name = (1..)
                        .map(|k| format!("{base}#{k}"))
                        .find(|n| !taken(n))
                        .expect("an endless range");
                    self.run(&Command::AddResource { name, body }, out)?;
                    let c = ResourceId(*out.created.last().expect("added"));
                    copies.insert(symbol, c);
                    c
                }
            };
            self.touch(id);
            if let Shape::Component { symbol: s, .. } =
                &mut self.doc.object_mut(id).expect("checked").shape
            {
                *s = copy;
            }
        }
        Ok(())
    }

    fn edit_component(&mut self, id: ObjectId, out: &mut Outcome) -> Result<(), CommandError> {
        let (inst, symbol, xf, parts, name) = self.instance(id)?;
        if !component::is_regular(&xf) {
            return Err(invalid(
                "the instance has a transform that cannot be inverted",
            ));
        }
        if self.doc.objects.values().any(|o| {
            o.data
                .get(EDIT_GROUP_KEY)
                .and_then(|v| v.get("symbol"))
                .and_then(crate::uid::AsUid::as_uid)
                == Some(symbol.0)
        }) {
            return Err(invalid("this component is being edited already"));
        }
        let version = self
            .doc
            .resources
            .symbol(symbol)
            .map(component::definition_version)
            .expect("checked by `instance`");
        self.check_group_depth(inst.parent, 1)?;
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
        g.data.insert(
            EDIT_GROUP_KEY.into(),
            serde_json::json!({
                "kind": "component",
                "symbol": symbol.0,
                "instance": id.0,
                "xf": component::raw(&xf),
                // The definition as it was when the edit started; the close compares it.
                "version": version,
            }),
        );
        self.add_new(g);
        self.spawn(
            component::materialize(&parts, &xf),
            gid,
            inst.layer,
            inst.class,
            true,
        )?;
        self.touch(id);
        self.doc
            .object_mut(id)
            .expect("checked")
            .data
            .insert(EDITING_KEY.into(), true.into());
        out.created.push(gid.0);
        Ok(())
    }

    fn close_component(
        &mut self,
        group: ObjectId,
        resolve: Option<component::Resolve>,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        self.editable(group)?;
        let g = self.doc.object(group).expect("checked");
        let info = g
            .data
            .get(EDIT_GROUP_KEY)
            .filter(|v| v.get("kind").and_then(Value::as_str) == Some("component"))
            .cloned()
            .ok_or_else(|| {
                invalid(format!(
                    "object {group} is not the edit group of a component. Use exit_symbol_edit for a 2D symbol"
                ))
            })?;
        let symbol = info
            .get("symbol")
            .and_then(crate::uid::AsUid::as_uid)
            .map(ResourceId)
            .ok_or_else(|| invalid("the edit group is damaged"))?;
        let instance = info
            .get("instance")
            .and_then(crate::uid::AsUid::as_uid)
            .map(ObjectId);
        let xf = info
            .get("xf")
            .and_then(Value::as_array)
            .filter(|a| a.len() == 12)
            .and_then(|a| {
                let mut raw = [0.0; 12];
                for (r, v) in raw.iter_mut().zip(a) {
                    *r = v.as_f64()?;
                }
                Some(component::affine(&raw))
            })
            .ok_or_else(|| invalid("the edit group is damaged"))?;
        let all = self.doc.with_members(&[group]);
        if all.iter().any(|i| {
            *i != group
                && self
                    .doc
                    .object(*i)
                    .is_some_and(|o| o.data.contains_key(EDIT_GROUP_KEY))
        }) {
            return Err(invalid(
                "a component inside is still open for edit. Close it first",
            ));
        }
        for &m in &all {
            self.editable(m)?;
        }
        let old = self
            .doc
            .resources
            .symbol(symbol)
            .ok_or(CommandError::NoResource(symbol))?
            .clone();
        // Does the definition still have the version that the edit started from? A peer or an
        // undo can have changed it. An edit group without a stored version (saved by an older
        // build) is an unknown version: it is never written without an explicit answer.
        let started = info.get("version").and_then(Value::as_u64);
        let must_ask = started.is_none_or(|v| v != component::definition_version(&old));
        let name_of = self
            .doc
            .resources
            .get(symbol)
            .map_or(String::new(), |r| r.name.clone());
        let mut target = symbol;
        match resolve {
            // Throw the edit away, with or without a conflict.
            Some(component::Resolve::Discard) => {}
            None if must_ask => {
                let why = if started.is_some() {
                    "was changed by somebody else while you edited it"
                } else {
                    "has no saved version from before the edit, so a change by somebody else cannot be excluded"
                };
                return Err(invalid(format!(
                    "conflict: component '{name_of}' {why}. Close again with resolve keep_as_new (keep your edit as a new component), overwrite (replace the component with your edit) or discard (throw your edit away)"
                )));
            }
            _ => {
                let to_def = if xf == DAffine3::IDENTITY {
                    DAffine3::IDENTITY
                } else {
                    xf.inverse()
                };
                let parts =
                    component::parts_from_objects(self.doc, &self.doc.children(group), &to_def)
                        .map_err(refuse)?;
                // An emptied component would leave every other instance without geometry.
                if parts.is_empty() {
                    return Err(invalid(format!(
                        "component '{name_of}' would be empty. Put geometry back, close with resolve discard to keep the old content, or delete the instance"
                    )));
                }
                let def = SymbolDef { parts, ..old };
                if resolve == Some(component::Resolve::KeepAsNew) {
                    let base = name_of
                        .rsplit_once('#')
                        .filter(|(_, n)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                        .map_or(name_of.as_str(), |(b, _)| b)
                        .to_string();
                    let taken = |n: &str| {
                        self.doc.resources.iter().any(|r| {
                            r.name.eq_ignore_ascii_case(n) && r.body.type_name() == "symbol"
                        })
                    };
                    let name = (1..)
                        .map(|k| format!("{base}#{k}"))
                        .find(|n| !taken(n))
                        .expect("an endless range");
                    self.run(
                        &Command::AddResource {
                            name,
                            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
                        },
                        out,
                    )?;
                    target = ResourceId(*out.created.last().expect("added"));
                } else {
                    let body = ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def)));
                    body.validate().map_err(invalid)?;
                    self.check_definition_size(&body)?;
                    self.check_graphic_refs(Some(symbol), &body)?;
                    self.check_instance_kinds(symbol, &body)?;
                    self.doc.resources.get_mut(symbol).expect("checked").body = body;
                    // Every definition that uses this one, and this one, must still be within
                    // the limits.
                    self.check_component_expansion(symbol)?;
                }
            }
        }
        if target != symbol
            && let Some(i) = instance.filter(|i| self.doc.object(*i).is_some())
        {
            self.touch(i);
            if let Shape::Component { symbol: s, .. } =
                &mut self.doc.object_mut(i).expect("exists").shape
            {
                *s = target;
            }
        }
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

    pub(super) fn run_component(
        &mut self,
        cmd: &Command,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        match cmd {
            Command::MakeGroup {
                model,
                faces,
                edges,
                objects,
                name,
            } => self.make_group(*model, faces, edges, objects, name, out),
            Command::MakeComponent {
                name,
                model,
                faces,
                edges,
                objects,
                replace,
            } => self.make_component(name, *model, faces, edges, objects, *replace, out),
            Command::Explode { ids } => self.explode(ids),
            Command::MakeUnique { ids } => self.make_unique(ids, out),
            Command::EditComponent { id } => self.edit_component(*id, out),
            Command::CloseComponent { group, resolve } => {
                self.close_component(*group, *resolve, out)
            }
            _ => unreachable!("run routes only component commands here"),
        }
    }
}
