//! Module patterns: one or more closed shapes (the unit types) repeated on a lattice of two vectors.
//!
//! The shapes of a module are the outlines at the centers of the joints, like the cells of the
//! four bonds. The gap is taken off by moving each shape inward by half a gap. So a module is
//! written once and works for any gap, and a module without gaps between its shapes fills the
//! plane exactly. The gap lines of a module are the outlines of its shapes.
//!
//! Presets build a module from a few numbers: [`hexagon`], [`octagon_square`], [`i_shape`].

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};
use crate::edit2d::offset::{Side, offset_shapes, outline_points};
use crate::model::{MAX_COORD, Shape};

/// Most unit types in a module.
pub const MAX_UNIT_TYPES: usize = 16;
/// Most vertices of the outline of a unit type.
pub const MAX_OUTLINE_VERTICES: usize = 2000;
/// Flattening tolerance of the curves of a module, in mm.
pub const TOLERANCE: f64 = 0.25;
/// Shortest repeat vector, in mm.
pub const MIN_VECTOR: f64 = 1.0;

/// One unit type: a name and a closed outline. The outline may have curves and may be concave.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModuleUnit {
    pub name: String,
    pub outline: Vec<Vertex>,
}

/// The unit types and the two repeat vectors (the lattice) of a module pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub units: Vec<ModuleUnit>,
    pub a: [f64; 2],
    pub b: [f64; 2],
}

/// A unit type, ready to lay.
#[derive(Clone, Debug)]
pub struct Kind {
    pub name: String,
    /// The outline at the center of the joints (flat, counterclockwise).
    pub cell: Vec<DVec2>,
    /// The pieces to lay: the outline moved inward by half a gap. Usually one.
    pub pieces: Vec<Vec<DVec2>>,
    /// The area of one piece list (all pieces), in mm2.
    pub area: f64,
    /// The box of the cell.
    pub lo: DVec2,
    pub hi: DVec2,
}

/// A module that was checked and flattened.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub kinds: Vec<Kind>,
    pub a: DVec2,
    pub b: DVec2,
    pub det: f64,
}

fn ring_area(r: &[DVec2]) -> f64 {
    (0..r.len())
        .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
        .sum::<f64>()
        / 2.0
}

fn paths(r: &[DVec2], t: DVec2) -> Vec<Vec<[f64; 2]>> {
    vec![r.iter().map(|p| [p.x + t.x, p.y + t.y]).collect()]
}

fn overlap_area(a: &[DVec2], b: &[DVec2], t: DVec2) -> f64 {
    let shapes =
        paths(a, DVec2::ZERO).overlay(&paths(b, t), OverlayRule::Intersect, FillRule::NonZero);
    shapes
        .iter()
        .map(|s| {
            s.iter()
                .enumerate()
                .map(|(i, r)| {
                    let pts: Vec<DVec2> = r.iter().map(|p| DVec2::new(p[0], p[1])).collect();
                    let a = ring_area(&pts).abs();
                    if i == 0 { a } else { -a }
                })
                .sum::<f64>()
        })
        .sum()
}

fn crosses_itself(r: &[DVec2]) -> bool {
    let n = r.len();
    for i in 0..n {
        let (p, q) = (r[i], r[(i + 1) % n]);
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (s, t) = (r[j], r[(j + 1) % n]);
            let d1 = (q - p).perp_dot(s - p);
            let d2 = (q - p).perp_dot(t - p);
            let d3 = (t - s).perp_dot(p - s);
            let d4 = (t - s).perp_dot(q - s);
            if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                return true;
            }
        }
    }
    false
}

