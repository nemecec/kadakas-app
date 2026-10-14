//! The library calls read document files with the file size limit of the session (DOC-19).
//! This file is its own test program, because it sets an environment variable for the whole process.

use kadakas_api::{Session, SessionLimits};
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, String> {
    s.handle(method, params).map_err(|e| e.message)
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {e}"))
}

#[test]
fn library_calls_use_the_file_size_limit_of_the_session() {
    let dir = std::env::temp_dir().join(format!("kadakas-api-liblimits-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Safe here: this test program has one test, so no other thread reads the environment.
    unsafe { std::env::set_var("KADAKAS_LIBRARY_DIR", &dir) };

    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "resource.create",
        json!({"type": "texture", "name": "Bark", "color": [1, 2, 3, 255]}),
    );
    ok(
        &mut s,
        "resource.export_to_file",
        json!({"library": "Shared"}),
    );
    let file = dir.join("Shared.kad");
    let len = std::fs::metadata(&file).unwrap().len();

    // A session whose limit is one byte too small refuses the file in every library call.
    let strict = SessionLimits {
        document_file_bytes: len - 1,
        ..SessionLimits::default()
    };
    let mut t = Session::with_sequential_ids();
    t.set_limits(strict);
    for (method, params) in [
        ("resource.list_file", json!({"library": "Shared"})),
        ("resource.list_file", json!({"path": file})),
        (
            "resource.import_from_file",
            json!({"library": "Shared", "all": true}),
        ),
    ] {
        let e = call(&mut t, method, params).unwrap_err();
        assert!(e.contains("limit"), "{method}: {e}");
    }
    // `library.list` skips a file that it may not read.
    assert_eq!(ok(&mut t, "library.list", json!({}))["files"], json!([]));
    // With the exact size, every call works.
    t.set_limits(SessionLimits {
        document_file_bytes: len,
        ..SessionLimits::default()
    });
    assert_eq!(
        ok(&mut t, "resource.list_file", json!({"library": "Shared"}))["resources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ok(&mut t, "library.list", json!({}))["files"],
        json!([{"file": "Shared.kad", "resources": 1}])
    );
    ok(
        &mut t,
        "resource.import_from_file",
        json!({"library": "Shared", "all": true}),
    );
    // A folder is no document file.
    let e = call(
        &mut t,
        "resource.list_file",
        json!({"path": dir.join("sub.kad")}),
    );
    assert!(e.is_err());
    std::fs::create_dir(dir.join("sub.kad")).unwrap();
    let e = call(
        &mut t,
        "resource.list_file",
        json!({"path": dir.join("sub.kad")}),
    )
    .unwrap_err();
    assert!(e.contains("not a regular file"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
