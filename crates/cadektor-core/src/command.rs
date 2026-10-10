//! Commands: the only way to change a document. The GUI, scripts and AI
//! agents all send the same commands, so every change is undoable and
//! scriptable.

use std::collections::{BTreeMap, HashSet};

use glam::DVec2;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::*;
use crate::resource::{ResourceBody, ResourceId, Resources};

mod annot_ops;
mod area_ops;
mod drawing_ops;
mod graphic_ops;
mod group_ops;
mod plant_ops;
mod present_ops;
mod record_ops;
mod retaining_ops;
mod table_ops;
mod transform_ops;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    // Groups, copies and symbols are at the end of the list.
    /// Add an object. Missing layer or class means the active one.
    AddObject {
        shape: Shape,
        #[serde(default)]
        layer: Option<LayerId>,
        #[serde(default)]
        class: Option<ClassId>,
        #[serde(default)]
        attrs: Attributes,
    },
    DeleteObjects {
        ids: Vec<ObjectId>,
    },
    /// Move objects. An ID that appears twice is moved once.
    MoveObjects {
        ids: Vec<ObjectId>,
        #[schemars(with = "[f64; 2]")]
        delta: DVec2,
    },
    /// Change properties of one object. Missing fields stay unchanged.
    ModifyObject {
        id: ObjectId,
        #[serde(default)]
        shape: Option<Shape>,
        #[serde(default)]
        layer: Option<LayerId>,
        #[serde(default)]
        class: Option<ClassId>,
        #[serde(default)]
        attrs: Option<Attributes>,
    },
    /// Change one vertex of a polyline, landscape area or hardscape.
    /// Missing fields stay unchanged. `kind` is a vertex kind such as
    /// `{"kind": "arc", "radius": 50}`.
    SetVertex {
        id: ObjectId,
        index: usize,
        #[serde(default)]
        kind: Option<VertexKind>,
        #[serde(default)]
        #[schemars(with = "Option<[f64; 2]>")]
        p: Option<DVec2>,
    },
    /// Change the record data of one object: `set` merges keys, `remove`
    /// deletes keys. Keys in `set` are applied first.
    SetData {
        id: ObjectId,
        #[serde(default)]
        set: BTreeMap<String, Value>,
        #[serde(default)]
        remove: Vec<String>,
    },
    AddLayer {
        name: String,
        #[serde(default)]
        elevation: f64,
        #[serde(default)]
        scale: Option<f64>,
        #[serde(default)]
        kind: Option<LayerKind>,
    },
    ModifyLayer {
        id: LayerId,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        visible: Option<bool>,
        #[serde(default)]
        locked: Option<bool>,
        #[serde(default)]
        elevation: Option<f64>,
        #[serde(default)]
        scale: Option<f64>,
    },
    /// Delete an empty layer.
    DeleteLayer {
        id: LayerId,
    },
    AddClass {
        name: String,
        #[serde(default)]
        attrs: Attributes,
    },
    ModifyClass {
        id: ClassId,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        visible: Option<bool>,
        #[serde(default)]
        attrs: Option<Attributes>,
    },
    SetActive {
        #[serde(default)]
        layer: Option<LayerId>,
        #[serde(default)]
        class: Option<ClassId>,
    },
    /// Set or clear the georeference of the document.
    SetGeoreference {
        georef: Option<crate::georef::Georeference>,
    },
    /// Replace the display rules: the N/E grid, the phase display and the data visualization.
    SetViewRules {
        rules: Box<crate::viewrules::ViewRules>,
    },
    /// Set or clear the sun (heliodon) of the document.
    SetSun {
        sun: Option<crate::present::SunSettings>,
    },
    /// Put a material on a slot (an object or one of its parts, a style part or a class), or clear the slot.
    SetMaterialSlot {
        target: crate::present::SlotTarget,
        #[serde(default)]
        material: Option<ResourceId>,
    },
    /// Choose the render style of the 3D view, or `null` for the built-in style.
    SetRenderStyle {
        #[serde(default)]
        style: Option<ResourceId>,
    },
    /// Add a resource, for example a plant style. Returns its ID in `created`.
    AddResource {
        name: String,
        body: ResourceBody,
    },
    /// Change the name or content of a resource. Plants that use a plant style follow the change.
    ModifyResource {
        id: ResourceId,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        body: Option<ResourceBody>,
    },
    /// Delete a resource. If objects use it, the command fails unless `reassign` names
    /// another resource of the same type that takes over.
    DeleteResource {
        id: ResourceId,
        #[serde(default)]
        reassign: Option<ResourceId>,
    },
    /// Add one plant per position, as one step. A plant refers to `style` (if given) and
    /// `overrides` replace style fields. Without a style the plants are by instance, with
    /// the default style fields and the overrides.
    AddPlants {
        #[serde(default)]
        style: Option<ResourceId>,
        #[serde(default)]
        overrides: serde_json::Map<String, Value>,
        #[schemars(with = "Vec<[f64; 2]>")]
        positions: Vec<DVec2>,
        #[serde(default)]
        layer: Option<LayerId>,
        #[serde(default)]
        class: Option<ClassId>,
        /// Random size and rotation, from a seed. Plant `i` gets the same values every time.
        #[serde(default)]
        variation: Option<crate::plants::variation::Variation>,
    },
    /// Link plants to a style, replace their overrides, or unlink them.
    SetPlantStyle {
        ids: Vec<ObjectId>,
        #[serde(default)]
        style: Option<ResourceId>,
        #[serde(default)]
        overrides: Option<serde_json::Map<String, Value>>,
        #[serde(default)]
        unlink: bool,
    },
    /// Attach a record to objects. `values` are checked against the record format. Fields
    /// without a value get the default. Each object can have a record once.
    AttachRecord {
        ids: Vec<ObjectId>,
        record: ResourceId,
        #[serde(default)]
        values: serde_json::Map<String, Value>,
    },
    /// Remove a record and its values from objects.
    DetachRecord {
        ids: Vec<ObjectId>,
        record: ResourceId,
    },
    /// Change field values of a record that is attached. Other fields stay unchanged.
    SetRecordFields {
        ids: Vec<ObjectId>,
        record: ResourceId,
        values: serde_json::Map<String, Value>,
    },
    /// Add a hardscape: a slab with a `boundary`, or a path when `patch.center` gives the center line.
    /// The patch sets the style, width, slope and so on (see `hardscape::HardscapePatch`).
    AddHardscape {
        #[serde(default)]
        boundary: Option<Vec<Vertex>>,
        #[serde(default)]
        patch: Box<crate::hardscape::HardscapePatch>,
        #[serde(default)]
        layer: Option<LayerId>,
        #[serde(default)]
        class: Option<ClassId>,
        #[serde(default)]
        attrs: Attributes,
    },
    /// Add a landscape area with a boundary, a name and settings (style, holes, drape).
    AddLandscapeArea {
        boundary: Vec<Vertex>,
        #[serde(default)]
        name: String,
        #[serde(default)]
        patch: Box<crate::landscape::LandscapePatch>,
        #[serde(default)]
        layer: Option<LayerId>,
        #[serde(default)]
        class: Option<ClassId>,
        #[serde(default)]
        attrs: Attributes,
    },
    /// Change hardscape settings of one or more hardscapes. Missing fields stay unchanged.
    SetHardscape {
        ids: Vec<ObjectId>,
        patch: crate::hardscape::HardscapePatch,
    },
    /// Change the style, holes or options of landscape areas.
    SetLandscapeArea {
        ids: Vec<ObjectId>,
        patch: crate::landscape::LandscapePatch,
    },
    /// Add a sheet layer with its page settings, as one step. Returns the layer ID in `created`.
    AddSheet {
        name: String,
        #[serde(default)]
        paper: Option<crate::sheets::PaperSize>,
        #[serde(default)]
        orientation: Option<crate::sheets::Orientation>,
        #[serde(default)]
        number: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
    /// Change the page of a sheet layer. Missing fields stay unchanged.
    /// `paper` is a size id (`a3`, `letter`, ...) or `{custom: {width_mm, height_mm}}`.
    SetSheet {
        layer: LayerId,
        #[serde(default)]
        paper: Option<crate::sheets::PaperSize>,
        #[serde(default)]
        orientation: Option<crate::sheets::Orientation>,
        #[serde(default)]
        number: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
    /// Change the project data that title blocks show. Missing fields stay unchanged.
    SetProject {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        client: Option<String>,
        #[serde(default)]
        address: Option<String>,
        #[serde(default)]
        drawn_by: Option<String>,
        #[serde(default)]
        date: Option<String>,
        #[serde(default)]
        revision: Option<String>,
    },
    /// Set the stable project GUID (22 characters of the IFC base64 alphabet). IFC export sets it once.
    SetProjectGuid {
        guid: String,
    },
    /// Store a saved view. A view with the same name is replaced.
    SaveView {
        view: Box<crate::sheets::SavedView>,
    },
    DeleteView {
        name: String,
    },
    /// Apply a saved view: layer and class visibility and the active layer and class.
    /// The camera part (zoom, pan, 3D view) is for the host to apply.
    RestoreView {
        name: String,
    },
    /// Place a worksheet in the drawing as a table. `at` is the top left corner and `size` the
    /// frame in mm (paper mm on a sheet layer). The table is drawn from the calculated worksheet.
    AddWorksheetTable {
        worksheet: ResourceId,
        #[schemars(with = "[f64; 2]")]
        at: DVec2,
        #[schemars(with = "[f64; 2]")]
        size: DVec2,
        #[serde(default)]
        text_height: Option<f64>,
        #[serde(default)]
        layer: Option<LayerId>,
        #[serde(default)]
        class: Option<ClassId>,
        #[serde(default)]
        attrs: Attributes,
    },
    /// Change tables: the worksheet, the text height or the size of the frame (the top left corner stays).
    SetWorksheetTable {
        ids: Vec<ObjectId>,
        #[serde(default)]
        worksheet: Option<ResourceId>,
        #[serde(default)]
        text_height: Option<f64>,
        #[serde(default)]
        #[schemars(with = "Option<[f64; 2]>")]
        size: Option<DVec2>,
    },
    /// Make a group of objects. They must be on one layer and in the same group (or in none).
    /// A click on a group selects it as a whole. Returns the ID of the group in `created`.
    Group {
        ids: Vec<ObjectId>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Add an object as a member of a group (on the layer of the group). Use it when you draw inside a group
    /// you have entered. Returns the ID of the object in `created`.
    AddObjectIn {
        parent: ObjectId,
        shape: Shape,
        #[serde(default)]
        class: Option<ClassId>,
        #[serde(default)]
        attrs: Attributes,
    },
    /// Move objects into a group (`parent`) or out of all groups (no parent). The objects must be on the
    /// layer of the group. A group that loses its last member goes.
    SetParent {
        ids: Vec<ObjectId>,
        #[serde(default)]
        parent: Option<ObjectId>,
    },
    /// Dissolve groups by one level: the members stay (in the group above, if any) and the group object goes.
    Ungroup {
        ids: Vec<ObjectId>,
    },
    /// Turn objects about a point. A group turns as one unit. Angles are radians, counterclockwise.
    /// Hardscapes, landscape areas and site models cannot be turned.
    RotateObjects {
        ids: Vec<ObjectId>,
        #[schemars(with = "[f64; 2]")]
        center: DVec2,
        angle: f64,
    },
    /// Copy objects and move the copies by `delta`. A group is copied with its members. Returns the
    /// IDs of the copies of `ids` in `created`, then the IDs of copied members.
    CopyObjects {
        ids: Vec<ObjectId>,
        #[serde(default)]
        #[schemars(with = "[f64; 2]")]
        delta: DVec2,
    },
    /// Make a symbol definition from objects (groups count as their members). The insertion point is
    /// in the drawing (default: the center of the objects). With `replace` the objects are replaced by one instance.
    /// Returns the ID of the definition and then, with `replace`, the ID of the instance in `created`.
    CreateSymbol {
        name: String,
        ids: Vec<ObjectId>,
        #[serde(default)]
        #[schemars(with = "Option<[f64; 2]>")]
        insertion: Option<DVec2>,
        #[serde(default)]
        replace: bool,
    },
    /// Replace symbol instances by groups of plain objects.
    ConvertSymbolToGroup {
        ids: Vec<ObjectId>,
    },
    /// Edit the definition of a symbol in place: its 2D components become a group at the instance
    /// (the instance is hidden meanwhile). Returns the ID of the group in `created`.
    EditSymbol {
        id: ObjectId,
    },
    /// End editing in place: the objects of the edit group become the 2D components of the definition.
    ExitSymbolEdit {
        group: ObjectId,
    },
    /// Set the visibility mode of layers and classes: visible, grey (drawn in grey, cannot be
    /// picked or edited) or invisible. See `crate::visibility`.
    SetVisibility {
        #[serde(default)]
        layers: Vec<LayerId>,
        #[serde(default)]
        classes: Vec<ClassId>,
        mode: crate::visibility::VisMode,
    },
    /// Scale, mirror, turn or move objects (a group as one unit), in place or as copies. With
    /// `copy` the originals stay and the copies are returned in `created`. See `edit2d::Transform`.
    TransformObjects {
        ids: Vec<ObjectId>,
        transform: crate::edit2d::Transform,
        #[serde(default)]
        copy: bool,
    },
    /// Add an object with a new shape that takes everything else from `source`: layer, class,
    /// attributes, group and record data (not the transient editing keys of symbols). The edit
    /// tools use it for the pieces they cut from an object. Returns the ID in `created`.
    AddObjectLike {
        source: ObjectId,
        shape: Shape,
    },
    /// Duplicate array: linear, rectangular or circular copies of objects (a group as one unit).
    /// Returns the IDs of the copies of `ids` in `created`. See `edit2d::ArrayParams`.
    ArrayObjects {
        ids: Vec<ObjectId>,
        array: crate::edit2d::ArrayParams,
    },
    /// Several commands as one undo step. Stops at the first error.
    Batch {
        commands: Vec<Command>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CommandError {
    #[error("object {0} does not exist")]
    NoObject(ObjectId),
    #[error("layer {0} does not exist")]
    NoLayer(LayerId),
    #[error("class {0} does not exist")]
    NoClass(ClassId),
    #[error("layer {0} is locked")]
    LayerLocked(LayerId),
    #[error("layer {0} is not empty")]
    LayerNotEmpty(LayerId),
    #[error("a document must keep at least one layer")]
    LastLayer,
    #[error("resource {0} does not exist")]
    NoResource(ResourceId),
    #[error("resource {0} is used by {1} object(s). Reassign them to another resource first")]
    ResourceInUse(ResourceId, usize),
    #[error("invalid value: {0}")]
    Invalid(String),
}

/// Result of a successful command.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    /// IDs of new objects, layers and classes, in creation order.
    pub created: Vec<u64>,
}

/// Snapshot of the document parts that are not objects.
#[derive(Clone, Debug, PartialEq)]
struct Meta {
    drawing: crate::sheets::Drawing,
    layers: Vec<Layer>,
    classes: Vec<Class>,
    resources: Resources,
    active_layer: LayerId,
    active_class: ClassId,
    georef: Option<crate::georef::Georeference>,
}

impl Meta {
    fn of(doc: &Document) -> Self {
        Meta {
            drawing: doc.drawing.clone(),
            layers: doc.layers.clone(),
            classes: doc.classes.clone(),
            resources: doc.resources.clone(),
            active_layer: doc.active_layer,
            active_class: doc.active_class,
            georef: doc.georef.clone(),
        }
    }

    fn restore(&self, doc: &mut Document) {
        doc.drawing = self.drawing.clone();
        doc.layers = self.layers.clone();
        doc.classes = self.classes.clone();
        doc.resources = self.resources.clone();
        doc.active_layer = self.active_layer;
        doc.active_class = self.active_class;
        doc.georef = self.georef.clone();
    }

    fn is_finite(&self) -> bool {
        self.drawing
            .saved_views
            .iter()
            .all(crate::sheets::SavedView::is_finite)
            && self
                .layers
                .iter()
                .all(|l| l.elevation.is_finite() && l.scale.is_finite())
            && self.classes.iter().all(|c| c.attrs.is_finite())
    }
}

/// Up to this many removed objects are put back one by one. More are
/// merged in one pass over the object list.
const FEW: usize = 8;

/// The change that one command made, with enough data to undo and redo it.
/// It holds only what changed: no copy of the whole object list.
#[derive(Clone, Debug)]
pub struct Patch {
    /// Removed objects with their index in the z-order before the command, ascending.
    removed: Vec<(usize, Object)>,
    /// New objects with their index in the z-order after the command, ascending.
    added: Vec<(usize, Object)>,
    /// Objects that changed in place, as (before, after).
    modified: Vec<(Object, Object)>,
    meta: Option<(Meta, Meta)>,
}

impl Patch {
    fn is_empty(&self) -> bool {
        self.removed.is_empty()
            && self.added.is_empty()
            && self.modified.is_empty()
            && self.meta.is_none()
    }

    fn apply(&self, doc: &mut Document, forward: bool) {
        if forward {
            for (_, after) in &self.modified {
                if let Some(slot) = doc.objects.get_mut(&after.id) {
                    *slot = after.clone();
                }
            }
            remove_at(&mut doc.objects, &self.removed);
            for (index, o) in &self.added {
                doc.objects.shift_insert(*index, o.id, o.clone());
            }
        } else {
            for (index, _) in self.added.iter().rev() {
                doc.objects.shift_remove_index(*index);
            }
            insert_at(&mut doc.objects, &self.removed);
            for (before, _) in &self.modified {
                if let Some(slot) = doc.objects.get_mut(&before.id) {
                    *slot = before.clone();
                }
            }
        }
        if let Some((before, after)) = &self.meta {
            if forward { after } else { before }.restore(doc);
        }
    }
}

/// Removes the objects at these (ascending) indices.
fn remove_at(objects: &mut Objects, items: &[(usize, Object)]) {
    if items.len() <= FEW {
        for (index, _) in items.iter().rev() {
            objects.shift_remove_index(*index);
        }
        return;
    }
    let mut skip = items.iter().map(|(i, _)| *i).peekable();
    let old = std::mem::take(objects);
    for (i, (id, o)) in old.into_iter().enumerate() {
        if skip.peek() == Some(&i) {
            skip.next();
        } else {
            objects.insert(id, o);
        }
    }
}

/// Inserts objects so that each ends up at its (ascending) index.
fn insert_at(objects: &mut Objects, items: &[(usize, Object)]) {
    if items.len() <= FEW {
        for (index, o) in items {
            objects.shift_insert(*index, o.id, o.clone());
        }
        return;
    }
    let old = std::mem::take(objects);
    let mut rest = old.into_iter();
    let mut pending = items.iter().peekable();
    let total = rest.len() + items.len();
    for i in 0..total {
        if let Some((_, o)) = pending.next_if(|(index, _)| *index == i) {
            objects.insert(o.id, o.clone());
        } else if let Some((id, o)) = rest.next() {
            objects.insert(id, o);
        }
    }
}

/// Removes repeated IDs and keeps the first of each.
fn unique(ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// Turns indices in the current object list into indices before earlier
/// removals. `removed` and `current` must be ascending.
fn indices_before(removed: &[usize], current: &[usize]) -> Vec<usize> {
    let mut j = 0;
    current
        .iter()
        .map(|&c| {
            while j < removed.len() && removed[j] <= c + j {
                j += 1;
            }
            c + j
        })
        .collect()
}

/// Records what a command changes, to build its `Patch`.
struct Tx<'a> {
    doc: &'a mut Document,
    /// Objects that existed before the command, as they were before their first change.
    before: IndexMap<ObjectId, Object>,
    /// Objects that the command created and has not removed, in creation order.
    added: Vec<ObjectId>,
    added_set: HashSet<ObjectId>,
    /// Pre-existing objects that the command removed: (index before the command, ID), ascending.
    removed: Vec<(usize, ObjectId)>,
}

impl<'a> Tx<'a> {
    fn new(doc: &'a mut Document) -> Self {
        Tx {
            doc,
            before: IndexMap::new(),
            added: Vec::new(),
            added_set: HashSet::new(),
            removed: Vec::new(),
        }
    }

    /// Call before changing an object that may be older than this command.
    fn touch(&mut self, id: ObjectId) {
        if !self.before.contains_key(&id)
            && !self.added_set.contains(&id)
            && let Some(o) = self.doc.object(id)
        {
            self.before.insert(id, o.clone());
        }
    }

    fn editable(&self, id: ObjectId) -> Result<(), CommandError> {
        let obj = self.doc.object(id).ok_or(CommandError::NoObject(id))?;
        self.check_layer(obj.layer)?;
        if self.doc.drawing.grey.classes.contains(&obj.class) {
            return Err(CommandError::Invalid(format!(
                "object {id} is in a grey class and cannot be edited. Make the class visible first"
            )));
        }
        Ok(())
    }

    fn check_layer(&self, id: LayerId) -> Result<(), CommandError> {
        match self.doc.layer(id) {
            None => Err(CommandError::NoLayer(id)),
            Some(l) if l.locked => Err(CommandError::LayerLocked(id)),
            Some(_) if self.doc.drawing.grey.layers.contains(&id) => {
                Err(CommandError::Invalid(format!(
                    "layer {id} is grey: its objects cannot be selected or edited. Make it visible first"
                )))
            }
            Some(_) => Ok(()),
        }
    }

    fn alloc(&mut self) -> Result<u64, CommandError> {
        self.doc
            .alloc_id()
            .ok_or_else(|| CommandError::Invalid("ids are exhausted".into()))
    }

    fn check_class(&self, id: ClassId) -> Result<(), CommandError> {
        self.doc
            .class(id)
            .map(|_| ())
            .ok_or(CommandError::NoClass(id))
    }

    /// Removes objects that exist and are editable. Objects made by this same command just disappear. Older ones are recorded.
    fn remove_objects(&mut self, ids: Vec<ObjectId>) {
        let new: HashSet<ObjectId> = ids
            .iter()
            .copied()
            .filter(|id| self.added_set.contains(id))
            .collect();
        let old: Vec<ObjectId> = ids.iter().copied().filter(|id| !new.contains(id)).collect();
        let mut current: Vec<(usize, ObjectId)> = old
            .iter()
            .map(|&id| (self.doc.objects.get_index_of(&id).expect("checked"), id))
            .collect();
        current.sort_unstable();
        let before_idx = indices_before(
            &self.removed.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            &current.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        );
        for &(_, id) in &current {
            self.touch(id);
        }
        self.removed.extend(
            before_idx
                .into_iter()
                .zip(current.iter().map(|(_, id)| *id)),
        );
        self.removed.sort_unstable();
        self.added.retain(|id| !new.contains(id));
        self.added_set.retain(|id| !new.contains(id));
        if ids.len() == 1 {
            self.doc.objects.shift_remove(&ids[0]);
        } else if !ids.is_empty() {
            let gone: HashSet<ObjectId> = ids.into_iter().collect();
            self.doc.objects.retain(|id, _| !gone.contains(id));
        }
    }

    fn run(&mut self, cmd: &Command, out: &mut Outcome) -> Result<(), CommandError> {
        match cmd {
            Command::AddObject {
                shape,
                layer,
                class,
                attrs,
            } => {
                let layer = layer.unwrap_or(self.doc.active_layer);
                let class = class.unwrap_or(self.doc.active_class);
                self.check_layer(layer)?;
                self.check_class(class)?;
                validate_shape(shape)?;
                self.check_shape_refs(shape)?;
                self.check_sheet_rules(layer, shape, None)?;
                validate_attrs(attrs)?;
                self.check_attr_refs(attrs)?;
                let id = ObjectId(self.alloc()?);
                self.added.push(id);
                self.added_set.insert(id);
                let mut o = Object::new(id, layer, class, shape.clone());
                o.attrs = attrs.clone();
                self.doc.objects.insert(id, o);
                self.sync_title_block(id);
                out.created.push(id.0);
            }
            Command::DeleteObjects { ids } => {
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                }
                // A group goes with its members, and a group that loses its last member goes too.
                let ids = self.with_group_members(&ids);
                for &id in &ids {
                    self.editable(id)?;
                }
                self.release_edited(&ids);
                self.remove_objects(ids);
            }
            Command::MoveObjects { ids, delta } => {
                if !delta.is_finite() {
                    return Err(CommandError::Invalid(NOT_FINITE.into()));
                }
                let ids = unique(ids);
                for &id in &ids {
                    self.editable(id)?;
                }
                // A group moves with its members.
                let ids = if ids.iter().any(|i| self.doc.is_group(*i)) {
                    self.doc.leaves(&ids)
                } else {
                    ids
                };
                for id in ids {
                    self.editable(id)?;
                    self.touch(id);
                    let o = self.doc.object_mut(id).expect("checked");
                    o.shape.translate(*delta);
                    crate::hardscape::translate_object_data(o, *delta);
                }
            }
            Command::ModifyObject {
                id,
                shape,
                layer,
                class,
                attrs,
            } => {
                self.editable(*id)?;
                if let Some(l) = layer {
                    self.check_layer(*l)?;
                }
                if let Some(c) = class {
                    self.check_class(*c)?;
                }
                if let Some(s) = shape {
                    validate_shape(s)?;
                    self.check_shape_refs(s)?;
                    self.guard_path(*id, Some(s))?;
                    if let Shape::LandscapeArea { boundary, .. } = s {
                        self.check_holes(*id, boundary)?;
                    }
                }
                if let Some(a) = attrs {
                    validate_attrs(a)?;
                    self.check_attr_refs(a)?;
                }
                {
                    let o = self.doc.object(*id).expect("checked");
                    // A member stays on the layer of its group, and a group with members on the layer of its members.
                    if layer.is_some_and(|l| l != o.layer)
                        && (o.parent.is_some() || self.doc.is_group(o.id))
                    {
                        return Err(CommandError::Invalid(
                            "an object in a group or a group keeps its layer. Ungroup it first"
                                .into(),
                        ));
                    }
                    if shape.as_ref().is_some_and(|s| {
                        matches!(s, Shape::Group { .. }) != matches!(o.shape, Shape::Group { .. })
                    }) {
                        return Err(CommandError::Invalid("a group cannot become another kind of object, and an object cannot become a group".into()));
                    }
                    self.check_sheet_rules(
                        layer.unwrap_or(o.layer),
                        shape.as_ref().unwrap_or(&o.shape),
                        Some(*id),
                    )?;
                }
                self.touch(*id);
                // A linked plant keeps its style when its shape is edited (see `plants::link`).
                let linked_before = shape
                    .as_ref()
                    .and_then(|_| self.doc.object(*id))
                    .filter(|o| crate::plants::link::link(o).is_some())
                    .cloned();
                let area_before = shape
                    .as_ref()
                    .and_then(|_| self.doc.object(*id))
                    .filter(|o| matches!(o.shape, Shape::Hardscape { .. }))
                    .cloned();
                let o = self.doc.object_mut(*id).expect("checked");
                if let Some(s) = shape {
                    o.shape = s.clone();
                }
                if let Some(l) = layer {
                    o.layer = *l;
                }
                if let Some(c) = class {
                    o.class = *c;
                }
                if let Some(a) = attrs {
                    o.attrs = a.clone();
                }
                if let Some(prev) = linked_before {
                    let mut now = self.doc.object(*id).expect("checked").clone();
                    crate::plants::link::adopt_shape_edit(self.doc, &prev, &mut now);
                    *self.doc.object_mut(*id).expect("checked") = now;
                }
                if let Some(prev) = area_before {
                    let mut now = self.doc.object(*id).expect("checked").clone();
                    crate::hardscape::data::fix_edge_elevations(&prev, &mut now);
                    *self.doc.object_mut(*id).expect("checked") = now;
                }
            }
            Command::SetVertex { id, index, kind, p } => {
                self.editable(*id)?;
                let len = self
                    .doc
                    .object(*id)
                    .and_then(|o| o.shape.vertices())
                    .map(Vec::len);
                match len {
                    None => return Err(CommandError::Invalid("object has no vertices".into())),
                    Some(len) if *index >= len => {
                        return Err(CommandError::Invalid(format!(
                            "vertex index {index} is out of range (0..{len})"
                        )));
                    }
                    Some(_) => {}
                }
                self.guard_path(*id, None)?;
                let mut v = self
                    .doc
                    .object(*id)
                    .expect("checked")
                    .shape
                    .vertices()
                    .expect("checked")[*index];
                if let Some(k) = kind {
                    v.kind = *k;
                }
                if let Some(p) = p {
                    v.p = *p;
                }
                validate_vertex(&v)?;
                if let Some(vs) = self.doc.object(*id).and_then(|o| o.shape.vertices()) {
                    let mut moved = vs.clone();
                    moved[*index] = v;
                    self.check_holes(*id, &moved)?;
                }
                self.touch(*id);
                self.doc
                    .object_mut(*id)
                    .expect("checked")
                    .shape
                    .vertices_mut()
                    .expect("checked")[*index] = v;
            }
            Command::SetData { id, set, remove } => {
                if let Some(k) = set
                    .keys()
                    .chain(remove.iter())
                    .find(|k| self.is_record_key(k))
                {
                    return Err(CommandError::Invalid(format!(
                        "the data key '{k}' is a record. Use the record commands"
                    )));
                }
                if let Some(k) = set.keys().chain(remove.iter()).find(|k| {
                    crate::plants::link::RESERVED_DATA_KEYS.contains(&k.as_str())
                        || crate::symbols::RESERVED_KEYS.contains(&k.as_str())
                        || k.as_str() == crate::worksheet_table::KEY
                }) {
                    return Err(CommandError::Invalid(format!(
                        "the data key '{k}' is reserved. Use the style commands (plants, hardscapes, landscape areas)"
                    )));
                }
                self.editable(*id)?;
                self.touch(*id);
                let o = self.doc.object_mut(*id).expect("checked");
                for (k, v) in set {
                    o.data.insert(k.clone(), v.clone());
                }
                for k in remove {
                    o.data.remove(k);
                }
            }
            Command::AddLayer {
                name,
                elevation,
                scale,
                kind,
            } => {
                validate_layer_numbers(Some(*elevation), *scale)?;
                let id = LayerId(self.alloc()?);
                self.doc.layers.push(Layer {
                    extra: Default::default(),
                    id,
                    name: name.clone(),
                    kind: kind.unwrap_or_default(),
                    visible: true,
                    locked: false,
                    elevation: *elevation,
                    scale: scale.unwrap_or(if *kind == Some(LayerKind::Sheet) {
                        1.0
                    } else {
                        100.0
                    }),
                });
                self.on_layer_added(id);
                out.created.push(id.0);
            }
            Command::ModifyLayer {
                id,
                name,
                visible,
                locked,
                elevation,
                scale,
            } => {
                validate_layer_numbers(*elevation, *scale)?;
                let l = self
                    .doc
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *id)
                    .ok_or(CommandError::NoLayer(*id))?;
                if let Some(v) = name {
                    l.name = v.clone();
                }
                if let Some(v) = visible {
                    l.visible = *v;
                }
                if let Some(v) = locked {
                    l.locked = *v;
                }
                if let Some(v) = elevation {
                    l.elevation = *v;
                }
                if let Some(v) = scale {
                    l.scale = *v;
                }
            }
            Command::DeleteLayer { id } => {
                self.doc.layer(*id).ok_or(CommandError::NoLayer(*id))?;
                if self.doc.objects.values().any(|o| o.layer == *id) {
                    return Err(CommandError::LayerNotEmpty(*id));
                }
                if self.doc.layers.len() == 1 {
                    return Err(CommandError::LastLayer);
                }
                self.doc.layers.retain(|l| l.id != *id);
                self.doc.drawing.sheets.remove(id);
                if self.doc.active_layer == *id {
                    self.doc.active_layer = self.doc.layers[0].id;
                }
            }
            Command::AddClass { name, attrs } => {
                validate_attrs(attrs)?;
                self.check_attr_refs(attrs)?;
                let id = ClassId(self.alloc()?);
                self.doc.classes.push(Class {
                    extra: Default::default(),
                    id,
                    name: name.clone(),
                    visible: true,
                    attrs: attrs.clone(),
                });
                out.created.push(id.0);
            }
            Command::ModifyClass {
                id,
                name,
                visible,
                attrs,
            } => {
                if let Some(a) = attrs {
                    validate_attrs(a)?;
                    self.check_attr_refs(a)?;
                }
                let c = self
                    .doc
                    .classes
                    .iter_mut()
                    .find(|c| c.id == *id)
                    .ok_or(CommandError::NoClass(*id))?;
                if let Some(v) = name {
                    c.name = v.clone();
                }
                if let Some(v) = visible {
                    c.visible = *v;
                }
                if let Some(v) = attrs {
                    c.attrs = v.clone();
                }
            }
            Command::SetActive { layer, class } => {
                if let Some(l) = layer {
                    self.doc.layer(*l).ok_or(CommandError::NoLayer(*l))?;
                    if self.doc.drawing.grey.layers.contains(l) {
                        return Err(CommandError::Invalid(
                            "the active layer cannot be grey".into(),
                        ));
                    }
                    self.doc.active_layer = *l;
                }
                if let Some(c) = class {
                    self.check_class(*c)?;
                    self.doc.active_class = *c;
                }
            }
            Command::SetGeoreference { georef } => {
                if georef
                    .as_ref()
                    .is_some_and(|g| !g.is_finite() || g.origin.abs().max_element() > MAX_COORD)
                {
                    return Err(CommandError::Invalid(NOT_FINITE.into()));
                }
                self.doc.georef = georef.clone();
            }
            Command::SetViewRules { rules } => {
                rules.validate().map_err(CommandError::Invalid)?;
                self.doc.drawing.view_rules = (**rules).clone();
            }
            Command::SetSun { .. }
            | Command::SetMaterialSlot { .. }
            | Command::SetRenderStyle { .. } => self.run_present(cmd)?,
            Command::AddResource { .. }
            | Command::ModifyResource { .. }
            | Command::DeleteResource { .. }
            | Command::AddPlants { .. }
            | Command::SetPlantStyle { .. } => self.run_plant(cmd, out)?,
            Command::Group { .. }
            | Command::Ungroup { .. }
            | Command::AddObjectIn { .. }
            | Command::SetParent { .. }
            | Command::RotateObjects { .. }
            | Command::CopyObjects { .. }
            | Command::CreateSymbol { .. }
            | Command::ConvertSymbolToGroup { .. }
            | Command::EditSymbol { .. }
            | Command::ExitSymbolEdit { .. } => self.run_group(cmd, out)?,
            Command::AttachRecord { .. }
            | Command::DetachRecord { .. }
            | Command::SetRecordFields { .. } => self.run_record(cmd)?,
            Command::AddWorksheetTable { .. } | Command::SetWorksheetTable { .. } => {
                self.run_table(cmd, out)?
            }
            Command::AddHardscape { .. }
            | Command::AddLandscapeArea { .. }
            | Command::SetHardscape { .. }
            | Command::SetLandscapeArea { .. } => self.run_area(cmd, out)?,
            Command::AddSheet { .. }
            | Command::SetSheet { .. }
            | Command::SetProject { .. }
            | Command::SetProjectGuid { .. }
            | Command::SaveView { .. }
            | Command::DeleteView { .. }
            | Command::RestoreView { .. } => self.run_drawing(cmd, out)?,
            Command::TransformObjects { .. }
            | Command::ArrayObjects { .. }
            | Command::AddObjectLike { .. } => self.run_transform(cmd, out)?,
            Command::SetVisibility {
                layers,
                classes,
                mode,
            } => {
                use crate::visibility::VisMode;
                if layers.is_empty() && classes.is_empty() {
                    return Err(CommandError::Invalid(
                        "give at least one layer or class".into(),
                    ));
                }
                for l in layers {
                    self.doc.layer(*l).ok_or(CommandError::NoLayer(*l))?;
                }
                for c in classes {
                    self.check_class(*c)?;
                }
                if *mode == VisMode::Grey && layers.contains(&self.doc.active_layer) {
                    return Err(CommandError::Invalid(
                        "the active layer cannot be grey. Make another layer active first".into(),
                    ));
                }
                for l in layers {
                    if let Some(layer) = self.doc.layers.iter_mut().find(|x| x.id == *l) {
                        layer.visible = *mode != VisMode::Invisible;
                    }
                    if *mode == VisMode::Grey {
                        self.doc.drawing.grey.layers.insert(*l);
                    } else {
                        self.doc.drawing.grey.layers.remove(l);
                    }
                }
                for c in classes {
                    if let Some(class) = self.doc.classes.iter_mut().find(|x| x.id == *c) {
                        class.visible = *mode != VisMode::Invisible;
                    }
                    if *mode == VisMode::Grey {
                        self.doc.drawing.grey.classes.insert(*c);
                    } else {
                        self.doc.drawing.grey.classes.remove(c);
                    }
                }
            }
            Command::Batch { commands } => {
                for c in commands {
                    self.run(c, out)?;
                }
            }
        }
        self.annot_after(cmd)?;
        self.retaining_after(cmd)?;
        self.present_prune();
        Ok(())
    }

    /// After a successful run: every object that the command touched must
    /// still have finite numbers, so the document can always be saved and
    /// reopened.
    fn check_result(&self) -> Result<(), CommandError> {
        // A shape of an unknown kind (from a newer file) cannot be made or changed here. Delete is allowed.
        if self
            .added
            .iter()
            .filter_map(|id| self.doc.object(*id))
            .any(|o| matches!(o.shape, Shape::Unknown { .. }))
        {
            return Err(CommandError::Invalid(
                "an object of an unknown kind cannot be created".into(),
            ));
        }
        for (id, before) in &self.before {
            if matches!(before.shape, Shape::Unknown { .. }) && self.doc.object(*id).is_some() {
                return Err(CommandError::Invalid(format!(
                    "object {} is of an unknown kind and cannot be edited. Only delete works",
                    id.0
                )));
            }
        }
        let mut touched = self
            .added
            .iter()
            .chain(self.before.keys())
            .filter_map(|id| self.doc.object(*id));
        if touched.clone().any(|o| !o.is_finite()) {
            Err(CommandError::Invalid(NOT_FINITE.into()))
        } else if touched.any(|o| !o.shape.is_in_range()) {
            Err(CommandError::Invalid(OUT_OF_RANGE.into()))
        } else {
            Ok(())
        }
    }

    fn finish(self, meta_before: Meta) -> Patch {
        let doc = &*self.doc;
        let removed = self
            .removed
            .iter()
            .map(|(i, id)| (*i, self.before[id].clone()))
            .collect();
        let added = self
            .added
            .iter()
            .filter_map(|id| doc.objects.get_full(id))
            .map(|(i, _, o)| (i, o.clone()))
            .collect();
        let modified = self
            .before
            .iter()
            .filter_map(|(id, b)| {
                doc.object(*id)
                    .filter(|a| *a != b)
                    .map(|a| (b.clone(), a.clone()))
            })
            .collect();
        let meta_after = Meta::of(doc);
        let meta = (meta_before != meta_after).then_some((meta_before, meta_after));
        Patch {
            removed,
            added,
            modified,
            meta,
        }
    }
}

const NOT_FINITE: &str = "numbers must be finite";
const OUT_OF_RANGE: &str = "numbers must not be larger than 1e9 mm";

fn validate_attrs(attrs: &Attributes) -> Result<(), CommandError> {
    if attrs.is_finite() {
        Ok(())
    } else {
        Err(CommandError::Invalid(NOT_FINITE.into()))
    }
}

fn validate_layer_numbers(elevation: Option<f64>, scale: Option<f64>) -> Result<(), CommandError> {
    if elevation.is_some_and(|e| !e.is_finite()) || scale.is_some_and(|s| !s.is_finite()) {
        return Err(CommandError::Invalid(NOT_FINITE.into()));
    }
    if elevation.is_some_and(|e| e.abs() > MAX_COORD) {
        return Err(CommandError::Invalid(OUT_OF_RANGE.into()));
    }
    if scale.is_some_and(|s| s <= 0.0) {
        return Err(CommandError::Invalid("scale must be positive".into()));
    }
    Ok(())
}

fn validate_vertex(v: &Vertex) -> Result<(), CommandError> {
    if !v.is_finite() {
        return Err(CommandError::Invalid(NOT_FINITE.into()));
    }
    if matches!(v.kind, VertexKind::Arc { radius } if radius < 0.0) {
        return Err(CommandError::Invalid(
            "vertex radius must be zero or more".into(),
        ));
    }
    Ok(())
}

fn validate_shape(shape: &Shape) -> Result<(), CommandError> {
    let bad = |m: &str| Err(CommandError::Invalid(m.into()));
    if matches!(shape, Shape::Unknown { .. }) {
        return bad("an object of an unknown kind cannot be created");
    }
    if let Some(m) = crate::sheets::shape_problem(shape) {
        return bad(&m);
    }
    if !shape.is_finite() {
        return bad(NOT_FINITE);
    }
    if !shape.is_in_range() {
        return bad(OUT_OF_RANGE);
    }
    if let Some(problem) = shape.problem() {
        return bad(&problem);
    }
    if let Some(vs) = shape.vertices() {
        vs.iter().try_for_each(validate_vertex)?;
    }
    if matches!(shape, Shape::Hardscape { .. } | Shape::LandscapeArea { .. })
        && shape
            .vertices()
            .is_some_and(|v| v.len() > crate::hardscape::data::MAX_VERTICES)
    {
        return bad("a boundary has at most 50000 points");
    }
    if !shape.has_valid_thickness() {
        return bad("thickness must be positive");
    }
    match shape {
        Shape::Circle { radius, .. } | Shape::Arc { radius, .. } if *radius <= 0.0 => {
            bad("radius must be positive")
        }
        Shape::Arc { start, end, .. } if arc_sweep(*start, *end) == 0.0 => {
            bad("arc sweep must not be zero")
        }
        Shape::Plant { spread, .. } if *spread <= 0.0 => bad("spread must be positive"),
        Shape::Irrigation(i) => i.problem().map_or(Ok(()), |m| bad(&m)),
        Shape::ExistingTree {
            dbh,
            spread,
            height,
            rpz_factor,
            ..
        } => {
            if *dbh <= 0.0 || *spread <= 0.0 || *height <= 0.0 {
                bad("trunk diameter, spread and height must be positive")
            } else if rpz_factor.is_some_and(|f| f < 0.0) {
                bad("the root protection factor must be zero or more")
            } else {
                Ok(())
            }
        }
        Shape::PlantTag { height, .. } if *height <= 0.0 => bad("tag text height must be positive"),
        Shape::Symbol { scale, scale_y, .. } if *scale == 0.0 || *scale_y == Some(0.0) => {
            bad("the scale of a symbol must not be zero")
        }
        Shape::Text { height, .. } if *height <= 0.0 => bad("text height must be positive"),
        Shape::Polyline { points, .. } if points.len() < 2 => {
            bad("polyline needs 2 or more points")
        }
        Shape::Polyline3d { points, .. } if points.len() < 2 => {
            bad("polyline3d needs 2 or more points")
        }
        Shape::SiteModel {
            settings, lines, ..
        } => match settings.problem() {
            Some(m) => bad(m),
            None if lines.iter().any(|l| l.points.len() < 2) => {
                bad("a site model line needs 2 or more points")
            }
            None => Ok(()),
        },
        Shape::LandscapeArea { boundary, .. }
        | Shape::Hardscape { boundary, .. }
        | Shape::SiteModifier { boundary, .. }
            if boundary.len() < 3 =>
        {
            bad("boundary needs 3 or more points")
        }
        _ => Ok(()),
    }
}

/// A document with undo and redo history.
pub struct Editor {
    doc: Document,
    undo: Vec<Patch>,
    redo: Vec<Patch>,
    revision: u64,
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new(Document::default())
    }
}

