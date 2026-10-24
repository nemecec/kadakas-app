//! A public sample file (IOP-11). The file is NOT in the repository: the test fetches it at
//! run time with `curl` from a fixed commit into `tests/data/step-public/` (ignored by git),
//! checks the SHA-256 and reads it. No network (or `KADAKAS_CORPUS=off`, or `CI` without
//! `KADAKAS_CORPUS=fetch`) means that the test prints SKIP and ends. A cache is always used.
//!
//! Source: CadQuery (https://github.com/CadQuery/cadquery), Apache-2.0 licence, the file
//! `tests/testdata/red_cube_blue_cylinder.step`, an AP214 file with two solids and their colors,
//! written by OpenCASCADE. The files of ruststep and truck are not used: the data of ruststep
//! comes from the ABC dataset, which has no clear permissive licence.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use kadakas_step::{StepOptions, read};

const COMMIT: &str = "60e107a6a99fb4cdf405b9c180cb00f8ea0a569b";
const PATH: &str = "tests/testdata/red_cube_blue_cylinder.step";
const SHA256: &str = "1fd5c7d9325bcfb9863365faf84554f89e7e7a5e86760a0fc8d5db00619d9ef5";

fn sha256_hex(bytes: &[u8]) -> String {
    kadakas_core::picture::sha256_hex(bytes)
}

fn cache() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step-public/cadquery")
        .join(&COMMIT[..8])
        .join("red_cube_blue_cylinder.step")
}

fn disabled() -> bool {
    match std::env::var("KADAKAS_CORPUS") {
        Ok(v) if v.eq_ignore_ascii_case("off") => true,
        Ok(v) if v.eq_ignore_ascii_case("fetch") => false,
        _ => std::env::var_os("CI").is_some(),
    }
}

fn fetch() -> Option<Vec<u8>> {
    let file = cache();
    if let Ok(b) = std::fs::read(&file) {
        assert_eq!(
            sha256_hex(&b),
            SHA256,
            "the cached file has a wrong hash: delete it"
        );
        return Some(b);
    }
    if disabled() {
        return None;
    }
    std::fs::create_dir_all(file.parent()?).ok()?;
    let url = format!("https://raw.githubusercontent.com/CadQuery/cadquery/{COMMIT}/{PATH}");
    let part = file.with_extension(format!("part-{}", std::process::id()));
    let out = Command::new("curl")
        .args(["-fsSL", "--connect-timeout", "8", "-m", "60", "-o"])
        .arg(&part)
        .arg(&url)
        .output()
        .ok()?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&part);
        // curl 22 is an HTTP error while the network works: the pinned file is wrong.
        assert_ne!(
            out.status.code(),
            Some(22),
            "the pinned file is not on the server"
        );
        return None;
    }
    let bytes = std::fs::read(&part).ok()?;
    let _ = std::fs::remove_file(&part);
    assert_eq!(sha256_hex(&bytes), SHA256, "the file has a wrong hash");
    let _ = std::fs::write(&file, &bytes);
    Some(bytes)
}

#[test]
fn an_opencascade_file_with_two_colored_solids() {
    let Some(bytes) = fetch() else {
        eprintln!("SKIP: no network and no cached copy of the public STEP sample");
        return;
    };
    let imp = read(&bytes, &StepOptions::default()).unwrap();
    let r = &imp.report;
    assert_eq!(r.entities_read, r.entities_used + r.entities_skipped);
    assert_eq!(
        imp.set.parts.len(),
        2,
        "{:?}",
        imp.set.parts.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    let colors: Vec<_> = imp.set.parts.iter().map(|p| p.color.unwrap()).collect();
    assert!(colors.iter().any(|c| c[0] > 180 && c[2] < 80), "{colors:?}");
    assert!(
        colors.iter().any(|c| c[2] > 150 && c[0] < 100),
        "{colors:?}"
    );
    // Both are closed and have a positive volume.
    for p in &imp.set.parts {
        let mut edges: HashMap<(u32, u32), (i32, i32)> = HashMap::new();
        let mut vol = 0.0;
        for t in &p.triangles {
            let q = |i: u32| glam::DVec3::from(p.positions[i as usize]);
            vol += q(t[0]).dot(q(t[1]).cross(q(t[2]))) / 6.0;
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let e = edges.entry((a.min(b), a.max(b))).or_default();
                if a < b { e.0 += 1 } else { e.1 += 1 }
            }
        }
        assert!(vol > 0.0, "{}", p.name);
        assert!(
            edges.values().all(|v| *v == (1, 1)),
            "{} is not closed",
            p.name
        );
    }
    assert!(imp.exact.iter().all(Option::is_some));
    let kinds: Vec<_> = imp
        .report
        .tessellated
        .iter()
        .map(|t| t.0.as_str())
        .collect();
    assert!(
        kinds.contains(&"PLANE") && kinds.contains(&"CYLINDRICAL_SURFACE"),
        "{kinds:?}"
    );
}
