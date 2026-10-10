//! The messages of a reset. They are JSON, as bytes in the memory of the module.
//!
//! Request: `{"params": {...}, "geometry": {"type": "point", "at": [x, y], "rotation": 0}, "units": "mm"}`.
//!
//! Answer: `{"status": "ok", "shapes": [{"kind": "line", "a": [0, 0], "b": [10, 0]}, ...]}` or
//! `{"status": "error", "message": "..."}`. The shapes use the same JSON as the shapes of a
//! Kadakas document, for the kinds `line`, `rect`, `circle`, `arc`, `polyline` and `text`.
//! Units are millimeters, the Y axis points up, and angles are radians.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::MAX_VALUE;
use crate::manifest::ShapeType;

/// Most points in a path.
pub const MAX_POINTS: usize = 100_000;

/// Where the object sits. Coordinates are document coordinates in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geometry {
    Point {
        at: [f64; 2],
        #[serde(default)]
        rotation: f64,
    },
    Linear {
        a: [f64; 2],
        b: [f64; 2],
    },
    Rectangle {
        min: [f64; 2],
        max: [f64; 2],
    },
    Path {
        points: Vec<[f64; 2]>,
        #[serde(default)]
        closed: bool,
    },
}

fn pt(v: &Value, what: &str) -> Result<[f64; 2], String> {
    let a = v.as_array().filter(|a| a.len() == 2);
    let x = a.and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]));
    x.filter(|p| p.iter().all(|c| c.is_finite() && c.abs() <= MAX_VALUE))
        .ok_or_else(|| format!("{what} must be [x, y] with finite numbers"))
}

impl Geometry {
    /// The shape type that fits this geometry.
    pub fn shape_type(&self) -> ShapeType {
        match self {
            Geometry::Point { .. } => ShapeType::Point,
            Geometry::Linear { .. } => ShapeType::Linear,
            Geometry::Rectangle { .. } => ShapeType::Rectangle,
            Geometry::Path { .. } => ShapeType::Path,
        }
    }

    /// Reads the geometry for a shape type from plain JSON. `point`: `{at, rotation?}`. `linear`:
    /// `{a, b}`. `rectangle`: `{min, max}`. `path`: `{points, closed?}`. The `type` key is ignored.
    pub fn from_value(ty: ShapeType, v: &Value) -> Result<Geometry, String> {
        let m = v.as_object().ok_or("geometry must be an object")?;
        let get = |k: &str| m.get(k).ok_or(format!("geometry needs '{k}'"));
        match ty {
            ShapeType::Point => {
                let rotation = m.get("rotation").map_or(Ok(0.0), |r| {
                    r.as_f64()
                        .filter(|r| r.is_finite())
                        .ok_or("rotation must be a finite number")
                })?;
                Ok(Geometry::Point {
                    at: pt(get("at")?, "at")?,
                    rotation,
                })
            }
            ShapeType::Linear => Ok(Geometry::Linear {
                a: pt(get("a")?, "a")?,
                b: pt(get("b")?, "b")?,
            }),
            ShapeType::Rectangle => {
                let (a, b) = (pt(get("min")?, "min")?, pt(get("max")?, "max")?);
                Ok(Geometry::Rectangle {
                    min: [a[0].min(b[0]), a[1].min(b[1])],
                    max: [a[0].max(b[0]), a[1].max(b[1])],
                })
            }
            ShapeType::Path => {
                let arr = get("points")?.as_array().ok_or("points must be a list")?;
                if arr.len() < 2 {
                    return Err("a path needs at least 2 points".into());
                }
                if arr.len() > MAX_POINTS {
                    return Err(format!("a path has at most {MAX_POINTS} points"));
                }
                let points = arr
                    .iter()
                    .enumerate()
                    .map(|(i, p)| pt(p, &format!("points[{i}]")))
                    .collect::<Result<Vec<_>, _>>()?;
                let closed = m.get("closed").map_or(Ok(false), |c| {
                    c.as_bool().ok_or("closed must be true or false")
                })?;
                Ok(Geometry::Path { points, closed })
            }
        }
    }

    /// The plain JSON (without the `type` key) that [`Geometry::from_value`] reads.
    pub fn to_value(&self) -> Value {
        match self {
            Geometry::Point { at, rotation } => json!({"at": at, "rotation": rotation}),
            Geometry::Linear { a, b } => json!({"a": a, "b": b}),
            Geometry::Rectangle { min, max } => json!({"min": min, "max": max}),
            Geometry::Path { points, closed } => json!({"points": points, "closed": closed}),
        }
    }

