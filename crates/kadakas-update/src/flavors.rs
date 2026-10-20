//! UI flavors: the programs in a version folder that the launcher can start.
//!
//! The main app, `kadakas-app`, is always a flavor when it is there. The 3D modeler
//! (`kadakas-modeler`, PLAT-11) is a flavor too, when its program is in the folder: the installers
//! and the update archives bring it, and no file lists it. Other programs are listed in a file
//! `flavors.json` in the version folder:
//!
//! ```json
//! { "flavors": [ { "id": "sketch-like", "name": "3D-first UI", "exe": "kadakas-sketch" } ] }
//! ```
//!
//! `exe` is a path inside the version folder, without the `.exe` ending on Windows. The launcher
//! adds it. A flavor whose program is missing is not listed.

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

/// The flavors that need no `flavors.json`: the id (which is also the name of the program), and
/// the name that the UI shows. A flavor is listed when its program is in the version folder.
pub const BUILT_IN: &[(&str, &str)] = &[("kadakas-modeler", "Kadakas Modeler (SketchUp-like)")];

fn exe_path(base: &str) -> PathBuf {
    PathBuf::from(format!("{base}{}", std::env::consts::EXE_SUFFIX))
}

/// The flavors that are installed in `dir`. The main app comes first.
pub fn discover(dir: &Path) -> Vec<Flavor> {
    let mut out = Vec::new();
    let main = exe_path(MAIN_FLAVOR);
    if dir.join(&main).is_file() {
        out.push(Flavor {
            id: MAIN_FLAVOR.into(),
            name: "Kadakas".into(),
            exe: main,
        });
    }
    for (id, name) in BUILT_IN {
        let exe = exe_path(id);
        if dir.join(&exe).is_file() {
            out.push(Flavor {
                id: (*id).into(),
                name: (*name).into(),
                exe,
            });
        }
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
    fn the_modeler_is_a_flavor_when_its_program_is_there_and_needs_no_file() {
        let d = tmp("flv-modeler");
        touch(&d, "kadakas-app");
        assert_eq!(discover(&d).len(), 1, "no modeler program, no flavor");
        touch(&d, "kadakas-modeler");
        let f = discover(&d);
        let ids: Vec<&str> = f.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["kadakas-app", "kadakas-modeler"]);
        assert_eq!(f[1].name, "Kadakas Modeler (SketchUp-like)");
        assert_eq!(
            pick(&f, "kadakas-modeler").unwrap().exe,
            exe_path("kadakas-modeler")
        );
        // A file cannot list the same id again.
        std::fs::write(
            d.join("flavors.json"),
            br#"{"flavors":[{"id":"kadakas-modeler","name":"Other","exe":"kadakas-modeler"}]}"#,
        )
        .unwrap();
        let f = discover(&d);
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].name, "Kadakas Modeler (SketchUp-like)");
        std::fs::remove_dir_all(&d).unwrap();
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
