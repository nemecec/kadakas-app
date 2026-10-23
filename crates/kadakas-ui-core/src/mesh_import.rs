//! The import of mesh files in the windows (MOD-12), without egui.
//!
//! The app and the modeler show the same form for an OBJ, STL or glTF file: the unit and the up
//! axis of the file, and the question "Soften edges under an angle?" (off by default, the angle
//! 20 degrees). The form builds the parameters of `import.file`, so the window and an agent use
//! one code path. The preview asks `import.file` with `preview: true`, which reads the file,
//! converts the mesh and answers with the number of edges that the angle would soften, and
//! changes nothing. The answer of the person to the question is part of the prefs of the program
//! ([`SoftenPref`]) and comes back at the next import.

use std::path::{Path, PathBuf};

use kadakas_l10n::tr;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The units of the choice, as the API names them.
pub const UNITS: &[&str] = &["mm", "cm", "m", "km", "inch", "foot", "yard", "mile"];

/// The angle that the question starts with, in degrees.
pub const DEFAULT_ANGLE: f64 = 20.0;

/// What a person answered to "Soften edges under an angle?", kept in the prefs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoftenPref {
    /// Off by default: the edges of the file stay as they are.
    pub on: bool,
    /// Degrees, 0 to 180.
    pub angle: f64,
}

impl Default for SoftenPref {
    fn default() -> Self {
        SoftenPref {
            on: false,
            angle: DEFAULT_ANGLE,
        }
    }
}

impl SoftenPref {
    /// The angle, kept in the range that `import.file` accepts. A value in the prefs file that is
    /// not a number or is out of range gives the default.
    pub fn valid(self) -> SoftenPref {
        if self.angle.is_finite() && (0.0..=180.0).contains(&self.angle) {
            self
        } else {
            SoftenPref {
                angle: DEFAULT_ANGLE,
                ..self
            }
        }
    }
}

/// The mesh formats of the import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKind {
    Obj,
    Stl,
    /// glTF and GLB: always meters, Y up.
    Gltf,
}

/// The mesh format of a file, by its extension.
pub fn kind_of(path: &Path) -> Option<MeshKind> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("obj") => Some(MeshKind::Obj),
        Some("stl") => Some(MeshKind::Stl),
        Some("gltf" | "glb") => Some(MeshKind::Gltf),
        _ => None,
    }
}

impl MeshKind {
    /// True for the formats whose unit and up axis the person may set.
    pub fn takes_frame(self) -> bool {
        self != MeshKind::Gltf
    }

    /// The unit of the format when the person chooses none.
    pub fn default_unit(self) -> &'static str {
        match self {
            MeshKind::Stl => "mm",
            _ => "m",
        }
    }

    /// True when the format has Y up unless the person chooses.
    pub fn default_y_up(self) -> bool {
        self != MeshKind::Stl
    }
}

/// The name of a unit in the language of the window.
pub fn unit_name(id: &str) -> String {
    let key = format!("enum-length-unit-{id}");
    let text = kadakas_l10n::translate(&key, &[]);
    if text == key { id.to_string() } else { text }
}

/// The choices of the person for one mesh file.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshImportForm {
    pub path: PathBuf,
    pub kind: MeshKind,
    /// Index in [`UNITS`], or `None` for the unit of the format.
    pub unit: Option<usize>,
    /// `Some(true)` Y up, `Some(false)` Z up, `None` the axis of the format.
    pub y_up: Option<bool>,
    /// Move the model to the origin.
    pub center: bool,
    pub soften: SoftenPref,
}

impl MeshImportForm {
    /// The form for a file, or `None` when the file is not a mesh file. The soften answer comes
    /// from the prefs.
    pub fn new(path: PathBuf, soften: SoftenPref) -> Option<MeshImportForm> {
        let kind = kind_of(&path)?;
        Some(MeshImportForm {
            path,
            kind,
            unit: None,
            y_up: None,
            center: false,
            soften: soften.valid(),
        })
    }

    /// The parameters of `import.file`. With `preview` the call changes nothing and counts the
    /// edges that the angle would soften.
    pub fn params(&self, preview: bool) -> Value {
        let mut o = Map::new();
        if self.kind.takes_frame() {
            if let Some(u) = self.unit {
                o.insert("unit".into(), json!(UNITS[u.min(UNITS.len() - 1)]));
            }
            if let Some(up) = self.y_up {
                o.insert("up_axis".into(), json!(if up { "y" } else { "z" }));
            }
        }
        if self.center {
            o.insert("center".into(), json!(true));
        }
        if self.soften.on {
            o.insert(
                "soften_below_angle".into(),
                json!(self.soften.valid().angle),
            );
        }
        if preview {
            o.insert("preview".into(), json!(true));
            // The preview always converts, to count the edges of the model.
            o.entry("to_surface").or_insert(json!(true));
        }
        json!({"path": self.path, "options": o})
    }

