//! The agent drives the view of the person and points (AGR-02, AGR-03).
//!
//! Methods: `view.get`, `view.zoom`, `view.pan`, `view.camera`, `view.highlight` and
//! `view.highlight_clear`. They need the window of the person: the camera, the size of the canvas
//! and the drawing of the pointers are in the app. The headless server (`kadakas serve`) has no
//! window, so it answers each of them with an error that names the app, as `view.screenshot`
//! does. The app answers them itself (`kadakas-app`, `agent_view.rs`); the logic that needs no
//! window is in `kadakas_ui_core::agent_view`.
//!
//! `view.set` and `view.list` (in `lib.rs`) work in both: `view.set` also takes the name of a
//! saved view, and `view.list` lists the saved views.

use serde_json::{Value, json};

use crate::RpcError;

pub const VIEW_GET: (&str, &str) = (
    "view.get",
    "What the person looks at now (only the app answers; the headless server gives an error). No params. Returns {mode (plan | 3d), window {width, height, pixels_per_point} (points), canvas {width, height} (points; the drawing area), plan {center [x, y], zoom (px per mm), mm_per_px, layer_scale (the drawing scale denominator of the active layer), visible_box {min, max} (drawing mm; only when mode is plan)}, camera {view, projection, render_mode, eye, target, up, fov_deg, distance, yaw and pitch (radians), fov (degrees, the same as fov_deg), aspect (canvas width over height), visible_box {min, max} (the ground the camera sees, drawing mm; only when mode is 3d; null if the camera does not look at the ground)}, selection [id], selection_parts {faces: {model id: [face id]}, edges: {model id: [edge id]}} (as selection.get), active_layer {id, name}, active_class {id, name}, visible_layers [{id, name}], visible_classes [{id, name}], animating, highlights [{id, kind, text, color, until}], back (how many views Back can return to), view_locked (the person turned off \"Agent may move my view\"), pointer (what the person points at, see below), pointer_reason (only when pointer is null)}. pointer (AGR-10) is null when the pointer is not over the drawing area (pointer_reason not_over_canvas) or when the person turned off \"Agents may see my pointer\" in Preferences (pointer_reason disabled_by_user). Else it is {over_canvas: true, canvas [x, y] (points from the top left corner of the drawing area, the frame of canvas), age_ms (time since the pointer last moved, counted when you ask, it does not count a camera that moves under a still pointer), navigating (the person moves the view now), focused (the window has the focus of the system: if not, the position can be old), mode (plan | 3d)}. In the plan it adds point [x, y] (drawing mm, not snapped) and ids (the object that a click at the cursor selects, a group when you are not inside it, [] when there is none). In 3D it adds ray {origin, direction} (drawing mm, direction is a unit vector, give both to mesh.pick as they are) and hit. hit is the answer of mesh.pick for that ray: {model, face, distance, point, local, side, normal}, plus edge when the point is within 6 points of the screen from a shown edge of that face. It is null when no visible surface model is hit. Grey models are skipped, as the person cannot pick them. If an object that is not a surface model (a component, a slab, a site model, a plant) is nearer than every surface model, hit is null and nearest_object {id, kind, distance, point} names that object. Only the app delivers the pointer. The 3D modeler program has no agent connection and kadakas serve has no window. The position stays in the app. It is not saved and not sent to collaborators, and only this call reads it. The person sees an eye in the status bar for 10 seconds after a read. A client gets at most 60 pointer answers a second, then the last answer again. The pointer works while the view is locked. In 3D, camera.visible_box is the ground (at the height of the target) that the four corners of the view reach, or null if a corner looks past the horizon. The 3D camera has the keys of the camera of render.png and mesh.render (target, yaw, pitch, distance, fov, projection): give it back as it is to render.png {camera, width, height} to draw what the person sees, or to view.camera to return to it. Read it before you move the view and when you want to know what the person means by \"this\" or \"here\": pointer says where the pointer is, selection says what is selected.",
);
pub const VIEW_ZOOM: (&str, &str) = (
    "view.zoom",
    "Zoom the view of the person (only the app). params: {ids | box | at (exactly one), scale?, margin?}. ids: objects to frame (in 3D the camera keeps its direction and frames them). box: [x0, y0, x1, y1] in drawing mm. at: [x, y] or [x, y, z]: the new middle of the view. scale: px per mm of the plan (as the zoom of a saved view; from 0.0001 to 100); with it the view keeps this zoom and only moves to the middle of the aim. margin: space around ids or box as a fraction of its size on each side (default 0.1, 0 to 10). The change is animated (about 250 ms; the person can turn animation off in Preferences). It never changes the document or the undo history. The person always wins: while they drag, zoom or type, use a tool, or ran a view command of their own a moment ago, the call fails with a conflict (reason person_navigating) and nothing moves; try again after a moment. If they turned off \"Agent may move my view\", it fails with reason view_locked: do not try again until they say so. The lock covers every call that moves the view: view.zoom, view.pan, view.camera, view.set and saved_view.restore. The person can step back with Back (changes within 120 ms of each other count as one step). Objects that are hidden (their layer or class is off) are not framed and are listed in hidden_ids; if all are hidden the call fails. In 3D, an at or box without a height uses the height of the ground there (site models, layer elevations). Returns {mode, plan?, camera?, clamped (the zoom limit was reached; the plan only), hidden_ids?}.",
);
pub const VIEW_PAN: (&str, &str) = (
    "view.pan",
    "Move the view of the person (only the app). params: {by | to (exactly one)}. by: [dx, dy] in drawing mm, east and north (3D also dz); to: [x, y] (3D also z): the new middle of the view. Animated, never changes the document, and the person wins over it as for view.zoom. Returns the same as view.zoom.",
);
pub const VIEW_CAMERA: (&str, &str) = (
    "view.camera",
    "Set the 3D camera of the person (only the app) and show the 3D view. params: {eye: [x, y, z] (or yaw, pitch in radians and distance in mm instead of eye), target: [x, y, z], up?, fov?, projection?}. The camera of view.get is accepted as it is. With both forms they must give the same eye, else the call is refused as camera_conflict. pitch is from -pi/2 to pi/2, as in render.png. The camera orbits its target and has no roll: up must point upward (z above 0) and fov must be 45 (degrees), which is the field of view of the camera. projection: ortho or perspective. Animated; never changes the document; the person wins as for view.zoom. Returns the same as view.zoom.",
);
pub const VIEW_HIGHLIGHT: (&str, &str) = (
    "view.highlight",
    "Point at objects or a place (only the app). params: {ids | box | at (exactly one), text?, color?, until?}. The person sees an outline glow on the objects or a marker at the place, with a short text (at most 80 characters, one line; a longer text is refused, not cut), in the plan and in 3D. color: agent (default), red, blue, green or orange; each color has its own mark and line style, so color is not the only sign. until: next_change (the pointer goes when the document changes next), dismissed (default; the person dismisses it) or a time in ms (1 to 3600000). At most 32 pointers at a time and 500 objects in one. Pointers are not saved and not part of the document or the undo history. The pointer also shows the agent as a collaborator: the agent cursor at the place, and Agent in the People palette. The person can list the pointers, zoom to one and dismiss them. Returns {id, count, hidden_ids?, note?}: objects that are hidden get no outline and are listed in hidden_ids. In 3D a box or a place without a height is drawn at the height of the ground. The number the person sees on the pointer is its id.",
);
pub const VIEW_HIGHLIGHT_CLEAR: (&str, &str) = (
    "view.highlight_clear",
    "Remove pointers (only the app). params: {id?}: one pointer, or all when id is left out. Returns {cleared}.",
);