/// Source of revision numbers. They are unique across all editors in the process,
/// so a cache keyed by revision never mixes up two documents.
static REVISIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_revision() -> u64 {
    REVISIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        Editor {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: next_revision(),
        }
    }

    /// The document. It changes only through `execute`, `undo` and `redo`.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// Changes whenever the document changes (command, undo or redo), and differs
    /// between editors. Use it to decide when derived data, like a 3D mesh, is stale.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Runs a command as one undo step. On error the document is unchanged.
    pub fn execute(&mut self, cmd: &Command) -> Result<Outcome, CommandError> {
        let meta_before = Meta::of(&self.doc);
        let mut tx = Tx::new(&mut self.doc);
        let mut out = Outcome::default();
        let result = tx.run(cmd, &mut out).and_then(|()| tx.check_result());
        let patch = tx.finish(meta_before);
        let result = result.and_then(|()| match &patch.meta {
            Some((_, after)) if !after.is_finite() => Err(CommandError::Invalid(NOT_FINITE.into())),
            _ => Ok(()),
        });
        match result {
            Ok(()) => {
                if !patch.is_empty() {
                    self.undo.push(patch);
                    self.redo.clear();
                    self.revision = next_revision();
                }
                Ok(out)
            }
            Err(e) => {
                patch.apply(&mut self.doc, false);
                Err(e)
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        self.undo
            .pop()
            .map(|p| {
                p.apply(&mut self.doc, false);
                self.redo.push(p);
                self.revision = next_revision();
            })
            .is_some()
    }

    pub fn redo(&mut self) -> bool {
        self.redo
            .pop()
            .map(|p| {
                p.apply(&mut self.doc, true);
                self.undo.push(p);
                self.revision = next_revision();
            })
            .is_some()
    }

    /// Number of steps that `undo` can take back.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Takes back the last `steps` steps and forgets them: they cannot be redone. For a host that
    /// refuses a call after it ran (see `SessionLimits`).
    pub fn rollback(&mut self, steps: usize) {
        let mut any = false;
        for _ in 0..steps {
            let Some(p) = self.undo.pop() else { break };
            p.apply(&mut self.doc, false);
            any = true;
        }
        if any {
            self.redo.clear();
            self.revision = next_revision();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x: f64) -> Command {
        Command::AddObject {
            shape: Shape::Line {
                a: DVec2::new(x, 0.0),
                b: DVec2::new(x, 10.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        }
    }

    #[test]
    fn set_georeference_undoes_and_rejects_bad_numbers() {
        use crate::georef::Georeference;
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(1.0, 2.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        let mut ed = Editor::default();
        ed.execute(&Command::SetGeoreference {
            georef: Some(g.clone()),
        })
        .unwrap();
        assert_eq!(ed.doc().georef, Some(g.clone()));
        let json = serde_json::to_string(ed.doc()).unwrap();
        assert_eq!(Document::from_json(&json).unwrap().georef, Some(g.clone()));
        assert!(ed.undo());
        assert_eq!(ed.doc().georef, None);
        assert!(ed.redo());
        let bad = Georeference {
            rotation: f64::NAN,
            ..g
        };
        assert!(
            ed.execute(&Command::SetGeoreference { georef: Some(bad) })
                .is_err()
        );
        ed.execute(&Command::SetGeoreference { georef: None })
            .unwrap();
        assert_eq!(ed.doc().georef, None);
    }

    #[test]
    fn exhausted_ids_return_an_error_instead_of_panicking() {
        let mut ed = Editor::default();
        ed.doc.set_next_id(u64::MAX);
        assert!(
            matches!(ed.execute(&line(1.0)), Err(CommandError::Invalid(m)) if m.contains("exhausted"))
        );
        assert!(
            ed.execute(&Command::AddClass {
                name: "c".into(),
                attrs: Attributes::default()
            })
            .is_err()
        );
        assert!(ed.doc.objects.is_empty());
    }

    #[test]
    fn undo_redo_restores_order() {
        let mut ed = Editor::default();
        let ids: Vec<ObjectId> = (0..4)
            .map(|i| ObjectId(ed.execute(&line(i as f64)).unwrap().created[0]))
            .collect();
        let full = ed.doc.clone();
        ed.execute(&Command::DeleteObjects {
            ids: vec![ids[1], ids[2]],
        })
        .unwrap();
        assert_eq!(ed.doc.objects.len(), 2);
        ed.undo();
        assert_eq!(ed.doc, full);
        ed.redo();
        assert_eq!(ed.doc.objects.len(), 2);
    }

    #[test]
    fn failed_batch_rolls_back() {
        let mut ed = Editor::default();
        let before = ed.doc.objects.clone();
        let err = ed.execute(&Command::Batch {
            commands: vec![
                line(0.0),
                Command::DeleteObjects {
                    ids: vec![ObjectId(999)],
                },
            ],
        });
        assert_eq!(err, Err(CommandError::NoObject(ObjectId(999))));
        assert_eq!(ed.doc.objects, before);
        assert!(!ed.can_undo());
    }

    #[test]
    fn locked_layer_rejects_edits() {
        let mut ed = Editor::default();
        let id = ObjectId(ed.execute(&line(0.0)).unwrap().created[0]);
        ed.execute(&Command::ModifyLayer {
            id: LayerId(1),
            name: None,
            visible: None,
            locked: Some(true),
            elevation: None,
            scale: None,
        })
        .unwrap();
        let r = ed.execute(&Command::MoveObjects {
            ids: vec![id],
            delta: DVec2::X,
        });
        assert_eq!(r, Err(CommandError::LayerLocked(LayerId(1))));
    }

    #[test]
    fn layer_changes_undo() {
        let mut ed = Editor::default();
        let lid = ed
            .execute(&Command::AddLayer {
                name: "Site".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::SetActive {
            layer: Some(LayerId(lid)),
            class: None,
        })
        .unwrap();
        ed.undo();
        assert_eq!(ed.doc.active_layer, FIRST_LAYER);
        ed.undo();
        assert_eq!(ed.doc.layers.len(), 1);
    }

    fn add_to(ed: &mut Editor, shape: Shape) -> ObjectId {
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

    fn circle() -> Shape {
        Shape::Circle {
            center: DVec2::ZERO,
            radius: 5.0,
        }
    }

    fn lock_layer(ed: &mut Editor, id: LayerId, locked: bool) {
        ed.execute(&Command::ModifyLayer {
            id,
            name: None,
            visible: None,
            locked: Some(locked),
            elevation: None,
            scale: None,
        })
        .unwrap();
    }

    #[test]
    fn add_object_uses_active_layer_and_class_by_default() {
        let mut ed = Editor::default();
        let lid = ed
            .execute(&Command::AddLayer {
                name: "Site".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        let cid = ed
            .execute(&Command::AddClass {
                name: "Trees".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::SetActive {
            layer: Some(LayerId(lid)),
            class: Some(ClassId(cid)),
        })
        .unwrap();

        let id = add_to(&mut ed, circle());
        let o = ed.doc.object(id).unwrap();
        assert_eq!((o.layer, o.class), (LayerId(lid), ClassId(cid)));

        // Explicit layer and class win over the active ones.
        let out = ed
            .execute(&Command::AddObject {
                shape: circle(),
                layer: Some(FIRST_LAYER),
                class: Some(NONE_CLASS),
                attrs: Attributes::default(),
            })
            .unwrap();
        let o = ed.doc.object(ObjectId(out.created[0])).unwrap();
        assert_eq!((o.layer, o.class), (FIRST_LAYER, NONE_CLASS));
    }

    #[test]
    fn add_object_rejects_missing_or_locked_layer_and_class() {
        let mut ed = Editor::default();
        let add = |layer, class| Command::AddObject {
            shape: circle(),
            layer,
            class,
            attrs: Attributes::default(),
        };
        assert_eq!(
            ed.execute(&add(Some(LayerId(99)), None)),
            Err(CommandError::NoLayer(LayerId(99)))
        );
        assert_eq!(
            ed.execute(&add(None, Some(ClassId(99)))),
            Err(CommandError::NoClass(ClassId(99)))
        );
        lock_layer(&mut ed, LayerId(1), true);
        assert_eq!(
            ed.execute(&add(None, None)),
            Err(CommandError::LayerLocked(LayerId(1)))
        );
        assert!(ed.doc.objects.is_empty());
    }

    #[test]
    fn invalid_shapes_are_rejected_with_a_reason() {
        let cases = [
            (
                Shape::Circle {
                    center: DVec2::ZERO,
                    radius: 0.0,
                },
                "radius must be positive",
            ),
            (
                Shape::Arc {
                    center: DVec2::ZERO,
                    radius: -1.0,
                    start: 0.0,
                    end: 1.0,
                },
                "radius must be positive",
            ),
            (
                Shape::Plant {
                    at: DVec2::ZERO,
                    species: "x".into(),
                    spread: 0.0,
                    height: 1.0,
                },
                "spread must be positive",
            ),
            (
                Shape::Text {
                    at: DVec2::ZERO,
                    text: "x".into(),
                    height: -2.0,
                },
                "text height must be positive",
            ),
            (
                Shape::Polyline {
                    points: crate::curve::corners(&[DVec2::ZERO]),
                    closed: false,
                },
                "polyline needs 2 or more points",
            ),
            (
                Shape::Polyline3d {
                    points: vec![glam::DVec3::ZERO],
                    closed: false,
                },
                "polyline3d needs 2 or more points",
            ),
            (
                Shape::LandscapeArea {
                    boundary: crate::curve::corners(&[DVec2::ZERO, DVec2::X]),
                    name: "a".into(),
                },
                "boundary needs 3 or more points",
            ),
            (
                Shape::Hardscape {
                    boundary: crate::curve::corners(&[DVec2::ZERO, DVec2::X]),
                    material: "m".into(),
                    thickness: None,
                    elevation: None,
                },
                "boundary needs 3 or more points",
            ),
        ];
        for (shape, msg) in cases {
            let mut ed = Editor::default();
            let r = ed.execute(&Command::AddObject {
                shape: shape.clone(),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            });
            assert_eq!(
                r,
                Err(CommandError::Invalid(msg.into())),
                "{}",
                shape.kind()
            );
            assert!(ed.doc.objects.is_empty());
            assert!(!ed.can_undo());
        }

        // The smallest valid shapes are accepted.
        let mut ed = Editor::default();
        add_to(
            &mut ed,
            Shape::Polyline {
                points: crate::curve::corners(&[DVec2::ZERO, DVec2::X]),
                closed: false,
            },
        );
        add_to(
            &mut ed,
            Shape::Hardscape {
                boundary: crate::curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]),
                material: "m".into(),
                thickness: None,
                elevation: None,
            },
        );
        add_to(
            &mut ed,
            Shape::Locus3d {
                p: glam::DVec3::new(1.0, 2.0, 3.0),
            },
        );
        add_to(
            &mut ed,
            Shape::Polyline3d {
                points: vec![glam::DVec3::ZERO, glam::DVec3::ONE],
                closed: false,
            },
        );
        assert_eq!(ed.doc.objects.len(), 4);
        let far = Shape::Locus3d {
            p: glam::DVec3::new(0.0, 0.0, 1e10),
        };
        let r = ed.execute(&Command::AddObject {
            shape: far,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        });
        assert_eq!(r, Err(CommandError::Invalid(OUT_OF_RANGE.into())));
    }

    #[test]
    fn modify_object_with_invalid_shape_changes_nothing() {
        let mut ed = Editor::default();
        let id = add_to(&mut ed, circle());
        let r = ed.execute(&Command::ModifyObject {
            id,
            shape: Some(Shape::Circle {
                center: DVec2::ZERO,
                radius: -1.0,
            }),
            layer: None,
            class: None,
            attrs: None,
        });
        assert_eq!(
            r,
            Err(CommandError::Invalid("radius must be positive".into()))
        );
        assert_eq!(ed.doc.object(id).unwrap().shape, circle());
    }

    #[test]
    fn delete_objects_removes_and_reports_errors() {
        let mut ed = Editor::default();
        let a = add_to(&mut ed, circle());
        let b = add_to(&mut ed, circle());
        assert_eq!(
            ed.execute(&Command::DeleteObjects {
                ids: vec![ObjectId(999)]
            }),
            Err(CommandError::NoObject(ObjectId(999)))
        );
        assert_eq!(ed.doc.objects.len(), 2);

        ed.execute(&Command::DeleteObjects { ids: vec![a] })
            .unwrap();
        assert!(ed.doc.object(a).is_none());
        assert!(ed.doc.object(b).is_some());

        lock_layer(&mut ed, LayerId(1), true);
        assert_eq!(
            ed.execute(&Command::DeleteObjects { ids: vec![b] }),
            Err(CommandError::LayerLocked(LayerId(1)))
        );
        assert!(ed.doc.object(b).is_some());
    }

    #[test]
    fn move_objects_translates_and_rolls_back_on_error() {
        let mut ed = Editor::default();
        let a = add_to(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        let b = add_to(
            &mut ed,
            Shape::Line {
                a: DVec2::Y,
                b: DVec2::ONE,
            },
        );

        ed.execute(&Command::MoveObjects {
            ids: vec![a, b],
            delta: DVec2::new(10.0, -1.0),
        })
        .unwrap();
        assert_eq!(
            ed.doc.object(a).unwrap().shape,
            Shape::Line {
                a: DVec2::new(10.0, -1.0),
                b: DVec2::new(11.0, -1.0)
            }
        );

        // The second id is missing, so the first move must be undone too.
        let before = ed.doc.clone();
        let r = ed.execute(&Command::MoveObjects {
            ids: vec![a, ObjectId(999)],
            delta: DVec2::X,
        });
        assert_eq!(r, Err(CommandError::NoObject(ObjectId(999))));
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn modify_object_changes_only_given_fields() {
        let mut ed = Editor::default();
        let lid = LayerId(
            ed.execute(&Command::AddLayer {
                name: "B".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0],
        );
        let cid = ClassId(
            ed.execute(&Command::AddClass {
                name: "C".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let id = add_to(&mut ed, circle());
        let original = ed.doc.object(id).unwrap().clone();

        // Only the attributes change.
        let attrs = Attributes {
            pen: Some(Color([1, 2, 3, 255])),
            fill: None,
            line_weight: Some(0.7),
            ..Default::default()
        };
        ed.execute(&Command::ModifyObject {
            id,
            shape: None,
            layer: None,
            class: None,
            attrs: Some(attrs.clone()),
        })
        .unwrap();
        let o = ed.doc.object(id).unwrap();
        assert_eq!(o.attrs, attrs);
        assert_eq!(
            (o.layer, o.class, &o.shape),
            (original.layer, original.class, &original.shape)
        );

        // Layer, class and shape change together.
        let new_shape = Shape::Rect {
            min: DVec2::ZERO,
            max: DVec2::ONE,
        };
        ed.execute(&Command::ModifyObject {
            id,
            shape: Some(new_shape.clone()),
            layer: Some(lid),
            class: Some(cid),
            attrs: None,
        })
        .unwrap();
        let o = ed.doc.object(id).unwrap();
        assert_eq!((o.layer, o.class), (lid, cid));
        assert_eq!(o.shape, new_shape);
        assert_eq!(o.attrs, attrs, "attrs untouched when None");
    }

    #[test]
    fn modify_object_errors_leave_object_unchanged() {
        let mut ed = Editor::default();
        let id = add_to(&mut ed, circle());
        let modify = |layer, class| Command::ModifyObject {
            id,
            shape: None,
            layer,
            class,
            attrs: None,
        };
        assert_eq!(
            ed.execute(&modify(Some(LayerId(99)), None)),
            Err(CommandError::NoLayer(LayerId(99)))
        );
        assert_eq!(
            ed.execute(&modify(None, Some(ClassId(99)))),
            Err(CommandError::NoClass(ClassId(99)))
        );
        assert_eq!(
            ed.execute(&Command::ModifyObject {
                id: ObjectId(999),
                shape: None,
                layer: None,
                class: None,
                attrs: None
            }),
            Err(CommandError::NoObject(ObjectId(999)))
        );

        // Moving an object into a locked layer is refused.
        let lid = LayerId(
            ed.execute(&Command::AddLayer {
                name: "Locked".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0],
        );
        lock_layer(&mut ed, lid, true);
        assert_eq!(
            ed.execute(&modify(Some(lid), None)),
            Err(CommandError::LayerLocked(lid))
        );
        assert_eq!(ed.doc.object(id).unwrap().layer, FIRST_LAYER);

        // An object on a locked layer cannot be modified at all.
        lock_layer(&mut ed, lid, false);
        ed.execute(&Command::SetActive {
            layer: Some(lid),
            class: None,
        })
        .unwrap();
        let id2 = add_to(&mut ed, circle());
        lock_layer(&mut ed, lid, true);
        let r = ed.execute(&Command::ModifyObject {
            id: id2,
            shape: None,
            layer: None,
            class: None,
            attrs: None,
        });
        assert_eq!(r, Err(CommandError::LayerLocked(lid)));
    }

    #[test]
    fn add_layer_defaults_and_custom_scale() {
        let mut ed = Editor::default();
        let a = ed
            .execute(&Command::AddLayer {
                name: "Ground".into(),
                elevation: -150.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        let b = ed
            .execute(&Command::AddLayer {
                name: "Detail".into(),
                elevation: 0.0,
                scale: Some(20.0),
                kind: None,
            })
            .unwrap()
            .created[0];
        let la = ed.doc.layer(LayerId(a)).unwrap();
        assert_eq!(
            (
                la.name.as_str(),
                la.elevation,
                la.scale,
                la.visible,
                la.locked
            ),
            ("Ground", -150.0, 100.0, true, false)
        );
        assert_eq!(ed.doc.layer(LayerId(b)).unwrap().scale, 20.0);
        assert_ne!(a, b, "ids are unique");
    }

    #[test]
    fn modify_layer_updates_fields_and_checks_scale() {
        let mut ed = Editor::default();
        ed.execute(&Command::ModifyLayer {
            id: LayerId(1),
            name: Some("Renamed".into()),
            visible: Some(false),
            locked: Some(true),
            elevation: Some(3.0),
            scale: Some(50.0),
        })
        .unwrap();
        let l = ed.doc.layer(FIRST_LAYER).unwrap();
        assert_eq!(
            (l.name.as_str(), l.visible, l.locked, l.elevation, l.scale),
            ("Renamed", false, true, 3.0, 50.0)
        );

        let r = ed.execute(&Command::ModifyLayer {
            id: LayerId(1),
            name: None,
            visible: None,
            locked: None,
            elevation: None,
            scale: Some(0.0),
        });
        assert_eq!(
            r,
            Err(CommandError::Invalid("scale must be positive".into()))
        );
        assert_eq!(ed.doc.layer(FIRST_LAYER).unwrap().scale, 50.0);

        let r = ed.execute(&Command::ModifyLayer {
            id: LayerId(99),
            name: None,
            visible: None,
            locked: None,
            elevation: None,
            scale: None,
        });
        assert_eq!(r, Err(CommandError::NoLayer(LayerId(99))));
    }

    #[test]
    fn delete_layer_rules() {
        let mut ed = Editor::default();
        assert_eq!(
            ed.execute(&Command::DeleteLayer { id: LayerId(99) }),
            Err(CommandError::NoLayer(LayerId(99)))
        );
        assert_eq!(
            ed.execute(&Command::DeleteLayer { id: LayerId(1) }),
            Err(CommandError::LastLayer)
        );

        let lid = LayerId(
            ed.execute(&Command::AddLayer {
                name: "B".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::SetActive {
            layer: Some(lid),
            class: None,
        })
        .unwrap();
        add_to(&mut ed, circle());
        assert_eq!(
            ed.execute(&Command::DeleteLayer { id: lid }),
            Err(CommandError::LayerNotEmpty(lid))
        );
        assert_eq!(ed.doc.layers.len(), 2);

        // Empty the active layer, then delete it: the active layer falls back to the first one.
        let id = ed.doc.objects[0].id;
        ed.execute(&Command::DeleteObjects { ids: vec![id] })
            .unwrap();
        ed.execute(&Command::DeleteLayer { id: lid }).unwrap();
        assert_eq!(ed.doc.layers.len(), 1);
        assert_eq!(ed.doc.active_layer, FIRST_LAYER);

        // Undo brings the layer and the active layer back.
        ed.undo();
        assert_eq!(ed.doc.layers.len(), 2);
        assert_eq!(ed.doc.active_layer, lid);
    }

    #[test]
    fn add_class_and_modify_class() {
        let mut ed = Editor::default();
        let attrs = Attributes {
            pen: Some(Color([9, 9, 9, 255])),
            fill: None,
            line_weight: Some(1.0),
            ..Default::default()
        };
        let cid = ClassId(
            ed.execute(&Command::AddClass {
                name: "Paths".into(),
                attrs: attrs.clone(),
            })
            .unwrap()
            .created[0],
        );
        let c = ed.doc.class(cid).unwrap();
        assert_eq!(
            (c.name.as_str(), c.visible, &c.attrs),
            ("Paths", true, &attrs)
        );

        ed.execute(&Command::ModifyClass {
            id: cid,
            name: Some("Walks".into()),
            visible: Some(false),
            attrs: None,
        })
        .unwrap();
        let c = ed.doc.class(cid).unwrap();
        assert_eq!(
            (c.name.as_str(), c.visible, &c.attrs),
            ("Walks", false, &attrs)
        );

        let new_attrs = Attributes::default();
        ed.execute(&Command::ModifyClass {
            id: cid,
            name: None,
            visible: None,
            attrs: Some(new_attrs.clone()),
        })
        .unwrap();
        assert_eq!(ed.doc.class(cid).unwrap().attrs, new_attrs);

        assert_eq!(
            ed.execute(&Command::ModifyClass {
                id: ClassId(99),
                name: None,
                visible: None,
                attrs: None
            }),
            Err(CommandError::NoClass(ClassId(99)))
        );
        ed.undo();
        assert_eq!(ed.doc.class(cid).unwrap().attrs, attrs);
    }

    #[test]
    fn set_active_changes_and_validates() {
        let mut ed = Editor::default();
        let cid = ClassId(
            ed.execute(&Command::AddClass {
                name: "C".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::SetActive {
            layer: None,
            class: Some(cid),
        })
        .unwrap();
        assert_eq!(ed.doc.active_class, cid);
        assert_eq!(ed.doc.active_layer, FIRST_LAYER);

        assert_eq!(
            ed.execute(&Command::SetActive {
                layer: Some(LayerId(99)),
                class: None
            }),
            Err(CommandError::NoLayer(LayerId(99)))
        );
        assert_eq!(
            ed.execute(&Command::SetActive {
                layer: None,
                class: Some(ClassId(99))
            }),
            Err(CommandError::NoClass(ClassId(99)))
        );
        assert_eq!(ed.doc.active_class, cid, "failed SetActive changes nothing");

        // A no-op command is not recorded in history.
        let mut fresh = Editor::default();
        fresh
            .execute(&Command::SetActive {
                layer: None,
                class: None,
            })
            .unwrap();
        assert!(!fresh.can_undo());

        ed.undo();
        assert_eq!(ed.doc.active_class, NONE_CLASS);
    }

    #[test]
    fn batch_is_one_undo_step_and_reports_all_created_ids() {
        let mut ed = Editor::default();
        let out = ed
            .execute(&Command::Batch {
                commands: vec![
                    Command::AddLayer {
                        name: "A".into(),
                        elevation: 0.0,
                        scale: None,
                        kind: None,
                    },
                    Command::AddLayer {
                        name: "B".into(),
                        elevation: 0.0,
                        scale: None,
                        kind: None,
                    },
                    Command::AddObject {
                        shape: circle(),
                        layer: None,
                        class: None,
                        attrs: Attributes::default(),
                    },
                ],
            })
            .unwrap();
        assert_eq!(out.created.len(), 3);
        assert_eq!(ed.doc.layers.len(), 3);
        assert_eq!(ed.doc.objects.len(), 1);

        assert!(ed.undo());
        assert_eq!(ed.doc.layers.len(), 1);
        assert!(ed.doc.objects.is_empty());
        assert!(!ed.can_undo());
    }

    #[test]
    fn failed_command_keeps_history_and_redo_is_cleared_by_new_commands() {
        let mut ed = Editor::default();
        assert!(!ed.undo());
        assert!(!ed.redo());

        add_to(&mut ed, circle());
        assert!(ed.undo());
        assert!(ed.can_redo());

        // A failed command does not touch the redo stack.
        assert!(
            ed.execute(&Command::DeleteObjects {
                ids: vec![ObjectId(999)]
            })
            .is_err()
        );
        assert!(ed.can_redo());

        // A successful command clears it.
        add_to(&mut ed, circle());
        assert!(!ed.can_redo());
        assert!(!ed.redo());
    }

    #[test]
    fn undo_and_redo_restore_layers_classes_and_active_state() {
        let mut ed = Editor::default();
        let lid = ed
            .execute(&Command::AddLayer {
                name: "L".into(),
                elevation: 2.0,
                scale: Some(10.0),
                kind: None,
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::ModifyLayer {
            id: LayerId(lid),
            name: Some("M".into()),
            visible: None,
            locked: None,
            elevation: None,
            scale: None,
        })
        .unwrap();
        let after = ed.doc.clone();

        ed.undo();
        assert_eq!(ed.doc.layer(LayerId(lid)).unwrap().name, "L");
        ed.undo();
        assert!(ed.doc.layer(LayerId(lid)).is_none());
        assert!(!ed.can_undo(), "both steps are undone");

        ed.redo();
        ed.redo();
        assert_eq!(ed.doc, after);
    }

    #[test]
    fn modify_object_undo_redo_round_trip() {
        let mut ed = Editor::default();
        let id = add_to(&mut ed, circle());
        let original = ed.doc.object(id).unwrap().clone();
        let new_shape = Shape::Circle {
            center: DVec2::new(1.0, 1.0),
            radius: 2.0,
        };
        ed.execute(&Command::ModifyObject {
            id,
            shape: Some(new_shape.clone()),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();

        ed.undo();
        assert_eq!(ed.doc.object(id), Some(&original));
        ed.redo();
        assert_eq!(ed.doc.object(id).unwrap().shape, new_shape);
    }

    #[test]
    fn command_error_messages() {
        assert_eq!(
            CommandError::NoObject(ObjectId(3)).to_string(),
            "object 3 does not exist"
        );
        assert_eq!(
            CommandError::NoLayer(LayerId(4)).to_string(),
            "layer 4 does not exist"
        );
        assert_eq!(
            CommandError::NoClass(ClassId(5)).to_string(),
            "class 5 does not exist"
        );
        assert_eq!(
            CommandError::LayerLocked(LayerId(6)).to_string(),
            "layer 6 is locked"
        );
        assert_eq!(
            CommandError::LayerNotEmpty(LayerId(7)).to_string(),
            "layer 7 is not empty"
        );
        assert_eq!(
            CommandError::LastLayer.to_string(),
            "a document must keep at least one layer"
        );
        assert_eq!(
            CommandError::Invalid("bad".into()).to_string(),
            "invalid value: bad"
        );
    }

    #[test]
    fn command_json_uses_op_tag() {
        let c: Command =
            serde_json::from_str(r#"{"op":"move_objects","ids":[2],"delta":[1.5,-2]}"#).unwrap();
        assert_eq!(
            c,
            Command::MoveObjects {
                ids: vec![ObjectId(2)],
                delta: DVec2::new(1.5, -2.0)
            }
        );
        let c: Command = serde_json::from_str(r#"{"op":"add_layer","name":"X"}"#).unwrap();
        assert_eq!(
            c,
            Command::AddLayer {
                name: "X".into(),
                elevation: 0.0,
                scale: None,
                kind: None
            }
        );
    }

    #[test]
    fn add_layer_rejects_non_positive_scale() {
        let mut ed = Editor::default();
        let r = ed.execute(&Command::AddLayer {
            name: "Bad".into(),
            elevation: 0.0,
            scale: Some(-5.0),
            kind: None,
        });
        assert_eq!(
            r,
            Err(CommandError::Invalid("scale must be positive".into()))
        );
        assert_eq!(ed.doc.layers.len(), 1);
    }

    // ---- regression tests for the review fixes ----

    fn ids_of(ed: &Editor) -> Vec<ObjectId> {
        ed.doc.objects.keys().copied().collect()
    }

    fn add_many(ed: &mut Editor, n: usize) -> Vec<ObjectId> {
        (0..n)
            .map(|i| {
                add_to(
                    ed,
                    Shape::Line {
                        a: DVec2::new(i as f64, 0.0),
                        b: DVec2::new(i as f64, 1.0),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn typed_ids_are_plain_numbers_in_json_and_not_truncated() {
        assert_eq!(
            serde_json::to_value(ObjectId(7)).unwrap(),
            serde_json::json!(7)
        );
        assert_eq!(serde_json::from_str::<LayerId>("3").unwrap(), LayerId(3));
        assert_eq!(ObjectId(9).to_string(), "9");
        assert_eq!(u64::from(ClassId(4)), 4);

        // An ID above u32::MAX must survive for layers and classes too.
        let mut doc = Document::default();
        let big = u32::MAX as u64 + 10;
        doc.objects.insert(
            ObjectId(big),
            Object::new(ObjectId(big), FIRST_LAYER, NONE_CLASS, circle()),
        );
        doc.validate().unwrap();
        let mut ed = Editor::new(doc);
        let lid = ed
            .execute(&Command::AddLayer {
                name: "L".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        let cid = ed
            .execute(&Command::AddClass {
                name: "C".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0];
        assert!(lid > big && cid > lid);
        assert!(ed.doc.layer(LayerId(lid)).is_some());
        assert!(ed.doc.class(ClassId(cid)).is_some());
    }

    #[test]
    fn undo_restores_exact_z_order_for_many_deleted_objects() {
        let mut ed = Editor::default();
        let ids = add_many(&mut ed, 30);
        let full = ids_of(&ed);
        // Delete scattered objects (more than the one-by-one limit) in two commands.
        let first: Vec<ObjectId> = ids.iter().copied().step_by(2).collect();
        let second: Vec<ObjectId> = ids.iter().copied().skip(1).step_by(4).collect();
        ed.execute(&Command::DeleteObjects { ids: first }).unwrap();
        let after_first = ids_of(&ed);
        ed.execute(&Command::DeleteObjects { ids: second }).unwrap();
        let after_second = ids_of(&ed);
        ed.undo();
        assert_eq!(ids_of(&ed), after_first);
        ed.undo();
        assert_eq!(ids_of(&ed), full);
        ed.redo();
        ed.redo();
        assert_eq!(ids_of(&ed), after_second);
    }

    #[test]
    fn batch_with_several_deletes_and_adds_undoes_to_exact_order() {
        let mut ed = Editor::default();
        let ids = add_many(&mut ed, 12);
        let full = ids_of(&ed);
        let batch = Command::Batch {
            commands: vec![
                Command::DeleteObjects {
                    ids: vec![ids[3], ids[9], ids[0]],
                },
                line(100.0),
                Command::DeleteObjects {
                    ids: vec![
                        ids[4], ids[11], ids[1], ids[2], ids[5], ids[6], ids[7], ids[8], ids[10],
                    ],
                },
                Command::MoveObjects {
                    ids: vec![],
                    delta: DVec2::X,
                },
                line(101.0),
            ],
        };
        ed.execute(&batch).unwrap();
        let after = ids_of(&ed);
        assert_eq!(after.len(), 2);
        ed.undo();
        assert_eq!(ids_of(&ed), full);
        ed.redo();
        assert_eq!(ids_of(&ed), after);
    }

    #[test]
    fn object_added_and_deleted_in_one_batch_leaves_no_history() {
        let mut ed = Editor::default();
        // The next ID is 2, so the batch deletes the object it just made.
        let r = ed.execute(&Command::Batch {
            commands: vec![
                line(0.0),
                Command::DeleteObjects {
                    ids: vec![ObjectId(2)],
                },
            ],
        });
        assert!(r.is_ok());
        assert!(ed.doc.objects.is_empty());
        assert!(!ed.can_undo());
    }

    #[test]
    fn move_and_modify_of_new_object_in_same_batch_undo_cleanly() {
        let mut ed = Editor::default();
        let keep = add_to(&mut ed, circle());
        let before = ed.doc.clone();
        ed.execute(&Command::Batch {
            commands: vec![
                line(0.0),
                Command::MoveObjects {
                    ids: vec![ObjectId(3), keep],
                    delta: DVec2::X,
                },
                Command::SetData {
                    id: ObjectId(3),
                    set: BTreeMap::from([("k".into(), Value::from(1))]),
                    remove: vec![],
                },
            ],
        })
        .unwrap();
        assert_eq!(ed.doc.objects.len(), 2);
        ed.undo();
        // Undo keeps the ID counter on purpose, so only the content is compared.
        assert_eq!(ed.doc.objects, before.objects);
        assert_eq!(ed.doc.object(keep), before.object(keep));
    }

    #[test]
    fn duplicate_ids_are_applied_once() {
        let mut ed = Editor::default();
        let a = add_to(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        ed.execute(&Command::MoveObjects {
            ids: vec![a, a, a],
            delta: DVec2::new(5.0, 0.0),
        })
        .unwrap();
        assert_eq!(
            ed.doc.object(a).unwrap().shape,
            Shape::Line {
                a: DVec2::new(5.0, 0.0),
                b: DVec2::new(6.0, 0.0)
            }
        );
        ed.undo();
        assert_eq!(
            ed.doc.object(a).unwrap().shape,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X
            }
        );

        let b = add_to(&mut ed, circle());
        let before = ids_of(&ed);
        ed.execute(&Command::DeleteObjects {
            ids: vec![b, a, b, a],
        })
        .unwrap();
        assert!(ed.doc.objects.is_empty());
        ed.undo();
        assert_eq!(ids_of(&ed), before);
    }

    #[test]
    fn non_finite_numbers_are_rejected_and_the_document_stays_saveable() {
        let mut ed = Editor::default();
        let a = add_to(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        let add = |shape| Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        let invalid = Err(CommandError::Invalid("numbers must be finite".into()));
        for shape in [
            Shape::Line {
                a: DVec2::new(f64::NAN, 0.0),
                b: DVec2::X,
            },
            Shape::Circle {
                center: DVec2::ZERO,
                radius: f64::INFINITY,
            },
            Shape::Plant {
                at: DVec2::ZERO,
                species: "x".into(),
                spread: f64::NAN,
                height: 1.0,
            },
            Shape::Text {
                at: DVec2::ZERO,
                text: "x".into(),
                height: f64::NAN,
            },
        ] {
            assert_eq!(ed.execute(&add(shape)), invalid);
        }
        assert_eq!(
            ed.execute(&Command::MoveObjects {
                ids: vec![a],
                delta: DVec2::new(f64::NAN, 0.0)
            }),
            invalid
        );
        let bad_attrs = Attributes {
            line_weight: Some(f64::NAN),
            ..Attributes::default()
        };
        assert_eq!(
            ed.execute(&Command::AddObject {
                shape: circle(),
                layer: None,
                class: None,
                attrs: bad_attrs.clone()
            }),
            invalid
        );
        assert_eq!(
            ed.execute(&Command::AddClass {
                name: "x".into(),
                attrs: bad_attrs.clone()
            }),
            invalid
        );
        assert_eq!(
            ed.execute(&Command::ModifyClass {
                id: NONE_CLASS,
                name: None,
                visible: None,
                attrs: Some(bad_attrs.clone())
            }),
            invalid
        );
        assert_eq!(
            ed.execute(&Command::ModifyObject {
                id: a,
                shape: None,
                layer: None,
                class: None,
                attrs: Some(bad_attrs)
            }),
            invalid
        );
        assert_eq!(
            ed.execute(&Command::AddLayer {
                name: "x".into(),
                elevation: f64::NAN,
                scale: None,
                kind: None
            }),
            invalid
        );
        assert_eq!(
            ed.execute(&Command::AddLayer {
                name: "x".into(),
                elevation: 0.0,
                scale: Some(f64::INFINITY),
                kind: None
            }),
            invalid
        );
        assert_eq!(
            ed.execute(&Command::ModifyLayer {
                id: FIRST_LAYER,
                name: None,
                visible: None,
                locked: None,
                elevation: Some(f64::NAN),
                scale: None
            }),
            invalid
        );
        assert_eq!(ed.doc.layers.len(), 1);
        assert_eq!(ed.doc.objects.len(), 1);
        assert!(ed.undo(), "only the first add is in the history");
        assert!(!ed.undo());
    }

    #[test]
    fn moving_out_of_range_is_rejected_and_rolled_back() {
        let mut ed = Editor::default();
        let a = add_to(
            &mut ed,
            Shape::Line {
                a: DVec2::splat(MAX_COORD),
                b: DVec2::splat(-MAX_COORD),
            },
        );
        let before = ed.doc.clone();
        let r = ed.execute(&Command::MoveObjects {
            ids: vec![a],
            delta: DVec2::splat(1.0),
        });
        assert_eq!(r, Err(CommandError::Invalid(OUT_OF_RANGE.into())));
        assert_eq!(ed.doc, before);
        let r = ed.execute(&Command::MoveObjects {
            ids: vec![a],
            delta: DVec2::splat(f64::MAX),
        });
        assert_eq!(r, Err(CommandError::Invalid(OUT_OF_RANGE.into())));
        assert_eq!(ed.doc, before);
        // Values at the limit are fine, and the file reopens.
        let json = serde_json::to_string(&ed.doc).unwrap();
        assert_eq!(Document::from_json(&json).unwrap(), ed.doc);
    }

    #[test]
    fn numbers_beyond_the_limit_and_bad_thickness_are_refused() {
        let mut ed = Editor::default();
        let add = |ed: &mut Editor, shape| {
            ed.execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
        };
        let range = Err(CommandError::Invalid(OUT_OF_RANGE.into()));
        assert_eq!(
            add(
                &mut ed,
                Shape::Circle {
                    center: DVec2::new(1.1e9, 0.0),
                    radius: 1.0
                }
            ),
            range
        );
        assert_eq!(
            add(
                &mut ed,
                Shape::Circle {
                    center: DVec2::ZERO,
                    radius: 1e39
                }
            ),
            range
        );
        assert_eq!(
            add(
                &mut ed,
                Shape::Plant {
                    at: DVec2::ZERO,
                    species: "p".into(),
                    spread: 1.0,
                    height: 1e10
                }
            ),
            range
        );
        let slab = |thickness, elevation| Shape::Hardscape {
            boundary: crate::curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]),
            material: "m".into(),
            thickness,
            elevation,
        };
        assert_eq!(add(&mut ed, slab(Some(1e39), None)), range);
        assert_eq!(add(&mut ed, slab(None, Some(-1e10))), range);
        for t in [0.0, -5.0] {
            assert_eq!(
                add(&mut ed, slab(Some(t), None)),
                Err(CommandError::Invalid("thickness must be positive".into()))
            );
        }
        assert!(add(&mut ed, slab(Some(MAX_COORD), Some(MAX_COORD))).is_ok());
        assert!(add(&mut ed, slab(None, None)).is_ok());
        // Layers: the elevation is limited too.
        assert!(
            ed.execute(&Command::AddLayer {
                name: "x".into(),
                elevation: 1e10,
                scale: None,
                kind: None
            })
            .is_err()
        );
        assert!(
            ed.execute(&Command::AddLayer {
                name: "x".into(),
                elevation: -1e9,
                scale: None,
                kind: None
            })
            .is_ok()
        );
        // A loaded document is checked the same way.
        let mut doc = ed.doc.clone();
        if let Some(o) = doc.objects.values_mut().next() {
            o.shape = slab(Some(-1.0), None);
        }
        assert!(doc.validate().is_err());
        let mut doc = ed.doc.clone();
        doc.layers[0].elevation = 1e12;
        assert!(doc.validate().is_err());
    }

    #[test]
    fn arc_with_zero_sweep_is_rejected_and_full_circle_is_accepted() {
        let mut ed = Editor::default();
        let arc = |start, end| Command::AddObject {
            shape: Shape::Arc {
                center: DVec2::ZERO,
                radius: 1.0,
                start,
                end,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        assert_eq!(
            ed.execute(&arc(1.0, 1.0)),
            Err(CommandError::Invalid("arc sweep must not be zero".into()))
        );
        ed.execute(&arc(0.0, std::f64::consts::TAU)).unwrap();
        ed.execute(&arc(0.0, -std::f64::consts::TAU)).unwrap();
        assert_eq!(ed.doc.objects.len(), 2);
    }

    #[test]
    fn set_data_merges_removes_and_undoes() {
        let mut ed = Editor::default();
        let id = add_to(&mut ed, circle());
        let set = |pairs: &[(&str, Value)]| {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        ed.execute(&Command::SetData {
            id,
            set: set(&[("a", Value::from(1)), ("b", Value::from("x"))]),
            remove: vec![],
        })
        .unwrap();
        ed.execute(&Command::SetData {
            id,
            set: set(&[("a", Value::from(2))]),
            remove: vec!["b".into(), "nope".into()],
        })
        .unwrap();
        assert_eq!(
            ed.doc.object(id).unwrap().data,
            set(&[("a", Value::from(2))])
        );
        ed.undo();
        assert_eq!(
            ed.doc.object(id).unwrap().data,
            set(&[("a", Value::from(1)), ("b", Value::from("x"))])
        );
        ed.undo();
        assert!(ed.doc.object(id).unwrap().data.is_empty());
        ed.redo();
        assert_eq!(ed.doc.object(id).unwrap().data.len(), 2);

        // A no-op is not recorded. Missing objects and locked layers are refused.
        let steps = ed.undo.len();
        ed.execute(&Command::SetData {
            id,
            set: BTreeMap::new(),
            remove: vec!["zzz".into()],
        })
        .unwrap();
        assert_eq!(ed.undo.len(), steps);
        assert_eq!(
            ed.execute(&Command::SetData {
                id: ObjectId(999),
                set: BTreeMap::new(),
                remove: vec![]
            }),
            Err(CommandError::NoObject(ObjectId(999)))
        );
        lock_layer(&mut ed, FIRST_LAYER, true);
        assert_eq!(
            ed.execute(&Command::SetData {
                id,
                set: set(&[("a", Value::from(3))]),
                remove: vec![]
            }),
            Err(CommandError::LayerLocked(FIRST_LAYER))
        );

        let c: Command =
            serde_json::from_str(r#"{"op":"set_data","id":2,"set":{"k":[1]},"remove":["z"]}"#)
                .unwrap();
        assert!(matches!(c, Command::SetData { .. }));
    }

    #[test]
    fn add_layer_can_make_a_sheet_layer() {
        let mut ed = Editor::default();
        let id = ed
            .execute(&Command::AddLayer {
                name: "Sheet".into(),
                elevation: 0.0,
                scale: None,
                kind: Some(LayerKind::Sheet),
            })
            .unwrap()
            .created[0];
        assert_eq!(ed.doc.layer(LayerId(id)).unwrap().kind, LayerKind::Sheet);
        assert_eq!(ed.doc.layer(FIRST_LAYER).unwrap().kind, LayerKind::Design);
        let json = serde_json::to_string(&ed.doc).unwrap();
        assert_eq!(Document::from_json(&json).unwrap(), ed.doc);
    }

    fn rounded_square() -> Shape {
        let arc = |x: f64, y: f64| Vertex::new(DVec2::new(x, y), VertexKind::Arc { radius: 10.0 });
        Shape::Hardscape {
            boundary: vec![
                arc(0.0, 0.0),
                arc(100.0, 0.0),
                arc(100.0, 100.0),
                arc(0.0, 100.0),
            ],
            material: "m".into(),
            thickness: None,
            elevation: None,
        }
    }

    #[test]
    fn set_vertex_changes_one_vertex_and_undoes() {
        let mut ed = Editor::default();
        let id = add_to(&mut ed, rounded_square());
        let before = ed.doc.clone();
        let set = |index, kind, p| Command::SetVertex { id, index, kind, p };
        ed.execute(&set(1, Some(VertexKind::Arc { radius: 25.0 }), None))
            .unwrap();
        ed.execute(&set(2, None, Some(DVec2::new(120.0, 110.0))))
            .unwrap();
        ed.execute(&set(
            3,
            Some(VertexKind::Bezier),
            Some(DVec2::new(-5.0, 99.0)),
        ))
        .unwrap();
        let vs = ed.doc.object(id).unwrap().shape.vertices().unwrap().clone();
        assert_eq!(
            vs[1],
            Vertex::new(DVec2::new(100.0, 0.0), VertexKind::Arc { radius: 25.0 })
        );
        assert_eq!(
            vs[2],
            Vertex::new(DVec2::new(120.0, 110.0), VertexKind::Arc { radius: 10.0 })
        );
        assert_eq!(
            vs[3],
            Vertex::new(DVec2::new(-5.0, 99.0), VertexKind::Bezier)
        );
        // Repeating a change is a no-op and is not recorded.
        assert_eq!(
            ed.execute(&set(1, Some(VertexKind::Arc { radius: 25.0 }), None))
                .unwrap()
                .created,
            Vec::<u64>::new()
        );
        for _ in 0..3 {
            assert!(ed.undo());
        }
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn set_vertex_errors_change_nothing() {
        let mut ed = Editor::default();
        let id = add_to(&mut ed, rounded_square());
        let circle_id = add_to(&mut ed, circle());
        let before = ed.doc.clone();
        let invalid = |m: &str| Err(CommandError::Invalid(m.into()));
        let set = |id, index, kind, p| Command::SetVertex { id, index, kind, p };
        assert_eq!(
            ed.execute(&set(id, 4, None, None)),
            invalid("vertex index 4 is out of range (0..4)")
        );
        assert_eq!(
            ed.execute(&set(circle_id, 0, None, None)),
            invalid("object has no vertices")
        );
        assert_eq!(
            ed.execute(&set(ObjectId(999), 0, None, None)),
            Err(CommandError::NoObject(ObjectId(999)))
        );
        for radius in [-1.0, f64::NAN, f64::INFINITY] {
            let kind = Some(VertexKind::Arc { radius });
            assert!(
                matches!(
                    ed.execute(&set(id, 0, kind, None)),
                    Err(CommandError::Invalid(_))
                ),
                "{radius}"
            );
        }
        assert_eq!(
            ed.execute(&set(id, 0, None, Some(DVec2::new(f64::NAN, 0.0)))),
            invalid(NOT_FINITE)
        );
        assert_eq!(ed.doc, before);
        // The same rules hold when a whole shape is added or modified.
        let bad = Shape::Polyline {
            points: vec![
                Vertex::corner(DVec2::ZERO),
                Vertex::new(DVec2::X, VertexKind::Arc { radius: -2.0 }),
            ],
            closed: false,
        };
        assert_eq!(
            ed.execute(&Command::AddObject {
                shape: bad,
                layer: None,
                class: None,
                attrs: Attributes::default()
            }),
            invalid("vertex radius must be zero or more")
        );
    }

    #[test]
    fn set_vertex_json_form() {
        let c: Command = serde_json::from_str(
            r#"{"op":"set_vertex","id":2,"index":1,"kind":{"kind":"arc","radius":30}}"#,
        )
        .unwrap();
        assert_eq!(
            c,
            Command::SetVertex {
                id: ObjectId(2),
                index: 1,
                kind: Some(VertexKind::Arc { radius: 30.0 }),
                p: None
            }
        );
        let c: Command =
            serde_json::from_str(r#"{"op":"set_vertex","id":2,"index":0,"p":[1,2]}"#).unwrap();
        assert_eq!(
            c,
            Command::SetVertex {
                id: ObjectId(2),
                index: 0,
                kind: None,
                p: Some(DVec2::new(1.0, 2.0))
            }
        );
    }
}