    /// The numbers that decide whether the preview must run again.
    pub fn preview_key(&self) -> (Option<usize>, Option<bool>, bool, bool, u64) {
        (
            self.unit,
            self.y_up,
            self.center,
            self.soften.on,
            self.soften.angle.to_bits(),
        )
    }
}

/// What the preview found, from the answer of `import.file`.
#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    /// The edges that the angle would soften, of all the edges of the surface models.
    Counted {
        softened: u64,
        edges: u64,
        faces: u64,
    },
    /// No part fits a surface model: the mesh stays a mesh and nothing is softened.
    TooLarge(String),
    /// The file cannot be read.
    Failed(String),
}

impl Preview {
    /// From the answer of `import.file` with `preview: true`.
    pub fn from_result(r: &Value) -> Preview {
        let t = &r["report"]["to_surface"];
        let converted = t["converted"].as_u64().unwrap_or(0);
        if converted == 0 {
            let reason = t["refused"][0]["reason"].as_str().unwrap_or("").to_string();
            return Preview::TooLarge(reason);
        }
        Preview::Counted {
            softened: r["softened_edges"].as_u64().unwrap_or(0),
            edges: t["edges"].as_u64().unwrap_or(0),
            faces: t["faces"].as_u64().unwrap_or(0),
        }
    }

    /// The line under the question.
    pub fn text(&self) -> String {
        match self {
            Preview::Counted {
                softened,
                edges,
                faces,
            } => tr!(
                "meshio-ui-preview",
                softened = *softened,
                edges = *edges,
                faces = *faces
            ),
            Preview::TooLarge(reason) => {
                tr!("meshio-ui-preview-too-large", reason = reason.as_str())
            }
            Preview::Failed(message) => tr!("meshio-ui-preview-failed", message = message.as_str()),
        }
    }
}

/// The status line after an import: the objects, the triangles and the edges softened.
pub fn imported_text(result: &Value) -> String {
    let triangles = result["report"]["triangles"].as_u64().unwrap_or(0);
    tr!(
        "meshio-ui-imported",
        count = result["imported"].as_u64().unwrap_or(0),
        triangles = triangles,
        softened = result["softened_edges"].as_u64().unwrap_or(0)
    )
}

