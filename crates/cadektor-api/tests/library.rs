//! The per-user library folder and favorites. This file is its own test program, because it sets
//! an environment variable for the whole process.

use cadektor_api::Session;
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, String> {
    s.handle(method, params).map_err(|e| e.message)
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {e}"))
}

#[test]
fn the_library_folder_lists_files_exports_and_keeps_favorites() {
    let dir = std::env::temp_dir().join(format!("cadektor-api-library-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Safe here: this test program has one test, so no other thread reads the environment.
    unsafe { std::env::set_var("CADEKTOR_LIBRARY_DIR", &dir) };

    let mut s = Session::with_sequential_ids();
    // No folder yet: an empty list.
    assert_eq!(ok(&mut s, "library.list", json!({}))["files"], json!([]));
    assert_eq!(
        ok(&mut s, "library.favorites", json!({}))["favorites"],
        json!([])
    );

    ok(
        &mut s,
        "resource.create",
        json!({"type": "hatch", "name": "Diag", "lines": [{"spacing": 2}]}),
    );
    ok(
        &mut s,
        "resource.create",
        json!({"type": "texture", "name": "Bark", "color": [1, 2, 3, 255]}),
    );
    let hatch = ok(&mut s, "resource.get", json!({"id": "Diag"}))["id"].clone();
    // Export only the hatch under a library name. Without "overwrite" a restricted session would refuse a second time.
    let e = ok(
        &mut s,
        "resource.export_to_file",
        json!({"library": "Shared", "ids": [hatch]}),
    );
    assert_eq!(e["count"], 1);
    assert!(e["path"].as_str().unwrap().ends_with("Shared.cdk"));
    // A file that is not a library is not listed.
    std::fs::write(dir.join("notes.cdk.json"), "not json").unwrap();
    std::fs::write(dir.join("readme.txt"), "x").unwrap();
    let list = ok(&mut s, "library.list", json!({}));
    assert_eq!(
        list["files"],
        json!([{"file": "Shared.cdk", "resources": 1}])
    );
    // The old extension is listed too, and a name without extension finds it when only it exists.
    std::fs::copy(dir.join("Shared.cdk"), dir.join("Old.cdk.json")).unwrap();
    let list = ok(&mut s, "library.list", json!({}));
    assert_eq!(
        list["files"],
        json!([
            {"file": "Old.cdk.json", "resources": 1},
            {"file": "Shared.cdk", "resources": 1}
        ])
    );
    let r = ok(&mut s, "resource.list_file", json!({"library": "Old"}));
    assert_eq!(r["resources"].as_array().unwrap().len(), 1);
    let r = ok(&mut s, "resource.list_file", json!({"library": "Shared"}));
    assert_eq!(r["resources"].as_array().unwrap().len(), 1);
    // A path request takes both extensions.
    let p = dir.join("Shared.cdk");
    let r = ok(&mut s, "resource.list_file", json!({"path": p}));
    assert_eq!(r["resources"].as_array().unwrap().len(), 1);
    let p = dir.join("Old.cdk.json");
    let r = ok(&mut s, "resource.list_file", json!({"path": p}));
    assert_eq!(r["resources"].as_array().unwrap().len(), 1);
    std::fs::remove_file(dir.join("Old.cdk.json")).unwrap();

    // A restricted session cannot replace a file without overwrite, and the library folder must be allowed.
    let mut restricted = Session::with_sequential_ids();
    restricted.paths = cadektor_api::PathPolicy::restricted([dir.clone()]);
    ok(
        &mut restricted,
        "resource.create",
        json!({"type": "texture", "name": "T", "color": [1, 1, 1, 255]}),
    );
    let err = call(
        &mut restricted,
        "resource.export_to_file",
        json!({"library": "Shared"}),
    )
    .unwrap_err();
    assert!(err.contains("overwrite"), "{err}");
    ok(
        &mut restricted,
        "resource.export_to_file",
        json!({"library": "Shared", "overwrite": true}),
    );
    restricted.paths =
        cadektor_api::PathPolicy::restricted([std::env::temp_dir().join("cadektor-nowhere")]);
    assert!(
        call(
            &mut restricted,
            "resource.list_file",
            json!({"library": "Shared"})
        )
        .is_err()
    );

    // Items by plain name; a name that two types share needs the type.
    let mut t = Session::with_sequential_ids();
    ok(
        &mut t,
        "resource.create",
        json!({"type": "texture", "name": "Same", "color": [1, 1, 1, 255]}),
    );
    ok(
        &mut t,
        "resource.create",
        json!({"type": "hatch", "name": "Same", "lines": [{"spacing": 2}]}),
    );
    ok(
        &mut t,
        "resource.export_to_file",
        json!({"library": "Twins"}),
    );
    let mut u = Session::with_sequential_ids();
    let err = call(
        &mut u,
        "resource.import_from_file",
        json!({"library": "Twins", "items": ["Same"]}),
    )
    .unwrap_err();
    assert!(err.contains("several resources"), "{err}");
    assert!(
        call(
            &mut u,
            "resource.import_from_file",
            json!({"library": "Twins", "items": [5]})
        )
        .unwrap_err()
        .contains("an item is")
    );
    assert!(
        call(
            &mut u,
            "resource.import_from_file",
            json!({"library": "Twins", "items": "x"})
        )
        .is_err()
    );
    assert!(
        call(
            &mut u,
            "resource.import_from_file",
            json!({"library": "Twins", "items": [{"type": "hatch"}]})
        )
        .unwrap_err()
        .contains("name")
    );
    ok(
        &mut u,
        "resource.import_from_file",
        json!({"library": "Twins", "items": [{"type": "hatch", "name": "Same"}]}),
    );
    assert_eq!(
        ok(&mut u, "resource.list", json!({}))["resources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // The name of a resource can be ambiguous in resource.get too.
    ok(
        &mut u,
        "resource.import_from_file",
        json!({"library": "Twins", "items": [{"type": "texture", "name": "Same"}]}),
    );
    assert!(
        call(&mut u, "resource.get", json!({"id": "Same"}))
            .unwrap_err()
            .contains("several")
    );
    assert_eq!(
        ok(
            &mut u,
            "resource.get",
            json!({"id": "Same", "type": "hatch"})
        )["type"],
        "hatch"
    );
    // Classes only.
    assert!(
        call(
            &mut u,
            "resource.import_from_file",
            json!({"library": "Twins", "classes": ["Nope"]})
        )
        .is_err()
    );
    assert!(
        call(
            &mut u,
            "resource.import_from_file",
            json!({"library": "Twins", "classes": "x"})
        )
        .is_err()
    );

    // Favorites: add twice, keep one, remove.
    let fav = json!({"library": "Shared.cdk.json", "type": "hatch", "name": "Diag"});
    ok(&mut s, "library.favorite_add", fav.clone());
    ok(&mut s, "library.favorite_add", fav.clone());
    assert_eq!(
        ok(&mut s, "library.favorites", json!({}))["favorites"],
        json!([fav])
    );
    assert!(call(&mut s, "library.favorite_add", json!({"library": "x"})).is_err());
    assert!(
        call(
            &mut s,
            "library.favorite_add",
            json!({"library": "x", "type": "hatch"})
        )
        .is_err()
    );
    assert!(
        call(
            &mut s,
            "library.favorite_add",
            json!({"type": "hatch", "name": "n"})
        )
        .is_err()
    );
    ok(&mut s, "library.favorite_remove", fav);
    assert_eq!(
        ok(&mut s, "library.favorites", json!({}))["favorites"],
        json!([])
    );
    // A damaged favorites file is reported.
    std::fs::write(dir.join("favorites.json"), "{").unwrap();
    assert!(call(&mut s, "library.favorites", json!({})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