    /// Moves the geometry.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        let mv = |p: &mut [f64; 2]| {
            p[0] += dx;
            p[1] += dy;
        };
        match self {
            Geometry::Point { at, .. } => mv(at),
            Geometry::Linear { a, b } => {
                mv(a);
                mv(b);
            }
            Geometry::Rectangle { min, max } => {
                mv(min);
                mv(max);
            }
            Geometry::Path { points, .. } => points.iter_mut().for_each(mv),
        }
    }
}

/// The request of a reset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResetRequest {
    /// The parameter values, checked against the schema of the manifest, with defaults filled in.
    pub params: Map<String, Value>,
    pub geometry: Geometry,
    /// Always `mm`.
    #[serde(default = "mm")]
    pub units: String,
}

fn mm() -> String {
    "mm".into()
}

impl ResetRequest {
    pub fn new(params: Map<String, Value>, geometry: Geometry) -> Self {
        ResetRequest {
            params,
            geometry,
            units: mm(),
        }
    }
}

/// The geometry of one shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeData {
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    Rect {
        min: [f64; 2],
        max: [f64; 2],
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
    /// Counterclockwise arc from `start` to `end`, in radians.
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        end: f64,
    },
    Polyline {
        points: Vec<[f64; 2]>,
        #[serde(default)]
        closed: bool,
    },
    Text {
        at: [f64; 2],
        text: String,
        height: f64,
    },
}

/// Graphic attributes of a shape. A color is `[r, g, b, a]`, each 0 to 255. An empty field uses the
/// class attribute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Attrs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 4]>,
    /// Line weight in mm on paper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weight: Option<f64>,
}

/// One shape of an answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutShape {
    #[serde(flatten)]
    pub shape: ShapeData,
    #[serde(default, skip_serializing_if = "is_default")]
    pub attrs: Attrs,
}

fn is_default(a: &Attrs) -> bool {
    *a == Attrs::default()
}

impl OutShape {
    /// A shape with the attributes of its class.
    pub fn new(shape: ShapeData) -> Self {
        OutShape {
            shape,
            attrs: Attrs::default(),
        }
    }

    /// The same shape with a pen color and a fill color.
    pub fn colored(mut self, pen: Option<[u8; 4]>, fill: Option<[u8; 4]>) -> Self {
        self.attrs.pen = pen;
        self.attrs.fill = fill;
        self
    }
}

