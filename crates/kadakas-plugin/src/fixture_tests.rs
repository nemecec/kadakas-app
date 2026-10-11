//! The committed example modules must not hold the path of the person who built them.
//! `scripts/build-plugin-fixtures.sh` remaps the home folder and the repository folder; this test
//! catches a module that was built by hand.

use std::path::Path;

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn the_example_modules_hold_no_path_of_a_home_folder() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "wasm") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        for bad in [&b"/Users/"[..], b"/home/", b"\\Users\\", b"/root/"] {
            assert!(
                !find(&bytes, bad),
                "{} holds {}",
                path.display(),
                String::from_utf8_lossy(bad)
            );
        }
        // The home folder of the person who runs the test, if it is known.
        if let Ok(home) = std::env::var("HOME")
            && home.len() > 4
        {
            assert!(!find(&bytes, home.as_bytes()), "{}", path.display());
        }
        checked += 1;
    }
    assert!(checked >= 2);
}
