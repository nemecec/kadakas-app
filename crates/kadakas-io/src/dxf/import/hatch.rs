//! Pattern hatches of the import (IOP-27): the pattern of a HATCH entity becomes a hatch fill of
//! the document.
//!
//! The pattern lines of the entity (group codes 53, 43, 44, 45, 46, 79 and 49) are stored in the
//! file after the turn and the scale of the pattern, so they are used as they are. They become the
//! lines of a hatch resource in mm (`PatternUnits::World`). An entity that has a pattern name but
//! no lines gets the lines of the standard pattern of that name, scaled by the code 41 and turned
//! by the code 52. A hatch resource that draws the same lines as one of the document, or as one of
//! this import, is used again, so a drawing with 500 hatches of ANSI31 makes one resource.
//!
//! Limits: a pattern keeps at most [`hatch::MAX_LINES`] lines (the rest are counted in a note), and a
//! hatch whose lines would be denser than the drawing engine draws stays an outline (a note).

use std::collections::{HashMap, HashSet};

use kadakas_core::DVec2;
use kadakas_core::graphics::{GraphicResource, HatchDef, PatternUnits};
use kadakas_core::resource::{ResourceBody, ResourceId};

use super::{Walker, Xf};
use crate::builder::Builder;
use crate::dxf::hatch::{self, PatLine};
use crate::dxf::parse::{self, Entity};

/// The pattern of a HATCH entity as the file stores it.
pub(super) struct Pattern {
    pub name: String,
    /// A double hatch: the lines and the same lines turned by 90 degrees.
    pub double: bool,
    pub scale: f64,
    pub angle: f64,
    pub lines: Vec<PatLine>,
    /// Lines beyond the read limit.
    pub unread: usize,
}

/// Reads the pattern data of a HATCH entity.
pub(super) fn read(e: &Entity) -> Pattern {
    let items = &e.items;
    // The pattern data follows the boundary paths. Code 75 (the style) is the first code after them.
    let from = items.iter().position(|(c, _)| *c == 75).unwrap_or(0);
    let tail = &items[from..];
    let num = |code: i32| {
        tail.iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| parse::parse_f64(v))
    };
    let mut p = Pattern {
        name: e.get(2).unwrap_or("").trim().to_string(),
        double: num(77).is_some_and(|v| v != 0.0),
        scale: num(41).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0),
        angle: num(52).filter(|a| a.is_finite()).unwrap_or(0.0),
        lines: Vec::new(),
        unread: 0,
    };
    let Some(start) = tail.iter().position(|(c, _)| *c == 78) else {
        return p;
    };
    let mut cur: Option<PatLine> = None;
    let done = |cur: &mut Option<PatLine>, p: &mut Pattern| {
        if let Some(l) = cur.take() {
            if p.lines.len() < hatch::MAX_READ {
                p.lines.push(l);
            } else {
                p.unread += 1;
            }
        }
    };
    let f = |v: &str| parse::parse_f64(v).unwrap_or(f64::NAN);
    for (c, v) in &tail[start + 1..] {
        match *c {
            53 => {
                done(&mut cur, &mut p);
                cur = Some(PatLine {
                    angle: f(v),
                    base: DVec2::ZERO,
                    offset: DVec2::ZERO,
                    dashes: Vec::new(),
                });
            }
            43 | 44 | 45 | 46 | 49 => {
                if let Some(l) = cur.as_mut() {
                    match *c {
                        43 => l.base.x = f(v),
                        44 => l.base.y = f(v),
                        45 => l.offset.x = f(v),
                        46 => l.offset.y = f(v),
                        _ => {
                            if l.dashes.len() < 4 * hatch::MAX_DASHES {
                                l.dashes.push(f(v));
                            }
                        }
                    }
                }
            }
            79 => {}
            _ => break,
        }
    }
    done(&mut cur, &mut p);
    p
}

/// The hatch fills of the document and of this import, to use a fill again.
#[derive(Default)]
pub(super) struct HatchLib {
    known: Vec<(ResourceId, String, HatchDef)>,
    /// The fills that this import made or found, by their numbers.
    cache: HashMap<String, ResourceId>,
    /// The lower case names of all hatches.
    names: HashSet<String>,
}

impl HatchLib {
    pub fn new(existing: &[(ResourceId, String, HatchDef)]) -> HatchLib {
        HatchLib {
            known: existing.to_vec(),
            cache: HashMap::new(),
            names: existing.iter().map(|(_, n, _)| n.to_lowercase()).collect(),
        }
    }

    fn key(def: &HatchDef) -> String {
        use std::fmt::Write;
        let mut k = String::new();
        for l in &def.lines {
            let _ = write!(
                k,
                "{:.5} {:.5} {:.5} {:.5} {:.5}",
                l.angle, l.spacing, l.offset, l.origin.x, l.origin.y
            );
            for d in &l.dash {
                let _ = write!(k, " {d:.5}");
            }
            k.push(';');
        }
        k
    }

