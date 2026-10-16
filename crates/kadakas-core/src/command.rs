//! Commands: the only way to change a document. The GUI, scripts and AI
//! agents all send the same commands, so every change is undoable and
//! scriptable.

use std::collections::{BTreeMap, HashMap, HashSet};

use glam::DVec2;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::change::{self, ChangeLog, ChangeSet, DocumentChanged, IdsByKind, Origin};
use crate::history::HistoryData;
use crate::model::*;
use crate::resource::{Resource, ResourceBody, ResourceId, Resources};
use crate::uid::Uid;

mod annot_ops;
mod area_ops;
mod drawing_ops;
mod graphic_ops;
mod group_ops;
mod issue_ops;
mod plant_ops;
mod present_ops;
mod record_ops;
mod retaining_ops;
mod table_ops;
mod transform_ops;

/// How `Command::Arrange` moves objects in the stacking order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Arrange {
    /// Above all other objects.
    Front,
    /// Below all other objects.
    Back,
    /// One step up, past the nearest object of the same layer that is not moved.
    Forward,
    /// One step down.
    Backward,
}

/// The order of all objects after an arrange: the moved objects keep their order among
/// themselves.
fn arranged(
    order: &[ObjectId],
    layers: &dyn Fn(ObjectId) -> LayerId,
    moved: &HashSet<ObjectId>,
    how: Arrange,
) -> Vec<ObjectId> {
    let mut rest: Vec<ObjectId> = order
        .iter()
        .copied()
        .filter(|i| !moved.contains(i))
        .collect();
    let mine: Vec<ObjectId> = order
        .iter()
        .copied()
        .filter(|i| moved.contains(i))
        .collect();
    match how {
        Arrange::Front => {
            rest.extend(mine);
            rest
        }
        Arrange::Back => {
            let mut out = mine;
            out.extend(rest);
            out
        }
        Arrange::Forward | Arrange::Backward => {
            // Walk the objects in the direction of the move. A moved object swaps with the
            // nearest object of its layer that is not moved.
            let mut out: Vec<ObjectId> = order.to_vec();
            let n = out.len();
            let idx: Vec<usize> = if how == Arrange::Forward {
                (0..n).rev().collect()
            } else {
                (0..n).collect()
            };
            for i in idx {
                if !moved.contains(&out[i]) {
                    continue;
                }
                let layer = layers(out[i]);
                let step = |j: usize| {
                    if how == Arrange::Forward {
                        j.checked_add(1).filter(|j| *j < n)
                    } else {
                        j.checked_sub(1)
                    }
                };
                let mut j = step(i);
                while let Some(k) = j {
                    if layers(out[k]) == layer && !moved.contains(&out[k]) {
                        out.swap(i, k);
                        break;
                    }
                    if layers(out[k]) == layer {
                        break;
                    }
                    j = step(k);
                }
            }
            out
        }
    }
}

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
    /// Set or clear the import source of a layer (`provenance::LayerSource`). Importers use it for
    /// the layers that they make.
    SetLayerSource {
        id: LayerId,
        #[serde(default)]
        source: Option<crate::provenance::LayerSource>,
    },
    /// Delete an empty layer.
    DeleteLayer {
        id: LayerId,
    },
    /// Move objects in the stacking order (z-order): to the front or the back of all objects, or
    /// one step forward or backward past the nearest object on the same layer that is not moved.
    Arrange {
        ids: Vec<ObjectId>,
        how: Arrange,
    },
    /// Move a layer to a place in the list of layers (0 is the first). A number past the end
    /// puts it last.
    MoveLayer {
        id: LayerId,
        index: usize,
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
    /// Set or clear the picture of a plant style, as one step. The picture is stored once as an
    /// `image` resource (a picture with the same hash is reused), and a picture that no style uses
    /// any more is removed. A new picture resource is returned in `created`.
    SetPlantPicture {
        style: ResourceId,
        #[serde(default)]
        picture: Option<Box<crate::picture::ImageResource>>,
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
        /// The ID of the new object. Without one a new ID is made. A program that makes the same
        /// objects again and again (the shapes of a plug-in object) gives IDs that follow from the
        /// source (`Uid::derive`), so two users who do it at the same time make the same IDs. It
        /// fails if an object with this ID exists.
        #[serde(default)]
        id: Option<ObjectId>,
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
    /// Add a review issue (REV-04). A nil `issue.id` (and nil viewpoint ids) make new IDs; an ID
    /// from a BCF file stays. `pictures` are the PNG snapshots of the viewpoints: each becomes an
    /// `image` resource, once per hash. Returns the ID of the issue first in `created`, then those
    /// of its viewpoints.
    AddIssue {
        issue: Box<crate::issue::Issue>,
        #[serde(default)]
        pictures: Vec<crate::picture::ImageResource>,
    },
    /// Change fields of an issue. Fields that are left out stay. `now` is the new change time
    /// (milliseconds since 1970). A patch that changes nothing makes no undo step.
    ModifyIssue {
        id: crate::issue::IssueId,
        patch: Box<crate::issue::IssuePatch>,
        now: i64,
    },
    /// Delete an issue with its comments. A snapshot picture that no other viewpoint uses goes too.
    DeleteIssue {
        id: crate::issue::IssueId,
    },
    /// Add a comment to the thread of an issue. A nil `comment.id` makes a new ID (returned in
    /// `created`).
    AddIssueComment {
        comment: Box<crate::issue::Comment>,
    },
    /// Change the text of a comment. `now` is the edit time.
    EditIssueComment {
        id: crate::issue::CommentId,
        text: String,
        now: i64,
    },
    /// Delete a comment.
    DeleteIssueComment {
        id: crate::issue::CommentId,
    },
    /// Add a viewpoint to an issue, with its PNG snapshot if it has one. Returns the ID in
    /// `created`.
    AddIssueViewpoint {
        issue: crate::issue::IssueId,
        viewpoint: Box<crate::issue::Viewpoint>,
        #[serde(default)]
        picture: Option<Box<crate::picture::ImageResource>>,
        now: i64,
    },
    /// Replace a viewpoint of an issue (the one with the same ID) with a new one, for example a
    /// new camera or a new snapshot. A snapshot that no viewpoint uses any more goes.
    ModifyIssueViewpoint {
        issue: crate::issue::IssueId,
        viewpoint: Box<crate::issue::Viewpoint>,
        #[serde(default)]
        picture: Option<Box<crate::picture::ImageResource>>,
        now: i64,
    },
    /// Delete a viewpoint of an issue. Comments that referred to it keep their text.
    DeleteIssueViewpoint {
        issue: crate::issue::IssueId,
        viewpoint: crate::issue::ViewpointId,
        now: i64,
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
    #[error("issue {0} does not exist")]
    NoIssue(crate::issue::IssueId),
    #[error("comment {0} does not exist")]
    NoComment(crate::issue::CommentId),
    #[error("invalid value: {0}")]
    Invalid(String),
}

/// Result of a successful command.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    /// IDs of new objects, layers and classes, in creation order.
    pub created: Vec<Uid>,
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

impl Command {
    /// The pictures that this command brings into the document, those of the commands in a batch
    /// too.
    pub fn pictures<'a>(&'a self, out: &mut Vec<&'a crate::picture::ImageResource>) {
        match self {
            Command::SetPlantPicture {
                picture: Some(p), ..
            } => out.push(p),
            Command::AddResource {
                body: ResourceBody::Image(i),
                ..
            } => out.push(i),
            Command::ModifyResource {
                body: Some(ResourceBody::Image(i)),
                ..
            } => out.push(i),
            Command::AddIssue { pictures, .. } => out.extend(pictures.iter()),
            Command::AddIssueViewpoint {
                picture: Some(p), ..
            }
            | Command::ModifyIssueViewpoint {
                picture: Some(p), ..
            } => out.push(p),
            Command::Batch { commands } => commands.iter().for_each(|c| c.pictures(out)),
            _ => {}
        }
    }
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
    /// The history of versions before and after, if the step changed it. It is apart from `meta`
    /// so that a new version does not copy the layers, classes and resources into the step.
    history: Option<(Option<HistoryData>, Option<HistoryData>)>,
    /// The issues before and after, if the step changed them (REV-04). The data is shared
    /// (`Arc`), so a step that did not touch the issues holds nothing.
    issues: Option<(crate::issue::Issues, crate::issue::Issues)>,
    /// A rough size in bytes, for the history limit (see [`Editor::set_history_limit`]).
    bytes: u64,
}

/// Costs in the estimate of the size of a patch, in bytes.
const OBJECT_BYTES: u64 = 320;
const META_PART_BYTES: u64 = 200;
/// What a picture resource counts in a patch. The text itself is shared and counts once, in the
/// document.
const IMAGE_PART_BYTES: u64 = 1024;

fn object_bytes(o: &Object) -> u64 {
    OBJECT_BYTES
        + match &o.shape {
            Shape::Underlay(u) => u.png.resident_len() as u64,
            _ => 0,
        }
}

fn issue_pairs<'a>(
    l: &[&'a crate::issue::Issue],
) -> Vec<(crate::issue::IssueId, &'a crate::issue::Issue)> {
    l.iter().map(|x| (x.id, *x)).collect()
}

fn comment_pairs<'a>(
    l: &[&'a crate::issue::Comment],
) -> Vec<(crate::issue::CommentId, &'a crate::issue::Comment)> {
    l.iter().map(|x| (x.id, *x)).collect()
}

/// The issues and comments that differ between two states. A change of a comment is also a
/// change of its issue, so a follower of the issues sees the activity. Only the chunks of the two
/// lists that the states do not share are compared: the rest did not change.
fn issue_changes(
    old: &crate::issue::Issues,
    new: &crate::issue::Issues,
    added: &mut IdsByKind,
    changed: &mut IdsByKind,
    removed: &mut IdsByKind,
) {
    if old == new {
        return;
    }
    let ((old_i, new_i), (old_c, new_c)) = new.unshared(old);
    let (a, c, r) = change::diff_by_id(&issue_pairs(&old_i), &issue_pairs(&new_i));
    (added.issues, changed.issues, removed.issues) = (a, c, r);
    let (a, c, r) = change::diff_by_id(&comment_pairs(&old_c), &comment_pairs(&new_c));
    (added.comments, changed.comments, removed.comments) = (a, c, r);
    let mut touched = Vec::new();
    for cid in added
        .comments
        .iter()
        .chain(&changed.comments)
        .chain(&removed.comments)
    {
        if let Some(c) = new_c.iter().chain(&old_c).find(|c| c.id == *cid) {
            touched.push(c.issue);
        }
    }
    for id in touched {
        if !changed.issues.contains(&id)
            && !added.issues.contains(&id)
            && !removed.issues.contains(&id)
        {
            changed.issues.push(id);
        }
    }
}

impl Meta {
    /// A rough size in bytes.
    fn approx_bytes(&self) -> u64 {
        // A picture is shared text: the steps of the history point to the same text. It counts as a
        // fixed cost, and the sum does not serialize megabytes of text.
        let resources: u64 = self
            .resources
            .iter()
            .map(|r| match &r.body {
                ResourceBody::Image(_) | ResourceBody::ImportBase(_) => IMAGE_PART_BYTES,
                _ => serde_json::to_vec(r).map_or(META_PART_BYTES, |v| v.len() as u64),
            })
            .sum();
        (self.layers.len() + self.classes.len()) as u64 * META_PART_BYTES + resources + 512
    }
}

impl Patch {
    /// What this patch changes, as a change set (forward) or what its undo changes (backward).
    fn change_set(&self, forward: bool, revision: u64, op: &str, origin: Origin) -> ChangeSet {
        let ids = |list: &[(usize, Object)]| list.iter().map(|(_, o)| o.id).collect::<Vec<_>>();
        let modified: Vec<ObjectId> = self.modified.iter().map(|(b, _)| b.id).collect();
        let (new, changed, gone) =
            change::object_changes(&ids(&self.added), &ids(&self.removed), &modified, forward);
        let mut added = IdsByKind {
            objects: new,
            ..Default::default()
        };
        let mut changed = IdsByKind {
            objects: changed,
            ..Default::default()
        };
        let mut removed = IdsByKind {
            objects: gone,
            ..Default::default()
        };
        let mut document = DocumentChanged {
            history: self.history.is_some(),
            ..DocumentChanged::default()
        };
        if let Some((before, after)) = &self.issues {
            let (old, new) = if forward {
                (before, after)
            } else {
                (after, before)
            };
            issue_changes(old, new, &mut added, &mut changed, &mut removed);
        }
        if let Some((before, after)) = &self.meta {
            let (old, new) = if forward {
                (before, after)
            } else {
                (after, before)
            };
            fn layers(m: &Meta) -> Vec<(LayerId, &Layer)> {
                m.layers.iter().map(|l| (l.id, l)).collect()
            }
            fn classes(m: &Meta) -> Vec<(ClassId, &Class)> {
                m.classes.iter().map(|l| (l.id, l)).collect()
            }
            fn resources(m: &Meta) -> Vec<(ResourceId, &Resource)> {
                m.resources.iter().map(|l| (l.id, l)).collect()
            }
            let (a, c, r) = change::diff_by_id(&layers(old), &layers(new));
            (added.layers, changed.layers, removed.layers) = (a, c, r);
            let (a, c, r) = change::diff_by_id(&classes(old), &classes(new));
            (added.classes, changed.classes, removed.classes) = (a, c, r);
            let (a, c, r) = change::diff_by_id(&resources(old), &resources(new));
            (added.resources, changed.resources, removed.resources) = (a, c, r);
            document = DocumentChanged {
                active_layer: old.active_layer != new.active_layer,
                active_class: old.active_class != new.active_class,
                georef: old.georef != new.georef,
                drawing: old.drawing != new.drawing,
                history: self.history.is_some(),
            };
        }
        ChangeSet {
            revision,
            op: op.to_string(),
            origin,
            added,
            changed,
            removed,
            document,
            detail: None,
        }
    }

    fn is_empty(&self) -> bool {
        self.removed.is_empty()
            && self.added.is_empty()
            && self.modified.is_empty()
            && self.meta.is_none()
            && self.history.is_none()
            && self.issues.is_none()
    }

    fn apply(&self, doc: &mut Document, forward: bool) {
        if forward {
            for (_, after) in &self.modified {
                if let Some(slot) = doc.objects.get_mut(&after.id) {
                    *slot = after.clone();
                }
            }
            remove_at(&mut doc.objects, &self.removed);
            insert_at(&mut doc.objects, &self.added);
        } else {
            remove_at(&mut doc.objects, &self.added);
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
        if let Some((before, after)) = &self.history {
            doc.history = if forward { after } else { before }.clone();
        }
        if let Some((before, after)) = &self.issues {
            doc.issues = if forward { after } else { before }.clone();
        }
    }
}

// Counts the calls that move all later entries of the list (`shift_insert`, `shift_remove`): each
// costs the size of the list, so a restore must make few of them. Tests read it.
#[cfg(test)]
thread_local! {
    static SHIFTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Removes the objects at these (ascending) indices.
fn remove_at(objects: &mut Objects, items: &[(usize, Object)]) {
    debug_assert!(
        items.windows(2).all(|w| w[0].0 < w[1].0),
        "indices must ascend"
    );
    if items.len() <= FEW {
        for (index, _) in items.iter().rev() {
            #[cfg(test)]
            SHIFTS.with(|c| c.set(c.get() + 1));
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
    debug_assert!(
        items.windows(2).all(|w| w[0].0 < w[1].0),
        "indices must ascend"
    );
    if items.len() <= FEW {
        for (index, o) in items {
            #[cfg(test)]
            SHIFTS.with(|c| c.set(c.get() + 1));
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

/// The items of the longest run that is increasing in the second field. Items are in the order of
/// the first list. O(n log n).
fn longest_increasing(items: &[(ObjectId, usize)]) -> Vec<ObjectId> {
    // tails[k]: the index of the item that ends the best run of length k + 1.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; items.len()];
    for (i, (_, p)) in items.iter().enumerate() {
        let k = tails.partition_point(|t| items[*t].1 < *p);
        prev[i] = k.checked_sub(1).map(|j| tails[j]);
        if k == tails.len() {
            tails.push(i);
        } else {
            tails[k] = i;
        }
    }
    let mut out = Vec::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        out.push(items[i].0);
        at = prev[i];
    }
    out.reverse();
    out
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

    /// Gives an object that this command made another ID.
    fn rename_new(&mut self, old: ObjectId, new: ObjectId) -> Result<(), CommandError> {
        if old == new {
            return Ok(());
        }
        if self.doc.objects.contains_key(&new) {
            return Err(CommandError::Invalid(format!(
                "an object with the ID {new} exists"
            )));
        }
        let at = self
            .doc
            .objects
            .get_index_of(&old)
            .expect("made by this command");
        let (_, mut o) = self.doc.objects.shift_remove_index(at).expect("checked");
        o.id = new;
        self.doc.objects.shift_insert(at, new, o);
        for id in self.added.iter_mut().filter(|i| **i == old) {
            *id = new;
        }
        if self.added_set.remove(&old) {
            self.added_set.insert(new);
        }
        Ok(())
    }

    fn alloc(&mut self) -> Result<Uid, CommandError> {
        Ok(self.doc.alloc_id())
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

    /// Makes the objects and the settings of the document equal to those of `target` (see
    /// `Editor::restore_content`). An object that is gone, changed or out of place in the z-order
    /// is removed and put back where `target` has it; the others stay.
    fn replace_content(&mut self, target: &Document) {
        let cur_ids: Vec<ObjectId> = self.doc.objects.keys().copied().collect();
        // The objects that keep their place: the longest run of the current order that is also in
        // the order of `target`, among the objects that did not change.
        let pos: HashMap<ObjectId, usize> = target
            .objects
            .keys()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
        let same: Vec<(ObjectId, usize)> = cur_ids
            .iter()
            .filter_map(|id| {
                let p = *pos.get(id)?;
                (self.doc.objects.get(id) == target.objects.get(id)).then_some((*id, p))
            })
            .collect();
        let keep: HashSet<ObjectId> = longest_increasing(&same).into_iter().collect();
        let remove: Vec<ObjectId> = cur_ids
            .iter()
            .copied()
            .filter(|id| !keep.contains(id))
            .collect();
        self.remove_objects(remove);
        // The new list in the order of `target`, made in one pass: the kept objects come from the
        // old list (they are in the same relative order), the others from `target`.
        let mut kept: HashMap<ObjectId, Object> =
            std::mem::take(&mut self.doc.objects).into_iter().collect();
        let mut objects = Objects::with_capacity(target.objects.len());
        for (id, o) in &target.objects {
            if keep.contains(id)
                && let Some(old) = kept.remove(id)
            {
                objects.insert(*id, old);
            } else {
                objects.insert(*id, o.clone());
                self.added.push(*id);
                self.added_set.insert(*id);
            }
        }
        self.doc.objects = objects;
        let doc = &mut *self.doc;
        doc.layers = target.layers.clone();
        doc.classes = target.classes.clone();
        // The pictures that issues use as snapshots are not part of a version: they stay.
        let snapshots: Vec<String> = doc
            .issues
            .snapshots()
            .into_iter()
            .map(String::from)
            .collect();
        let mut resources = target.resources.clone();
        for h in &snapshots {
            if resources.find_image(h).is_none()
                && let Some(rid) = doc.resources.find_image(h)
                && let Some(r) = doc.resources.get(rid)
            {
                resources.push(r.clone());
            }
        }
        doc.resources = resources;
        doc.drawing = target.drawing.clone();
        doc.georef = target.georef.clone();
        if doc.layer(doc.active_layer).is_none() {
            doc.active_layer = target.active_layer;
        }
        if doc.class(doc.active_class).is_none() {
            doc.active_class = target.active_class;
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
                #[cfg(feature = "fault-injection")]
                if crate::fault::take() {
                    panic!("injected fault after the first change");
                }
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
                    if let Shape::LandscapeArea { boundary, .. }
                    | Shape::Hardscape { boundary, .. } = s
                    {
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
                if let Some(v) = set.get(crate::provenance::KEY) {
                    crate::provenance::check(v).map_err(CommandError::Invalid)?;
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
            Command::SetLayerSource { id, source } => {
                if let Some(why) = source.as_ref().and_then(|s| s.problem()) {
                    return Err(CommandError::Invalid(format!(
                        "the layer source is not valid: {why}"
                    )));
                }
                let l = self
                    .doc
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *id)
                    .ok_or(CommandError::NoLayer(*id))?;
                match source {
                    Some(s) => l.extra.insert(
                        crate::provenance::LAYER_KEY.into(),
                        serde_json::to_value(s).expect("plain data"),
                    ),
                    None => l.extra.remove(crate::provenance::LAYER_KEY),
                };
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
            | Command::SetPlantStyle { .. }
            | Command::SetPlantPicture { .. } => self.run_plant(cmd, out)?,
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
            Command::AddIssue { .. }
            | Command::ModifyIssue { .. }
            | Command::DeleteIssue { .. }
            | Command::AddIssueComment { .. }
            | Command::EditIssueComment { .. }
            | Command::DeleteIssueComment { .. }
            | Command::AddIssueViewpoint { .. }
            | Command::ModifyIssueViewpoint { .. }
            | Command::DeleteIssueViewpoint { .. } => self.run_issue(cmd, out)?,
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
            Command::Arrange { ids, how } => {
                let ids = unique(ids);
                if ids.is_empty() {
                    return Err(CommandError::Invalid("give at least one object".into()));
                }
                for &id in &ids {
                    self.editable(id)?;
                }
                let moved: HashSet<ObjectId> = ids.iter().copied().collect();
                let order: Vec<ObjectId> = self.doc.objects.keys().copied().collect();
                let doc = &*self.doc;
                let layer_of = |i: ObjectId| doc.object(i).map_or(FIRST_LAYER, |o| o.layer);
                let target = arranged(&order, &layer_of, &moved, *how);
                if target != order {
                    // The moved objects leave and come back at their new places. That is how
                    // the patch records a change of the stacking order.
                    let mut objs: Vec<(ObjectId, Object)> = Vec::new();
                    for &id in &order {
                        if moved.contains(&id) {
                            objs.push((id, self.doc.object(id).expect("checked").clone()));
                        }
                    }
                    self.remove_objects(ids.clone());
                    let mut place: Vec<(usize, ObjectId)> = target
                        .iter()
                        .enumerate()
                        .filter(|(_, i)| moved.contains(i))
                        .map(|(k, i)| (k, *i))
                        .collect();
                    place.sort_unstable();
                    let mut by_id: std::collections::HashMap<ObjectId, Object> =
                        objs.into_iter().collect();
                    for (k, id) in place {
                        let o = by_id.remove(&id).expect("moved");
                        self.doc.objects.shift_insert(k, id, o);
                        self.added.push(id);
                        self.added_set.insert(id);
                    }
                }
            }
            Command::MoveLayer { id, index } => {
                let from = self
                    .doc
                    .layers
                    .iter()
                    .position(|l| l.id == *id)
                    .ok_or(CommandError::NoLayer(*id))?;
                let l = self.doc.layers.remove(from);
                let to = (*index).min(self.doc.layers.len());
                self.doc.layers.insert(to, l);
            }
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

    fn finish(
        self,
        meta_before: Meta,
        history_before: Option<HistoryData>,
        issues_before: crate::issue::Issues,
    ) -> Patch {
        let doc = &*self.doc;
        let removed: Vec<(usize, Object)> = self
            .removed
            .iter()
            .map(|(i, id)| (*i, self.before[id].clone()))
            .collect();
        // In the order of the list, not in the order they were made: an object that was made and
        // then arranged to the back comes first. `insert_at` and `remove_at` need this order.
        let mut added: Vec<(usize, Object)> = self
            .added
            .iter()
            .filter_map(|id| doc.objects.get_full(id))
            .map(|(i, _, o)| (i, o.clone()))
            .collect();
        added.sort_unstable_by_key(|(i, _)| *i);
        let modified: Vec<(Object, Object)> = self
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
        let history =
            (history_before != doc.history).then(|| (history_before, doc.history.clone()));
        let issues =
            (issues_before != doc.issues).then(|| (issues_before.lean(), doc.issues.lean()));
        let objects = |l: &[(usize, Object)]| l.iter().map(|(_, o)| object_bytes(o)).sum::<u64>();
        let bytes = objects(&removed)
            + objects(&added)
            + modified
                .iter()
                .map(|(b, a): &(Object, Object)| object_bytes(b) + object_bytes(a))
                .sum::<u64>()
            + meta
                .as_ref()
                .map_or(0, |(b, a)| b.approx_bytes() + a.approx_bytes())
            // The history is large and its parts are shared: a step counts what the new value
            // holds that the old one does not, and only if the step changed it. A layer edit
            // with a 16 MiB history in the document does not cost 32 MiB of undo budget, and a
            // new version costs its own change, not the whole history again.
            // A step of the issues keeps the pointers of the two states and the parts that they
            // do not share (the rest is shared with the document and counted there).
            + issues
                .as_ref()
                .map_or(0, |(b, a)| crate::issue::Issues::step_bytes(b, a))
            + history.as_ref().map_or(0, |(b, a)| {
                a.as_ref()
                    .map_or(0, |h| h.bytes_not_in(b.as_ref()) as u64)
            });
        Patch {
            removed,
            added,
            modified,
            meta,
            history,
            issues,
            bytes,
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
    /// Who makes the changes now (see [`Editor::set_origin`]).
    origin: Origin,
    /// The latest change sets (see `crate::change`).
    log: ChangeLog,
    /// Most bytes that the undo steps may hold (see [`Editor::set_history_limit`]).
    history_limit: u64,
    /// The largest picture file and the most pixels that a command may bring (see
    /// [`Editor::set_picture_limits`]).
    picture_limits: (usize, u64),
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
    /// An editor for an empty document whose IDs count 2, 3, 4 and so on (see
    /// `Document::with_sequential_ids`). For tests and fixtures.
    pub fn with_sequential_ids() -> Self {
        Editor::new(Document::with_sequential_ids())
    }

    /// Makes the IDs of this document count on from its largest small ID (see
    /// `Document::use_sequential_ids_after_existing`).
    pub fn use_sequential_ids(&mut self) {
        self.doc.use_sequential_ids_after_existing();
    }

    pub fn new(doc: Document) -> Self {
        let revision = next_revision();
        Editor {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            revision,
            origin: Origin::User,
            // A client that asks for changes before this editor existed gets `truncated`.
            log: ChangeLog::starting_at(revision),
            history_limit: u64::MAX,
            picture_limits: (
                crate::picture::MAX_PICTURE_BYTES,
                crate::picture::MAX_PICTURE_PIXELS,
            ),
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
    ///
    /// If the command panics, the changes it made so far are taken back before the panic goes on:
    /// the document, the revision and the history are what they were. (A program built with
    /// `panic = "abort"`, such as the web build, never gets here.)
    pub fn execute(&mut self, cmd: &Command) -> Result<Outcome, CommandError> {
        self.check_pictures(cmd)?;
        self.transact(
            || change::op_name(cmd).unwrap_or_else(|| "command".into()),
            None,
            |tx, out| tx.run(cmd, out),
        )
    }

    /// Runs a command and sets the history of the document (the named versions) in the same undo
    /// step, so that undo takes back both. `op` names the step in the change set, with an optional
    /// `detail` (see [`Editor::set_history_with`]). An update from DXF (IOP-23) saves its versions
    /// "before" and "after" with its changes this way.
    pub fn execute_with_history(
        &mut self,
        cmd: &Command,
        history: Option<crate::history::HistoryData>,
        op: &str,
        detail: Option<Value>,
    ) -> Result<Outcome, CommandError> {
        self.check_pictures(cmd)?;
        self.transact(
            || op.to_string(),
            detail,
            |tx, out| {
                tx.run(cmd, out)?;
                tx.doc.history = history;
                Ok(())
            },
        )
    }

    /// Runs `body` on a transaction as one undo step. The name of the step is made only when the
    /// step changed something.
    fn transact(
        &mut self,
        op: impl FnOnce() -> String,
        detail: Option<Value>,
        body: impl FnOnce(&mut Tx, &mut Outcome) -> Result<(), CommandError>,
    ) -> Result<Outcome, CommandError> {
        let meta_before = Meta::of(&self.doc);
        let history_before = self.doc.history.clone();
        // Without the index: the live data keeps it from one command to the next.
        let issues_before = self.doc.issues.lean();
        let mut tx = Tx::new(&mut self.doc);
        let mut out = Outcome::default();
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            body(&mut tx, &mut out).and_then(|()| tx.check_result())
        }));
        let result = match ran {
            Ok(r) => r,
            Err(payload) => {
                // Every change was recorded by `touch` and the lists of the transaction, so the
                // patch of a failed command undoes it.
                let patch = tx.finish(meta_before, history_before, issues_before);
                patch.apply(&mut self.doc, false);
                std::panic::resume_unwind(payload);
            }
        };
        let patch = tx.finish(meta_before, history_before, issues_before);
        let result = result.and_then(|()| match &patch.meta {
            Some((_, after)) if !after.is_finite() => Err(CommandError::Invalid(NOT_FINITE.into())),
            _ => Ok(()),
        });
        match result {
            Ok(()) => {
                if !patch.is_empty() {
                    self.revision = next_revision();
                    let mut cs = patch.change_set(true, self.revision, &op(), self.origin);
                    cs.detail = detail;
                    self.log.push(cs);
                    self.undo.push(patch);
                    self.redo.clear();
                    self.trim_history();
                }
                Ok(out)
            }
            Err(e) => {
                patch.apply(&mut self.doc, false);
                Err(e)
            }
        }
    }

    /// Puts the history that waited in the file into the document (DOC-21), with the notes of
    /// the check. It is no change of the document: there is no undo step and the revision stays,
    /// because the history was always the document's, only not read yet.
    pub fn adopt_history(
        &mut self,
        history: Option<crate::history::HistoryData>,
        notes: Vec<String>,
    ) {
        self.doc.history = history;
        self.doc.lazy_history = None;
        for n in notes {
            self.doc.add_load_note(n);
        }
    }

    /// Replaces the pictures that cannot be found by placeholders (see
    /// `Document::replace_missing_pictures`) as one undo step, so the person can take it back. The
    /// undo brings back the handles of the pictures that are not there; the next save asks again.
    /// Returns how many pictures were replaced.
    pub fn replace_missing_pictures_with_undo(
        &mut self,
        missing: &std::collections::HashSet<String>,
    ) -> usize {
        let mut count = 0;
        let ids: Vec<ObjectId> = self
            .doc
            .objects
            .iter()
            .filter(|(_, o)| {
                matches!(&o.shape, Shape::Underlay(u)
                    if u.png.lazy().is_some_and(|l| missing.contains(l.hash())))
            })
            .map(|(id, _)| *id)
            .collect();
        let _ = self.transact(
            || "replace_missing_pictures".to_string(),
            None,
            |tx, _| {
                for id in &ids {
                    tx.touch(*id);
                }
                count = tx.doc.replace_missing_pictures(missing);
                Ok(())
            },
        );
        count
    }

    /// Changes the document without an undo step and without a new revision. For a repair that
    /// makes the memory equal to the file that was just saved (the pictures that could not be
    /// found became placeholders in the file): it is no edit by the person.
    pub fn repair_without_undo(&mut self, f: impl FnOnce(&mut crate::model::Document)) {
        f(&mut self.doc);
    }

    /// Sets or clears the history of the document (the named versions, see `crate::history`) as
    /// one undo step. Returns false if nothing changed.
    pub fn set_history(&mut self, history: Option<crate::history::HistoryData>) -> bool {
        self.set_history_with(history, "set_history", None)
    }

    /// Like [`Editor::set_history`], with the name of the step (the `op` of the change set) and a
    /// detail that goes into the change set (what the change was, for the ones who follow the
    /// events).
    pub fn set_history_with(
        &mut self,
        history: Option<crate::history::HistoryData>,
        op: &str,
        detail: Option<Value>,
    ) -> bool {
        let before = self.revision;
        let _ = self.transact(
            || op.to_string(),
            detail,
            |tx, _| {
                tx.doc.history = history;
                Ok(())
            },
        );
        self.revision != before
    }

    /// Makes the content of the document equal to `target` as one undo step (REV-01 restore): the
    /// objects with their z-order, the layers, classes, resources, drawing data and the
    /// georeference. The history is set to `history`. The active layer and class stay if `target`
    /// has them. Objects that did not change keep their place in the undo step, so a restore of a
    /// big document costs what changed. Returns false if nothing changed.
    pub fn restore_content(
        &mut self,
        target: &Document,
        history: Option<crate::history::HistoryData>,
    ) -> Result<bool, CommandError> {
        self.restore_content_with(target, history, "restore", None)
    }

    /// Like [`Editor::restore_content`], with the name of the step and a detail for the change
    /// set (see [`Editor::set_history_with`]).
    pub fn restore_content_with(
        &mut self,
        target: &Document,
        history: Option<crate::history::HistoryData>,
        op: &str,
        detail: Option<Value>,
    ) -> Result<bool, CommandError> {
        let before = self.revision;
        self.transact(
            || op.to_string(),
            detail,
            |tx, _| {
                tx.replace_content(target);
                tx.doc.history = history;
                Ok(())
            },
        )?;
        Ok(self.revision != before)
    }

    pub fn undo(&mut self) -> bool {
        let Some(p) = self.undo.pop() else {
            return false;
        };
        p.apply(&mut self.doc, false);
        self.revision = next_revision();
        self.log
            .push(p.change_set(false, self.revision, "undo", self.origin));
        self.redo.push(p);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(p) = self.redo.pop() else {
            return false;
        };
        p.apply(&mut self.doc, true);
        self.revision = next_revision();
        self.log
            .push(p.change_set(true, self.revision, "redo", self.origin));
        self.undo.push(p);
        true
    }

    /// Makes this document equal to `doc`, which came from other users of a shared document
    /// (collaboration, PLAT-07). It is not a command: it has no undo step and it empties the undo
    /// and redo history, because the steps no longer fit the document. It logs one change set with
    /// the origin `Remote` that lists the added, changed and removed things, so subscribers learn
    /// exactly what changed and keep their caches for the rest. Returns `None` (and does nothing)
    /// if the document is already equal. The generator of IDs of the current document stays.
    pub fn apply_remote(&mut self, mut doc: Document) -> Option<ChangeSet> {
        // The history is local to the file: it does not travel with a shared document.
        doc.history = self.doc.history.clone();
        if doc == self.doc {
            return None;
        }
        fn objs(d: &Document) -> Vec<(ObjectId, &Object)> {
            d.objects.iter().map(|(k, v)| (*k, v)).collect()
        }
        fn layers(m: &Meta) -> Vec<(LayerId, &Layer)> {
            m.layers.iter().map(|l| (l.id, l)).collect()
        }
        fn classes(m: &Meta) -> Vec<(ClassId, &Class)> {
            m.classes.iter().map(|l| (l.id, l)).collect()
        }
        fn resources(m: &Meta) -> Vec<(ResourceId, &Resource)> {
            m.resources.iter().map(|l| (l.id, l)).collect()
        }
        let (a, c, r) = change::diff_by_id(&objs(&self.doc), &objs(&doc));
        let mut added = IdsByKind {
            objects: a,
            ..Default::default()
        };
        let mut changed = IdsByKind {
            objects: c,
            ..Default::default()
        };
        let mut removed = IdsByKind {
            objects: r,
            ..Default::default()
        };
        let (old, new) = (Meta::of(&self.doc), Meta::of(&doc));
        (added.layers, changed.layers, removed.layers) =
            change::diff_by_id(&layers(&old), &layers(&new));
        (added.classes, changed.classes, removed.classes) =
            change::diff_by_id(&classes(&old), &classes(&new));
        (added.resources, changed.resources, removed.resources) =
            change::diff_by_id(&resources(&old), &resources(&new));
        issue_changes(
            &self.doc.issues,
            &doc.issues,
            &mut added,
            &mut changed,
            &mut removed,
        );
        let document = DocumentChanged {
            active_layer: old.active_layer != new.active_layer,
            active_class: old.active_class != new.active_class,
            georef: old.georef != new.georef,
            drawing: old.drawing != new.drawing,
            history: false,
        };
        // The generator of IDs belongs to this editor, not to the document that came in.
        doc.swap_ids(&mut self.doc);
        self.doc = doc;
        self.undo.clear();
        self.redo.clear();
        self.revision = next_revision();
        let cs = ChangeSet {
            revision: self.revision,
            op: "remote".into(),
            origin: Origin::Remote,
            added,
            changed,
            removed,
            document,
            detail: None,
        };
        self.log.push(cs.clone());
        Some(cs)
    }

    /// Writes stacking keys into the objects and layers (the field `order` in `extra`) without a
    /// command, an undo step, a revision or a change set. The keys are bookkeeping for
    /// collaboration (see `kadakas-collab`): the document must carry them so that a saved file
    /// keeps them, but they are not a change that a user made or that anything draws, exports or
    /// caches (the revision exists to find stale caches, and nothing reads the keys but the
    /// collaboration code). An undo step needs no key either: it restores the objects it saved, and
    /// the collaboration code writes the keys again at the next sync. Returns true if a key
    /// changed.
    pub fn stamp_order_keys(
        &mut self,
        objects: &std::collections::HashMap<ObjectId, String>,
        layers: &std::collections::HashMap<LayerId, String>,
    ) -> bool {
        let mut changed = false;
        let set = |extra: &mut serde_json::Map<String, Value>, key: &String| {
            if extra.get("order").and_then(Value::as_str) == Some(key.as_str()) {
                return false;
            }
            extra.insert("order".into(), Value::String(key.clone()));
            true
        };
        for (id, key) in objects {
            if let Some(o) = self.doc.object_mut(*id) {
                changed |= set(&mut o.extra, key);
            }
        }
        for l in &mut self.doc.layers {
            if let Some(key) = layers.get(&l.id) {
                changed |= set(&mut l.extra, key);
            }
        }
        changed
    }

    /// Who makes the changes from now on. The change sets say it (see `crate::change`). A new
    /// editor starts with `Origin::User`.
    pub fn set_origin(&mut self, origin: Origin) {
        self.origin = origin;
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// The latest change sets: one for every command, undo and redo since the editor was made
    /// (older ones go when the log is full).
    pub fn change_log(&self) -> &ChangeLog {
        &self.log
    }

    /// The change sets with a revision above `revision`, oldest first, and whether older ones
    /// that the caller needs were dropped.
    pub fn changes_since(&self, revision: u64) -> change::ChangesSince<'_> {
        self.log.since(revision)
    }

    /// A rough size of the undo and redo steps in bytes.
    pub fn history_bytes(&self) -> u64 {
        self.undo.iter().chain(&self.redo).map(|p| p.bytes).sum()
    }

    /// A rough size of the change log in bytes.
    pub fn log_bytes(&self) -> u64 {
        self.log.approx_bytes()
    }

    /// Caps the undo steps by size: when the steps hold more bytes, the oldest ones are dropped
    /// (the newest step always stays). `u64::MAX`: no cap.
    /// Sets the largest picture file (bytes) and the most pixels that a command may bring into the
    /// document. The limits never go above those of the file format. Every way to add a picture
    /// passes here: `SetPlantPicture`, `AddResource`, `ModifyResource` and `Batch`.
    pub fn set_picture_limits(&mut self, max_bytes: usize, max_pixels: u64) {
        self.picture_limits = (
            max_bytes.min(crate::picture::MAX_PICTURE_BYTES),
            max_pixels.min(crate::picture::MAX_PICTURE_PIXELS),
        );
    }

    fn check_pictures(&self, cmd: &Command) -> Result<(), CommandError> {
        let (bytes, pixels) = self.picture_limits;
        let mut found = Vec::new();
        cmd.pictures(&mut found);
        for p in found {
            // A picture that stays in the file has a size up to 2 bytes too big (the padding is
            // not known without reading it): give it that much room, so a file of exactly the
            // limit is never refused for it.
            let len = if p.data.is_lazy() {
                p.byte_len().saturating_sub(2)
            } else {
                p.byte_len()
            };
            if len > bytes {
                return Err(CommandError::Invalid(format!(
                    "the picture file is larger than {bytes} bytes"
                )));
            }
            if u64::from(p.width) * u64::from(p.height) > pixels {
                return Err(CommandError::Invalid(format!(
                    "the picture has more than {pixels} pixels"
                )));
            }
        }
        Ok(())
    }

    pub fn set_history_limit(&mut self, bytes: u64) {
        self.history_limit = bytes;
        self.trim_history();
    }

    fn trim_history(&mut self) {
        if self.history_limit == u64::MAX {
            return;
        }
        let mut total = self.history_bytes();
        let mut drop = 0;
        while total > self.history_limit && drop + 1 < self.undo.len() {
            total -= self.undo[drop].bytes;
            drop += 1;
        }
        self.undo.drain(..drop);
        // Redo steps count too: they go first when the size is over the cap.
        while total > self.history_limit && !self.redo.is_empty() {
            total -= self.redo.remove(0).bytes;
        }
    }

    /// Number of steps that `undo` can take back.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Takes back the last `steps` steps and forgets them: they cannot be redone. For a host that
    /// refuses a call after it ran (see `SessionLimits`).
    pub fn rollback(&mut self, steps: usize) {
        if steps == 0 || self.undo.is_empty() {
            return;
        }
        for _ in 0..steps {
            let Some(p) = self.undo.pop() else { break };
            p.apply(&mut self.doc, false);
            self.revision = next_revision();
            self.log
                .push(p.change_set(false, self.revision, "rollback", self.origin));
        }
        self.redo.clear();
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

    fn line_json(x: f64) -> serde_json::Value {
        serde_json::json!({"op": "add_object", "shape":
            {"kind": "line", "a": [x, 0.0], "b": [x + 1.0, 1.0]}})
    }

    /// A batch that makes `n` lines and then arranges the ones in `which` to the back or front:
    /// the new objects are not made in the order they end up in. Redo and undo must give exactly
    /// the document, whatever `n` is (the helpers have a path for few objects and one for many).
    fn batch_with_arrange(n: usize, which: &[usize], how: &str) {
        // Some objects that were there before.
        let fresh = || {
            let mut ed = Editor::with_sequential_ids();
            for k in 0..3 {
                ed.execute(&serde_json::from_value(line_json(100.0 + f64::from(k))).unwrap())
                    .unwrap();
            }
            ed
        };
        let mut ed = fresh();
        let before = ed.doc().clone();
        // The ids of the new lines are the next ones in a sequential editor.
        let mut probe = fresh();
        let made: Vec<ObjectId> = (0..n)
            .map(|k| {
                let out = probe
                    .execute(&serde_json::from_value(line_json(f64::from(k as u32))).unwrap())
                    .unwrap();
                ObjectId(out.created[0])
            })
            .collect();
        let mut commands: Vec<serde_json::Value> = (0..n).map(|k| line_json(k as f64)).collect();
        for &w in which {
            commands.push(serde_json::json!({"op": "arrange", "ids": [made[w % n]], "how": how}));
        }
        let batch: Command =
            serde_json::from_value(serde_json::json!({"op": "batch", "commands": commands}))
                .unwrap();
        ed.execute(&batch).unwrap();
        let after = ed.doc().clone();
        assert_eq!(after.objects.len(), 3 + n);
        assert!(ed.undo(), "undo, n = {n}");
        assert_eq!(ed.doc(), &before, "undo left something behind, n = {n}");
        assert!(ed.redo(), "redo, n = {n}");
        assert_eq!(ed.doc(), &after, "redo, n = {n}");
        assert!(ed.undo());
        assert_eq!(ed.doc(), &before);
    }

    #[test]
    fn a_batch_that_arranges_what_it_made_undoes_and_redoes_exactly() {
        for n in 1..=20 {
            batch_with_arrange(n, &[n - 1], "back");
            batch_with_arrange(n, &[0], "front");
            batch_with_arrange(n, &[n - 1, 0], "back");
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn batches_with_arrange_undo_and_redo_exactly(
            n in 1usize..30,
            which in prop::collection::vec(0usize..30, 1..5),
            how in prop::sample::select(vec!["back", "front", "forward", "backward"]),
        ) {
            batch_with_arrange(n, &which, how);
        }
    }

    /// A restore of a big drawing, its undo and its redo move the entries of the list a few times,
    /// not once for each object (that was quadratic).
    #[test]
    fn a_restore_of_many_objects_does_not_shift_the_list_for_each_one() {
        let mut ed = Editor::with_sequential_ids();
        for i in 0..3000 {
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::new(f64::from(i), 0.0),
                    radius: 1.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        }
        let before = ed.doc().clone();
        // The target: every second object changed, and the order reversed for the others.
        let mut target = before.clone();
        let mut items: Vec<_> = target.objects.drain(..).collect();
        items.reverse();
        for (n, (id, mut o)) in items.into_iter().enumerate() {
            if n % 2 == 0 {
                o.shape = Shape::Circle {
                    center: DVec2::new(5.0, f64::from(n as u32)),
                    radius: 2.0,
                };
            }
            target.objects.insert(id, o);
        }
        SHIFTS.with(|c| c.set(0));
        assert!(ed.restore_content(&target, None).unwrap());
        assert_eq!(
            ed.doc().objects.keys().collect::<Vec<_>>(),
            target.objects.keys().collect::<Vec<_>>()
        );
        assert!(ed.doc().objects.values().eq(target.objects.values()));
        assert!(ed.undo());
        assert_eq!(ed.doc(), &before);
        assert!(ed.redo());
        assert!(ed.doc().objects.values().eq(target.objects.values()));
        let shifts = SHIFTS.with(std::cell::Cell::get);
        assert!(shifts <= 3 * FEW, "{shifts} shifts of a list of 3000");
    }
    use super::*;
    use proptest::prelude::*;

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
                    ids: vec![ObjectId::from_num(999)],
                },
            ],
        });
        assert_eq!(err, Err(CommandError::NoObject(ObjectId::from_num(999))));
        assert_eq!(ed.doc.objects, before);
        assert!(!ed.can_undo());
    }

    #[test]
    fn locked_layer_rejects_edits() {
        let mut ed = Editor::default();
        let id = ObjectId(ed.execute(&line(0.0)).unwrap().created[0]);
        ed.execute(&Command::ModifyLayer {
            id: LayerId::from_num(1),
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
        assert_eq!(r, Err(CommandError::LayerLocked(LayerId::from_num(1))));
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
            ed.execute(&add(Some(LayerId::from_num(99)), None)),
            Err(CommandError::NoLayer(LayerId::from_num(99)))
        );
        assert_eq!(
            ed.execute(&add(None, Some(ClassId::from_num(99)))),
            Err(CommandError::NoClass(ClassId::from_num(99)))
        );
        lock_layer(&mut ed, LayerId::from_num(1), true);
        assert_eq!(
            ed.execute(&add(None, None)),
            Err(CommandError::LayerLocked(LayerId::from_num(1)))
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
                ids: vec![ObjectId::from_num(999)]
            }),
            Err(CommandError::NoObject(ObjectId::from_num(999)))
        );
        assert_eq!(ed.doc.objects.len(), 2);

        ed.execute(&Command::DeleteObjects { ids: vec![a] })
            .unwrap();
        assert!(ed.doc.object(a).is_none());
        assert!(ed.doc.object(b).is_some());

        lock_layer(&mut ed, LayerId::from_num(1), true);
        assert_eq!(
            ed.execute(&Command::DeleteObjects { ids: vec![b] }),
            Err(CommandError::LayerLocked(LayerId::from_num(1)))
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
            ids: vec![a, ObjectId::from_num(999)],
            delta: DVec2::X,
        });
        assert_eq!(r, Err(CommandError::NoObject(ObjectId::from_num(999))));
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
            ed.execute(&modify(Some(LayerId::from_num(99)), None)),
            Err(CommandError::NoLayer(LayerId::from_num(99)))
        );
        assert_eq!(
            ed.execute(&modify(None, Some(ClassId::from_num(99)))),
            Err(CommandError::NoClass(ClassId::from_num(99)))
        );
        assert_eq!(
            ed.execute(&Command::ModifyObject {
                id: ObjectId::from_num(999),
                shape: None,
                layer: None,
                class: None,
                attrs: None
            }),
            Err(CommandError::NoObject(ObjectId::from_num(999)))
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
            id: LayerId::from_num(1),
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
            id: LayerId::from_num(1),
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
            id: LayerId::from_num(99),
            name: None,
            visible: None,
            locked: None,
            elevation: None,
            scale: None,
        });
        assert_eq!(r, Err(CommandError::NoLayer(LayerId::from_num(99))));
    }

    #[test]
    fn delete_layer_rules() {
        let mut ed = Editor::default();
        assert_eq!(
            ed.execute(&Command::DeleteLayer {
                id: LayerId::from_num(99)
            }),
            Err(CommandError::NoLayer(LayerId::from_num(99)))
        );
        assert_eq!(
            ed.execute(&Command::DeleteLayer {
                id: LayerId::from_num(1)
            }),
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
                id: ClassId::from_num(99),
                name: None,
                visible: None,
                attrs: None
            }),
            Err(CommandError::NoClass(ClassId::from_num(99)))
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
                layer: Some(LayerId::from_num(99)),
                class: None
            }),
            Err(CommandError::NoLayer(LayerId::from_num(99)))
        );
        assert_eq!(
            ed.execute(&Command::SetActive {
                layer: None,
                class: Some(ClassId::from_num(99))
            }),
            Err(CommandError::NoClass(ClassId::from_num(99)))
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
                ids: vec![ObjectId::from_num(999)]
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
            CommandError::NoObject(ObjectId::from_num(3)).to_string(),
            format!("object {} does not exist", ObjectId::from_num(3)).as_str()
        );
        assert_eq!(
            CommandError::NoLayer(LayerId::from_num(4)).to_string(),
            format!("layer {} does not exist", LayerId::from_num(4)).as_str()
        );
        assert_eq!(
            CommandError::NoClass(ClassId::from_num(5)).to_string(),
            format!("class {} does not exist", ClassId::from_num(5)).as_str()
        );
        assert_eq!(
            CommandError::LayerLocked(LayerId::from_num(6)).to_string(),
            format!("layer {} is locked", LayerId::from_num(6)).as_str()
        );
        assert_eq!(
            CommandError::LayerNotEmpty(LayerId::from_num(7)).to_string(),
            format!("layer {} is not empty", LayerId::from_num(7)).as_str()
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
            serde_json::from_str(r#"{"op":"move_objects","ids":["00000000-0000-0000-0000-000000000002"],"delta":[1.5,-2]}"#).unwrap();
        assert_eq!(
            c,
            Command::MoveObjects {
                ids: vec![ObjectId::from_num(2)],
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
    fn typed_ids_are_uuid_texts_in_json() {
        assert_eq!(
            serde_json::to_value(ObjectId::from_num(7)).unwrap(),
            serde_json::json!("00000000-0000-0000-0000-000000000007")
        );
        assert_eq!(
            serde_json::from_str::<LayerId>("\"00000000-0000-0000-0000-000000000003\"").unwrap(),
            LayerId::from_num(3)
        );
        assert!(
            serde_json::from_str::<LayerId>("3").is_err(),
            "a number is not an ID"
        );
        assert_eq!(
            ObjectId::from_num(9).to_string(),
            "00000000-0000-0000-0000-000000000009"
        );
        assert_eq!(Uid::from(ClassId::from_num(4)), Uid::from_num(4));

        // New layers, classes and objects get different, version 7 IDs, in creation order.
        let mut ed = Editor::default();
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
        assert!(lid < cid);
        assert_eq!((lid.version(), cid.version()), (7, 7));
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
        // The batch deletes the object it just made: its ID is planned.
        let planned = ObjectId(ed.doc.next_ids(1)[0]);
        let r = ed.execute(&Command::Batch {
            commands: vec![line(0.0), Command::DeleteObjects { ids: vec![planned] }],
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
        let new = ObjectId(ed.doc.next_ids(1)[0]);
        ed.execute(&Command::Batch {
            commands: vec![
                line(0.0),
                Command::MoveObjects {
                    ids: vec![new, keep],
                    delta: DVec2::X,
                },
                Command::SetData {
                    id: new,
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
                id: ObjectId::from_num(999),
                set: BTreeMap::new(),
                remove: vec![]
            }),
            Err(CommandError::NoObject(ObjectId::from_num(999)))
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
            serde_json::from_str(r#"{"op":"set_data","id":"00000000-0000-0000-0000-000000000002","set":{"k":[1]},"remove":["z"]}"#)
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
            Vec::<Uid>::new()
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
            ed.execute(&set(ObjectId::from_num(999), 0, None, None)),
            Err(CommandError::NoObject(ObjectId::from_num(999)))
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
            r#"{"op":"set_vertex","id":"00000000-0000-0000-0000-000000000002","index":1,"kind":{"kind":"arc","radius":30}}"#,
        )
        .unwrap();
        assert_eq!(
            c,
            Command::SetVertex {
                id: ObjectId::from_num(2),
                index: 1,
                kind: Some(VertexKind::Arc { radius: 30.0 }),
                p: None
            }
        );
        let c: Command =
            serde_json::from_str(r#"{"op":"set_vertex","id":"00000000-0000-0000-0000-000000000002","index":0,"p":[1,2]}"#).unwrap();
        assert_eq!(
            c,
            Command::SetVertex {
                id: ObjectId::from_num(2),
                index: 0,
                kind: None,
                p: Some(DVec2::new(1.0, 2.0))
            }
        );
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;

    #[test]
    fn apply_remote_logs_a_remote_change_set() {
        let mut ed = Editor::with_sequential_ids();
        let cmd: Command = serde_json::from_value(serde_json::json!({
            "op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5, 5]}
        }))
        .unwrap();
        // The same document, edited elsewhere.
        let mut ed2 = Editor::new(ed.doc().clone());
        let id = ed2.execute(&cmd).unwrap().created[0];
        let other = ed2.doc().clone();
        let rev = ed.revision();
        assert_eq!(ed.apply_remote(ed.doc().clone()), None);
        let cs = ed.apply_remote(other.clone()).unwrap();
        assert_eq!(cs.origin, Origin::Remote);
        assert_eq!(cs.added.objects, vec![ObjectId(id)]);
        assert_ne!(ed.revision(), rev);
        assert_eq!(ed.changes_since(rev).changes.len(), 1);
        assert_eq!(ed.doc().objects.len(), 1);
        assert!(!ed.can_undo());
        let mut third = other.clone();
        third.objects.clear();
        let cs = ed.apply_remote(third).unwrap();
        assert_eq!(cs.removed.objects, vec![ObjectId(id)]);
        assert_eq!(Origin::parse("remote"), Some(Origin::Remote));
        assert_eq!(Origin::Remote.name(), "remote");
    }
}

#[cfg(test)]
mod arrange_tests {
    use super::*;

    fn rect(ed: &mut Editor, layer: Option<LayerId>) -> ObjectId {
        let cmd: Command = serde_json::from_value(serde_json::json!({
            "op": "add_object", "layer": layer,
            "shape": {"kind": "rect", "min": [0, 0], "max": [5, 5]}
        }))
        .unwrap();
        ObjectId(ed.execute(&cmd).unwrap().created[0])
    }

    fn order(ed: &Editor) -> Vec<ObjectId> {
        ed.doc().objects.keys().copied().collect()
    }

    fn arrange(ed: &mut Editor, ids: &[ObjectId], how: Arrange) {
        ed.execute(&Command::Arrange {
            ids: ids.to_vec(),
            how,
        })
        .unwrap();
    }

    #[test]
    fn arrange_moves_in_the_stacking_order_and_undoes() {
        let mut ed = Editor::with_sequential_ids();
        let ids: Vec<ObjectId> = (0..5).map(|_| rect(&mut ed, None)).collect();
        let start = order(&ed);
        arrange(&mut ed, &[ids[1]], Arrange::Front);
        assert_eq!(order(&ed), vec![ids[0], ids[2], ids[3], ids[4], ids[1]]);
        arrange(&mut ed, &[ids[3], ids[4]], Arrange::Back);
        assert_eq!(order(&ed), vec![ids[3], ids[4], ids[0], ids[2], ids[1]]);
        arrange(&mut ed, &[ids[3]], Arrange::Forward);
        assert_eq!(order(&ed), vec![ids[4], ids[3], ids[0], ids[2], ids[1]]);
        arrange(&mut ed, &[ids[2]], Arrange::Backward);
        assert_eq!(order(&ed), vec![ids[4], ids[3], ids[2], ids[0], ids[1]]);
        // The moved object is in the change set as changed, not added or removed.
        let rev = ed.revision();
        arrange(&mut ed, &[ids[0]], Arrange::Front);
        let cs = ed
            .changes_since(rev)
            .changes
            .last()
            .cloned()
            .cloned()
            .unwrap();
        assert_eq!(cs.changed.objects, vec![ids[0]]);
        assert!(cs.added.objects.is_empty() && cs.removed.objects.is_empty());
        // Undo and redo bring the order back, step by step.
        for _ in 0..5 {
            assert!(ed.undo());
        }
        assert_eq!(order(&ed), start);
        for _ in 0..5 {
            assert!(ed.redo());
        }
        assert_eq!(order(&ed), vec![ids[4], ids[3], ids[2], ids[1], ids[0]]);
        // An arrange that changes nothing makes no step.
        let depth = ed.undo_depth();
        arrange(&mut ed, &[ids[0]], Arrange::Front);
        assert_eq!(ed.undo_depth(), depth);
        assert!(matches!(
            ed.execute(&Command::Arrange {
                ids: vec![],
                how: Arrange::Front
            }),
            Err(CommandError::Invalid(_))
        ));
    }

    #[test]
    fn forward_and_backward_pass_only_objects_of_the_same_layer() {
        let mut ed = Editor::with_sequential_ids();
        ed.execute(&Command::AddLayer {
            name: "B".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        })
        .unwrap();
        let other = ed.doc().layers[1].id;
        let a = rect(&mut ed, None);
        let x = rect(&mut ed, Some(other));
        let b = rect(&mut ed, None);
        arrange(&mut ed, &[a], Arrange::Forward);
        assert_eq!(order(&ed), vec![x, b, a]);
        arrange(&mut ed, &[a], Arrange::Forward);
        assert_eq!(order(&ed), vec![x, b, a], "already on top of its layer");
        arrange(&mut ed, &[a], Arrange::Backward);
        assert_eq!(order(&ed), vec![x, a, b]);
    }

    #[test]
    fn layers_move_and_undo() {
        let mut ed = Editor::with_sequential_ids();
        for n in ["B", "C"] {
            ed.execute(&Command::AddLayer {
                name: n.into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap();
        }
        let names = |ed: &Editor| -> Vec<String> {
            ed.doc().layers.iter().map(|l| l.name.clone()).collect()
        };
        let c = ed.doc().layers[2].id;
        ed.execute(&Command::MoveLayer { id: c, index: 0 }).unwrap();
        assert_eq!(names(&ed), vec!["C", "Layer 1", "B"]);
        ed.execute(&Command::MoveLayer { id: c, index: 99 })
            .unwrap();
        assert_eq!(names(&ed), vec!["Layer 1", "B", "C"]);
        assert!(ed.undo());
        assert_eq!(names(&ed), vec!["C", "Layer 1", "B"]);
        assert!(matches!(
            ed.execute(&Command::MoveLayer {
                id: LayerId::from_num(99),
                index: 0
            }),
            Err(CommandError::NoLayer(_))
        ));
    }

    #[test]
    fn a_command_and_the_history_are_one_undo_step() {
        use crate::history::HistoryData;
        let mut ed = Editor::new(Document::with_sequential_ids());
        let add = Command::AddObject {
            shape: Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        };
        let history = HistoryData::new(serde_json::json!({"versions": []}));
        ed.execute_with_history(
            &add,
            Some(history.clone()),
            "import_update",
            Some(serde_json::json!({"x": 1})),
        )
        .unwrap();
        assert_eq!(ed.doc().objects.len(), 1);
        assert_eq!(ed.doc().history, Some(history.clone()));
        // Undo takes back the object and the history together; redo brings both.
        assert!(ed.undo());
        assert!(ed.doc().objects.is_empty() && ed.doc().history.is_none());
        assert!(ed.redo());
        assert_eq!(ed.doc().objects.len(), 1);
        assert_eq!(ed.doc().history, Some(history));
        // A failing command changes nothing, also not the history.
        let before = ed.doc().clone();
        let bad = Command::DeleteObjects {
            ids: vec![ObjectId::from_num(999)],
        };
        assert!(
            ed.execute_with_history(&bad, None, "import_update", None)
                .is_err()
        );
        assert_eq!(ed.doc(), &before);
    }
}