/// The methods of the app.
pub const APP_ONLY: [&str; 6] = [
    "view.get",
    "view.zoom",
    "view.pan",
    "view.camera",
    "view.highlight",
    "view.highlight_clear",
];

/// The headless server has no window: the error names the app.
pub fn handle(method: &str) -> Option<Result<Value, RpcError>> {
    APP_ONLY.contains(&method).then(|| {
        Err(RpcError::app(format!(
            "{method} needs the live window of the app. Start the app with --agent-port; the headless server has no window and no view of a person. Use render.png for a picture of the 3D model, or render.svg for the plan"
        )))
    })
}

/// Extra text for `describe`: the review of what the agent shows the person.
pub fn describe() -> Value {
    json!({
        "concept": "A person reviews what an agent does in the app. The agent can read what the person looks at (view.get), move the view (view.zoom, view.pan, view.camera, view.set with a saved view) and point at things (view.highlight). These methods work only in the app (kadakas app --agent-port); the headless server answers with an error that names the app. None of them changes the document or the undo history, and pointers are not saved.",
        "person_in_control": "The person always wins. While the person drags, zooms, types or works with a tool, or just after a view command of their own (zoom to extents, a standard view, the view history, Back, an opened issue), a view call of the agent fails with a conflict (reason person_navigating); nothing moves. A running animation stops when the person changes the view. The person can turn off \"Agent may move my view\" in the panel or in the People palette: then every call that moves the view (view.zoom, view.pan, view.camera, view.set and saved_view.restore) fails with reason view_locked. Back always returns to the newest view that the person made themselves, even after many changes of the agent. The Back button (and the key) in the drawing returns the person to the view they had before the last view change of the agent, one change per press. Animation can be turned off in Preferences. The person can list the pointers, zoom to one and dismiss them; the agent cursor shows in the People palette as Agent.",
        "bounds": {
            "highlights": 32,
            "text_chars": 80,
            "objects_per_highlight": 500,
            "until_ms_max": 3_600_000,
            "back_views": 20,
        },
        "pointer": "view.get has the field pointer (AGR-10): {over_canvas, canvas, age_ms, navigating, focused, mode} and, in the plan, point and ids, in 3D, ray {origin, direction}, hit and nearest_object. canvas is in points from the top left corner of the drawing area (view.get canvas has its size). age_ms is the time since the pointer last moved. It does not count a camera that moves under a still pointer. The 3D hit is the answer of mesh.pick for the ray, with edge added when the point is within 6 points of the screen from an edge of the face. Grey models are skipped. An object that is not a surface model and is nearer than every surface model makes hit null and is named in nearest_object. In 3D a member of a group is named by its model, while in the plan ids names the group. The field is null when the pointer is not over the drawing area, when the window is hidden, when the drawing area is not on the screen, or when the person turned off the setting (pointer_reason says which). Only the app delivers the pointer. The 3D modeler program has no agent connection, and the headless server has no window. If the window has no focus (focused is false), the system may not tell the app where the pointer is, so the position can be old. Privacy: the position stays in the app. Only this call reads it. It is not saved, not in the document and not sent to collaborators. The person turns it off in Preferences (Agents may see my pointer), sees an eye in the status bar for 10 seconds after a read, and sees a line Read pointer in the Agent Activity panel (at most one a minute). A client gets at most 60 answers a second, then the last answer again. Locking the view does not touch the pointer.",
        "colors": "agent (default, diamond mark), red (triangle), blue (square), green (circle), orange (plus). The mark and the line style (solid, dashed, dotted) tell the colors apart without color vision.",
        "camera": "The 3D camera orbits a target and has no roll, and its field of view is 45 degrees. view.camera takes eye and target (or target, yaw, pitch and distance, the camera of view.get); up is checked (it must point upward); fov must be 45.",
        "saved_views": "view.set {view: <saved view name>} applies the camera of a saved view (the plan middle and zoom, or the standard 3D view, projection and mode). It does not change which layers and classes are visible, nor the active layer and class, because that is a change of the document: use saved_view.restore for that (one undo step). layers_not_applied in the answer is true when the saved view holds visibility or an active layer or class that differs from now (it covers classes and the active layer and class too). The person sees \"Agent showed view X (camera only; layers unchanged)\". view.list lists the saved views.",
        "examples": [
            {"method": "view.get", "params": {}},
            {"method": "view.zoom", "params": {"ids": ["00000000-0000-0000-0000-000000000001"], "margin": 0.2}},
            {"method": "view.zoom", "params": {"box": [0, 0, 20000, 15000]}},
            {"method": "view.zoom", "params": {"at": [5000, 3000], "scale": 0.2}},
            {"method": "view.pan", "params": {"by": [1000, 0]}},
            {"method": "view.camera", "params": {"eye": [20000, -20000, 12000], "target": [5000, 5000, 0], "projection": "perspective"}},
            {"method": "view.highlight", "params": {"ids": ["00000000-0000-0000-0000-000000000001"], "text": "Not paved", "color": "red", "until": "next_change"}},
            {"method": "view.highlight", "params": {"at": [5000, 3000], "text": "Check the level here", "until": 30000}},
            {"method": "view.highlight_clear", "params": {}}
        ],
        "recipes": [
            "Zoom to the paved areas: report.areas lists the hardscape areas with their IDs; call view.zoom {ids: [...]}.",
            "Highlight the islands that are not paved: list the islands (objects.list), take the IDs that are not covered by a hardscape (report.areas, unit_pattern.check), call view.highlight {ids, text: \"Not paved\", color: \"red\"}, then view.zoom {ids}.",
            "Read what the person looks at: view.get gives the selection, the visible box and the layers; use it before you answer \"this\" or \"here\".",
            "Find out what \"this\" means: view.get gives pointer. In the plan, pointer.ids is the object under it. In 3D, pointer.hit has the model, face and point, and pointer.hit.edge the edge when the pointer is on one. If pointer.nearest_object is set, the person points at an object that is not a surface model. If pointer is null, read pointer_reason, then ask the person or use selection.",
            "Look along the pointer in 3D: call mesh.pick with pointer.ray.origin and pointer.ray.direction to get every hit (all: true), or mesh.near around pointer.hit.point.",
        ],
    })
}
