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
    "What the person looks at now (only the app answers; the headless server gives an error). No params. Returns {mode (plan | 3d), window {width, height, pixels_per_point} (points), canvas {width, height} (points; the drawing area), plan {center [x, y], zoom (px per mm), mm_per_px, layer_scale (the drawing scale denominator of the active layer), visible_box {min, max} (drawing mm; only when mode is plan)}, camera {view, projection, render_mode, eye, target, up, fov_deg, distance, visible_box {min, max} (the ground the camera sees, drawing mm; only when mode is 3d; null if the camera does not look at the ground)}, selection [id], active_layer {id, name}, active_class {id, name}, visible_layers [{id, name}], visible_classes [{id, name}], animating, highlights [{id, kind, text, color, until}], back (how many views Back can return to)}. Read it before you move the view and when you want to know what the person means by \"this\".",
);
pub const VIEW_ZOOM: (&str, &str) = (
    "view.zoom",
    "Zoom the view of the person (only the app). params: {ids | box | at (exactly one), scale?, margin?}. ids: objects to frame (in 3D the camera keeps its direction and frames them). box: [x0, y0, x1, y1] in drawing mm. at: [x, y] or [x, y, z]: the new middle of the view. scale: px per mm of the plan (as the zoom of a saved view; from 0.0001 to 100); with it the view keeps this zoom and only moves to the middle of the aim. margin: space around ids or box as a fraction of its size on each side (default 0.1, 0 to 10). The change is animated (about 250 ms; the person can turn animation off in Preferences). It never changes the document or the undo history. The person always wins: while they drag or zoom the view, the call fails with a conflict (reason person_navigating) and nothing moves; try again after a moment. The person can step back with Back. Returns {mode, plan?, camera?, clamped (the zoom limit was reached)}.",
);
pub const VIEW_PAN: (&str, &str) = (
    "view.pan",
    "Move the view of the person (only the app). params: {by | to (exactly one)}. by: [dx, dy] in drawing mm, east and north (3D also dz); to: [x, y] (3D also z): the new middle of the view. Animated, never changes the document, and the person wins over it as for view.zoom. Returns the same as view.zoom.",
);
pub const VIEW_CAMERA: (&str, &str) = (
    "view.camera",
    "Set the 3D camera of the person (only the app) and show the 3D view. params: {eye: [x, y, z], target: [x, y, z], up?, fov?, projection?}. The camera orbits its target and has no roll: up must point upward (z above 0) and fov must be 45 (degrees), which is the field of view of the camera. projection: ortho or perspective. Animated; never changes the document; the person wins as for view.zoom. Returns the same as view.zoom.",
);
pub const VIEW_HIGHLIGHT: (&str, &str) = (
    "view.highlight",
    "Point at objects or a place (only the app). params: {ids | box | at (exactly one), text?, color?, until?}. The person sees an outline glow on the objects or a marker at the place, with a short text (at most 80 characters, one line; a longer text is refused, not cut), in the plan and in 3D. color: agent (default), red, blue, green or orange; each color has its own mark and line style, so color is not the only sign. until: next_change (the pointer goes when the document changes next), dismissed (default; the person dismisses it) or a time in ms (1 to 3600000). At most 32 pointers at a time and 500 objects in one. Pointers are not saved and not part of the document or the undo history. The pointer also shows the agent as a collaborator: the agent cursor at the place, and Agent in the People palette. The person can list the pointers, zoom to one and dismiss them. Returns {id, count}.",
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
        "person_in_control": "The person always wins. While the person drags or zooms the view, a view call of the agent fails with a conflict (reason person_navigating); nothing moves. A running animation stops when the person navigates. The Back button (and the key) in the drawing returns the person to the view they had before the last view change of the agent, one change per press. Animation can be turned off in Preferences. The person can list the pointers, zoom to one and dismiss them; the agent cursor shows in the People palette as Agent.",
        "bounds": {
            "highlights": 32,
            "text_chars": 80,
            "objects_per_highlight": 500,
            "until_ms_max": 3_600_000,
            "back_views": 20,
        },
        "colors": "agent (default, diamond mark), red (triangle), blue (square), green (circle), orange (plus). The mark and the line style (solid, dashed, dotted) tell the colors apart without color vision.",
        "camera": "The 3D camera orbits a target and has no roll, and its field of view is 45 degrees. view.camera takes eye and target; up is checked (it must point upward); fov must be 45.",
        "saved_views": "view.set {view: <saved view name>} applies the camera of a saved view (the plan middle and zoom, or the standard 3D view, projection and mode). It does not change which layers and classes are visible, because that is a change of the document: use saved_view.restore for that (one undo step). view.list lists the saved views.",
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
        ],
    })
}
