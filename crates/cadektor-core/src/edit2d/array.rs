//! Duplicate array: the transforms of the copies of linear, rectangular and circular arrays.
//!
//! The words follow the Duplicate Array command of Vectorworks. A linear array makes `count`
//! copies along an offset. A rectangular array fills `columns` x `rows` places. A circular array
//! turns copies about a center. Each copy can also be resized and turned a bit more than the one
//! before. The reference point of the objects is the center of their bounds (`origin`).

use glam::DVec2;

use super::xform::Xf;

/// The most copies one array makes.
pub const MAX_COPIES: usize = 10_000;

/// The most objects that one array or transform command creates, groups and their members counted.
pub const MAX_CREATED: usize = 100_000;

/// The shape of an array.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    /// `count` copies. Copy k is moved by k times `step`.
    Linear { count: usize, step: DVec2 },
    /// `columns` x `rows` places. `column_step` is the distance between columns along the grid x
    /// axis, `row_step` along the grid y axis. The grid is turned by `angle` radians.
    Rectangular {
        columns: usize,
        rows: usize,
        column_step: f64,
        row_step: f64,
        angle: f64,
    },
    /// `count` copies about `center`. Copy k is turned by k times `angle` (radians).
    Circular {
        count: usize,
        center: DVec2,
        angle: f64,
        /// Turn the copies too, as if they were fixed to a wheel.
        rotate_objects: bool,
    },
}

/// A complete array.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArraySpec {
    pub layout: Layout,
    /// Each copy is this many times larger than the one before (1: same size).
    pub resize: f64,
    /// Each copy is turned this much more than the one before, in radians, about its own center.
    /// In a circular array with `rotate_objects`, this replaces the turn of the wheel.
    pub rotate: Option<f64>,
}

impl ArraySpec {
    pub fn new(layout: Layout) -> ArraySpec {
        ArraySpec {
            layout,
            resize: 1.0,
            rotate: None,
        }
    }

    /// How many copies of the objects the array makes (before the numbers are checked).
    pub fn copies(&self) -> usize {
        match self.layout {
            Layout::Linear { count, .. } | Layout::Circular { count, .. } => count,
            Layout::Rectangular { columns, rows, .. } => columns.saturating_mul(rows),
        }
    }

    /// Checks the numbers.
    pub fn problem(&self) -> Option<String> {
        let finite = match self.layout {
            Layout::Linear { step, .. } => step.is_finite(),
            Layout::Rectangular {
                column_step,
                row_step,
                angle,
                ..
            } => column_step.is_finite() && row_step.is_finite() && angle.is_finite(),
            Layout::Circular { center, angle, .. } => center.is_finite() && angle.is_finite(),
        } && self.resize.is_finite()
            && self.rotate.is_none_or(f64::is_finite);
        if !finite {
            return Some("numbers must be finite".into());
        }
        if self.resize <= 0.0 {
            return Some("resize must be above zero".into());
        }
        let total = match self.layout {
            Layout::Linear { count, .. } | Layout::Circular { count, .. } => count,
            Layout::Rectangular { columns, rows, .. } => columns.saturating_mul(rows),
        };
        if total == 0 {
            return Some("an array needs at least one copy".into());
        }
        if total > MAX_COPIES {
            return Some(format!("an array makes at most {MAX_COPIES} copies"));
        }
        None
    }
}

