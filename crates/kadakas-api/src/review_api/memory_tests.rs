//! The memory of the undo steps of versions.

use serde_json::{Value, json};

use super::*;

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn rect(s: &mut Session, x: i32) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape":
            {"kind": "rect", "min": [x, 0], "max": [x + 10, 10]}}}),
    )["created"][0]
        .clone()
}

/// 40 objects, then 60 times: move two of them, and save a version if `versions`. Returns the
/// session.
fn run(versions: bool) -> Session {
    let mut s = Session::with_sequential_ids();
    let mut ids = Vec::new();
    for i in 0..40 {
        ids.push(rect(&mut s, i * 20));
    }
    ok(
        &mut s,
        "version.create",
        json!({"name": "start", "now_ms": 1}),
    );
    for v in 0..60u64 {
        // The change since the first version grows with each edit.
        let pick = [
            ids[(v as usize * 3) % 40].clone(),
            ids[(v as usize * 3 + 1) % 40].clone(),
        ];
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "move_objects", "ids": pick, "delta": [1, 0]}}),
        );
        if versions {
            ok(
                &mut s,
                "version.create",
                json!({"name": format!("v{v}"), "now_ms": 2 + v}),
            );
        }
    }
    s
}

/// Each undo step holds the history before and after. The versions that did not change, the
/// pictures and the operations of the change since the file was opened are the same memory in
/// all of them, so the steps cost what changed, not the whole history again.
#[test]
fn many_versions_do_not_copy_the_history_into_every_undo_step() {
    let mut s = run(true);
    let edits_only = run(false).editor.history_bytes();
    let history = s.editor.doc().history.clone().unwrap();
    let steps = s.editor.history_bytes() - edits_only;
    // The old accounting counted the whole history in each of the 60 steps (about 30 times its
    // size). Now the sum of the steps is a small multiple of the history.
    assert!(
        steps < 4 * history.bytes() as u64,
        "{steps} bytes in the steps, the history has {}",
        history.bytes()
    );
    // And the steps still undo every version, one by one.
    for k in (0..60u64).rev() {
        assert!(s.editor.undo());
        assert!(s.editor.undo());
        let n = ok(&mut s, "version.list", json!({"current": false}))["count"]
            .as_u64()
            .unwrap();
        assert_eq!(n, k + 1);
    }
}
