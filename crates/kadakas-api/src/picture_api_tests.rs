//! Tests of the plant picture methods (PLT-16).

use std::path::{Path, PathBuf};

use kadakas_core::picture::{encode_base64, sha256_hex};
use kadakas_render::picture::test_png;
use serde_json::{Value, json};

use crate::{PathPolicy, Session, SessionLimits};

const JPEG: &[u8] = include_bytes!("../../kadakas-render/testdata/red_blue.jpg");

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> crate::RpcError {
    s.handle(method, p).expect_err(method)
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kad-pic-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(d).unwrap()
}

fn maple(s: &mut Session) -> Value {
    ok(
        s,
        "plant_style.create",
        json!({"name": "Field maple", "category": "tree"}),
    )["id"]
        .clone()
}

fn b64(bytes: &[u8]) -> String {
    encode_base64(bytes)
}

#[test]
fn set_get_and_clear_a_picture_from_base64() {
    let mut s = Session::with_sequential_ids();
    let id = maple(&mut s);
    assert_eq!(
        ok(&mut s, "plant_style.get_picture", json!({"id": id}))["has_picture"],
        false
    );
    assert!(ok(&mut s, "plant_style.get", json!({"id": id}))["picture"].is_null());
    let png = test_png(64, 32);
    let r = ok(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "base64": b64(&png), "name": "dir/maple.png"}),
    );
    assert_eq!(r["created"], true);
    assert_eq!(r["picture"]["sha256"], sha256_hex(&png));
    assert_eq!(r["picture"]["source"], "maple.png");
    assert_eq!(r["picture"]["width"], 64);
    let g = ok(
        &mut s,
        "plant_style.get_picture",
        json!({"id": id, "thumbnail": true, "bytes": true}),
    );
    assert_eq!(g["picture"]["size_bytes"], png.len());
    assert_eq!(g["base64"], b64(&png));
    assert!(g["thumbnail_base64"].as_str().unwrap().len() < png.len() * 2);
    let plain = ok(&mut s, "plant_style.get_picture", json!({"id": id}));
    assert!(plain.get("base64").is_none() && plain.get("thumbnail_base64").is_none());
    // The style shows the metadata, and the resource is listed as an image.
    let st = ok(&mut s, "plant_style.get", json!({"id": id}));
    assert_eq!(st["picture"]["format"], "png");
    assert_eq!(st["image"], r["image"]);
    let list = ok(&mut s, "resource.list", json!({"type": "image"}));
    assert_eq!(list["resources"].as_array().unwrap().len(), 1);
    // Clear: the resource goes with it.
    let c = ok(&mut s, "plant_style.clear_picture", json!({"id": id}));
    assert_eq!(c["had_picture"], true);
    let c = ok(&mut s, "plant_style.clear_picture", json!({"id": id}));
    assert_eq!(c["had_picture"], false);
    let list = ok(&mut s, "resource.list", json!({"type": "image"}));
    assert!(list["resources"].as_array().unwrap().is_empty());
}

#[test]
fn the_same_file_is_stored_once_and_undo_works() {
    let mut s = Session::with_sequential_ids();
    let a = maple(&mut s);
    let b = ok(&mut s, "plant_style.create", json!({"name": "Oak"}))["id"].clone();
    let png = b64(&test_png(8, 8));
    let first = ok(
        &mut s,
        "plant_style.set_picture",
        json!({"id": a, "base64": png}),
    );
    let second = ok(
        &mut s,
        "plant_style.set_picture",
        json!({"id": b, "base64": png}),
    );
    assert_eq!(first["created"], true);
    assert_eq!(second["created"], false);
    assert_eq!(first["image"], second["image"]);
    ok(&mut s, "undo", json!({}));
    assert!(ok(&mut s, "plant_style.get", json!({"id": b}))["picture"].is_null());
    assert!(!ok(&mut s, "plant_style.get", json!({"id": a}))["picture"].is_null());
    // A data URL and base64 with line breaks work too.
    let jpg = format!(
        "data:image/jpeg;base64,{}",
        b64(JPEG).replace("AAAA", "AAAA\n")
    );
    let r = ok(
        &mut s,
        "plant_style.set_picture",
        json!({"id": b, "base64": jpg}),
    );
    assert_eq!(r["picture"]["format"], "jpeg");
}

