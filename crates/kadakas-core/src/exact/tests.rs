use std::sync::Arc;

use glam::{DAffine3, DMat3, DVec3};

use super::*;

/// A box of 10 x 20 x 30 as six planes, and a cylinder face with a bore.
fn sample() -> ExactSolid {
    let mut s = ExactSolid {
        version: VERSION,
        product: "Part".into(),
        unit_mm: 1.0,
        tolerance_mm: 0.05,
        angle_deg: 15.0,
        vertices: vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 20.0, 0.0),
            DVec3::new(0.0, 20.0, 0.0),
        ],
        edges: vec![
            ExactEdge {
                curve: None,
                start: 0,
                end: 1,
                same_sense: true,
            },
            ExactEdge {
                curve: None,
                start: 1,
                end: 2,
                same_sense: true,
            },
            ExactEdge {
                curve: None,
                start: 2,
                end: 3,
                same_sense: true,
            },
            ExactEdge {
                curve: None,
                start: 3,
                end: 0,
                same_sense: true,
            },
        ],
        shells: vec![ExactShell {
            closed: true,
            outward: Some(true),
        }],
        faces: Vec::new(),
        mesh_hash: 7,
        extra: Default::default(),
    };
    let ring = |reverse: bool| -> Vec<LoopItem> {
        (0..4)
            .map(|e| LoopItem::Edge {
                edge: e,
                forward: !reverse,
            })
            .collect()
    };
    s.faces.push(ExactFace {
        shell: 0,
        surface: Some(Surface::Plane {
            f: DAffine3::from_translation(DVec3::new(0.0, 0.0, 30.0)),
        }),
        same_sense: true,
        flip: false,
        loops: vec![ExactLoop {
            outer: true,
            items: ring(false),
        }],
        color: Some([1, 2, 3, 255]),
        source: 5,
    });
    s.faces.push(ExactFace {
        shell: 0,
        surface: Some(Surface::Plane {
            f: DAffine3::IDENTITY,
        }),
        same_sense: false,
        flip: false,
        loops: vec![ExactLoop {
            outer: true,
            items: ring(true),
        }],
        color: None,
        source: 0,
    });
    s.faces.push(ExactFace {
        shell: 0,
        surface: Some(Surface::Cylinder {
            f: DAffine3::IDENTITY,
            r: 4.0,
        }),
        same_sense: false,
        flip: false,
        loops: Vec::new(),
        color: None,
        source: 0,
    });
    s.faces.push(ExactFace {
        shell: 0,
        surface: None,
        same_sense: true,
        flip: false,
        loops: vec![ExactLoop {
            outer: true,
            items: vec![
                LoopItem::Vertex { vertex: 0 },
                LoopItem::Vertex { vertex: 1 },
                LoopItem::Vertex { vertex: 2 },
            ],
        }],
        color: None,
        source: 0,
    });
    s
}

#[test]
fn json_round_trip_and_unknown_fields() {
    let s = sample();
    let text = serde_json::to_string(&s).unwrap();
    assert_eq!(serde_json::from_str::<ExactSolid>(&text).unwrap(), s);
    // A later version adds a field: it is kept.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["future_field"] = json!({"a": [1, 2]});
    let back: ExactSolid = serde_json::from_value(v).unwrap();
    assert_eq!(back.extra["future_field"], json!({"a": [1, 2]}));
    let again = serde_json::to_value(&back).unwrap();
    assert_eq!(again["future_field"], json!({"a": [1, 2]}));
    assert!(s.validate().is_ok());
    assert!(s.approx_bytes() > 1000 && s.approx_bytes() < 5000);
}

#[test]
fn uniform_scale_changes_the_sizes() {
    let s = sample();
    let map = DAffine3 {
        matrix3: DMat3::from_rotation_z(0.5) * 2.0,
        translation: DVec3::new(100.0, 0.0, 5.0),
    };
    let t = s.transformed(&map).unwrap();
    let Some(Surface::Cylinder { r, f }) = &t.faces[2].surface else {
        panic!()
    };
    assert!((r - 8.0).abs() < 1e-12);
    assert!((f.matrix3.x_axis.length() - 1.0).abs() < 1e-12);
    assert!((t.vertices[2] - map.transform_point3(s.vertices[2])).length() < 1e-12);
    assert!((t.tolerance_mm - 0.1).abs() < 1e-12);
    // A mirror turns the faces around, a stretch is refused.
    let mirror = DAffine3 {
        matrix3: DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)),
        translation: DVec3::ZERO,
    };
    let m = s.transformed(&mirror).unwrap();
    assert_eq!(m.faces[0].same_sense, !s.faces[0].same_sense);
    let stretch = DAffine3 {
        matrix3: DMat3::from_diagonal(DVec3::new(2.0, 1.0, 1.0)),
        translation: DVec3::ZERO,
    };
    assert!(s.transformed(&stretch).is_err());
    let flat = DAffine3 {
        matrix3: DMat3::ZERO,
        translation: DVec3::ZERO,
    };
    assert!(s.transformed(&flat).is_err());
    assert!(Similarity::of(&DAffine3::from_translation(DVec3::splat(f64::NAN))).is_none());
}