impl Module {
    /// Checks the module and flattens it for the gap `gap`. The shapes of one module must not
    /// overlap each other or their copies on the neighbouring lattice points (a, b, a+b, a-b and
    /// the opposites); spaces between shapes are allowed and show as gaps.
    pub fn compile(&self, gap: f64) -> Result<Compiled, String> {
        if self.units.is_empty() {
            return Err("a module needs at least one unit shape".into());
        }
        if self.units.len() > MAX_UNIT_TYPES {
            return Err(format!("a module has at most {MAX_UNIT_TYPES} unit shapes"));
        }
        let (a, b) = (DVec2::from(self.a), DVec2::from(self.b));
        let finite = |v: DVec2| v.is_finite() && v.abs().max_element() <= MAX_COORD;
        if !finite(a) || !finite(b) {
            return Err("the repeat vectors a and b must be finite numbers".into());
        }
        if a.length() < MIN_VECTOR || b.length() < MIN_VECTOR {
            return Err(format!(
                "the repeat vectors a and b must be at least {MIN_VECTOR} mm long"
            ));
        }
        let det = a.perp_dot(b);
        if det.abs() < 1e-6 * a.length() * b.length() {
            return Err(
                "the repeat vectors a and b point the same way: the lattice has no area".into(),
            );
        }
        if !(gap.is_finite() && (0.0..=MAX_COORD).contains(&gap)) {
            return Err("the gap must be a finite number of zero or more".into());
        }
        let mut kinds: Vec<Kind> = Vec::new();
        for (n, u) in self.units.iter().enumerate() {
            let label = if u.name.trim().is_empty() {
                format!("unit shape {}", n + 1)
            } else {
                format!("unit shape '{}'", u.name)
            };
            if u.outline.len() < 3 {
                return Err(format!("{label} needs 3 or more points"));
            }
            if u.outline.len() > MAX_OUTLINE_VERTICES {
                return Err(format!(
                    "{label} has more than {MAX_OUTLINE_VERTICES} points"
                ));
            }
            if u.outline
                .iter()
                .any(|v| !v.is_finite() || v.p.abs().max_element() > MAX_COORD)
            {
                return Err(format!("{label} has a number that is not finite"));
            }
            let mut cell =
                crate::geom::open_ring(&curve::flatten(&u.outline, true, TOLERANCE), 1e-9);
            if cell.len() < 3 {
                return Err(format!("{label} has no area"));
            }
            if ring_area(&cell) < 0.0 {
                cell.reverse();
            }
            let area = ring_area(&cell);
            if area <= 0.0 {
                return Err(format!("{label} has no area"));
            }
            if crosses_itself(&cell) {
                return Err(format!("{label} crosses itself"));
            }
            let pieces: Vec<Vec<DVec2>> = if gap > 0.0 {
                let shape = Shape::Polyline {
                    points: curve::corners(&cell),
                    closed: true,
                };
                offset_shapes(&shape, gap / 2.0, Side::Inside)
                    .map_err(|e| format!("{label} cannot take a gap of {gap} mm: {e}"))?
                    .iter()
                    .filter_map(outline_points)
                    .map(|mut r| {
                        r = crate::geom::open_ring(&r, 1e-9);
                        if ring_area(&r) < 0.0 {
                            r.reverse();
                        }
                        r
                    })
                    .filter(|r| r.len() >= 3 && ring_area(r) > 1e-6)
                    .collect()
            } else {
                vec![cell.clone()]
            };
            if pieces.is_empty() {
                return Err(format!("{label} disappears with a gap of {gap} mm"));
            }
            let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            for p in &cell {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            kinds.push(Kind {
                name: u.name.clone(),
                area: pieces.iter().map(|r| ring_area(r)).sum(),
                cell,
                pieces,
                lo,
                hi,
            });
        }
        let total: f64 = kinds.iter().map(|k| ring_area(&k.cell)).sum();
        if total > det.abs() * (1.0 + 1e-6) {
            return Err(format!(
                "the shapes together ({:.0} mm2) are larger than the lattice cell ({:.0} mm2): they overlap their copies",
                total,
                det.abs()
            ));
        }
        // Overlaps: between the shapes of one cell, and with the copies on the next lattice points.
        let steps = [a, b, a + b, a - b];
        for i in 0..kinds.len() {
            for j in 0..kinds.len() {
                let small = ring_area(&kinds[i].cell).min(ring_area(&kinds[j].cell));
                let limit = 1e-6 * small;
                let mut shifts: Vec<(DVec2, &str)> = Vec::new();
                if i < j {
                    shifts.push((DVec2::ZERO, "in the same cell"));
                }
                for (k, s) in steps.iter().enumerate() {
                    let what = ["a", "b", "a + b", "a - b"][k];
                    shifts.push((*s, what));
                    shifts.push((-*s, what));
                }
                for (t, what) in shifts {
                    if overlap_area(&kinds[i].cell, &kinds[j].cell, t) > limit {
                        let (n1, n2) = (&kinds[i].name, &kinds[j].name);
                        return Err(if t == DVec2::ZERO {
                            format!("the unit shapes '{n1}' and '{n2}' overlap")
                        } else {
                            format!(
                                "the unit shape '{n1}' overlaps '{n2}' moved by {what} (check the repeat vectors)"
                            )
                        });
                    }
                }
            }
        }
        Ok(Compiled { kinds, a, b, det })
    }

    /// The names of the unit shapes.
    pub fn names(&self) -> Vec<String> {
        self.units.iter().map(|u| u.name.clone()).collect()
    }
}

impl Compiled {
    /// The lattice coordinates (u, v) of a point: p = u a + v b.
    pub fn lattice_of(&self, p: DVec2) -> (f64, f64) {
        (p.perp_dot(self.b) / self.det, self.a.perp_dot(p) / self.det)
    }
}

fn ring(points: &[DVec2]) -> Vec<Vertex> {
    curve::corners(points)
}

/// A regular hexagon with the given side, flat side up. Lattice centers are 1.5 sides apart.
pub fn hexagon(side: f64) -> Result<Module, String> {
    if !(side.is_finite() && (5.0..=1.0e5).contains(&side)) {
        return Err("the hexagon side must be between 5 mm and 100000 mm".into());
    }
    let h = side * 3.0f64.sqrt() / 2.0;
    let pts: Vec<DVec2> = (0..6)
        .map(|k| DVec2::from_angle(std::f64::consts::FRAC_PI_3 * k as f64) * side)
        .collect();
    Ok(Module {
        units: vec![ModuleUnit {
            name: "hexagon".into(),
            outline: ring(&pts),
        }],
        a: [1.5 * side, h],
        b: [0.0, 2.0 * h],
    })
}

/// A regular hexagon from the distance across the flats.
pub fn hexagon_across_flats(across: f64) -> Result<Module, String> {
    hexagon(across / 3.0f64.sqrt())
}

/// A regular octagon with the given side, and the small square (turned 45 degrees, same side)
/// that fills the space between four octagons.
pub fn octagon_square(side: f64) -> Result<Module, String> {
    if !(side.is_finite() && (5.0..=1.0e5).contains(&side)) {
        return Err("the octagon side must be between 5 mm and 100000 mm".into());
    }
    let w = side * (1.0 + std::f64::consts::SQRT_2);
    let r = w / 2.0 / (std::f64::consts::PI / 8.0).cos();
    let oct: Vec<DVec2> = (0..8)
        .map(|k| {
            DVec2::from_angle(std::f64::consts::FRAC_PI_4 * k as f64 + std::f64::consts::PI / 8.0)
                * r
        })
        .collect();
    let c = DVec2::splat(w / 2.0);
    let d = side / std::f64::consts::SQRT_2;
    let square = vec![
        c + DVec2::new(-d, 0.0),
        c + DVec2::new(0.0, -d),
        c + DVec2::new(d, 0.0),
        c + DVec2::new(0.0, d),
    ];
    Ok(Module {
        units: vec![
            ModuleUnit {
                name: "octagon".into(),
                outline: ring(&oct),
            },
            ModuleUnit {
                name: "square".into(),
                outline: ring(&square),
            },
        ],
        a: [w, 0.0],
        b: [0.0, w],
    })
}

/// An I-shaped (double-T) interlocking paver. `length` is the long side and must be a multiple
/// of 4 flanges: each flange is a quarter of the length, the waist between them half. `width` is
/// the width over the flanges and `notch` how far the waist is set in from each long side. Rows
/// are shifted by half a length and touch at the flanges, so the pavers lock into each other.
pub fn i_shape(length: f64, width: f64, notch: f64) -> Result<Module, String> {
    if !(length.is_finite() && width.is_finite() && notch.is_finite()) {
        return Err("the sizes of the I-shaped paver must be finite numbers".into());
    }
    if length < 20.0 || width < 20.0 || notch <= 0.0 || notch * 2.0 >= width {
        return Err(
            "the I-shaped paver needs a length and width of 20 mm or more, and a notch that is above 0 and below half the width".into(),
        );
    }
    let f = length / 4.0;
    // The notches are on both long sides, in the middle half.
    let pts = vec![
        DVec2::new(0.0, 0.0),
        DVec2::new(f, 0.0),
        DVec2::new(f, notch),
        DVec2::new(length - f, notch),
        DVec2::new(length - f, 0.0),
        DVec2::new(length, 0.0),
        DVec2::new(length, width),
        DVec2::new(length - f, width),
        DVec2::new(length - f, width - notch),
        DVec2::new(f, width - notch),
        DVec2::new(f, width),
        DVec2::new(0.0, width),
    ];
    Ok(Module {
        units: vec![ModuleUnit {
            name: "i_shape".into(),
            outline: ring(&pts),
        }],
        a: [length, 0.0],
        b: [length / 2.0, width - notch],
    })
}

/// A preset by name with its numbers (`side`, `across_flats`, `length`, `width`, `notch`).
pub fn preset(name: &str, number: &dyn Fn(&str) -> Option<f64>) -> Result<Module, String> {
    let need = |k: &str| number(k).ok_or_else(|| format!("the preset '{name}' needs '{k}'"));
    match name
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "hexagon" => match (number("side"), number("across_flats")) {
            (Some(s), None) => hexagon(s),
            (None, Some(a)) => hexagon_across_flats(a),
            (Some(_), Some(_)) => {
                Err("give 'side' or 'across_flats' for a hexagon, not both".into())
            }
            (None, None) => Err("a hexagon needs 'side' or 'across_flats' (mm)".into()),
        },
        "octagon_square" | "octagon" => octagon_square(need("side")?),
        "i_shape" | "i" => i_shape(
            number("length").unwrap_or(200.0),
            number("width").unwrap_or(165.0),
            number("notch").unwrap_or(25.0),
        ),
        other => Err(format!(
            "no preset '{other}'. Presets: {}",
            PRESETS.join(", ")
        )),
    }
}

