//! Pattern hatches of DXF (IOP-27): the pattern lines of a HATCH entity and the hatch fill of the
//! document (`HatchDef`).
//!
//! A DXF pattern line has an angle, a base point, an offset vector and a dash list. The offset is
//! in the world frame (the codes 45 and 46 of a HATCH hold the vector after the turn of the
//! pattern). The next line of the family starts at `base + offset`. A document hatch line has an
//! angle, a spacing across the lines, an offset along them and an origin. This module converts one
//! into the other without loss, and names a pattern from the table of standard patterns.
//!
//! The table (`std_patterns.txt`) is made by `tests/data/make_std_patterns.py` from the metric
//! table of ezdxf (MIT licence). It is used to name a pattern, and to draw a hatch whose entity
//! has a pattern name but no pattern lines.

use std::sync::OnceLock;

use kadakas_core::DVec2;
use kadakas_core::graphics::{HatchDef, HatchLine, PatternUnits};

/// Most lines of one pattern that the import keeps. The standard patterns have at most 30.
pub const MAX_LINES: usize = 128;
/// Most items of a dash list that the import keeps (the limit of a hatch resource).
pub const MAX_DASHES: usize = 64;
/// Most pattern lines that the import reads from one HATCH entity. More are counted and ignored.
pub const MAX_READ: usize = 10_000;
/// The length of a dot (a dash of length 0 in DXF) in mm.
pub const DOT_MM: f64 = 0.1;
/// The least distance between two lines of a family, in mm. A closer family is not a pattern.
pub const MIN_SPACING_MM: f64 = 1.0e-4;

/// One line family of a DXF pattern.
#[derive(Clone, Debug, PartialEq)]
pub struct PatLine {
    /// Degrees, counterclockwise from the X axis.
    pub angle: f64,
    pub base: DVec2,
    /// The vector from a line to the next one of the family (world frame).
    pub offset: DVec2,
    /// Positive: dash. Negative: gap. Zero: dot.
    pub dashes: Vec<f64>,
}

impl PatLine {
    fn dir(&self) -> DVec2 {
        DVec2::from_angle(self.angle.to_radians())
    }

    /// The line family after a linear map (columns `cols`) and a move `t`. The dashes follow the
    /// stretch of the map along the lines.
    pub fn mapped(&self, cols: [DVec2; 2], t: DVec2) -> PatLine {
        let lin = |v: DVec2| cols[0] * v.x + cols[1] * v.y;
        let u = lin(self.dir());
        let stretch = u.length();
        let angle = if stretch > 0.0 {
            u.y.atan2(u.x).to_degrees().rem_euclid(360.0)
        } else {
            0.0
        };
        PatLine {
            angle,
            base: lin(self.base) + t,
            offset: lin(self.offset),
            dashes: self.dashes.iter().map(|d| d * stretch).collect(),
        }
    }

    /// The same family turned by 90 degrees about the origin (the second half of a double hatch).
    pub fn turned_90(&self) -> PatLine {
        let r = |v: DVec2| DVec2::new(-v.y, v.x);
        PatLine {
            angle: (self.angle + 90.0).rem_euclid(360.0),
            base: r(self.base),
            offset: r(self.offset),
            dashes: self.dashes.clone(),
        }
    }

    /// The hatch line of the document. `None` when the offset has no part across the line (the lines
    /// of the family would lie on each other), or a number is not usable.
    pub fn to_hatch(&self) -> Option<HatchLine> {
        let u = self.dir();
        let n = DVec2::new(-u.y, u.x);
        let (perp, along) = (self.offset.dot(n), self.offset.dot(u));
        if !(self.angle.is_finite()
            && self.base.is_finite()
            && perp.is_finite()
            && along.is_finite()
            && perp.abs() >= MIN_SPACING_MM
            && perp.abs() <= kadakas_core::model::MAX_COORD
            && along.abs() <= kadakas_core::model::MAX_COORD
            && self.base.abs().max_element() <= kadakas_core::model::MAX_COORD)
        {
            return None;
        }
        Some(HatchLine {
            angle: self.angle.rem_euclid(360.0),
            spacing: perp.abs(),
            // A family that runs to the other side (a negative part across) counts its lines the
            // other way round, so the stagger changes its sign.
            offset: along * perp.signum(),
            origin: self.base,
            dash: dash_list(&self.dashes),
            color: None,
            weight: None,
        })
    }
}