/// The options of an STL export for the window: ASCII or binary.
pub fn stl_export_options(ascii: bool) -> Value {
    if ascii {
        json!({"encoding": "ascii"})
    } else {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(name: &str) -> MeshImportForm {
        MeshImportForm::new(PathBuf::from(name), SoftenPref::default()).unwrap()
    }

    #[test]
    fn the_format_comes_from_the_extension() {
        assert_eq!(kind_of(Path::new("a.OBJ")), Some(MeshKind::Obj));
        assert_eq!(kind_of(Path::new("a.stl")), Some(MeshKind::Stl));
        assert_eq!(kind_of(Path::new("a.glb")), Some(MeshKind::Gltf));
        assert_eq!(kind_of(Path::new("a.gltf")), Some(MeshKind::Gltf));
        assert_eq!(kind_of(Path::new("a.dxf")), None);
        assert!(MeshImportForm::new("a.ifc".into(), SoftenPref::default()).is_none());
        assert_eq!(MeshKind::Stl.default_unit(), "mm");
        assert_eq!(MeshKind::Obj.default_unit(), "m");
        assert!(MeshKind::Obj.default_y_up() && !MeshKind::Stl.default_y_up());
        assert!(MeshKind::Obj.takes_frame() && !MeshKind::Gltf.takes_frame());
    }

    #[test]
    fn the_soften_question_is_off_with_20_degrees_by_default() {
        let p = SoftenPref::default();
        assert!(!p.on);
        assert_eq!(p.angle, 20.0);
        let f = form("a.obj");
        assert_eq!(f.params(false)["options"], json!({}));
        assert_eq!(f.params(false)["path"], "a.obj");
    }

    #[test]
    fn the_answers_go_into_the_options() {
        let mut f = form("a.stl");
        f.unit = Some(2);
        f.y_up = Some(true);
        f.center = true;
        f.soften = SoftenPref {
            on: true,
            angle: 35.5,
        };
        assert_eq!(
            f.params(false)["options"],
            json!({"unit": "m", "up_axis": "y", "center": true, "soften_below_angle": 35.5})
        );
        assert_eq!(
            f.params(true)["options"],
            json!({"unit": "m", "up_axis": "y", "center": true, "soften_below_angle": 35.5, "preview": true, "to_surface": true})
        );
        // A glTF file takes no unit and no axis.
        let mut g = form("a.glb");
        g.unit = Some(0);
        g.y_up = Some(false);
        assert_eq!(g.params(false)["options"], json!({}));
        // The preview of a file without the question still converts, to count the edges.
        assert_eq!(
            form("a.obj").params(true)["options"],
            json!({"preview": true, "to_surface": true})
        );
    }

    #[test]
    fn a_bad_angle_in_the_prefs_gives_the_default() {
        for bad in [f64::NAN, -1.0, 181.0, f64::INFINITY] {
            let p = SoftenPref {
                on: true,
                angle: bad,
            }
            .valid();
            assert!(p.on);
            assert_eq!(p.angle, 20.0);
        }
        let p = SoftenPref {
            on: true,
            angle: 0.0,
        }
        .valid();
        assert_eq!(p.angle, 0.0);
        let f = MeshImportForm::new(
            "a.obj".into(),
            SoftenPref {
                on: true,
                angle: 999.0,
            },
        )
        .unwrap();
        assert_eq!(f.soften.angle, 20.0);
        // The prefs file keeps the answer.
        let text = serde_json::to_string(&SoftenPref {
            on: true,
            angle: 12.5,
        })
        .unwrap();
        let back: SoftenPref = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back,
            SoftenPref {
                on: true,
                angle: 12.5
            }
        );
        let partial: SoftenPref = serde_json::from_str(r#"{"on": true}"#).unwrap();
        assert_eq!(partial.angle, 20.0);
    }

    #[test]
    fn the_preview_key_changes_with_each_choice() {
        let a = form("a.obj");
        let mut b = a.clone();
        assert_eq!(a.preview_key(), b.preview_key());
        b.soften.on = true;
        assert_ne!(a.preview_key(), b.preview_key());
        let mut c = b.clone();
        c.soften.angle = 30.0;
        assert_ne!(b.preview_key(), c.preview_key());
        c.unit = Some(1);
        assert_ne!(b.preview_key(), c.preview_key());
    }

    #[test]
    fn the_preview_reads_the_answer_in_both_languages() {
        let r = json!({"softened_edges": 150, "report": {"to_surface": {"converted": 1, "edges": 480, "faces": 320, "refused": []}}});
        let p = Preview::from_result(&r);
        assert_eq!(
            p,
            Preview::Counted {
                softened: 150,
                edges: 480,
                faces: 320
            }
        );
        let en = p.text();
        assert!(en.contains("150") && en.contains("480"), "{en}");
        let et = kadakas_l10n::with_language("et", || p.text());
        assert!(et.contains("150") && et != en, "{et}");
        let big = json!({"softened_edges": 0, "report": {"to_surface": {"converted": 0, "refused": [{"reason": "too many faces"}]}}});
        let p = Preview::from_result(&big);
        assert_eq!(p, Preview::TooLarge("too many faces".into()));
        assert!(p.text().contains("too many faces"));
        let failed = Preview::Failed("bad file".into());
        assert!(failed.text().contains("bad file"));
        assert_ne!(
            kadakas_l10n::with_language("et", || failed.text()),
            failed.text()
        );
    }

    #[test]
    fn the_status_line_names_the_counts() {
        let r = json!({"imported": 2, "softened_edges": 40, "report": {"triangles": 1234}});
        let en = imported_text(&r);
        assert!(
            en.contains('2') && en.contains("1234") && en.contains("40"),
            "{en}"
        );
        let et = kadakas_l10n::with_language("et", || imported_text(&r));
        assert!(et != en && et.contains("1234"), "{et}");
    }

    #[test]
    fn units_have_names_and_stl_options_are_small() {
        for u in UNITS {
            assert!(!unit_name(u).is_empty());
        }
        assert_eq!(unit_name("furlong"), "furlong");
        assert_eq!(stl_export_options(true), json!({"encoding": "ascii"}));
        assert_eq!(stl_export_options(false), json!({}));
    }
}
