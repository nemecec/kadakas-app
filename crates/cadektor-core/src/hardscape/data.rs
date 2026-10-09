//! The settings of a hardscape object, kept in `Object::data["hardscape"]`.
//!
//! - A hardscape without this key is a plain slab: it keeps its own material, thickness and
//!   elevation in its shape. Files from before hardscape styles load as these.
//! - A hardscape with the key can have a style, a path outline, a slope and 3D options. The
//!   shape still holds the outline (`boundary`), and a copy of the material and the total
//!   thickness, so bounds, picking and drawing need no access to the style. Commands keep
//!   the copy up to date.
//! - For a path, the center line is the source and `boundary` is derived from it.

use glam::DVec2;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::offset::{self, Join};
use crate::curve::{self, Vertex};
use crate::model::{Document, MAX_COORD, Object, Shape};
use crate::resource::ResourceId;

/// The key in `Object::data` that holds the settings.
pub const KEY: &str = "hardscape";

/// Largest cross slope in percent.
pub const MAX_SLOPE_PERCENT: f64 = 1000.0;
/// Flattening tolerance of curves, in mm.
pub const TOLERANCE: f64 = 1.0;

/// A closed area, or a line with a width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Slab,
    Path,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Slab => "slab",
            Mode::Path => "path",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        [Mode::Slab, Mode::Path].into_iter().find(|m| m.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// How the top surface is inclined. All slopes make a plane.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Slope {
    /// Flat at the elevation.
    #[default]
    None,
    /// Heights above the layer. A slab gives one height per boundary corner, and a plane through
    /// them is fitted. A path gives two: at the start and at the end of the center line.
    Edges { elevations: Vec<f64> },
    /// A fall of `percent` toward `angle_deg` (counterclockwise from +X). The elevation applies in
    /// the middle. A path falls to the right of its direction from start to end, and ignores the angle.
    Cross {
        percent: f64,
        #[serde(default)]
        angle_deg: f64,
    },
}

impl Slope {
    pub fn id(&self) -> &'static str {
        match self {
            Slope::None => "none",
            Slope::Edges { .. } => "edges",
            Slope::Cross { .. } => "cross",
        }
    }
}

fn default_width() -> f64 {
    1200.0
}

/// The settings of a hardscape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HardscapeData {
    pub style: Option<ResourceId>,
    pub mode: Mode,
    /// The center line of a path.
    pub center: Vec<Vertex>,
    /// The width of a path.
    pub width: f64,
    pub join: Join,
    pub slope: Slope,
    /// Follow the site model on the layer. The elevation is then the height above the ground.
    pub drape: bool,
    /// Act as a pad on the site model.
    pub site_modifier: bool,
}

impl Default for HardscapeData {
    fn default() -> Self {
        HardscapeData {
            style: None,
            mode: Mode::Slab,
            center: Vec::new(),
            width: default_width(),
            join: Join::Miter,
            slope: Slope::None,
            drape: false,
            site_modifier: false,
        }
    }
}

impl HardscapeData {
    /// Checks the numbers and the rules between the fields.
    pub fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite() || self.width <= 0.0 || self.width > MAX_COORD {
            return Err("the width must be a positive, finite number".into());
        }
        if self.center.iter().any(|v| !v.is_finite()) {
            return Err("numbers must be finite".into());
        }
        if self.mode == Mode::Path && self.center.len() < 2 {
            return Err("a path needs a center line of 2 or more points".into());
        }
        match &self.slope {
            Slope::None => {}
            Slope::Edges { elevations } => {
                if elevations.iter().any(|e| !e.is_finite() || e.abs() > MAX_COORD) {
                    return Err("edge elevations must be finite numbers".into());
                }
                let need = if self.mode == Mode::Path { 2 } else { 3 };
                if elevations.len() < need {
                    return Err(format!("a slope by edge elevations needs {need} or more elevations"));
                }
            }
            Slope::Cross { percent, angle_deg } => {
                if !percent.is_finite() || percent.abs() > MAX_SLOPE_PERCENT || !angle_deg.is_finite() {
                    return Err(format!("the slope must be a finite number, at most {MAX_SLOPE_PERCENT} percent"));
                }
            }
        }
        Ok(())
    }

    /// The center line as points.
    pub fn center_points(&self) -> Vec<DVec2> {
        offset::dedupe(&curve::flatten(&self.center, false, TOLERANCE))
    }

    /// The outline of a path, as corner vertices. Empty for a slab or a path without length.
    pub fn outline(&self) -> Vec<Vertex> {
        if self.mode != Mode::Path {
            return Vec::new();
        }
        curve::corners(&offset::path_outline(&self.center_points(), self.width, self.join))
    }
}

