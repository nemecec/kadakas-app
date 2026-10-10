//! The parameters of the transform and array commands, as they appear in JSON.

use glam::DVec2;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::array::{ArraySpec, Layout};
use super::xform::Xf;

/// A transform of objects. Angles are radians, counterclockwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Transform {
    /// Turn about `center`.
    Rotate {
        #[schemars(with = "[f64; 2]")]
        center: DVec2,
        angle: f64,
    },
    /// Scale about `center`. `sy` defaults to `sx`. A negative factor mirrors along that axis.
    Scale {
        #[schemars(with = "[f64; 2]")]
        center: DVec2,
        sx: f64,
        #[serde(default)]
        sy: Option<f64>,
    },
    /// Mirror on the line through `a` and `b`.
    Mirror {
        #[schemars(with = "[f64; 2]")]
        a: DVec2,
        #[schemars(with = "[f64; 2]")]
        b: DVec2,
    },
    /// Move by `delta`.
    Move {
        #[schemars(with = "[f64; 2]")]
        delta: DVec2,
    },
}

impl Transform {
    /// The plane transform, or the reason why there is none.
    pub fn xf(&self) -> Result<Xf, String> {
        let xf = match self {
            Transform::Rotate { center, angle } => {
                if !center.is_finite() || !angle.is_finite() {
                    return Err("numbers must be finite".into());
                }
                Xf::rotate(*center, *angle)
            }
            Transform::Scale { center, sx, sy } => {
                let sy = sy.unwrap_or(*sx);
                if !center.is_finite() || !sx.is_finite() || !sy.is_finite() {
                    return Err("numbers must be finite".into());
                }
                if *sx == 0.0 || sy == 0.0 {
                    return Err("a scale factor must not be zero".into());
                }
                Xf::scale(*center, *sx, sy)
            }
            Transform::Mirror { a, b } => {
                if !a.is_finite() || !b.is_finite() {
                    return Err("numbers must be finite".into());
                }
                Xf::mirror(*a, *b)
                    .ok_or_else(|| "the mirror line needs two different points".to_string())?
            }
            Transform::Move { delta } => {
                if !delta.is_finite() {
                    return Err("numbers must be finite".into());
                }
                Xf::translate(*delta)
            }
        };
        Ok(xf)
    }
}

fn yes() -> bool {
    true
}

/// The layout part of `ArrayParams`. Names follow the Duplicate Array command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum ArrayLayout {
    /// `count` copies along `offset` (x, y) from one copy to the next. Or give `distance` and
    /// `angle` (radians) for a polar offset.
    Linear {
        count: usize,
        #[serde(default)]
        #[schemars(with = "Option<[f64; 2]>")]
        offset: Option<DVec2>,
        #[serde(default)]
        distance: Option<f64>,
        #[serde(default)]
        angle: Option<f64>,
    },
    /// `columns` x `rows` places, `column_spacing` and `row_spacing` apart. The grid can be turned
    /// by `angle`. The place at column 0, row 0 is the original.
    Rectangular {
        columns: usize,
        rows: usize,
        column_spacing: f64,
        row_spacing: f64,
        #[serde(default)]
        angle: f64,
    },
    /// `count` copies about `center`, `angle` apart. The default angle spreads the copies
    /// and the original evenly over a full turn.
    Circular {
        count: usize,
        #[schemars(with = "[f64; 2]")]
        center: DVec2,
        #[serde(default)]
        angle: Option<f64>,
        /// Turn the copies too (default true).
        #[serde(default = "yes")]
        rotate_objects: bool,
    },
}

/// A duplicate array.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArrayParams {
    #[serde(flatten)]
    pub layout: ArrayLayout,
    /// Each copy is this many times larger than the one before (default 1).
    #[serde(default)]
    pub resize: Option<f64>,
    /// Each copy is turned this much more than the one before, in radians, about its own center.
    #[serde(default)]
    pub rotate: Option<f64>,
    /// Keep the original (default true). If not, the original goes and the array fills its place too.
    #[serde(default = "yes")]
    pub retain: bool,
}

