//! The logic of the Scenes panel and the style picker of the 3D modeler (MOD-10).
//!
//! A scene is a saved view of the document (`Drawing::saved_views`): the visibility of tags
//! (classes) and layers, the active tag and layer, the camera and the render style. The list of
//! saved views is the list of scenes, in its order. The Vectorworks-like app shows the same list
//! in its Saved Views palette, so one file shows the same scenes in both programs.
//!
//! This module has no egui. It reads the document and gives back rows and commands, and the
//! window draws the rows and runs the commands. Every command is one undo step. The camera of a
//! scene is a free orbit camera ([`ViewCamera`]); a saved view that the app made has a standard
//! view instead, and the module turns that into a camera too.
//!
//! The wording of the feature follows the public SketchUp help (scenes, styles), not verified
//! against the product.

use glam::Vec3;
use kadakas_core::command::Command;
use kadakas_core::model::Document;
use kadakas_core::present::RenderStyle;
use kadakas_core::sheets::{SavedView, ViewCamera};
use kadakas_render::{Camera, Projection, StandardView};

/// The length of the camera transition when a scene is chosen, in seconds.
pub const TRANSITION_SECONDS: f64 = 0.5;

/// Why a scene command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Empty,
    /// Another scene has the name (the comparison ignores case).
    Taken,
    NoSuchScene,
    /// The camera has a value that is not finite.
    BadCamera,
}

/// One row of the list of scenes.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    /// Place in the list, from 0.
    pub index: usize,
    /// The scene that was chosen last, if its saved view still shows.
    pub active: bool,
    /// The scene has a free camera of its own (a view of the app has a standard view).
    pub has_camera: bool,
    /// The name of the render style of the scene, if it has one.
    pub style: Option<String>,
    /// How many tags and layers the scene hides.
    pub hidden: usize,
}

/// The rows of the panel, in the order of the saved views. `active` is the name of the scene that
/// was chosen last.
pub fn rows(doc: &Document, active: Option<&str>) -> Vec<Row> {
    doc.drawing
        .saved_views
        .iter()
        .enumerate()
        .map(|(index, v)| Row {
            name: v.name.clone(),
            index,
            active: active.is_some_and(|a| a.eq_ignore_ascii_case(&v.name)),
            has_camera: v.camera.is_some(),
            style: v.render_style.clone(),
            hidden: v.layers.values().filter(|on| !**on).count()
                + v.classes.values().filter(|on| !**on).count(),
        })
        .collect()
}

/// The scene with this name.
pub fn find<'a>(doc: &'a Document, name: &str) -> Option<&'a SavedView> {
    doc.drawing.saved_view(name)
}

/// A name for a new scene that no scene has: `Scene 1`, `Scene 2` and so on. The name goes into
/// the file, so it is the same in every language (like `Layer 3`).
pub fn new_name(doc: &Document) -> String {
    (1..)
        .map(|n| format!("Scene {n}"))
        .find(|name| find(doc, name).is_none())
        .expect("a free number exists")
}

fn check_name(doc: &Document, name: &str, except: Option<&str>) -> Result<String, Problem> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Problem::Empty);
    }
    let taken = doc.drawing.saved_views.iter().any(|v| {
        !except.is_some_and(|e| e.trim().eq_ignore_ascii_case(&v.name))
            && v.name.eq_ignore_ascii_case(name)
    });
    if taken {
        Err(Problem::Taken)
    } else {
        Ok(name.to_string())
    }
}

/// The saved camera of an orbit camera.
pub fn camera_to_saved(c: &Camera) -> ViewCamera {
    ViewCamera {
        target: [c.target.x as f64, c.target.y as f64, c.target.z as f64],
        yaw: c.yaw as f64,
        pitch: c.pitch as f64,
        distance: c.distance as f64,
        fov: c.fov_y.to_degrees() as f64,
        projection: c.projection.id().to_string(),
    }
}