/// The transforms of the copies, one per copy. `origin` is the reference point of the objects.
/// For a rectangular array the first place (column 0, row 0) is the original; it is the first
/// transform and is the identity. A linear and a circular array do not include the original.
pub fn placements(spec: &ArraySpec, origin: DVec2) -> Result<Vec<Xf>, String> {
    if let Some(p) = spec.problem() {
        return Err(p);
    }
    if !origin.is_finite() {
        return Err("numbers must be finite".into());
    }
    let own = |k: usize, turn: f64| -> Xf {
        // Resize and turn about the own center, in this order.
        let s = spec.resize.powi(k as i32);
        let mut xf = Xf::scale(origin, s, s);
        if turn != 0.0 {
            xf = xf.then(Xf::rotate(origin, turn));
        }
        xf
    };
    Ok(match spec.layout {
        Layout::Linear { count, step } => (1..=count)
            .map(|k| {
                own(k, spec.rotate.unwrap_or(0.0) * k as f64).then(Xf::translate(step * k as f64))
            })
            .collect(),
        Layout::Rectangular {
            columns,
            rows,
            column_step,
            row_step,
            angle,
        } => {
            let (u, w) = if angle == 0.0 {
                (DVec2::X, DVec2::Y)
            } else {
                (DVec2::from_angle(angle), DVec2::from_angle(angle).perp())
            };
            let mut out = Vec::with_capacity(columns * rows);
            let mut k = 0;
            for r in 0..rows {
                for c in 0..columns {
                    let d = u * (column_step * c as f64) + w * (row_step * r as f64);
                    out.push(own(k, spec.rotate.unwrap_or(0.0) * k as f64).then(Xf::translate(d)));
                    k += 1;
                }
            }
            out
        }
        Layout::Circular {
            count,
            center,
            angle,
            rotate_objects,
        } => (1..=count)
            .map(|k| {
                let a = angle * k as f64;
                let moved = center + DVec2::from_angle(a).rotate(origin - center);
                let turn = if rotate_objects {
                    spec.rotate.map_or(a, |r| r * k as f64)
                } else {
                    spec.rotate.unwrap_or(0.0) * k as f64
                };
                own(k, turn).then(Xf::translate(moved - origin))
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, TAU};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn linear_copies_step_along() {
        let spec = ArraySpec::new(Layout::Linear {
            count: 3,
            step: v(10.0, 5.0),
        });
        let p = placements(&spec, v(1.0, 1.0)).unwrap();
        assert_eq!(p.len(), 3);
        assert!(p[2].apply(v(1.0, 1.0)).distance(v(31.0, 16.0)) < 1e-9);
        assert!(p[0].is_translation());
    }

    #[test]
    fn linear_resize_and_rotate_about_the_own_center() {
        let mut spec = ArraySpec::new(Layout::Linear {
            count: 2,
            step: v(100.0, 0.0),
        });
        spec.resize = 2.0;
        spec.rotate = Some(FRAC_PI_2);
        let p = placements(&spec, v(0.0, 0.0)).unwrap();
        // Copy 2: size x4, turned by 180 degrees, moved by 200.
        assert!(p[1].apply(v(1.0, 0.0)).distance(v(196.0, 0.0)) < 1e-9);
        assert!(p[1].apply(v(0.0, 0.0)).distance(v(200.0, 0.0)) < 1e-9);
    }

    #[test]
    fn rectangular_fills_the_grid_row_by_row() {
        let spec = ArraySpec::new(Layout::Rectangular {
            columns: 3,
            rows: 2,
            column_step: 10.0,
            row_step: 20.0,
            angle: 0.0,
        });
        let p = placements(&spec, v(0.0, 0.0)).unwrap();
        assert_eq!(p.len(), 6);
        assert!(p[0].is_translation() && p[0].t == DVec2::ZERO);
        assert_eq!(p[2].t, v(20.0, 0.0));
        assert!(p[3].t.distance(v(0.0, 20.0)) < 1e-9);
        assert!(p[5].t.distance(v(20.0, 20.0)) < 1e-9);
        let turned = ArraySpec::new(Layout::Rectangular {
            columns: 2,
            rows: 1,
            column_step: 10.0,
            row_step: 0.0,
            angle: FRAC_PI_2,
        });
        assert!(
            placements(&turned, v(0.0, 0.0)).unwrap()[1]
                .t
                .distance(v(0.0, 10.0))
                < 1e-9
        );
    }

    #[test]
    fn circular_copies_turn_about_the_center() {
        let spec = ArraySpec::new(Layout::Circular {
            count: 3,
            center: v(0.0, 0.0),
            angle: TAU / 4.0,
            rotate_objects: true,
        });
        let p = placements(&spec, v(10.0, 0.0)).unwrap();
        assert!(p[0].apply(v(10.0, 0.0)).distance(v(0.0, 10.0)) < 1e-9);
        assert!(p[1].apply(v(10.0, 0.0)).distance(v(-10.0, 0.0)) < 1e-9);
        // Turned objects: a point to the right of the origin point goes along the circle.
        assert!(p[0].apply(v(11.0, 0.0)).distance(v(0.0, 11.0)) < 1e-9);
        assert!(p[0].is_rotation());
        let fixed = ArraySpec::new(Layout::Circular {
            count: 1,
            center: v(0.0, 0.0),
            angle: FRAC_PI_2,
            rotate_objects: false,
        });
        let q = placements(&fixed, v(10.0, 0.0)).unwrap();
        // Not turned: a point beside the origin point is only moved.
        assert!(q[0].apply(v(11.0, 0.0)).distance(v(1.0, 10.0)) < 1e-9);
        assert!(q[0].is_translation());
    }

    #[test]
    fn bad_specs_are_refused() {
        let lin = |count, step| ArraySpec::new(Layout::Linear { count, step });
        assert!(placements(&lin(0, v(1.0, 0.0)), v(0.0, 0.0)).is_err());
        assert!(placements(&lin(MAX_COPIES + 1, v(1.0, 0.0)), v(0.0, 0.0)).is_err());
        assert!(placements(&lin(2, v(f64::NAN, 0.0)), v(0.0, 0.0)).is_err());
        assert!(placements(&lin(2, v(1.0, 0.0)), v(f64::INFINITY, 0.0)).is_err());
        let mut s = lin(2, v(1.0, 0.0));
        s.resize = 0.0;
        assert!(s.problem().unwrap().contains("resize"));
        s.resize = 1.0;
        s.rotate = Some(f64::NAN);
        assert!(s.problem().is_some());
        let r = ArraySpec::new(Layout::Rectangular {
            columns: 200,
            rows: 200,
            column_step: 1.0,
            row_step: 1.0,
            angle: 0.0,
        });
        assert!(r.problem().unwrap().contains("at most"));
    }
}