/// The settings of a hardscape object. `None` for other objects, for a hardscape without
/// settings, and for settings that cannot be read.
pub fn data(obj: &Object) -> Option<HardscapeData> {
    if !matches!(obj.shape, Shape::Hardscape { .. }) {
        return None;
    }
    serde_json::from_value(obj.data.get(KEY)?.clone()).ok()
}

/// Writes the settings of a hardscape object.
pub fn set_data(obj: &mut Object, d: &HardscapeData) {
    obj.data.insert(KEY.into(), serde_json::to_value(d).expect("settings are JSON"));
}

/// True if the object has settings that cannot be read. Such objects are left alone.
pub fn has_unreadable_data(obj: &Object) -> bool {
    obj.data.get(KEY).is_some_and(|v| !matches!(v, Value::Null)) && data(obj).is_none() && matches!(obj.shape, Shape::Hardscape { .. })
}

/// The hardscape style an object refers to, if the style exists.
pub fn style_of(doc: &Document, obj: &Object) -> Option<(ResourceId, super::style::HardscapeStyle)> {
    let id = data(obj)?.style?;
    Some((id, doc.resources.hardscape_style(id)?.clone()))
}

/// The ID of the hardscape style that an object refers to, without checking that it exists.
pub fn style_id(obj: &Object) -> Option<ResourceId> {
    obj.data.get(KEY)?.get("style")?.as_u64().map(ResourceId)
}

/// Copies the material and the total thickness of the style into the shape.
pub fn sync_shape(doc: &Document, obj: &mut Object) {
    let Some((id, style)) = style_of(doc, obj) else { return };
    let name = doc.resources.get(id).map(|r| r.name.clone()).unwrap_or_default();
    if let Shape::Hardscape { material, thickness, .. } = &mut obj.shape {
        *material = style.label(&name);
        *thickness = Some(style.total_thickness());
    }
}

/// A change of hardscape settings. Missing fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HardscapePatch {
    pub style: Option<ResourceId>,
    /// Remove the style. The hardscape keeps the material and thickness of the style.
    pub unstyle: bool,
    pub mode: Option<Mode>,
    pub center: Option<Vec<Vertex>>,
    pub width: Option<f64>,
    pub join: Option<Join>,
    pub slope: Option<Slope>,
    pub drape: Option<bool>,
    pub site_modifier: Option<bool>,
    /// Material of a hardscape without style.
    pub material: Option<String>,
    /// Thickness of a hardscape without style.
    pub thickness: Option<f64>,
    /// Height of the top surface above the layer (or above the ground, when draped).
    pub elevation: Option<f64>,
}

impl HardscapePatch {
    /// True if the patch changes nothing.
    pub fn is_empty(&self) -> bool {
        *self == HardscapePatch::default()
    }
}

/// Applies a patch to a hardscape object: updates the settings, the outline of a path and the
/// copies in the shape. The style must exist (the caller checks). Nothing changes on error.
pub fn apply_patch(doc: &Document, obj: &mut Object, patch: &HardscapePatch) -> Result<(), String> {
    if !matches!(obj.shape, Shape::Hardscape { .. }) {
        return Err(format!("object {} is not a hardscape", obj.id));
    }
    if has_unreadable_data(obj) {
        return Err(format!("object {} has hardscape data that cannot be read", obj.id));
    }
    if patch.unstyle && patch.style.is_some() {
        return Err("give a style or unstyle, not both".into());
    }
    let mut d = data(obj).unwrap_or_default();
    let mut shape = obj.shape.clone();
    if let Some(m) = patch.mode {
        d.mode = m;
    }
    if let Some(c) = &patch.center {
        d.center = c.clone();
        if patch.mode.is_none() {
            d.mode = Mode::Path;
        }
    }
    if let Some(w) = patch.width {
        d.width = w;
    }
    if let Some(j) = patch.join {
        d.join = j;
    }
    if let Some(s) = &patch.slope {
        d.slope = s.clone();
    }
    if let Some(v) = patch.drape {
        d.drape = v;
    }
    if let Some(v) = patch.site_modifier {
        d.site_modifier = v;
    }
    if patch.unstyle {
        // Keep what the style gave.
        d.style = None;
    }
    if let Some(s) = patch.style {
        d.style = Some(s);
    }
    d.validate()?;
    let Shape::Hardscape { boundary, material, thickness, elevation } = &mut shape else { unreachable!("checked above") };
    if d.mode == Mode::Path {
        let outline = d.outline();
        if outline.len() < 3 {
            return Err("the path has no length. Give a center line with two different points".into());
        }
        *boundary = outline;
    } else if patch.mode == Some(Mode::Slab) {
        d.center.clear();
    }
    if let Some(m) = &patch.material {
        *material = m.clone();
    }
    if let Some(t) = patch.thickness {
        if !t.is_finite() || t <= 0.0 || t > MAX_COORD {
            return Err("thickness must be a positive, finite number".into());
        }
        *thickness = Some(t);
    }
    if let Some(e) = patch.elevation {
        if !e.is_finite() || e.abs() > MAX_COORD {
            return Err("elevation must be a finite number".into());
        }
        *elevation = Some(e);
    }
    if let Slope::Edges { elevations } = &d.slope
        && d.mode == Mode::Slab
        && elevations.len() != boundary.len()
    {
        return Err(format!("give one elevation per boundary corner ({} corners)", boundary.len()));
    }
    obj.shape = shape;
    set_data(obj, &d);
    sync_shape(doc, obj);
    Ok(())
}