impl ArrayParams {
    pub fn to_spec(&self) -> Result<ArraySpec, String> {
        let layout = match &self.layout {
            ArrayLayout::Linear {
                count,
                offset,
                distance,
                angle,
            } => {
                let step = match (offset, distance) {
                    (Some(o), None) => *o,
                    (None, Some(d)) => DVec2::from_angle(angle.unwrap_or(0.0)) * *d,
                    (None, None) => return Err("give offset, or distance and angle".into()),
                    (Some(_), Some(_)) => {
                        return Err("give offset, or distance and angle, not both".into());
                    }
                };
                Layout::Linear {
                    count: *count,
                    step,
                }
            }
            ArrayLayout::Rectangular {
                columns,
                rows,
                column_spacing,
                row_spacing,
                angle,
            } => Layout::Rectangular {
                columns: *columns,
                rows: *rows,
                column_step: *column_spacing,
                row_step: *row_spacing,
                angle: *angle,
            },
            ArrayLayout::Circular {
                count,
                center,
                angle,
                rotate_objects,
            } => {
                let all = *count as f64 + if self.retain { 1.0 } else { 0.0 };
                Layout::Circular {
                    count: *count,
                    center: *center,
                    angle: angle.unwrap_or(std::f64::consts::TAU / all.max(1.0)),
                    rotate_objects: *rotate_objects,
                }
            }
        };
        let mut spec = ArraySpec::new(layout);
        spec.resize = self.resize.unwrap_or(1.0);
        spec.rotate = self.rotate;
        match spec.problem() {
            Some(p) => Err(p),
            None => Ok(spec),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn transforms_read_from_json() {
        let t: Transform =
            serde_json::from_value(json!({"type": "scale", "center": [1, 2], "sx": 2})).unwrap();
        let xf = t.xf().unwrap();
        assert_eq!(xf.apply(DVec2::new(3.0, 2.0)), DVec2::new(5.0, 2.0));
        let m: Transform =
            serde_json::from_value(json!({"type": "mirror", "a": [0, 0], "b": [0, 1]})).unwrap();
        assert!(m.xf().unwrap().is_reflection());
        let bad: Transform =
            serde_json::from_value(json!({"type": "mirror", "a": [0, 0], "b": [0, 0]})).unwrap();
        assert!(bad.xf().is_err());
        let zero = Transform::Scale {
            center: DVec2::ZERO,
            sx: 0.0,
            sy: None,
        };
        assert!(zero.xf().unwrap_err().contains("zero"));
        assert!(
            Transform::Rotate {
                center: DVec2::ZERO,
                angle: f64::NAN
            }
            .xf()
            .is_err()
        );
        assert!(
            Transform::Move {
                delta: DVec2::new(f64::INFINITY, 0.0)
            }
            .xf()
            .is_err()
        );
        assert!(
            Transform::Scale {
                center: DVec2::ZERO,
                sx: 1.0,
                sy: Some(f64::NAN)
            }
            .xf()
            .is_err()
        );
        assert!(
            Transform::Mirror {
                a: DVec2::NAN,
                b: DVec2::X
            }
            .xf()
            .is_err()
        );
    }

    #[test]
    fn arrays_read_from_json() {
        let a: ArrayParams = serde_json::from_value(
            json!({"shape": "linear", "count": 3, "distance": 10, "angle": 0}),
        )
        .unwrap();
        assert!(a.retain);
        let spec = a.to_spec().unwrap();
        assert_eq!(
            spec.layout,
            Layout::Linear {
                count: 3,
                step: DVec2::new(10.0, 0.0)
            }
        );
        let c: ArrayParams =
            serde_json::from_value(json!({"shape": "circular", "count": 3, "center": [0, 0]}))
                .unwrap();
        let Layout::Circular {
            angle,
            rotate_objects,
            ..
        } = c.to_spec().unwrap().layout
        else {
            panic!()
        };
        assert!((angle - std::f64::consts::TAU / 4.0).abs() < 1e-12);
        assert!(rotate_objects);
        let no_keep: ArrayParams = serde_json::from_value(
            json!({"shape": "circular", "count": 4, "center": [0, 0], "retain": false}),
        )
        .unwrap();
        let Layout::Circular { angle, .. } = no_keep.to_spec().unwrap().layout else {
            panic!()
        };
        assert!((angle - std::f64::consts::TAU / 4.0).abs() < 1e-12);
        let both: ArrayParams = serde_json::from_value(
            json!({"shape": "linear", "count": 3, "offset": [1, 1], "distance": 2}),
        )
        .unwrap();
        assert!(both.to_spec().is_err());
        let none: ArrayParams =
            serde_json::from_value(json!({"shape": "linear", "count": 3})).unwrap();
        assert!(none.to_spec().is_err());
        let rect: ArrayParams = serde_json::from_value(json!({"shape": "rectangular", "columns": 2, "rows": 2, "column_spacing": 5, "row_spacing": 5, "resize": 1.5, "rotate": 0.1})).unwrap();
        let s = rect.to_spec().unwrap();
        assert_eq!((s.resize, s.rotate), (1.5, Some(0.1)));
        let big: ArrayParams =
            serde_json::from_value(json!({"shape": "linear", "count": 0, "offset": [1, 0]}))
                .unwrap();
        assert!(big.to_spec().is_err());
    }
}