/// The names of the presets.
pub const PRESETS: [&str; 3] = ["hexagon", "octagon_square", "i_shape"];

/// A module from closed shapes laid in the drawing. Without `a` and `b` the lattice is the box
/// of all shapes (a stack): `a` along x and `b` along y. The module is moved so that the corner of
/// the box is the origin. Returns the module and a note when the lattice was taken from the box.
pub fn from_outlines(
    units: Vec<ModuleUnit>,
    a: Option<[f64; 2]>,
    b: Option<[f64; 2]>,
) -> Result<(Module, Option<String>), String> {
    if units.is_empty() {
        return Err("give at least one closed shape".into());
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for u in &units {
        for p in curve::flatten(&u.outline, true, TOLERANCE) {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let moved: Vec<ModuleUnit> = units
        .into_iter()
        .map(|u| ModuleUnit {
            name: u.name,
            outline: u
                .outline
                .into_iter()
                .map(|v| Vertex {
                    p: v.p - lo,
                    kind: v.kind,
                })
                .collect(),
        })
        .collect();
    let size = hi - lo;
    let (a, b, note) = match (a, b) {
        (Some(a), Some(b)) => (a, b, None),
        (None, None) => (
            [size.x, 0.0],
            [0.0, size.y],
            Some(format!(
                "no repeat vectors given: the lattice is the box of the shapes, a = [{:.1}, 0] and b = [0, {:.1}] (a stack)",
                size.x, size.y
            )),
        ),
        _ => return Err("give both 'a' and 'b', or neither".into()),
    };
    Ok((Module { units: moved, a, b }, note))
}