/// Moves the center line of a path with the object.
pub fn translate_data(obj: &mut Object, delta: DVec2) {
    let Some(mut d) = data(obj) else { return };
    if d.center.is_empty() {
        return;
    }
    for v in &mut d.center {
        v.p += delta;
    }
    // The outline comes from the center line again, so the two always agree to the last bit.
    if d.mode == Mode::Path
        && let Shape::Hardscape { boundary, .. } = &mut obj.shape
    {
        let outline = d.outline();
        if outline.len() >= 3 {
            *boundary = outline;
        }
    }
    set_data(obj, &d);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClassId, LayerId, ObjectId};
    use crate::resource::{Resource, ResourceBody};

    fn slab() -> Object {
        let b = curve::corners(&[DVec2::ZERO, DVec2::new(3000.0, 0.0), DVec2::new(3000.0, 2000.0), DVec2::new(0.0, 2000.0)]);
        Object::new(ObjectId(5), LayerId(1), ClassId(0), Shape::Hardscape { boundary: b, material: "Gravel".into(), thickness: None, elevation: None })
    }

    fn doc_with_style() -> (Document, ResourceId) {
        let mut doc = Document::default();
        let id = ResourceId(doc.alloc_id().unwrap());
        let style = super::super::style::HardscapeStyle { material: "Setts".into(), thickness: 60.0, components: vec![super::super::style::Component { thickness: 140.0, ..Default::default() }], ..Default::default() };
        doc.resources.push(Resource { id, name: "Setts".into(), body: ResourceBody::HardscapeStyle(Box::new(style)) });
        (doc, id)
    }

    #[test]
    fn plain_hardscape_has_no_data() {
        let o = slab();
        assert!(data(&o).is_none());
        assert!(!has_unreadable_data(&o));
        let mut text = Object::new(ObjectId(6), LayerId(1), ClassId(0), Shape::Line { a: DVec2::ZERO, b: DVec2::X });
        text.data.insert(KEY.into(), serde_json::json!({"mode": "path"}));
        assert!(data(&text).is_none(), "only hardscapes have settings");
    }

    #[test]
    fn patch_sets_style_and_copies_into_the_shape() {
        let (doc, sid) = doc_with_style();
        let mut o = slab();
        apply_patch(&doc, &mut o, &HardscapePatch { style: Some(sid), elevation: Some(30.0), ..Default::default() }).unwrap();
        let d = data(&o).unwrap();
        assert_eq!(d.style, Some(sid));
        assert_eq!(style_id(&o), Some(sid));
        let Shape::Hardscape { material, thickness, elevation, .. } = &o.shape else { panic!() };
        assert_eq!((material.as_str(), *thickness, *elevation), ("Setts", Some(200.0), Some(30.0)));
        assert_eq!(style_of(&doc, &o).unwrap().0, sid);
        // Unstyle keeps the copied fields.
        apply_patch(&doc, &mut o, &HardscapePatch { unstyle: true, ..Default::default() }).unwrap();
        assert!(data(&o).unwrap().style.is_none());
        assert!(style_of(&doc, &o).is_none());
        let Shape::Hardscape { material, .. } = &o.shape else { panic!() };
        assert_eq!(material, "Setts");
        // A style and unstyle together are refused.
        assert!(apply_patch(&doc, &mut o, &HardscapePatch { style: Some(sid), unstyle: true, ..Default::default() }).is_err());
    }

    #[test]
    fn patch_makes_a_path_outline() {
        let (doc, _) = doc_with_style();
        let mut o = slab();
        let before = o.clone();
        let p = HardscapePatch { center: Some(curve::corners(&[DVec2::ZERO, DVec2::new(4000.0, 0.0)])), width: Some(1000.0), ..Default::default() };
        apply_patch(&doc, &mut o, &p).unwrap();
        let d = data(&o).unwrap();
        assert_eq!(d.mode, Mode::Path);
        assert!((o.shape.area().unwrap() - 4.0e6).abs() < 1e-6);
        // A new width changes the outline.
        apply_patch(&doc, &mut o, &HardscapePatch { width: Some(2000.0), ..Default::default() }).unwrap();
        assert!((o.shape.area().unwrap() - 8.0e6).abs() < 1e-6);
        // Moving the object moves the center line.
        translate_data(&mut o, DVec2::new(10.0, 20.0));
        assert_eq!(data(&o).unwrap().center[0].p, DVec2::new(10.0, 20.0));
        // Back to a slab keeps the outline.
        apply_patch(&doc, &mut o, &HardscapePatch { mode: Some(Mode::Slab), ..Default::default() }).unwrap();
        assert!(data(&o).unwrap().center.is_empty());
        // Errors leave the object alone.
        let mut o2 = before.clone();
        let bad = HardscapePatch { center: Some(curve::corners(&[DVec2::ZERO, DVec2::ZERO])), ..Default::default() };
        assert!(apply_patch(&doc, &mut o2, &bad).unwrap_err().contains("no length"));
        assert_eq!(o2, before);
        let bad = HardscapePatch { mode: Some(Mode::Path), ..Default::default() };
        assert!(apply_patch(&doc, &mut o2, &bad).unwrap_err().contains("center line"));
        let bad = HardscapePatch { width: Some(-1.0), ..Default::default() };
        assert!(apply_patch(&doc, &mut o2, &bad).is_err());
    }

    #[test]
    fn patch_checks_numbers_and_slopes() {
        let (doc, _) = doc_with_style();
        let mut o = slab();
        let try_patch = |o: &mut Object, p: HardscapePatch| apply_patch(&doc, o, &p);
        assert!(try_patch(&mut o, HardscapePatch { thickness: Some(0.0), ..Default::default() }).is_err());
        assert!(try_patch(&mut o, HardscapePatch { elevation: Some(f64::NAN), ..Default::default() }).is_err());
        assert!(try_patch(&mut o, HardscapePatch { slope: Some(Slope::Cross { percent: f64::NAN, angle_deg: 0.0 }), ..Default::default() }).is_err());
        assert!(try_patch(&mut o, HardscapePatch { slope: Some(Slope::Cross { percent: 5000.0, angle_deg: 0.0 }), ..Default::default() }).is_err());
        assert!(try_patch(&mut o, HardscapePatch { slope: Some(Slope::Edges { elevations: vec![0.0, 1.0] }), ..Default::default() }).is_err());
        assert!(try_patch(&mut o, HardscapePatch { slope: Some(Slope::Edges { elevations: vec![0.0, 1.0, f64::INFINITY, 2.0] }), ..Default::default() }).is_err());
        assert!(try_patch(&mut o, HardscapePatch { slope: Some(Slope::Edges { elevations: vec![0.0, 1.0, 2.0] }), ..Default::default() }).is_err(), "four corners need four elevations");
        assert!(try_patch(&mut o, HardscapePatch { slope: Some(Slope::Edges { elevations: vec![0.0, 1.0, 2.0, 3.0] }), material: Some("Slate".into()), thickness: Some(80.0), ..Default::default() }).is_ok());
        let Shape::Hardscape { material, thickness, .. } = &o.shape else { panic!() };
        assert_eq!((material.as_str(), *thickness), ("Slate", Some(80.0)));
        assert_eq!(data(&o).unwrap().slope.id(), "edges");
        assert!(HardscapePatch::default().is_empty());
        assert!(!HardscapePatch { drape: Some(true), ..Default::default() }.is_empty());
        // Not a hardscape.
        let mut line = Object::new(ObjectId(9), LayerId(1), ClassId(0), Shape::Line { a: DVec2::ZERO, b: DVec2::X });
        assert!(try_patch(&mut line, HardscapePatch { drape: Some(true), ..Default::default() }).is_err());
        // Unreadable data.
        let mut weird = slab();
        weird.data.insert(KEY.into(), serde_json::json!({"mode": 5}));
        assert!(has_unreadable_data(&weird));
        assert!(try_patch(&mut weird, HardscapePatch { drape: Some(true), ..Default::default() }).is_err());
    }

    #[test]
    fn slope_and_mode_names() {
        assert_eq!(Slope::None.id(), "none");
        assert_eq!(Slope::Cross { percent: 1.0, angle_deg: 0.0 }.id(), "cross");
        assert_eq!(Mode::from_name(" Path "), Some(Mode::Path));
        assert_eq!(Mode::from_name("x"), None);
        assert_eq!(Mode::Slab.id(), "slab");
        let d: HardscapeData = serde_json::from_value(serde_json::json!({"slope": {"type": "cross", "percent": 2}})).unwrap();
        assert_eq!(d.slope, Slope::Cross { percent: 2.0, angle_deg: 0.0 });
        assert_eq!(d.width, 1200.0);
    }
}