    /// The hatch fill that draws these lines: one that exists, else a new resource with this name
    /// (or the first free name that starts with it).
    pub fn resolve(&mut self, b: &mut Builder, name: &str, def: HatchDef) -> ResourceId {
        let key = Self::key(&def);
        if let Some(id) = self.cache.get(&key) {
            return *id;
        }
        let same_name = |n: &str| n.eq_ignore_ascii_case(name);
        let found = self
            .known
            .iter()
            .find(|(_, n, d)| same_name(n) && hatch::same_def(d, &def))
            .or_else(|| self.known.iter().find(|(_, _, d)| hatch::same_def(d, &def)))
            .map(|(id, _, _)| *id);
        if let Some(id) = found {
            self.cache.insert(key, id);
            return id;
        }
        let mut unique = name.to_string();
        let mut n = 2;
        while self.names.contains(&unique.to_lowercase()) {
            unique = format!("{name} ({n})");
            n += 1;
        }
        self.names.insert(unique.to_lowercase());
        let id = b.resource(
            &unique,
            ResourceBody::Graphic(Box::new(GraphicResource::Hatch(def.clone()))),
        );
        self.known.push((id, unique, def));
        self.cache.insert(key, id);
        id
    }
}

/// A hatch fill made from the pattern of an entity.
struct Made {
    def: HatchDef,
    /// The lines come from the table of standard patterns, not from the entity.
    from_table: bool,
    /// Pattern lines left out because of the limit.
    cut: usize,
    /// Pattern lines left out because they cannot be used.
    bad: usize,
}

/// Why a pattern hatch stayed an outline.
enum Outline {
    /// No pattern lines, and the name is not a standard pattern.
    NoLines,
    /// No pattern line could be used (the lines would lie on each other, or a number is wrong).
    Unusable,
    /// The lines are closer than the engine draws for the size of the hatch.
    Dense,
}

impl Walker<'_> {
    /// The hatch fill for a pattern hatch, or `None` when the hatch stays an outline (the reason is
    /// counted in the tally). `flip` is true when the OCS runs the other way (extrusion -Z).
    /// `extent_mm` is the larger side of the box around the boundary.
    pub(super) fn pattern_fill(
        &mut self,
        e: &Entity,
        xf: &Xf,
        flip: bool,
        extent_mm: f64,
    ) -> Option<ResourceId> {
        let raw = hatch_name(e);
        let result = self.pattern_lines(e, xf, flip, extent_mm);
        match result {
            Ok(Made {
                def,
                from_table,
                cut,
                bad,
            }) => {
                // The name of the file stays. A standard name gets the scale and the turn of the
                // lines, so a pattern at another size is told apart.
                let named = hatch::identify_as(&def, Some(&raw))
                    .filter(|n| n.name.eq_ignore_ascii_case(&raw));
                let name = match &named {
                    Some(n) => hatch::display_name(n),
                    None => raw.clone(),
                };
                if from_table {
                    self.tally.pattern_from_table += 1;
                }
                self.tally.pattern_lines_cut += cut;
                self.tally.pattern_lines_unusable += bad;
                *self.tally.pattern_fills.entry(name.clone()).or_default() += 1;
                if self.b.is_probe() {
                    return None;
                }
                let id = self.hatches.resolve(self.b, &name, def);
                Some(id)
            }
            Err(why) => {
                *self.tally.pattern_hatches.entry(raw).or_default() += 1;
                match why {
                    Outline::NoLines => self.tally.pattern_no_lines += 1,
                    Outline::Unusable => self.tally.pattern_unusable += 1,
                    Outline::Dense => self.tally.pattern_dense += 1,
                }
                None
            }
        }
    }

    fn pattern_lines(
        &self,
        e: &Entity,
        xf: &Xf,
        flip: bool,
        extent_mm: f64,
    ) -> Result<Made, Outline> {
        let p = read(e);
        let mut lines = p.lines;
        let mut bad = p.unread;
        let mut from_table = false;
        if lines.is_empty() {
            // A pattern name without lines: the standard pattern, scaled and turned as the codes say.
            let std = hatch::standard(&p.name).ok_or(Outline::NoLines)?;
            let r = DVec2::from_angle(p.angle.to_radians()) * p.scale;
            let cols = [r, DVec2::new(-r.y, r.x)];
            lines = std
                .lines
                .iter()
                .map(|l| l.mapped(cols, DVec2::ZERO))
                .collect();
            from_table = true;
        }
        if p.double && lines.len() * 2 <= hatch::MAX_LINES {
            let turned: Vec<PatLine> = lines.iter().map(PatLine::turned_90).collect();
            lines.extend(turned);
        }
        let cut = lines.len().saturating_sub(hatch::MAX_LINES);
        lines.truncate(hatch::MAX_LINES);
        let sx = if flip { -1.0 } else { 1.0 };
        let cols = [xf.cols[0] * sx, xf.cols[1]];
        let shift = xf.t - self.b.offset();
        let mut out = Vec::new();
        for l in &lines {
            match l.mapped(cols, shift).to_hatch() {
                Some(h) => out.push(hatch::reduced(&h)),
                None => bad += 1,
            }
        }
        if out.is_empty() {
            return Err(Outline::Unusable);
        }
        let dense = out
            .iter()
            .any(|h| extent_mm / h.spacing > kadakas_core::pattern::MAX_LINES as f64);
        if dense {
            return Err(Outline::Dense);
        }
        let def = HatchDef {
            units: PatternUnits::World,
            lines: out,
        };
        Ok(Made {
            def,
            from_table,
            cut,
            bad,
        })
    }
}

/// The name of the pattern in the entity, for the report and for the name of the resource.
fn hatch_name(e: &Entity) -> String {
    let name: String = e
        .get(2)
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("_USER") {
        "DXF pattern".to_string()
    } else {
        name.chars().take(100).collect()
    }
}