/// The orbit camera of a saved camera. A projection that is not known gives the perspective one.
pub fn camera_from_saved(v: &ViewCamera) -> Camera {
    Camera {
        target: Vec3::new(v.target[0] as f32, v.target[1] as f32, v.target[2] as f32),
        yaw: v.yaw as f32,
        pitch: v.pitch as f32,
        distance: v.distance as f32,
        projection: Projection::from_name(&v.projection).unwrap_or(Projection::Perspective),
        fov_y: (v.fov as f32).to_radians(),
    }
}

/// The saved view of the present state: the visibility of all layers and tags, the active layer
/// and tag, the camera, the standard view that is nearest to it (the app uses that one) and the
/// active render style. The zoom and pan of the 2D view are left out: a scene is a 3D view.
pub fn capture(doc: &Document, name: &str, camera: &Camera) -> SavedView {
    SavedView {
        name: name.trim().to_string(),
        layers: doc.layers.iter().map(|l| (l.id, l.visible)).collect(),
        classes: doc.classes.iter().map(|c| (c.id, c.visible)).collect(),
        active_layer: Some(doc.active_layer),
        active_class: Some(doc.active_class),
        zoom: None,
        pan: None,
        view: Some(nearest_standard(camera).id().to_string()),
        projection: Some(camera.projection.id().to_string()),
        mode: None,
        camera: Some(camera_to_saved(camera)),
        render_style: active_style_name(doc),
    }
}

/// The name of the active render style of the document. `None`: the built-in shaded style is
/// active and the document does not hold it.
pub fn active_style_name(doc: &Document) -> Option<String> {
    doc.drawing
        .presentation
        .active_style
        .and_then(|id| doc.resources.get(id))
        .map(|r| r.name.clone())
}

/// The standard view whose direction is nearest to the camera. The app shows a scene by this
/// view, since it has no free camera.
pub fn nearest_standard(camera: &Camera) -> StandardView {
    let angle = |a: f32, b: f32| {
        let d = (a - b).rem_euclid(std::f32::consts::TAU);
        d.min(std::f32::consts::TAU - d)
    };
    StandardView::ALL
        .into_iter()
        .filter(|v| !matches!(v, StandardView::Plan))
        .min_by(|a, b| {
            let cost = |v: &StandardView| {
                let (yaw, pitch) = v.yaw_pitch();
                angle(camera.yaw, yaw) + (camera.pitch - pitch).abs()
            };
            cost(a).total_cmp(&cost(b))
        })
        .unwrap_or(StandardView::RightIsometric)
}

/// Add Scene: stores the present state under a new name.
pub fn add(doc: &Document, name: &str, camera: &Camera) -> Result<Command, Problem> {
    let name = check_name(doc, name, None)?;
    save(capture(doc, &name, camera))
}

/// Update: stores the present camera, visibility and style in the scene that already exists. It
/// keeps its place in the list.
pub fn update(doc: &Document, name: &str, camera: &Camera) -> Result<Command, Problem> {
    let found = find(doc, name).ok_or(Problem::NoSuchScene)?;
    save(capture(doc, &found.name, camera))
}

fn save(view: SavedView) -> Result<Command, Problem> {
    if !view.is_finite() {
        return Err(Problem::BadCamera);
    }
    Ok(Command::SaveView {
        view: Box::new(view),
    })
}

/// Rename.
pub fn rename(doc: &Document, name: &str, new_name: &str) -> Result<Command, Problem> {
    let found = find(doc, name).ok_or(Problem::NoSuchScene)?;
    let new_name = check_name(doc, new_name, Some(&found.name))?;
    Ok(Command::RenameView {
        name: found.name.clone(),
        new_name,
    })
}

/// Delete. The panel asks first when the person wants a question.
pub fn delete(doc: &Document, name: &str) -> Result<Command, Problem> {
    let found = find(doc, name).ok_or(Problem::NoSuchScene)?;
    Ok(Command::DeleteView {
        name: found.name.clone(),
    })
}