/// The dash list of a hatch line (dash, gap, dash, gap, ...) from the signed items of DXF.
/// A dot becomes a short dash, and the gap after it is shorter by that much, so the period stays
/// the same. A list of dashes only is a solid line. At most `MAX_DASHES` items.
pub fn dash_list(items: &[f64]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    // What the dots in each dash added to it. The gap after the dash gives that back.
    let mut owed: Vec<f64> = Vec::new();
    // `true` when the next item of the output is a dash.
    let mut want_dash = true;
    for &d in items {
        if !d.is_finite() {
            continue;
        }
        if d < 0.0 {
            if out.is_empty() {
                out.push(0.0);
                owed.push(0.0);
                want_dash = false;
            }
            if want_dash {
                // A gap after a gap.
                *out.last_mut().expect("not empty") += -d;
            } else {
                out.push(-d);
                want_dash = true;
            }
        } else {
            let (len, dot) = if d == 0.0 { (DOT_MM, DOT_MM) } else { (d, 0.0) };
            if want_dash {
                out.push(len);
                owed.push(dot);
                want_dash = false;
            } else {
                // A dash after a dash.
                *out.last_mut().expect("not empty") += len;
                *owed.last_mut().expect("not empty") += dot;
            }
        }
    }
    // A list of dashes only draws a solid line. It must still have an even count.
    if out.len() == 1 {
        return Vec::new();
    }
    if out.len() % 2 == 1 {
        // The last dash runs on into the first one when the list repeats: a gap of 0 closes it.
        out.push(0.0);
    }
    // A dot took DOT_MM of the period. The gap after it gives it back, else the gap before it.
    let len = out.len();
    for (i, o) in owed.iter().enumerate() {
        let mut rest = *o;
        for j in [2 * i + 1, (2 * i + len - 1) % len] {
            let take = rest.min(out[j]);
            out[j] -= take;
            rest -= take;
        }
    }
    out.truncate(MAX_DASHES);
    if out.iter().sum::<f64>() <= 0.0 {
        Vec::new()
    } else {
        out
    }
}

/// The signed DXF items of the dash list of a hatch line.
pub fn dash_items(dash: &[f64]) -> Vec<f64> {
    if dash.iter().sum::<f64>() <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pair in dash.chunks(2) {
        let (d, gap) = (pair[0], pair.get(1).copied().unwrap_or(0.0));
        if d <= DOT_MM * 1.5 {
            // A dot: the gap after it takes back the length of the short dash.
            out.push(0.0);
            if gap + d > 0.0 {
                out.push(-(gap + d));
            }
        } else {
            out.push(d);
            if gap > 0.0 {
                out.push(-gap);
            }
        }
    }
    out
}

impl From<&HatchLine> for PatLine {
    fn from(h: &HatchLine) -> PatLine {
        let u = DVec2::from_angle(h.angle.to_radians());
        let n = DVec2::new(-u.y, u.x);
        PatLine {
            angle: h.angle.rem_euclid(360.0),
            base: h.origin,
            offset: u * h.offset + n * h.spacing,
            dashes: dash_items(&h.dash),
        }
    }
}

/// The origin of a line family reduced to the smallest one that gives the same lines and dashes.
/// A move by the offset vector, or along a solid line, changes nothing. Equal patterns then get
/// equal numbers wherever the hatch lies, so one resource serves them all.
pub fn reduced(line: &HatchLine) -> HatchLine {
    let u = DVec2::from_angle(line.angle.to_radians());
    let n = DVec2::new(-u.y, u.x);
    let v = u * line.offset + n * line.spacing;
    let across = line.origin.dot(n);
    let steps = (across / line.spacing).floor();
    let mut o = line.origin - v * steps;
    let period: f64 = line.dash.iter().sum();
    let mut along = o.dot(u);
    if period > 0.0 {
        along = along.rem_euclid(period);
    } else {
        along = 0.0;
    }
    // The part across stays as it is. The part along is reduced.
    o = n * o.dot(n) + u * along;
    let mut out = line.clone();
    out.origin = DVec2::new(round(o.x), round(o.y));
    out
}

fn round(v: f64) -> f64 {
    let r = (v * 1.0e6).round() / 1.0e6;
    if r == 0.0 { 0.0 } else { r }
}

/// A standard pattern: its name and its lines at scale 1, in mm.
pub struct StdPattern {
    pub name: &'static str,
    pub lines: Vec<PatLine>,
}

fn table() -> &'static [StdPattern] {
    static TABLE: OnceLock<Vec<StdPattern>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out: Vec<StdPattern> = Vec::new();
        for row in include_str!("std_patterns.txt").lines() {
            let f: Vec<&str> = row.split_whitespace().collect();
            match f.len() {
                1 => out.push(StdPattern {
                    name: f[0],
                    lines: Vec::new(),
                }),
                6 => {
                    let n = |i: usize| f[i].parse::<f64>().unwrap_or(0.0);
                    let dashes = if f[5] == "-" {
                        Vec::new()
                    } else {
                        f[5].split(',').filter_map(|d| d.parse().ok()).collect()
                    };
                    if let Some(p) = out.last_mut() {
                        p.lines.push(PatLine {
                            angle: n(0),
                            base: DVec2::new(n(1), n(2)),
                            offset: DVec2::new(n(3), n(4)),
                            dashes,
                        });
                    }
                }
                _ => {}
            }
        }
        out
    })
}