#[test]
fn files_are_read_by_path_under_the_policy() {
    let dir = tmp("path");
    let other = tmp("path-other");
    std::fs::write(dir.join("maple.png"), test_png(10, 10)).unwrap();
    std::fs::write(other.join("oak.jpg"), JPEG).unwrap();
    std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
    let mut s = Session {
        paths: PathPolicy::restricted([dir.clone()]),
        ..Session::with_sequential_ids()
    };
    let id = maple(&mut s);
    let path = dir.join("maple.png");
    let r = ok(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "path": path}),
    );
    assert_eq!(r["picture"]["source"], "maple.png");
    let e = fail(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "path": other.join("oak.jpg")}),
    );
    assert_eq!(e.kind, "policy");
    let e = fail(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "path": dir.join("notes.txt")}),
    );
    assert_eq!(e.kind, "policy");
    let e = fail(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "path": dir.join("missing.png")}),
    );
    assert_eq!(e.kind, "policy");
    // Without a policy, a missing file is a plain error.
    let mut free = Session::with_sequential_ids();
    let fid = maple(&mut free);
    let e = fail(
        &mut free,
        "plant_style.set_picture",
        json!({"id": fid, "path": dir.join("missing.png")}),
    );
    assert_eq!(e.kind, "error");
}

#[test]
fn bad_input_is_refused_and_changes_nothing() {
    let mut s = Session::with_sequential_ids();
    let id = maple(&mut s);
    let png = test_png(8, 8);
    for (params, kind) in [
        (json!({"id": id}), "invalid_params"),
        (
            json!({"id": id, "path": "a.png", "base64": "AAAA"}),
            "invalid_params",
        ),
        (json!({"id": id, "base64": 5}), "invalid_params"),
        (json!({"id": id, "path": 5}), "invalid_params"),
        (json!({"id": id, "base64": "!!!"}), "invalid_params"),
        (
            json!({"id": "nobody", "base64": b64(&png)}),
            "invalid_params",
        ),
        (
            json!({"id": id, "base64": b64(b"GIF89a....")}),
            "validation",
        ),
        (
            json!({"id": id, "base64": b64(&png[..png.len() / 2])}),
            "validation",
        ),
    ] {
        assert_eq!(fail(&mut s, "plant_style.set_picture", params).kind, kind);
    }
    assert_eq!(
        fail(
            &mut s,
            "plant_style.get_picture",
            json!({"id": id, "bytes": 1})
        )
        .kind,
        "invalid_params"
    );
    assert_eq!(
        fail(&mut s, "plant_style.get_picture", json!([1])).kind,
        "invalid_params"
    );
    assert!(ok(&mut s, "plant_style.get", json!({"id": id}))["picture"].is_null());
    assert!(
        ok(&mut s, "resource.list", json!({"type": "image"}))["resources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_limits_of_the_session_apply() {
    let mut s = Session::with_sequential_ids();
    s.set_limits(SessionLimits {
        picture_bytes: 600,
        picture_pixels: 100,
        ..SessionLimits::default()
    });
    let id = maple(&mut s);
    // Too many pixels for the limit.
    let e = fail(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "base64": b64(&test_png(20, 20))}),
    );
    assert_eq!(e.kind, "validation");
    // A text longer than the file limit is refused before it is decoded.
    let e = fail(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "base64": "A".repeat(5000)}),
    );
    assert_eq!(e.kind, "limit");
    // A file larger than the limit is refused when it is read.
    let dir = tmp("limit");
    std::fs::write(dir.join("big.png"), vec![0u8; 5000]).unwrap();
    let e = fail(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "path": dir.join("big.png")}),
    );
    assert_eq!(e.kind, "limit");
    // The limits of a host never go above the format limit.
    let huge = SessionLimits {
        picture_bytes: u64::MAX,
        picture_pixels: u64::MAX,
        ..SessionLimits::default()
    };
    assert_eq!(
        huge.picture_limits(),
        kadakas_render::picture::PictureLimits::default()
    );
    assert!(SessionLimits::strict().picture_bytes < SessionLimits::default().picture_bytes);
}