/// Move a scene to a place of the list.
pub fn move_to(doc: &Document, name: &str, to: usize) -> Result<Command, Problem> {
    let found = find(doc, name).ok_or(Problem::NoSuchScene)?;
    Ok(Command::MoveView {
        name: found.name.clone(),
        to,
    })
}

/// Move a scene one place up (`-1`) or down (`1`). `None` at the ends of the list.
pub fn move_by(doc: &Document, name: &str, delta: isize) -> Result<Option<Command>, Problem> {
    let found = find(doc, name).ok_or(Problem::NoSuchScene)?;
    let i = doc
        .drawing
        .saved_views
        .iter()
        .position(|v| v.name == found.name)
        .ok_or(Problem::NoSuchScene)?;
    let to = i as isize + delta;
    if to < 0 || to as usize >= doc.drawing.saved_views.len() {
        return Ok(None);
    }
    move_to(doc, name, to as usize).map(Some)
}

/// Where the camera goes when a scene is chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    /// The scene has a free camera.
    Camera(Camera),
    /// A saved view of the app: a standard view, with the projection of the view if it has one.
    /// The window frames the model for it.
    Standard(StandardView, Option<Projection>),
    /// The scene has no camera (it is a 2D view of the app): the camera stays.
    Stay,
}

/// What choosing a scene does: one command (visibility, active tag and layer, render style: one
/// undo step) and the camera goal.
#[derive(Clone, Debug, PartialEq)]
pub struct Activation {
    pub command: Command,
    pub goal: Goal,
}

/// Choose a scene. The command is `RestoreView`, which also sets the render style, so the same
/// call works through `saved_view.restore` for an agent.
pub fn activate(doc: &Document, name: &str) -> Result<Activation, Problem> {
    let v = find(doc, name).ok_or(Problem::NoSuchScene)?;
    let goal = match (
        &v.camera,
        v.view.as_deref().and_then(StandardView::from_name),
    ) {
        (Some(c), _) if c.is_valid() => Goal::Camera(camera_from_saved(c)),
        (_, Some(s)) if !s.is_plan() => {
            Goal::Standard(s, v.projection.as_deref().and_then(Projection::from_name))
        }
        (_, Some(_)) => Goal::Standard(StandardView::Top, None),
        _ => Goal::Stay,
    };
    Ok(Activation {
        command: Command::RestoreView {
            name: v.name.clone(),
        },
        goal,
    })
}

/// Does the scene still match the present state? Used to show that the active scene has changes
/// that Update would store (the camera moved, a tag was hidden or the style changed).
pub fn is_modified(doc: &Document, name: &str, camera: &Camera) -> bool {
    let Some(v) = find(doc, name) else {
        return false;
    };
    let now = capture(doc, &v.name, camera);
    let camera_same = match (&v.camera, &now.camera) {
        (Some(a), Some(b)) => same_camera(a, b),
        _ => true,
    };
    let layers_same = v
        .layers
        .iter()
        .all(|(id, on)| doc.layer(*id).is_none_or(|l| l.visible == *on));
    let classes_same = v
        .classes
        .iter()
        .all(|(id, on)| doc.class(*id).is_none_or(|c| c.visible == *on));
    let now_style = now.render_style.as_deref().unwrap_or("Shaded");
    let style_same = v
        .render_style
        .as_deref()
        .is_none_or(|s| s.eq_ignore_ascii_case(now_style));
    !(camera_same && layers_same && classes_same && style_same)
}

