//! Reading helpers for an IFC file: attribute access, units, placements and transforms.

use std::rc::Rc;

use glam::{DAffine2, DAffine3, DMat3, DVec2, DVec3};

use crate::reader::StepIndex;
use crate::step::{Entity, Value};

static NULL: Value = Value::Null;

pub fn attr(e: &Entity, i: usize) -> &Value {
    e.attrs.get(i).unwrap_or(&NULL)
}

/// A number from an integer, a real, or a defined type around one.
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Real(x) if x.is_finite() => Some(*x),
        Value::Int(i) => Some(*i as f64),
        Value::Typed(_, inner) => num(inner),
        _ => None,
    }
}

pub fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Real(x) if x.fract() == 0.0 && x.abs() < 1e15 => Some(*x as i64),
        Value::Typed(_, inner) => int(inner),
        _ => None,
    }
}

pub fn text(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s),
        Value::Typed(_, inner) => text(inner),
        _ => None,
    }
}

pub fn enum_of(v: &Value) -> Option<&str> {
    match v {
        Value::Enum(s) => Some(s),
        Value::Typed(_, inner) => enum_of(inner),
        _ => None,
    }
}

pub fn flag(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Enum(e) if e == "U" => None,
        Value::Typed(_, inner) => flag(inner),
        _ => None,
    }
}

pub fn items(v: &Value) -> &[Value] {
    match v {
        Value::List(l) => l,
        _ => &[],
    }
}

pub fn numbers(v: &Value) -> Vec<f64> {
    items(v).iter().filter_map(num).collect()
}

/// The factor of an SI prefix.
pub fn prefix_factor(p: &str) -> f64 {
    match p {
        "EXA" => 1e18,
        "PETA" => 1e15,
        "TERA" => 1e12,
        "GIGA" => 1e9,
        "MEGA" => 1e6,
        "KILO" => 1e3,
        "HECTO" => 1e2,
        "DECA" => 1e1,
        "DECI" => 1e-1,
        "CENTI" => 1e-2,
        "MILLI" => 1e-3,
        "MICRO" => 1e-6,
        "NANO" => 1e-9,
        "PICO" => 1e-12,
        "FEMTO" => 1e-15,
        "ATTO" => 1e-18,
        _ => 1.0,
    }
}

/// The units of the project, as factors to the base units used here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Units {
    /// Millimeters per length unit of the file.
    pub length_mm: f64,
    /// Square meters per area unit.
    pub area_m2: f64,
    /// Cubic meters per volume unit.
    pub volume_m3: f64,
    /// Radians per plane angle unit.
    pub angle_rad: f64,
}

impl Default for Units {
    fn default() -> Self {
        Units {
            length_mm: 1000.0,
            area_m2: 1.0,
            volume_m3: 1.0,
            angle_rad: 1.0,
        }
    }
}

pub struct Ctx<'a> {
    pub x: &'a StepIndex,
    pub units: Units,
}

