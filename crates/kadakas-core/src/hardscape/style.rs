//! The hardscape style: a reusable definition of a paved surface.
//!
//! A style has a surface (main material and thickness), build-up components below the surface
//! (base layers), an optional border and an optional joint pattern for the 2D drawing.
//! Lengths are in mm.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Color, MAX_COORD};

/// Most components a style may have.
pub const MAX_COMPONENTS: usize = 20;
/// Smallest paving unit in a joint pattern, in mm.
pub const MIN_UNIT: f64 = 10.0;

/// One layer of the build-up below the surface, for example a gravel base.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Component {
    pub name: String,
    pub material: String,
    pub thickness: f64,
    /// Color in 3D. `None` picks a shade of the surface color.
    pub color: Option<Color>,
}

impl Default for Component {
    fn default() -> Self {
        Component {
            name: String::new(),
            material: String::new(),
            thickness: 100.0,
            color: None,
        }
    }
}

/// Which edges of a path have a border. A slab has the border on all edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sides {
    #[default]
    Both,
    Left,
    Right,
}

impl Sides {
    pub fn id(self) -> &'static str {
        match self {
            Sides::Both => "both",
            Sides::Left => "left",
            Sides::Right => "right",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        [Sides::Both, Sides::Left, Sides::Right]
            .into_iter()
            .find(|x| x.id().eq_ignore_ascii_case(s.trim()))
    }

    /// How many edges of a path have the border.
    pub fn count(self) -> usize {
        if self == Sides::Both { 2 } else { 1 }
    }
}

/// A border along the edge of the surface, made of another material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Border {
    pub width: f64,
    pub material: String,
    pub color: Option<Color>,
    pub sides: Sides,
}

impl Default for Border {
    fn default() -> Self {
        Border {
            width: 150.0,
            material: String::new(),
            color: None,
            sides: Sides::Both,
        }
    }
}

/// The layout of paving units (the bond). The bonds are defined by the layout engine.
pub use crate::layout::Bond as JointKind;

/// A joint pattern: paving units of `unit_length` by `unit_width`, with `spacing` between them,
/// turned by `angle` degrees.
///
/// With the kind `module` the pattern lays your own unit shapes (`units`) on the lattice of the two
/// repeat vectors `a` and `b`; the unit length and width are not used then.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JointPattern {
    pub kind: JointKind,
    pub unit_length: f64,
    pub unit_width: f64,
    /// The width of a joint.
    pub spacing: f64,
    /// Rotation of the pattern in degrees, counterclockwise.
    pub angle: f64,
    pub color: Option<Color>,
    /// Module: the unit shapes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<crate::layout::ModuleUnit>,
    /// Module: the repeat vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<[f64; 2]>,
}

impl JointPattern {
    /// The module of a module pattern, if this is one.
    pub fn module(&self) -> Option<crate::layout::Module> {
        (self.kind == JointKind::Module).then(|| crate::layout::Module {
            units: self.units.clone(),
            a: self.a.unwrap_or([0.0, 0.0]),
            b: self.b.unwrap_or([0.0, 0.0]),
        })
    }

    /// The pattern for the layout engine (the angle goes in the placement).
    pub fn layout(&self) -> crate::layout::Pattern {
        match self.module() {
            Some(m) => crate::layout::Pattern::of_module(m, self.spacing),
            None => crate::layout::Pattern::new(
                self.kind,
                self.unit_length,
                self.unit_width,
                self.spacing,
            ),
        }
    }
}

impl Default for JointPattern {
    fn default() -> Self {
        JointPattern {
            kind: JointKind::default(),
            unit_length: 200.0,
            unit_width: 100.0,
            spacing: 5.0,
            angle: 0.0,
            color: None,
            units: Vec::new(),
            a: None,
            b: None,
        }
    }
}

/// A hardscape style. The name is the name of the resource that holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HardscapeStyle {
    /// Material of the surface layer, for example `Granite setts`.
    pub material: String,
    /// Fill color in 2D and 3D. `None` uses the class color.
    pub fill: Option<Color>,
    pub pen: Option<Color>,
    /// Thickness of the surface layer.
    pub thickness: f64,
    /// Layers below the surface, from top to bottom.
    pub components: Vec<Component>,
    pub border: Option<Border>,
    pub joints: Option<JointPattern>,
    /// Free fields, for example `supplier`.
    pub custom: BTreeMap<String, String>,
}

impl Default for HardscapeStyle {
    fn default() -> Self {
        HardscapeStyle {
            material: String::new(),
            fill: None,
            pen: None,
            thickness: 60.0,
            components: Vec::new(),
            border: None,
            joints: None,
            custom: BTreeMap::new(),
        }
    }
}

fn positive(name: &str, v: f64) -> Result<(), String> {
    if !v.is_finite() || v <= 0.0 {
        Err(format!("{name} must be a positive, finite number"))
    } else if v > MAX_COORD {
        Err(format!("{name} must not be larger than 1e9 mm"))
    } else {
        Ok(())
    }
}