#[test]
fn pictures_count_in_the_memory_estimate() {
    let mut s = Session::with_sequential_ids();
    let id = maple(&mut s);
    let before = s.estimated_bytes();
    ok(
        &mut s,
        "plant_style.set_picture",
        json!({"id": id, "base64": b64(&test_png(64, 64))}),
    );
    assert!(s.estimated_bytes() > before + 100);
}

fn csv_session(dir: &Path) -> Session {
    Session {
        paths: PathPolicy::restricted([dir.to_path_buf()]),
        ..Session::with_sequential_ids()
    }
}

#[test]
fn the_csv_import_reads_pictures_relative_to_a_folder() {
    let dir = tmp("csv");
    std::fs::create_dir_all(dir.join("pics")).unwrap();
    std::fs::write(dir.join("pics").join("maple.png"), test_png(12, 12)).unwrap();
    std::fs::write(dir.join("pics").join("oak.jpg"), JPEG).unwrap();
    let mut s = csv_session(&dir);
    let csv = "name,spread,image\nMaple,5,pics/maple.png\nOak,6,pics/oak.jpg\nElm,4,pics/missing.png\nAsh,3,../outside.png\nBeech,3,/etc/passwd\nPine,3,\n";
    let r = ok(
        &mut s,
        "plant_style.import_csv",
        json!({"csv": csv, "unit": "m", "pictures_dir": dir}),
    );
    assert_eq!(r["pictures"], 2);
    let warnings = r["warnings"].to_string();
    assert!(warnings.contains("Elm") && warnings.contains("Ash") && warnings.contains("Beech"));
    assert!(!ok(&mut s, "plant_style.get", json!({"id": "Maple"}))["picture"].is_null());
    assert_eq!(
        ok(&mut s, "plant_style.get", json!({"id": "Oak"}))["picture"]["format"],
        "jpeg"
    );
    assert!(ok(&mut s, "plant_style.get", json!({"id": "Elm"}))["picture"].is_null());
    // Without a folder the pictures are skipped with a warning, and the styles are still made.
    let mut t = csv_session(&dir);
    let r = ok(
        &mut t,
        "plant_style.import_csv",
        json!({"csv": csv, "unit": "m"}),
    );
    assert_eq!(r["pictures"], 0);
    assert!(r["warnings"].to_string().contains("pictures_dir"));
    assert_eq!(r["created_count"], 6);
    // A skipped duplicate row does not change the picture of the style that exists.
    let r = ok(
        &mut s,
        "plant_style.import_csv",
        json!({"csv": "name,image\nMaple,pics/oak.jpg\n", "pictures_dir": dir}),
    );
    assert_eq!(r["pictures"], 0);
    assert_eq!(
        ok(&mut s, "plant_style.get", json!({"id": "Maple"}))["picture"]["format"],
        "png"
    );
    // With "update" the picture of the row replaces it.
    let r = ok(
        &mut s,
        "plant_style.import_csv",
        json!({"csv": "name,image\nMaple,pics/oak.jpg\n", "pictures_dir": dir, "on_duplicate": "update"}),
    );
    assert_eq!(r["pictures"], 1);
    assert_eq!(
        ok(&mut s, "plant_style.get", json!({"id": "Maple"}))["picture"]["format"],
        "jpeg"
    );
    let e = fail(
        &mut s,
        "plant_style.import_csv",
        json!({"csv": csv, "pictures_dir": 5}),
    );
    assert_eq!(e.kind, "invalid_params");
}

#[test]
fn the_methods_are_listed_and_described() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in [
        "plant_style.set_picture",
        "plant_style.clear_picture",
        "plant_style.get_picture",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    assert_eq!(d["plant_pictures"]["formats"], json!(["png", "jpeg"]));
    assert!(crate::picture_api::handle(&mut s, "other", Value::Null).is_none());
}
