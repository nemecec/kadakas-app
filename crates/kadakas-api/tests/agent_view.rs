//! The agent drives the view and points (AGR-02, AGR-03): what the headless server answers.
//! The app answers these methods itself (its tests are in `kadakas-app`).

use kadakas_api::{METHODS, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
}

fn fail(s: &mut Session, m: &str, p: Value) -> (i64, String) {
    match s.handle(m, p) {
        Ok(v) => panic!("{m} should fail, got {v}"),
        Err(e) => (e.code, e.message),
    }
}

const APP_METHODS: [&str; 6] = [
    "view.get",
    "view.zoom",
    "view.pan",
    "view.camera",
    "view.highlight",
    "view.highlight_clear",
];

#[test]
fn the_headless_server_names_the_app() {
    let mut s = Session::with_sequential_ids();
    let params = [
        json!({}),
        json!({"at": [0, 0]}),
        json!({"by": [1, 1]}),
        json!({"eye": [1000, 0, 0], "target": [0, 0, 0]}),
        json!({"at": [0, 0], "text": "Here"}),
        json!({}),
    ];
    for (m, p) in APP_METHODS.iter().zip(params) {
        assert!(
            METHODS.iter().any(|(n, d)| n == m && !d.is_empty()),
            "{m} is listed"
        );
        let (code, msg) = fail(&mut s, m, p);
        assert_eq!(code, 1, "{m}");
        assert!(msg.contains("live window of the app"), "{m}: {msg}");
        assert!(msg.contains("--agent-port"), "{m}: {msg}");
    }
}

#[test]
fn an_unknown_parameter_is_named_before_the_error_of_the_app() {
    let mut s = Session::with_sequential_ids();
    // A parameter that the method does not take is named, as for every method.
    let (code, msg) = fail(&mut s, "view.zoom", json!({"idz": []}));
    assert_eq!(code, -32602);
    assert!(msg.contains("idz") && msg.contains("ids"), "{msg}");
}

#[test]
fn the_view_does_not_touch_the_document() {
    let mut s = Session::with_sequential_ids();
    let rev = s.editor.revision();
    for m in APP_METHODS {
        let _ = s.handle(m, json!({"at": [0, 0]}));
    }
    assert_eq!(s.editor.revision(), rev);
    assert!(!s.editor.can_undo());
}

#[test]
fn view_list_shows_the_saved_views_and_view_set_takes_their_names() {
    let mut s = Session::with_sequential_ids();
    assert_eq!(
        call(&mut s, "view.list", json!({}))["saved_views"],
        json!([])
    );
    call(
        &mut s,
        "saved_view.save",
        json!({"name": "Overview", "zoom": 0.1, "pan": [5, 6]}),
    );
    let listed = call(&mut s, "view.list", json!({}));
    assert_eq!(listed["saved_views"], json!([{"name": "Overview"}]));
    s.take_view_changed();
    let rev = s.editor.revision();
    let r = call(&mut s, "view.set", json!({"view": "overview"}));
    assert_eq!(r["saved_view"], "Overview");
    assert_eq!(r["layers_not_applied"], false, "nothing differs from now");
    assert_eq!(s.editor.revision(), rev, "the document is not changed");
    // The host takes the zoom and the middle of the plan.
    assert!(s.has_view_change());
    let applied = s.take_applied_view().expect("applied");
    assert_eq!(applied.zoom, Some(0.1));
    assert!(!s.has_view_change());
    // A name that is neither a standard view nor a saved view lists both.
    let (code, msg) = fail(&mut s, "view.set", json!({"view": "nowhere"}));
    assert_eq!(code, -32602);
    assert!(msg.contains("unknown view"), "{msg}");
}

#[test]
fn a_saved_view_with_visibility_is_applied_without_the_visibility() {
    let mut s = Session::with_sequential_ids();
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Trees"}}),
    );
    // The saved view hides Trees.
    let trees = s
        .editor
        .doc()
        .layers
        .iter()
        .find(|l| l.name == "Trees")
        .map(|l| l.id)
        .unwrap();
    call(&mut s, "saved_view.save", json!({"name": "Plan only"}));
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": trees, "visible": false}}),
    );
    call(&mut s, "saved_view.save", json!({"name": "No trees"}));
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": trees, "visible": true}}),
    );
    let rev = s.editor.revision();
    let r = call(&mut s, "view.set", json!({"view": "No trees"}));
    assert_eq!(r["layers_not_applied"], true);
    assert_eq!(s.editor.revision(), rev);
    assert!(s.editor.doc().layer(trees).unwrap().visible);
}

#[test]
fn describe_has_the_review_section_with_examples_that_fit_the_parameters() {
    let mut s = Session::with_sequential_ids();
    let d = call(&mut s, "describe", json!({}));
    let section = &d["review"]["agent_view"];
    assert!(
        section["person_in_control"]
            .as_str()
            .unwrap()
            .contains("Back"),
        "{d}"
    );
    assert_eq!(section["bounds"]["highlights"], 32);
    assert!(section["examples"].as_array().unwrap().len() >= 8);
    let one = call(&mut s, "describe", json!({"method": "view.highlight"}));
    assert!(one.to_string().contains("next_change"));
    assert!(one.to_string().contains("until"));
}