/// The standard pattern of this name (compared without case).
pub fn standard(name: &str) -> Option<&'static StdPattern> {
    let name = name.trim();
    table().iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// The names of all standard patterns.
#[cfg(test)]
pub fn standard_names() -> impl Iterator<Item = &'static str> {
    table().iter().map(|p| p.name)
}

/// A hatch that is a standard pattern drawn larger and turned.
#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    pub name: &'static str,
    pub scale: f64,
    /// Degrees in [0, 360).
    pub angle: f64,
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0e-4 * (1.0 + a.abs().max(b.abs()))
}

fn same_angle(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d) <= 1.0e-4
}

/// The standard pattern that a hatch is (at a scale and a turn), if there is one. The origin of
/// the hatch does not matter. A pattern of one family of lines is ANSI31 at any turn, so it is
/// named only at the turn 0. Other patterns are named at a turn of a multiple of 15 degrees.
pub fn identify(def: &HatchDef) -> Option<Named> {
    identify_as(def, None)
}

/// Like [`identify`]. The pattern called `prefer` is tried first and at any turn, so a file that
/// names its pattern keeps that name when the lines agree with it.
pub fn identify_as(def: &HatchDef, prefer: Option<&str>) -> Option<Named> {
    let preferred = prefer.and_then(standard);
    if let Some(n) = preferred.and_then(|std| fits(std, def, true)) {
        return Some(n);
    }
    table().iter().find_map(|std| fits(std, def, false))
}

fn fits(std: &StdPattern, def: &HatchDef, any_turn: bool) -> Option<Named> {
    let first = def.lines.first()?;
    if std.lines.len() != def.lines.len() {
        return None;
    }
    let std_lines: Vec<HatchLine> = std.lines.iter().filter_map(PatLine::to_hatch).collect();
    if std_lines.len() != def.lines.len() {
        return None;
    }
    // Rounded to 5 places: the table has 6.
    let scale = (first.spacing / std_lines[0].spacing * 1.0e5).round() / 1.0e5;
    let turn = (first.angle - std_lines[0].angle).rem_euclid(360.0);
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    if !any_turn {
        let step = if std.lines.len() == 1 { 360.0 } else { 15.0 };
        let q = turn / step;
        if (q - q.round()).abs() > 1.0e-3 {
            return None;
        }
    }
    for (a, b) in def.lines.iter().zip(&std_lines) {
        let dashes_match = a.dash.len() == b.dash.len()
            && a.dash
                .iter()
                .zip(&b.dash)
                .all(|(x, y)| close(*x, y * scale));
        if !(same_angle(a.angle, b.angle + turn)
            && close(a.spacing, b.spacing * scale)
            && close(a.offset, b.offset * scale)
            && dashes_match)
        {
            return None;
        }
    }
    let turn = if same_angle(turn, 0.0) { 0.0 } else { turn };
    Some(Named {
        name: std.name,
        scale,
        angle: turn,
    })
}

/// The name that a resource gets: the standard name, with the scale and the turn when they are not 1 and 0.
pub fn display_name(n: &Named) -> String {
    let mut s = n.name.to_string();
    if !close(n.scale, 1.0) {
        s.push_str(&format!(" x{}", trim(n.scale)));
    }
    if n.angle != 0.0 {
        s.push_str(&format!(" {} deg", trim(n.angle)));
    }
    s
}

fn trim(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// True when two hatch definitions draw the same lines (to within 1e-6 mm).
pub fn same_def(a: &HatchDef, b: &HatchDef) -> bool {
    let eq = |x: f64, y: f64| (x - y).abs() <= 1.0e-6 * (1.0 + x.abs().max(y.abs()));
    a.units == PatternUnits::World
        && b.units == PatternUnits::World
        && a.lines.len() == b.lines.len()
        && a.lines.iter().zip(&b.lines).all(|(p, q)| {
            eq(p.angle, q.angle)
                && eq(p.spacing, q.spacing)
                && eq(p.offset, q.offset)
                && eq(p.origin.x, q.origin.x)
                && eq(p.origin.y, q.origin.y)
                && p.dash.len() == q.dash.len()
                && p.dash.iter().zip(&q.dash).all(|(x, y)| eq(*x, *y))
                && p.color == q.color
                && p.weight == q.weight
        })
}

#[cfg(test)]
mod tests;
