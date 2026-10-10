//! A host runs many documents in one process (PLAT-12). Two sessions with different documents
//! must not share any cache or state: not the site analysis, the scene, the worksheets, the
//! SVG, the limits or the revision numbers.

use std::sync::Arc;

use cadektor_api::{Session, SessionLimits};
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn exec(s: &mut Session, command: Value) -> u64 {
    ok(s, "execute", json!({ "command": command }))["created"][0]
        .as_u64()
        .unwrap()
}

/// A document with a site model (the same points in every call), plants of one species at a
/// place that depends on `x`, and a plant list worksheet. Returns the session and the site ID.
fn document(species: &str, x: f64) -> (Session, u64) {
    let mut s = Session::default();
    let mut ids = Vec::new();
    for i in 0..9 {
        for j in 0..9 {
            ids.push(exec(
                &mut s,
                json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}}),
            ));
        }
    }
    let site = ok(&mut s, "site.create", json!({"source_ids": ids}))["id"]
        .as_u64()
        .unwrap();
    for k in 0..3 {
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "plant", "at": [x + k as f64 * 700.0, 0], "species": species, "spread": 500, "height": 800}}),
        );
    }
    ok(
        &mut s,
        "worksheet.create",
        json!({"name": "Plants", "template": "plant_list"}),
    );
    (s, site)
}

fn analysis(s: &mut Session, site: u64) -> Value {
    ok(
        s,
        "site.analysis",
        json!({"id": site, "mode": "slope", "surface": "existing", "breaks": [2, 5, 10]}),
    )
}

fn csv(s: &mut Session) -> String {
    ok(
        s,
        "worksheet.export",
        json!({"id": "Plants", "format": "csv"}),
    )["csv"]
        .as_str()
        .unwrap()
        .to_string()
}

fn svg(s: &mut Session) -> String {
    ok(s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .to_string()
}

/// What a session shows: the analysis bands, the plant list and the SVG.
fn view_of(s: &mut Session, site: u64) -> (Value, String, String) {
    (analysis(s, site)["bands"].clone(), csv(s), svg(s))
}

#[test]
fn two_sessions_share_no_cache_or_state() {
    let (mut a, site_a) = document("Quercus robur", 0.0);
    let (mut b, site_b) = document("Betula pendula", 50_000.0);

    // The site models have the same content, so a cache that crosses sessions would give the
    // second analysis for free. Each session computes its own.
    analysis(&mut a, site_a);
    assert_eq!(a.site_cache().analyses_computed(), 1);
    assert_eq!(b.site_cache().analyses_computed(), 0);
    analysis(&mut b, site_b);
    assert_eq!(
        b.site_cache().analyses_computed(),
        1,
        "the analysis of session A must not serve session B"
    );
    analysis(&mut a, site_a);
    assert_eq!(a.site_cache().analyses_computed(), 1, "its own cache works");

    // Scenes, revisions, project GUIDs and site results are separate.
    assert!(!Arc::ptr_eq(&a.scene(), &b.scene()));
    assert_ne!(a.editor.revision(), b.editor.revision());
    assert_ne!(
        a.editor.doc().drawing.project.guid,
        b.editor.doc().drawing.project.guid
    );
    let ra = a.site_cache().get(
        a.editor.revision(),
        a.editor.doc(),
        cadektor_core::model::ObjectId(site_a),
    );
    let rb = b.site_cache().get(
        b.editor.revision(),
        b.editor.doc(),
        cadektor_core::model::ObjectId(site_b),
    );
    assert!(!Arc::ptr_eq(&ra.unwrap(), &rb.unwrap()));

    // Worksheets and drawings show only their own document, also when the calls interleave.
    let (ca, cb) = (csv(&mut a), csv(&mut b));
    assert!(ca.contains("Quercus") && !ca.contains("Betula"), "{ca}");
    assert!(cb.contains("Betula") && !cb.contains("Quercus"), "{cb}");
    let (sa, sb) = (svg(&mut a), svg(&mut b));
    assert_ne!(sa, sb);
    exec(
        &mut a,
        json!({"op": "add_object", "shape": {"kind": "plant", "at": [9000, 0], "species": "Acer", "spread": 500, "height": 800}}),
    );
    assert!(csv(&mut a).contains("Acer"));
    assert_eq!(csv(&mut b), cb, "a change in A leaves B alone");
    assert_eq!(svg(&mut b), sb);
    assert_ne!(svg(&mut a), sa);

    // Undo in one session does not touch the other.
    ok(&mut a, "undo", json!({}));
    assert!(!csv(&mut a).contains("Acer"));
    assert_eq!(csv(&mut b), cb);

    // Limits belong to the session.
    a.set_limits(SessionLimits::strict());
    assert_ne!(a.limits(), b.limits());
    assert_eq!(b.limits(), &SessionLimits::default());
    b.replace_document(Default::default());
    assert_eq!(a.limits(), &SessionLimits::strict());
    assert!(!csv(&mut a).contains("Betula"));
}

#[test]
fn sessions_on_two_threads_give_the_same_results_as_alone() {
    // The results of each document when it runs alone.
    let alone = |species: &'static str, x: f64| {
        let (mut s, site) = document(species, x);
        view_of(&mut s, site)
    };
    let (want_a, want_b) = (
        alone("Quercus robur", 0.0),
        alone("Betula pendula", 50_000.0),
    );
    assert_ne!(want_a.1, want_b.1);

    let run = |species: &'static str, x: f64| {
        std::thread::spawn(move || {
            let (mut s, site) = document(species, x);
            let mut last = view_of(&mut s, site);
            for i in 0..12 {
                // Edit, undo and look again, so the caches fill and empty many times.
                exec(
                    &mut s,
                    json!({"op": "add_object", "shape": {"kind": "circle", "center": [i * 10, 0], "radius": 5}}),
                );
                view_of(&mut s, site);
                ok(&mut s, "undo", json!({}));
                last = view_of(&mut s, site);
            }
            last
        })
    };
    let (ta, tb) = (run("Quercus robur", 0.0), run("Betula pendula", 50_000.0));
    let (got_a, got_b) = (ta.join().unwrap(), tb.join().unwrap());
    assert_eq!(got_a, want_a);
    assert_eq!(got_b, want_b);
}
