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