impl<'a> Ctx<'a> {
    pub fn new(x: &'a StepIndex) -> Ctx<'a> {
        let mut c = Ctx {
            x,
            units: Units::default(),
        };
        c.units = c.read_units();
        c
    }

    pub fn ent(&self, id: u64) -> Option<Rc<Entity>> {
        self.x.entity(id)
    }

    /// The entity that a reference value points to.
    pub fn get(&self, v: &Value) -> Option<Rc<Entity>> {
        self.x.entity(v.as_ref_id()?)
    }

    /// The entities that a list of references points to.
    pub fn get_all(&self, v: &Value) -> Vec<Rc<Entity>> {
        items(v).iter().filter_map(|r| self.get(r)).collect()
    }

    pub fn name_of(&self, v: &Value) -> Option<&str> {
        self.x.name_of(v.as_ref_id()?)
    }

    /// Meters per unit for an `IfcSIUnit` or an `IfcConversionBasedUnit` (power 1 for lengths,
    /// the prefix applies to the power for areas and volumes).
    fn unit_factor(&self, unit: &Entity, power: i32, depth: usize) -> Option<f64> {
        if depth > 4 {
            return None;
        }
        match unit.name.as_str() {
            "IFCSIUNIT" => {
                let prefix = enum_of(attr(unit, 2)).map_or(1.0, prefix_factor);
                Some(prefix.powi(power))
            }
            "IFCCONVERSIONBASEDUNIT" => {
                let m = self.get(attr(unit, 3))?;
                let value = num(attr(&m, 0))?;
                let base = self.get(attr(&m, 1))?;
                Some(value * self.unit_factor(&base, power, depth + 1)?)
            }
            "IFCDERIVEDUNIT" | "IFCMONETARYUNIT" => None,
            _ => None,
        }
    }

    fn read_units(&self) -> Units {
        let mut u = Units::default();
        let Some(project) = self
            .x
            .ids_of("IFCPROJECT")
            .first()
            .and_then(|i| self.ent(*i))
        else {
            return u;
        };
        let Some(assign) = self.get(attr(&project, 8)) else {
            return u;
        };
        for unit in self.get_all(attr(&assign, 0)) {
            if !matches!(unit.name.as_str(), "IFCSIUNIT" | "IFCCONVERSIONBASEDUNIT") {
                continue;
            }
            match enum_of(attr(&unit, 1)) {
                Some("LENGTHUNIT") => {
                    if let Some(f) = self.unit_factor(&unit, 1, 0) {
                        u.length_mm = f * 1000.0;
                    }
                }
                Some("AREAUNIT") => {
                    if let Some(f) = self.unit_factor(&unit, 2, 0) {
                        u.area_m2 = f;
                    }
                }
                Some("VOLUMEUNIT") => {
                    if let Some(f) = self.unit_factor(&unit, 3, 0) {
                        u.volume_m3 = f;
                    }
                }
                Some("PLANEANGLEUNIT") => {
                    if let Some(f) = self.unit_factor(&unit, 1, 0) {
                        u.angle_rad = f;
                    }
                }
                _ => {}
            }
        }
        // A bad factor would break every length. Fall back to millimeters.
        if !(u.length_mm.is_finite() && u.length_mm > 0.0) {
            u.length_mm = 1.0;
        }
        for f in [&mut u.area_m2, &mut u.volume_m3, &mut u.angle_rad] {
            if !(f.is_finite() && *f > 0.0) {
                *f = 1.0;
            }
        }
        u
    }

    // ---------------------------------------------------------------- points and axes

    pub fn point(&self, v: &Value) -> Option<DVec3> {
        let e = self.get(v)?;
        point_of(&e)
    }

    pub fn direction(&self, v: &Value) -> Option<DVec3> {
        let e = self.get(v)?;
        let n = numbers(attr(&e, 0));
        if n.len() < 2 {
            return None;
        }
        Some(DVec3::new(n[0], n[1], n.get(2).copied().unwrap_or(0.0)))
    }

    /// An `IfcAxis2Placement3D` as a transform. `None` if the entity is another kind.
    pub fn axis3(&self, e: &Entity) -> Option<DAffine3> {
        if e.name == "IFCAXIS2PLACEMENT2D" {
            let a = self.axis2(e)?;
            let (x, y, t) = (a.matrix2.x_axis, a.matrix2.y_axis, a.translation);
            return Some(DAffine3 {
                matrix3: DMat3::from_cols(x.extend(0.0), y.extend(0.0), DVec3::Z),
                translation: t.extend(0.0),
            });
        }
        if e.name != "IFCAXIS2PLACEMENT3D" {
            return None;
        }
        let loc = self.point(attr(e, 0)).unwrap_or(DVec3::ZERO);
        let z = self
            .direction(attr(e, 1))
            .and_then(|d| d.try_normalize())
            .unwrap_or(DVec3::Z);
        let r = self.direction(attr(e, 2));
        Some(DAffine3 {
            matrix3: frame(z, r),
            translation: loc,
        })
    }

    pub fn axis2(&self, e: &Entity) -> Option<DAffine2> {
        if e.name != "IFCAXIS2PLACEMENT2D" {
            return None;
        }
        let loc = self.point(attr(e, 0)).unwrap_or(DVec3::ZERO).truncate();
        let x = self
            .direction(attr(e, 1))
            .map(|d| d.truncate())
            .and_then(|d| d.try_normalize())
            .unwrap_or(DVec2::X);
        Some(DAffine2::from_cols(x, x.perp(), loc))
    }

    /// The transform of an `IfcLocalPlacement` chain. A loop or a chain longer than 256 stops there.
    pub fn placement(&self, id: u64) -> DAffine3 {
        let mut chain: Vec<DAffine3> = Vec::new();
        let mut seen: Vec<u64> = Vec::new();
        let mut cur = Some(id);
        while let Some(i) = cur {
            if seen.contains(&i) || seen.len() >= 256 {
                break;
            }
            seen.push(i);
            let Some(e) = self.ent(i) else { break };
            if e.name != "IFCLOCALPLACEMENT" {
                break;
            }
            if let Some(rel) = self.get(attr(&e, 1))
                && let Some(a) = self.axis3(&rel)
            {
                chain.push(a);
            }
            cur = attr(&e, 0).as_ref_id();
        }
        chain
            .iter()
            .rev()
            .fold(DAffine3::IDENTITY, |acc, a| acc * *a)
    }

    /// An `IfcCartesianTransformationOperator` (2D or 3D, uniform or not) as a transform.
    pub fn operator(&self, v: &Value) -> Option<DAffine3> {
        let e = self.get(v)?;
        let is3 = e.name.starts_with("IFCCARTESIANTRANSFORMATIONOPERATOR3D");
        if !is3 && !e.name.starts_with("IFCCARTESIANTRANSFORMATIONOPERATOR2D") {
            return None;
        }
        let origin = self.point(attr(&e, 2)).unwrap_or(DVec3::ZERO);
        let scale = num(attr(&e, 3)).filter(|s| *s != 0.0).unwrap_or(1.0);
        let a1 = self.direction(attr(&e, 0));
        let a2 = self.direction(attr(&e, 1));
        let (m, scales) = if is3 {
            let a3 = self.direction(attr(&e, 4));
            let (sy, sz) = if e.name.ends_with("NONUNIFORM") {
                (
                    num(attr(&e, 5)).unwrap_or(scale),
                    num(attr(&e, 6)).unwrap_or(scale),
                )
            } else {
                (scale, scale)
            };
            let z = a3.and_then(|d| d.try_normalize()).unwrap_or(DVec3::Z);
            let x = a1.and_then(|d| d.try_normalize());
            let mut m = frame(z, x);
            if let (Some(y), None) = (a2.and_then(|d| d.try_normalize()), a1) {
                // Only the Y axis is given: build the frame from it.
                let x = y.cross(z).try_normalize().unwrap_or(DVec3::X);
                m = DMat3::from_cols(x, z.cross(x), z);
            }
            (m, DVec3::new(scale, sy, sz))
        } else {
            let sy = if e.name.ends_with("NONUNIFORM") {
                num(attr(&e, 4)).unwrap_or(scale)
            } else {
                scale
            };
            let x = a1
                .map(|d| d.truncate())
                .and_then(|d| d.try_normalize())
                .unwrap_or(DVec2::X);
            (
                DMat3::from_cols(x.extend(0.0), x.perp().extend(0.0), DVec3::Z),
                DVec3::new(scale, sy, 1.0),
            )
        };
        let m = DMat3::from_cols(
            m.x_axis * scales.x,
            m.y_axis * scales.y,
            m.z_axis * scales.z,
        );
        Some(DAffine3 {
            matrix3: m,
            translation: origin,
        })
    }
}

/// A right-handed frame with Z along `z` and X as close as possible to `x_hint`.
pub fn frame(z: DVec3, x_hint: Option<DVec3>) -> DMat3 {
    let z = z.try_normalize().unwrap_or(DVec3::Z);
    let mut x = x_hint.map_or(DVec3::ZERO, |r| r - z * r.dot(z));
    if x.length_squared() < 1e-18 {
        let other = if z.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
        x = other - z * other.dot(z);
    }
    let x = x.normalize();
    DMat3::from_cols(x, z.cross(x), z)
}

pub fn point_of(e: &Entity) -> Option<DVec3> {
    if e.name != "IFCCARTESIANPOINT" {
        return None;
    }
    let n = numbers(attr(e, 0));
    if n.len() < 2 {
        return None;
    }
    Some(DVec3::new(n[0], n[1], n.get(2).copied().unwrap_or(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(data: &str) -> StepIndex {
        let text = format!("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{data}\nENDSEC;\n");
        StepIndex::from_bytes(text.into_bytes(), crate::reader::Limits::default()).unwrap()
    }

    #[test]
    fn units_from_si_and_conversion_based() {
        let x = index(
            "#1=IFCPROJECT('g',$,'p',$,$,$,$,$,#2);\n#2=IFCUNITASSIGNMENT((#3,#4,#5,#6));\n#3=IFCSIUNIT(*,.LENGTHUNIT.,.CENTI.,.METRE.);\n#4=IFCSIUNIT(*,.AREAUNIT.,.MILLI.,.SQUARE_METRE.);\n#5=IFCCONVERSIONBASEDUNIT(#9,.PLANEANGLEUNIT.,'DEGREE',#7);\n#7=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.01745329252),#8);\n#8=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);\n#9=IFCDIMENSIONALEXPONENTS(0,0,0,0,0,0,0);\n#6=IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.);",
        );
        let c = Ctx::new(&x);
        assert!((c.units.length_mm - 10.0).abs() < 1e-9);
        assert!((c.units.area_m2 - 1e-6).abs() < 1e-15);
        assert!((c.units.angle_rad - 0.01745329252).abs() < 1e-12);
        assert_eq!(c.units.volume_m3, 1.0);
        let feet = index(
            "#1=IFCPROJECT('g',$,'p',$,$,$,$,$,#2);\n#2=IFCUNITASSIGNMENT((#5));\n#5=IFCCONVERSIONBASEDUNIT(#9,.LENGTHUNIT.,'FOOT',#7);\n#7=IFCMEASUREWITHUNIT(IFCLENGTHMEASURE(0.3048),#8);\n#8=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#9=IFCDIMENSIONALEXPONENTS(1,0,0,0,0,0,0);",
        );
        assert!((Ctx::new(&feet).units.length_mm - 304.8).abs() < 1e-9);
        // No project: meters.
        assert_eq!(Ctx::new(&index("#1=IFCX($);")).units.length_mm, 1000.0);
        // A broken factor falls back to millimeters.
        let bad = index(
            "#1=IFCPROJECT('g',$,'p',$,$,$,$,$,#2);\n#2=IFCUNITASSIGNMENT((#5));\n#5=IFCCONVERSIONBASEDUNIT(#9,.LENGTHUNIT.,'X',#7);\n#7=IFCMEASUREWITHUNIT(IFCLENGTHMEASURE(0.),#8);\n#8=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);",
        );
        assert_eq!(Ctx::new(&bad).units.length_mm, 1.0);
    }

    #[test]
    fn placement_chain_and_operators() {
        let x = index(
            "#1=IFCCARTESIANPOINT((10.,0.,0.));\n#2=IFCAXIS2PLACEMENT3D(#1,$,$);\n#3=IFCLOCALPLACEMENT($,#2);\n#4=IFCCARTESIANPOINT((0.,5.,1.));\n#5=IFCDIRECTION((0.,1.,0.));\n#6=IFCAXIS2PLACEMENT3D(#4,$,#5);\n#7=IFCLOCALPLACEMENT(#3,#6);\n#8=IFCLOCALPLACEMENT(#8,#2);\n#9=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#4,2.,$);",
        );
        let c = Ctx::new(&x);
        let p = c.placement(7);
        assert!((p.transform_point3(DVec3::ZERO) - DVec3::new(10.0, 5.0, 1.0)).length() < 1e-9);
        // The local X axis points along the reference direction (global Y).
        assert!((p.transform_vector3(DVec3::X) - DVec3::Y).length() < 1e-9);
        // A loop in the chain stops.
        assert!(c.placement(8).transform_point3(DVec3::ZERO).is_finite());
        let op = c.operator(&Value::Ref(9)).unwrap();
        assert!((op.transform_point3(DVec3::X) - DVec3::new(2.0, 5.0, 1.0)).length() < 1e-9);
        assert!(c.operator(&Value::Ref(1)).is_none());
        assert_eq!(c.placement(99), DAffine3::IDENTITY);
    }

    #[test]
    fn frames_handle_degenerate_hints() {
        let m = frame(DVec3::Z, Some(DVec3::Z));
        assert!((m.z_axis - DVec3::Z).length() < 1e-12);
        assert!(m.x_axis.dot(m.z_axis).abs() < 1e-12);
        let m = frame(DVec3::ZERO, None);
        assert!((m.z_axis - DVec3::Z).length() < 1e-12);
        let m = frame(DVec3::X, None);
        assert!((m.determinant() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn value_helpers() {
        assert_eq!(num(&Value::Int(3)), Some(3.0));
        assert_eq!(num(&Value::Real(f64::NAN)), None);
        assert_eq!(
            num(&Value::Typed(
                "IFCLENGTHMEASURE".into(),
                Box::new(Value::Real(2.5))
            )),
            Some(2.5)
        );
        assert_eq!(int(&Value::Real(4.0)), Some(4));
        assert_eq!(int(&Value::Real(4.5)), None);
        assert_eq!(flag(&Value::Enum("U".into())), None);
        assert_eq!(flag(&Value::Bool(true)), Some(true));
        assert!(items(&Value::Null).is_empty());
        assert_eq!(text(&Value::Null), None);
        assert_eq!(enum_of(&Value::Enum("A".into())), Some("A"));
        assert_eq!(prefix_factor("KILO"), 1000.0);
        assert_eq!(prefix_factor("BOGUS"), 1.0);
    }
}