impl HardscapeStyle {
    /// Thickness of the surface and all components together.
    pub fn total_thickness(&self) -> f64 {
        self.thickness + self.components.iter().map(|c| c.thickness).sum::<f64>()
    }

    /// Checks the numbers and limits.
    pub fn validate(&self) -> Result<(), String> {
        positive("thickness", self.thickness)?;
        if self.components.len() > MAX_COMPONENTS {
            return Err(format!("a style has at most {MAX_COMPONENTS} components"));
        }
        for (i, c) in self.components.iter().enumerate() {
            positive(&format!("thickness of component {}", i + 1), c.thickness)?;
        }
        if let Some(b) = &self.border {
            positive("border width", b.width)?;
        }
        if let Some(j) = &self.joints {
            if j.kind != JointKind::Module {
                positive("unit length", j.unit_length)?;
                positive("unit width", j.unit_width)?;
                if j.unit_length < MIN_UNIT || j.unit_width < MIN_UNIT {
                    return Err(format!("paving units must be at least {MIN_UNIT} mm"));
                }
            }
            if !j.spacing.is_finite() || j.spacing < 0.0 || j.spacing > MAX_COORD {
                return Err("joint spacing must be a finite number of zero or more".into());
            }
            if !j.angle.is_finite() || j.angle.abs() > 1e6 {
                return Err("joint angle must be a finite number".into());
            }
            // The pattern must work: a module that overlaps itself, a basket weave that does not fit.
            j.layout().validate()?;
        }
        Ok(())
    }

    /// The label of a style: the material, else the style name.
    pub fn label(&self, style_name: &str) -> String {
        if self.material.trim().is_empty() {
            style_name.to_string()
        } else {
            self.material.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_total_thickness_adds_up() {
        let mut s = HardscapeStyle::default();
        assert!(s.validate().is_ok());
        assert_eq!(s.total_thickness(), 60.0);
        s.components.push(Component {
            thickness: 40.0,
            ..Component::default()
        });
        s.components.push(Component {
            thickness: 150.0,
            ..Component::default()
        });
        assert_eq!(s.total_thickness(), 250.0);
        assert_eq!(s.label("Style"), "Style");
        s.material = "Setts".into();
        assert_eq!(s.label("Style"), "Setts");
    }

    #[test]
    fn validate_rejects_bad_numbers() {
        let bad = |f: &dyn Fn(&mut HardscapeStyle)| {
            let mut s = HardscapeStyle::default();
            f(&mut s);
            s.validate().is_err()
        };
        assert!(bad(&|s| s.thickness = 0.0));
        assert!(bad(&|s| s.thickness = f64::NAN));
        assert!(bad(&|s| s.thickness = 2.0 * MAX_COORD));
        assert!(bad(&|s| s.components.push(Component {
            thickness: -1.0,
            ..Component::default()
        })));
        assert!(bad(
            &|s| s.components = vec![Component::default(); MAX_COMPONENTS + 1]
        ));
        assert!(bad(&|s| s.border = Some(Border {
            width: 0.0,
            ..Border::default()
        })));
        assert!(bad(&|s| s.joints = Some(JointPattern {
            unit_length: 5.0,
            ..JointPattern::default()
        })));
        assert!(bad(&|s| s.joints = Some(JointPattern {
            unit_width: f64::INFINITY,
            ..JointPattern::default()
        })));
        assert!(bad(&|s| s.joints = Some(JointPattern {
            spacing: -1.0,
            ..JointPattern::default()
        })));
        assert!(bad(&|s| s.joints = Some(JointPattern {
            angle: f64::NAN,
            ..JointPattern::default()
        })));
        assert!(!bad(&|s| s.joints = Some(JointPattern::default())));
        assert!(!bad(&|s| s.border = Some(Border::default())));
    }

    #[test]
    fn names_round_trip() {
        for k in JointKind::ALL {
            assert_eq!(JointKind::from_name(k.id()), Some(k));
            assert_eq!(serde_json::to_value(k).unwrap(), k.id());
        }
        assert_eq!(
            JointKind::from_name("Running bond"),
            Some(JointKind::RunningBond)
        );
        assert_eq!(
            JointKind::from_name("basket-weave"),
            Some(JointKind::BasketWeave)
        );
        assert_eq!(JointKind::from_name("x"), None);
        for s in [Sides::Both, Sides::Left, Sides::Right] {
            assert_eq!(Sides::from_name(s.id()), Some(s));
        }
        assert_eq!(Sides::from_name("up"), None);
        assert_eq!(Sides::Both.count(), 2);
        assert_eq!(Sides::Left.count(), 1);
    }

    #[test]
    fn partial_json_uses_defaults() {
        let s: HardscapeStyle =
            serde_json::from_str(r#"{"material": "Setts", "joints": {"kind": "stack"}}"#).unwrap();
        assert_eq!(s.thickness, 60.0);
        let j = s.joints.unwrap();
        assert_eq!(j.kind, JointKind::Stack);
        assert_eq!(j.unit_length, 200.0);
    }
}