#[test]
fn every_surface_and_curve_survives_a_map() {
    let c = Curve::Circle {
        f: DAffine3::IDENTITY,
        r: 3.0,
    };
    let line = Curve::Line {
        o: DVec3::ZERO,
        d: DVec3::X,
    };
    let sims = Similarity::of(&DAffine3 {
        matrix3: DMat3::from_rotation_x(1.0) * 3.0,
        translation: DVec3::new(1.0, 2.0, 3.0),
    })
    .unwrap();
    let mirror = Similarity::of(&DAffine3 {
        matrix3: DMat3::from_diagonal(DVec3::new(1.0, -1.0, 1.0)),
        translation: DVec3::ZERO,
    })
    .unwrap();
    assert!(mirror.mirror && !sims.mirror);
    let curves = vec![
        c.clone(),
        line.clone(),
        Curve::Ellipse {
            f: DAffine3::IDENTITY,
            a: 2.0,
            b: 1.0,
        },
        Curve::Poly {
            points: vec![DVec3::ZERO, DVec3::X],
        },
        Curve::Trimmed {
            basis: Arc::new(c.clone()),
            t0: 0.0,
            t1: 1.0,
        },
        Curve::Composite {
            segments: vec![(Arc::new(line.clone()), true)],
        },
        Curve::Spline(nurbs::BSplineCurve {
            knots: nurbs::Knots::clamped(1, 2).unwrap(),
            points: vec![DVec3::ZERO, DVec3::X],
            weights: Some(vec![1.0, 2.0]),
        }),
    ];
    for cv in &curves {
        assert!(cv.validate().is_ok());
        for m in [&sims, &mirror] {
            let t = cv.transformed(m);
            assert!(t.validate().is_ok());
            // The point at a parameter moves like the map.
            let (a, b) = cv.natural_range().unwrap_or((0.0, 1.0));
            let p = cv.eval((a + b) / 2.0);
            let q = t.eval((a + b) / 2.0);
            assert!((q - m.point(p)).length() < 1e-9, "{cv:?}");
        }
    }
    let surfaces = vec![
        Surface::Plane {
            f: DAffine3::IDENTITY,
        },
        Surface::Cylinder {
            f: DAffine3::IDENTITY,
            r: 2.0,
        },
        Surface::Cone {
            f: DAffine3::IDENTITY,
            r: 2.0,
            angle: 0.3,
        },
        Surface::Sphere {
            f: DAffine3::IDENTITY,
            r: 2.0,
        },
        Surface::Torus {
            f: DAffine3::IDENTITY,
            big: 5.0,
            small: 1.0,
        },
        Surface::Revolution {
            curve: Arc::new(line.clone()),
            o: DVec3::ZERO,
            axis: DVec3::Z,
        },
        Surface::Extrusion {
            curve: Arc::new(c.clone()),
            dir: DVec3::Z,
        },
        Surface::Spline(nurbs::BSplineSurface {
            u: nurbs::Knots::clamped(1, 2).unwrap(),
            v: nurbs::Knots::clamped(1, 2).unwrap(),
            points: vec![DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(1.0, 1.0, 1.0)],
            weights: None,
        }),
    ];
    for s in &surfaces {
        assert!(s.validate().is_ok(), "{s:?}");
        for m in [&sims, &mirror] {
            let t = s.transformed(m);
            // The same points, found again by their position (the lengths in the parameters
            // of a plane or a cylinder scale with the map).
            let p = m.point(s.eval(0.4, 0.3));
            let (u, v) = t.uv_of(p, None).unwrap();
            assert!((t.eval(u, v) - p).length() < 1e-5, "{}", s.kind());
        }
        assert!(!s.short_kind().is_empty());
        assert!(s.describe()["kind"].is_string());
    }
}