fn same_camera(a: &ViewCamera, b: &ViewCamera) -> bool {
    let near = |x: f64, y: f64, tol: f64| (x - y).abs() <= tol;
    let scale = a.distance.max(b.distance).max(1.0);
    a.projection == b.projection
        && a.target
            .iter()
            .zip(&b.target)
            .all(|(x, y)| near(*x, *y, scale * 1e-4))
        && near(a.yaw, b.yaw, 1e-4)
        && near(a.pitch, b.pitch, 1e-4)
        && near(a.distance, b.distance, scale * 1e-4)
        && near(a.fov, b.fov, 1e-3)
}

// --- Styles -----------------------------------------------------------------------------

/// One entry of the style picker.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRow {
    pub name: String,
    /// A built-in style. It joins the document when the person picks it.
    pub builtin: bool,
    /// The document does not hold it yet (a built-in style that was never used).
    pub in_document: bool,
    pub active: bool,
}

/// The entries of the style picker: the built-in styles first, then the other styles of the
/// document. A built-in style that the document holds is one entry. With no active style, the
/// built-in Shaded style is the active one.
pub fn styles(doc: &Document) -> Vec<StyleRow> {
    let active = active_style_name(doc);
    let doc_styles: Vec<&str> = doc
        .resources
        .iter()
        .filter(|r| kadakas_core::present::render_style(doc, r.id).is_some())
        .map(|r| r.name.as_str())
        .collect();
    let has = |n: &str| doc_styles.iter().any(|d| d.eq_ignore_ascii_case(n));
    let mut rows: Vec<StyleRow> = RenderStyle::presets()
        .into_iter()
        .map(|(n, _)| StyleRow {
            name: n.to_string(),
            builtin: true,
            in_document: has(n),
            active: match &active {
                Some(a) => a.eq_ignore_ascii_case(n),
                None => n == "Shaded",
            },
        })
        .collect();
    for n in &doc_styles {
        if !rows.iter().any(|r| r.name.eq_ignore_ascii_case(n)) {
            rows.push(StyleRow {
                name: (*n).to_string(),
                builtin: false,
                in_document: true,
                active: active.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(n)),
            });
        }
    }
    rows
}

/// Choose a style: one command, one undo step. `None` when the style is the active one already.
pub fn use_style(doc: &Document, name: &str) -> Option<Command> {
    let row = styles(doc)
        .into_iter()
        .find(|r| r.name.eq_ignore_ascii_case(name.trim()))?;
    (!row.active).then(|| Command::UseRenderStyle { name: row.name })
}

// --- The camera transition --------------------------------------------------------------

/// An animated move of the camera from one view to another. The window asks for the camera at
/// each frame until `done`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    from: Camera,
    to: Camera,
    start: f64,
    seconds: f64,
}

impl Transition {
    pub fn new(from: Camera, to: Camera, now: f64) -> Transition {
        Transition {
            from,
            to,
            start: now,
            seconds: TRANSITION_SECONDS,
        }
    }

    /// The camera at time `now` (seconds) and whether the move is over.
    pub fn at(&self, now: f64) -> (Camera, bool) {
        let t = ((now - self.start) / self.seconds).clamp(0.0, 1.0);
        if !t.is_finite() || t >= 1.0 {
            return (self.to, true);
        }
        let e = (t * t * (3.0 - 2.0 * t)) as f32;
        let (a, b) = (&self.from, &self.to);
        // The turn goes the short way round.
        let dyaw = (b.yaw - a.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let c = Camera {
            target: a.target.lerp(b.target, e),
            yaw: a.yaw + dyaw * e,
            pitch: a.pitch + (b.pitch - a.pitch) * e,
            // Distances multiply, so a zoom feels even.
            distance: (a.distance.max(1e-3).ln() * (1.0 - e) + b.distance.max(1e-3).ln() * e).exp(),
            // A change of projection happens at the start, since two projections cannot blend.
            projection: b.projection,
            fov_y: a.fov_y + (b.fov_y - a.fov_y) * e,
        };
        (c, false)
    }

    pub fn goal(&self) -> Camera {
        self.to
    }
}

#[cfg(test)]
mod tests;
