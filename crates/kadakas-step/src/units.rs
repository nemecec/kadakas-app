//! Length and angle units of a representation context, and colors of styled items.

use crate::ctx::{Ctx, enum_name, list, num, reference, text};
use crate::part21::Value;
use crate::tess::Rgba;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnitKind {
    Length,
    Angle,
}

fn prefix(name: &str) -> f64 {
    match name {
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

impl<'a> Ctx<'a> {
    /// The kind of a unit entity and its size in metres or radians.
    fn unit_factor(&mut self, id: u64, depth: usize) -> Option<(UnitKind, f64)> {
        if depth > 6 {
            return None;
        }
        let e = self.ent(id).ok()?;
        if let Some(a) = e.part("SI_UNIT") {
            let p = a.first().and_then(enum_name).map_or(1.0, prefix);
            return match a.get(1).and_then(enum_name)? {
                "METRE" => Some((UnitKind::Length, p)),
                "RADIAN" => Some((UnitKind::Angle, p)),
                _ => None,
            };
        }
        if let Some(a) = e.part("CONVERSION_BASED_UNIT") {
            let m = self.ent(a.get(1).and_then(reference)?).ok()?;
            // A plain measure with unit, or a complex instance that has the measure as a part.
            let parts = if m.name == crate::part21::COMPLEX {
                m.part("MEASURE_WITH_UNIT")?
            } else {
                m.attrs.as_slice()
            };
            let value = num(parts.first()?)?;
            let (kind, base) = self.unit_factor(reference(parts.get(1)?)?, depth + 1)?;
            let f = value * base;
            return (f.is_finite() && f > 0.0).then_some((kind, f));
        }
        None
    }

    /// Millimeters in one length unit and radians in one angle unit of a context. `None` for
    /// the length when the context names no length unit.
    pub fn context_units(&mut self, context: u64) -> (Option<f64>, f64) {
        let mut length = None;
        let mut angle = 1.0;
        let Ok(e) = self.ent(context) else {
            return (None, angle);
        };
        let Some(a) = e.part("GLOBAL_UNIT_ASSIGNED_CONTEXT") else {
            return (None, angle);
        };
        let ids: Vec<u64> = a
            .first()
            .and_then(list)
            .map(|l| l.iter().filter_map(reference).collect())
            .unwrap_or_default();
        for id in ids {
            match self.unit_factor(id, 0) {
                Some((UnitKind::Length, m)) => length = Some(m * 1000.0),
                Some((UnitKind::Angle, r)) => angle = r,
                None => {}
            }
        }
        (length, angle)
    }

    /// A color from a style (a list or a reference), found by following the references in the
    /// order of the file. The color of a surface wins over the color of a curve.
    pub fn style_color(&mut self, v: &Value) -> Option<Rgba> {
        let mut alpha = 1.0f64;
        let (mut surface, mut curve): (Option<[u8; 3]>, Option<[u8; 3]>) = (None, None);
        // (entity, depth, reached through a curve style)
        let mut stack: Vec<(u64, usize, bool)> = Vec::new();
        let push = |v: &Value, stack: &mut Vec<(u64, usize, bool)>, d: usize, c: bool| {
            let mut ids = Vec::new();
            v.each_ref(&mut |r| ids.push(r));
            // The first reference is looked at first.
            stack.extend(ids.into_iter().rev().map(|r| (r, d, c)));
        };
        push(v, &mut stack, 0, false);
        let mut visited = 0;
        while let Some((id, d, in_curve)) = stack.pop() {
            visited += 1;
            if d > 10 || visited > 200 {
                continue;
            }
            let Ok(e) = self.ent(id) else { continue };
            let slot = if in_curve { &mut curve } else { &mut surface };
            match e.name.as_str() {
                "COLOUR_RGB" => {
                    let c: Vec<f64> = (1..4)
                        .filter_map(|i| e.attrs.get(i).and_then(num))
                        .collect();
                    if c.len() == 3 && slot.is_none() {
                        let b = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                        *slot = Some([b(c[0]), b(c[1]), b(c[2])]);
                    }
                }
                "DRAUGHTING_PRE_DEFINED_COLOUR" => {
                    let rgb = match e
                        .attrs
                        .first()
                        .and_then(text)
                        .map(str::to_ascii_lowercase)
                        .as_deref()
                    {
                        Some("red") => Some([255, 0, 0]),
                        Some("green") => Some([0, 255, 0]),
                        Some("blue") => Some([0, 0, 255]),
                        Some("yellow") => Some([255, 255, 0]),
                        Some("cyan") => Some([0, 255, 255]),
                        Some("magenta") => Some([255, 0, 255]),
                        Some("black") => Some([0, 0, 0]),
                        Some("white") => Some([255, 255, 255]),
                        _ => None,
                    };
                    if slot.is_none() {
                        *slot = rgb;
                    }
                }
                "SURFACE_STYLE_TRANSPARENT" => {
                    if let Some(t) = e.attrs.first().and_then(num) {
                        alpha = (1.0 - t).clamp(0.0, 1.0);
                    }
                }
                "PRESENTATION_STYLE_ASSIGNMENT"
                | "SURFACE_STYLE_USAGE"
                | "SURFACE_SIDE_STYLE"
                | "SURFACE_STYLE_FILL_AREA"
                | "FILL_AREA_STYLE"
                | "FILL_AREA_STYLE_COLOUR"
                | "SURFACE_STYLE_RENDERING"
                | "SURFACE_STYLE_RENDERING_WITH_PROPERTIES"
                | "CURVE_STYLE"
                | "PRESENTATION_STYLE_BY_CONTEXT" => {
                    let c = in_curve || e.name == "CURVE_STYLE";
                    for a in e.attrs.iter().rev() {
                        push(a, &mut stack, d + 1, c);
                    }
                }
                _ => {}
            }
        }
        match (surface, curve) {
            (Some(c), _) => Some([c[0], c[1], c[2], (alpha * 255.0).round() as u8]),
            (None, Some(c)) => Some([c[0], c[1], c[2], 255]),
            _ => None,
        }
    }

    /// Reads all styled items: the colors of items. A style that overrides another one wins,
    /// whatever the order of the file. A style for one instance only (context dependent) does
    /// not give its item a color for all instances: it is counted as skipped.
    pub fn read_colors(&mut self) {
        let file = self.file;
        let mut overriding: Vec<(u64, Rgba)> = Vec::new();
        for e in &file.entities {
            // The representation that lists the styled items.
            if e.name.ends_with("PRESENTATION_REPRESENTATION") {
                self.use_id(e.id);
            }
            let kind = e.name.as_str();
            if !matches!(
                kind,
                "STYLED_ITEM"
                    | "OVER_RIDING_STYLED_ITEM"
                    | "CONTEXT_DEPENDENT_OVER_RIDING_STYLED_ITEM"
            ) {
                continue;
            }
            let (Some(styles), Some(item)) = (e.attrs.get(1), e.attrs.get(2).and_then(reference))
            else {
                continue;
            };
            if kind == "CONTEXT_DEPENDENT_OVER_RIDING_STYLED_ITEM" {
                self.skip_entity(e, "the color is for one instance of the item only");
                continue;
            }
            self.use_id(e.id);
            if let Some(c) = self.style_color(styles) {
                if kind == "OVER_RIDING_STYLED_ITEM" {
                    overriding.push((item, c));
                } else {
                    self.colors.entry(item).or_insert(c);
                }
            }
        }
        for (item, c) in overriding {
            self.colors.insert(item, c);
        }
    }
}