#[test]
fn validation_refuses_damaged_data() {
    let good = sample();
    let mut s = good.clone();
    s.version = 99;
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.edges[0].end = 99;
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.faces[0].shell = 3;
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.faces[0].loops[0].items[0] = LoopItem::Edge {
        edge: 40,
        forward: true,
    };
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.faces[3].loops[0].items[0] = LoopItem::Vertex { vertex: 40 };
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.vertices[0].x = f64::NAN;
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.unit_mm = f64::INFINITY;
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.faces[2].surface = Some(Surface::Cylinder {
        f: DAffine3::IDENTITY,
        r: -1.0,
    });
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.edges[0].curve = Some(Curve::Circle {
        f: DAffine3::IDENTITY,
        r: f64::NAN,
    });
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.faces[0].surface = Some(Surface::Spline(nurbs::BSplineSurface {
        u: nurbs::Knots {
            degree: 3,
            values: vec![0.0],
            count: 4,
        },
        v: nurbs::Knots::clamped(1, 2).unwrap(),
        points: Vec::new(),
        weights: None,
    }));
    assert!(s.validate().is_err());
    let mut s = good.clone();
    s.vertices = vec![DVec3::ZERO; MAX_VERTICES + 1];
    assert!(s.validate().is_err());
    // A big spline is over the cap of one object.
    let mut s = good;
    let n = 200_000usize;
    s.edges[0].curve = Some(Curve::Spline(nurbs::BSplineCurve {
        knots: nurbs::Knots::clamped(1, n).unwrap(),
        points: vec![DVec3::ZERO; n],
        weights: None,
    }));
    assert!(s.approx_bytes() > MAX_OBJECT_BYTES);
    assert!(s.validate().is_err());
    // Bad curves and surfaces one by one.
    assert!(
        Curve::Line {
            o: DVec3::ZERO,
            d: DVec3::ZERO
        }
        .validate()
        .is_err()
    );
    assert!(
        Curve::Poly {
            points: vec![DVec3::ZERO]
        }
        .validate()
        .is_err()
    );
    assert!(Curve::Composite { segments: vec![] }.validate().is_err());
    assert!(
        Curve::Trimmed {
            basis: Arc::new(Curve::Line {
                o: DVec3::ZERO,
                d: DVec3::X
            }),
            t0: f64::NAN,
            t1: 1.0
        }
        .validate()
        .is_err()
    );
    let mut deep = Curve::Line {
        o: DVec3::ZERO,
        d: DVec3::X,
    };
    for _ in 0..12 {
        deep = Curve::Trimmed {
            basis: Arc::new(deep),
            t0: 0.0,
            t1: 1.0,
        };
    }
    assert!(deep.validate().is_err());
    assert!(
        Surface::Cone {
            f: DAffine3::IDENTITY,
            r: 1.0,
            angle: 0.0
        }
        .validate()
        .is_err()
    );
    assert!(
        Surface::Torus {
            f: DAffine3::IDENTITY,
            big: 1.0,
            small: 2.0
        }
        .validate()
        .is_err()
    );
    assert!(
        Surface::Extrusion {
            curve: Arc::new(Curve::Line {
                o: DVec3::ZERO,
                d: DVec3::X
            }),
            dir: DVec3::Z
        }
        .validate()
        .is_err()
    );
    assert!(
        Surface::Revolution {
            curve: Arc::new(Curve::Line {
                o: DVec3::ZERO,
                d: DVec3::X
            }),
            o: DVec3::ZERO,
            axis: DVec3::ZERO
        }
        .validate()
        .is_err()
    );
}

#[test]
fn fingerprint_summary_and_info() {
    let v = vec![DVec3::ZERO, DVec3::X, DVec3::Y];
    let t = vec![[0, 1, 2]];
    let a = fingerprint(&v, &t);
    assert_eq!(a, fingerprint(&v, &t));
    let mut v2 = v.clone();
    v2[1].x += 1e-9;
    assert_ne!(a, fingerprint(&v2, &t));
    assert_ne!(a, fingerprint(&v, &[[0, 2, 1]]));
    let s = sample();
    let text = s.summary();
    assert!(text.starts_with("Exact data kept: 4 faces"), "{text}");
    assert!(
        text.contains("cylinder x 1") && text.contains("plane x 3"),
        "{text}"
    );
    let info = s.info(10);
    assert_eq!(info["faces"], 4);
    assert_eq!(info["listed"], 4);
    let list = info["list"].as_array().unwrap();
    // The first plane faces +Z, the second has its normal turned around by same_sense = false.
    assert_eq!(list[0]["normal"], json!([0.0, 0.0, 1.0]));
    assert_eq!(list[1]["normal"], json!([-0.0, -0.0, -1.0]));
    assert_eq!(list[2]["kind"], "cylinder");
    assert_eq!(list[2]["radius"], 4.0);
    assert_eq!(list[2]["concave"], true);
    assert_eq!(list[3]["kind"], "plane");
    assert!(list[3]["normal"].is_array());
    assert_eq!(list[0]["color"], json!([1, 2, 3, 255]));
    assert_eq!(s.info(2)["listed"], 2);
}

#[test]
fn merge_and_retain_keep_the_numbers_right() {
    let a = sample();
    let mut m = a.clone();
    m.merge(&a);
    assert_eq!(m.faces.len(), 8);
    assert_eq!(m.vertices.len(), 8);
    assert_eq!(m.shells.len(), 2);
    assert!(m.validate().is_ok());
    // Keep only the coloured face of each copy.
    m.retain_faces(|f| f.color.is_some());
    assert_eq!(m.faces.len(), 2);
    assert_eq!(m.shells.len(), 2);
    assert_eq!(m.vertices.len(), 8);
    assert_eq!(m.edges.len(), 8);
    assert!(m.validate().is_ok());
    // The loops still walk the same corners.
    let LoopItem::Edge { edge, .. } = m.faces[1].loops[0].items[0] else {
        panic!()
    };
    assert_eq!(m.edges[edge as usize].start, 4);
    // Faces without edges or corners leave nothing behind.
    let mut e = a.clone();
    e.retain_faces(|f| f.loops.is_empty());
    assert_eq!(
        (
            e.faces.len(),
            e.edges.len(),
            e.vertices.len(),
            e.shells.len()
        ),
        (1, 0, 0, 1)
    );
    let mut n = a;
    n.retain_faces(|_| false);
    assert!(n.faces.is_empty() && n.shells.is_empty() && n.validate().is_ok());
}