/// The answer of a reset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResetResponse {
    Ok { shapes: Vec<OutShape> },
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_reads_plain_json_for_each_shape_type() {
        let p = Geometry::from_value(ShapeType::Point, &json!({"at": [1, 2], "rotation": 0.5}))
            .unwrap();
        assert_eq!(
            p,
            Geometry::Point {
                at: [1.0, 2.0],
                rotation: 0.5
            }
        );
        assert_eq!(p.shape_type(), ShapeType::Point);
        let default_rot = Geometry::from_value(ShapeType::Point, &json!({"at": [1, 2]})).unwrap();
        assert_eq!(
            default_rot,
            Geometry::Point {
                at: [1.0, 2.0],
                rotation: 0.0
            }
        );
        let l =
            Geometry::from_value(ShapeType::Linear, &json!({"a": [0, 0], "b": [5, 0]})).unwrap();
        assert_eq!(l.shape_type(), ShapeType::Linear);
        // Corners come out in order.
        let r = Geometry::from_value(ShapeType::Rectangle, &json!({"min": [9, 9], "max": [1, 2]}))
            .unwrap();
        assert_eq!(
            r,
            Geometry::Rectangle {
                min: [1.0, 2.0],
                max: [9.0, 9.0]
            }
        );
        assert_eq!(r.shape_type(), ShapeType::Rectangle);
        let path = Geometry::from_value(
            ShapeType::Path,
            &json!({"points": [[0, 0], [1, 1]], "closed": true}),
        )
        .unwrap();
        assert_eq!(path.shape_type(), ShapeType::Path);
        for g in [p, l, r, path] {
            let again = Geometry::from_value(g.shape_type(), &g.to_value()).unwrap();
            assert_eq!(again, g);
        }
    }

    #[test]
    fn bad_geometry_is_refused() {
        let bad = |ty, v: Value, what: &str| {
            let e = Geometry::from_value(ty, &v).unwrap_err();
            assert!(e.contains(what), "{e}");
        };
        bad(ShapeType::Point, json!(1), "must be an object");
        bad(ShapeType::Point, json!({}), "needs 'at'");
        bad(ShapeType::Point, json!({"at": [1]}), "at must be");
        bad(ShapeType::Point, json!({"at": [1e12, 0]}), "at must be");
        bad(
            ShapeType::Point,
            json!({"at": [0, 0], "rotation": "x"}),
            "rotation",
        );
        bad(ShapeType::Linear, json!({"a": [0, 0]}), "needs 'b'");
        bad(ShapeType::Rectangle, json!({"min": [0, 0]}), "needs 'max'");
        bad(ShapeType::Path, json!({"points": 1}), "must be a list");
        bad(ShapeType::Path, json!({"points": [[0, 0]]}), "at least 2");
        bad(
            ShapeType::Path,
            json!({"points": [[0, 0], "x"]}),
            "points[1]",
        );
        bad(
            ShapeType::Path,
            json!({"points": [[0, 0], [1, 1]], "closed": 3}),
            "closed",
        );
        let many: Vec<Value> = (0..MAX_POINTS + 1).map(|i| json!([i, 0])).collect();
        bad(ShapeType::Path, json!({"points": many}), "at most");
    }

    #[test]
    fn geometry_moves() {
        let mut gs = vec![
            Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
            Geometry::Linear {
                a: [0.0, 0.0],
                b: [1.0, 1.0],
            },
            Geometry::Rectangle {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
            },
            Geometry::Path {
                points: vec![[0.0, 0.0], [1.0, 1.0]],
                closed: false,
            },
        ];
        for g in &mut gs {
            g.translate(10.0, 20.0);
        }
        assert_eq!(
            gs[0],
            Geometry::Point {
                at: [10.0, 20.0],
                rotation: 0.0
            }
        );
        assert_eq!(
            gs[1],
            Geometry::Linear {
                a: [10.0, 20.0],
                b: [11.0, 21.0]
            }
        );
        assert_eq!(
            gs[2],
            Geometry::Rectangle {
                min: [10.0, 20.0],
                max: [11.0, 21.0]
            }
        );
        assert_eq!(
            gs[3],
            Geometry::Path {
                points: vec![[10.0, 20.0], [11.0, 21.0]],
                closed: false
            }
        );
    }

    #[test]
    fn the_request_and_the_answer_are_json() {
        let req = ResetRequest::new(
            json!({"w": 1}).as_object().unwrap().clone(),
            Geometry::Point {
                at: [1.0, 2.0],
                rotation: 0.0,
            },
        );
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["geometry"]["type"], "point");
        assert_eq!(v["units"], "mm");
        let back: ResetRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back, req);
        // `units` may be left out.
        let r: ResetRequest = serde_json::from_value(
            json!({"params": {}, "geometry": {"type": "linear", "a": [0, 0], "b": [1, 0]}}),
        )
        .unwrap();
        assert_eq!(r.units, "mm");

        let line = OutShape::new(ShapeData::Line {
            a: [0.0, 0.0],
            b: [1.0, 0.0],
        });
        let colored = OutShape::new(ShapeData::Circle {
            center: [0.0, 0.0],
            radius: 5.0,
        })
        .colored(Some([1, 2, 3, 255]), None);
        let ok = ResetResponse::Ok {
            shapes: vec![line.clone(), colored.clone()],
        };
        let v = serde_json::to_value(&ok).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(
            v["shapes"][0],
            json!({"kind": "line", "a": [0.0, 0.0], "b": [1.0, 0.0]})
        );
        assert_eq!(v["shapes"][1]["attrs"]["pen"], json!([1, 2, 3, 255]));
        assert_eq!(serde_json::from_value::<ResetResponse>(v).unwrap(), ok);
        let err: ResetResponse =
            serde_json::from_str(r#"{"status":"error","message":"no"}"#).unwrap();
        assert_eq!(
            err,
            ResetResponse::Error {
                message: "no".into()
            }
        );
        // Every shape kind survives the round trip.
        let kinds = vec![
            ShapeData::Rect {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
            },
            ShapeData::Arc {
                center: [0.0, 0.0],
                radius: 1.0,
                start: 0.0,
                end: 1.0,
            },
            ShapeData::Polyline {
                points: vec![[0.0, 0.0], [1.0, 1.0]],
                closed: true,
            },
            ShapeData::Text {
                at: [0.0, 0.0],
                text: "x".into(),
                height: 10.0,
            },
        ];
        for k in kinds {
            let s = OutShape::new(k);
            let v = serde_json::to_value(&s).unwrap();
            assert_eq!(serde_json::from_value::<OutShape>(v).unwrap(), s);
        }
    }
}
