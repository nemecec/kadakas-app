//! UI flavors: the programs in a version folder that the launcher can start.
//!
//! Today there is one: the main app, `kadakas-app`. Later versions can ship more programs and
//! list them in a file `flavors.json` in the version folder:
//!
//! ```json
//! { "flavors": [ { "id": "sketch-like", "name": "3D-first UI", "exe": "kadakas-sketch" } ] }
//! ```
//!
//! `exe` is a path inside the version folder, without the `.exe` ending on Windows. The launcher
//! adds it. A flavor whose program is missing is not listed.
//!
//! **macOS.** A version folder holds one app bundle, `Kadakas.app`, and the program of the app is
//! the main executable of that bundle: `Kadakas.app/Contents/MacOS/kadakas-app`. The launcher
//! starts that path itself (not `open`), so it can pass arguments and the run variables. Because
//! the path is inside a `.app`, the system finds the bundle (its name, its icon in the Dock) from
//! the path. A folder with the program directly in it (a plain build) works as well, on every
//! system.

use crate::archive::safe_relative;
use crate::settings::{MAIN_FLAVOR, valid_name};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flavor {
    pub id: String,
    /// The name that the UI shows.
    pub name: String,
    /// The program, relative to the version folder.
    pub exe: PathBuf,
}

#[derive(Deserialize)]
struct FlavorFile {
    flavors: Vec<RawFlavor>,
}

#[derive(Deserialize)]
struct RawFlavor {
    id: String,
    name: String,
    exe: String,
}

/// The name of the app bundle in a version folder on macOS.
pub const APP_BUNDLE: &str = "Kadakas.app";

fn exe_path(base: &str) -> PathBuf {
    PathBuf::from(format!("{base}{}", std::env::consts::EXE_SUFFIX))
}

/// The main executable of a program inside the app bundle of a version folder.
pub fn bundle_exe(base: &str) -> PathBuf {
    Path::new(APP_BUNDLE)
        .join("Contents")
        .join("MacOS")
        .join(base)
}

/// The program `base` in the folder `dir`: directly in it, or inside the app bundle. `None` when
/// it is in neither place.
pub fn find_exe(dir: &Path, base: &str) -> Option<PathBuf> {
    [exe_path(base), bundle_exe(base)]
        .into_iter()
        .find(|rel| dir.join(rel).is_file())
}

/// True when the folder holds a program that can start (a version folder, plain or a bundle).
pub fn has_app(dir: &Path) -> bool {
    find_exe(dir, MAIN_FLAVOR).is_some()
}

/// The flavors that are installed in `dir`. The main app comes first.
pub fn discover(dir: &Path) -> Vec<Flavor> {
    let mut out = Vec::new();
    if let Some(main) = find_exe(dir, MAIN_FLAVOR) {
        out.push(Flavor {
            id: MAIN_FLAVOR.into(),
            name: "Kadakas".into(),
            exe: main,
        });
    }
    let extra: Vec<RawFlavor> = std::fs::read(dir.join("flavors.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<FlavorFile>(&b).ok())
        .map(|f| f.flavors)
        .unwrap_or_default();
    for raw in extra {
        let name = raw.name.trim();
        let Ok(rel) = safe_relative(Path::new(&raw.exe)) else {
            continue;
        };
        let exe = exe_path(&rel.to_string_lossy());
        let fresh = !out.iter().any(|f| f.id == raw.id);
        if valid_name(&raw.id) && !name.is_empty() && fresh && dir.join(&exe).is_file() {
            out.push(Flavor {
                id: raw.id,
                name: name.to_string(),
                exe,
            });
        }
    }
    out
}

/// The flavor with this id, or the first one when the id is not installed.
pub fn pick<'a>(flavors: &'a [Flavor], id: &str) -> Option<&'a Flavor> {
    flavors.iter().find(|f| f.id == id).or(flavors.first())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::tmp;

    fn touch(dir: &Path, name: &str) {
        let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn main_app_only() {
        let d = tmp("flv-main");
        assert!(discover(&d).is_empty());
        touch(&d, "kadakas-app");
        let f = discover(&d);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].id, "kadakas-app");
        assert_eq!(pick(&f, "other").unwrap().id, "kadakas-app");
        assert!(pick(&[], "x").is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_program_inside_an_app_bundle_is_found() {
        let d = tmp("flv-bundle");
        assert!(!has_app(&d));
        touch(&d, "Kadakas.app/Contents/MacOS/kadakas-app");
        touch(&d, "Kadakas.app/Contents/MacOS/kadakas-sketch");
        assert!(has_app(&d));
        std::fs::write(
            d.join("flavors.json"),
            r#"{"flavors":[{"id":"sketch","name":"3D","exe":"Kadakas.app/Contents/MacOS/kadakas-sketch"}]}"#,
        )
        .unwrap();
        let f = discover(&d);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].exe, bundle_exe("kadakas-app"));
        assert_eq!(
            f[0].exe,
            Path::new("Kadakas.app/Contents/MacOS/kadakas-app")
        );
        assert_eq!(f[1].id, "sketch");
        // A plain program next to the bundle wins (a plain build is the same as before).
        touch(&d, "kadakas-app");
        assert_eq!(
            discover(&d)[0].exe,
            Path::new(&format!("kadakas-app{}", std::env::consts::EXE_SUFFIX))
        );
        // A folder with a different bundle name is no version folder.
        let e = tmp("flv-other");
        touch(&e, "Other.app/Contents/MacOS/kadakas-app");
        assert!(!has_app(&e));
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&e).unwrap();
    }

    #[test]
    fn extra_flavors_from_the_file() {
        let d = tmp("flv-extra");
        touch(&d, "kadakas-app");
        touch(&d, "kadakas-sketch");
        touch(&d, "bin/third");
        std::fs::write(
            d.join("flavors.json"),
            r#"{"flavors":[
                {"id":"sketch","name":"3D-first","exe":"kadakas-sketch"},
                {"id":"sketch","name":"duplicate","exe":"kadakas-sketch"},
                {"id":"third","name":" Third ","exe":"bin/third"},
                {"id":"gone","name":"Missing program","exe":"nothing"},
                {"id":"evil","name":"Escapes","exe":"../outside"},
                {"id":"abs","name":"Absolute","exe":"/bin/sh"},
                {"id":"bad id","name":"Bad id","exe":"kadakas-sketch"},
                {"id":"noname","name":"  ","exe":"kadakas-sketch"}
            ]}"#,
        )
        .unwrap();
        let f = discover(&d);
        let ids: Vec<&str> = f.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["kadakas-app", "sketch", "third"]);
        assert_eq!(f[2].name, "Third");
        assert_eq!(pick(&f, "sketch").unwrap().name, "3D-first");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn damaged_flavor_file_is_ignored() {
        let d = tmp("flv-bad");
        touch(&d, "kadakas-app");
        std::fs::write(d.join("flavors.json"), b"[[").unwrap();
        assert_eq!(discover(&d).len(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
