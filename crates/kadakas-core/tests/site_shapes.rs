//! Site model and site modifier shapes in the core: data, commands, files, 2D and 3D hooks.

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::curve::corners;
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::svg;

fn site(points: &[(f64, f64, f64)]) -> Shape {
    Shape::SiteModel {
        points: points
            .iter()
            .map(|(x, y, z)| DVec3::new(*x, *y, *z))
            .collect(),
        lines: vec![SiteLine {
            points: vec![DVec3::new(0.0, 0.0, 1.0), DVec3::new(50.0, 5.0, 2.0)],
            closed: false,
        }],
        settings: SiteSettings::default(),
    }
}

fn pad() -> Shape {
    Shape::SiteModifier {
        modifier: ModifierKind::Pad {
            elevation: 100.0,
            slope: None,
        },
        boundary: corners(&[
            DVec2::new(10.0, 10.0),
            DVec2::new(20.0, 10.0),
            DVec2::new(20.0, 30.0),
            DVec2::new(10.0, 30.0),
        ]),
    }
}

fn add(ed: &mut Editor, shape: Shape) -> Result<ObjectId, CommandError> {
    ed.execute(&Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .map(|o| ObjectId(o.created[0]))
}

#[test]
fn site_shapes_report_kind_bounds_area_and_move_in_xy() {
    let mut s = site(&[(0.0, 0.0, 5.0), (100.0, 40.0, 9.0), (30.0, 80.0, 1.0)]);
    assert_eq!(s.kind(), "site_model");
    assert_eq!(s.bounds(), (DVec2::new(0.0, 0.0), DVec2::new(100.0, 80.0)));
    assert!(s.vertices().is_none() && s.area().is_none());
    s.translate(DVec2::new(5.0, -5.0));
    let Shape::SiteModel { points, lines, .. } = &s else {
        panic!()
    };
    assert_eq!(points[0], DVec3::new(5.0, -5.0, 5.0));
    assert_eq!(lines[0].points[1], DVec3::new(55.0, 0.0, 2.0));

    let mut m = pad();
    assert_eq!(m.kind(), "site_modifier");
    assert_eq!(m.area(), Some(200.0));
    assert_eq!(m.vertices().unwrap().len(), 4);
    assert_eq!(m.bounds(), (DVec2::new(10.0, 10.0), DVec2::new(20.0, 30.0)));
    m.vertices_mut().unwrap().pop();
    assert_eq!(m.vertices().unwrap().len(), 3);
    m.translate(DVec2::new(1.0, 1.0));
    assert_eq!(m.bounds().0, DVec2::new(11.0, 11.0));
}

#[test]
fn site_shapes_check_finite_and_range() {
    assert!(site(&[(0.0, 0.0, 0.0)]).is_finite() && site(&[(0.0, 0.0, 0.0)]).is_in_range());
    assert!(!site(&[(0.0, 0.0, f64::NAN)]).is_finite());
    assert!(!site(&[(0.0, 0.0, 2.0 * MAX_COORD)]).is_in_range());
    let mut s = site(&[(0.0, 0.0, 0.0)]);
    if let Shape::SiteModel {
        settings, lines, ..
    } = &mut s
    {
        settings.datum = f64::INFINITY;
        assert!(
            !Shape::SiteModel {
                points: vec![],
                lines: lines.clone(),
                settings: settings.clone()
            }
            .is_finite()
        );
    }
    let mut s = site(&[(0.0, 0.0, 0.0)]);
    if let Shape::SiteModel { lines, .. } = &mut s {
        lines[0].points[0].z = f64::NAN;
    }
    assert!(!s.is_finite());
    assert!(pad().is_finite() && pad().is_in_range());
    let bad = Shape::SiteModifier {
        modifier: ModifierKind::Pad {
            elevation: f64::NAN,
            slope: None,
        },
        boundary: vec![],
    };
    assert!(!bad.is_finite());
    let far = Shape::SiteModifier {
        modifier: ModifierKind::Pad {
            elevation: 5.0 * MAX_COORD,
            slope: None,
        },
        boundary: vec![],
    };
    assert!(!far.is_in_range());
}

#[test]
fn commands_validate_site_shapes() {
    let mut ed = Editor::default();
    let ok = add(
        &mut ed,
        site(&[(0.0, 0.0, 0.0), (10.0, 0.0, 1.0), (0.0, 10.0, 2.0)]),
    )
    .unwrap();
    add(&mut ed, pad()).unwrap();
    let mut bad = site(&[(0.0, 0.0, 0.0)]);
    if let Shape::SiteModel { settings, .. } = &mut bad {
        settings.minor_interval = -5.0;
    }
    let e = add(&mut ed, bad).unwrap_err();
    assert!(e.to_string().contains("minor contour interval"), "{e}");
    let mut short = site(&[]);
    if let Shape::SiteModel { lines, .. } = &mut short {
        lines[0].points.truncate(1);
    }
    assert!(
        add(&mut ed, short)
            .unwrap_err()
            .to_string()
            .contains("2 or more")
    );
    let two = Shape::SiteModifier {
        modifier: ModifierKind::GradeLimit,
        boundary: corners(&[DVec2::ZERO, DVec2::X]),
    };
    assert!(
        add(&mut ed, two)
            .unwrap_err()
            .to_string()
            .contains("3 or more")
    );
    // Moving a site model moves its sources in XY only, and undo brings them back.
    ed.execute(&Command::MoveObjects {
        ids: vec![ok],
        delta: DVec2::new(100.0, 0.0),
    })
    .unwrap();
    let Shape::SiteModel { points, .. } = &ed.doc().object(ok).unwrap().shape else {
        panic!()
    };
    assert_eq!(points[1], DVec3::new(110.0, 0.0, 1.0));
    assert!(ed.undo());
    let Shape::SiteModel { points, .. } = &ed.doc().object(ok).unwrap().shape else {
        panic!()
    };
    assert_eq!(points[1], DVec3::new(10.0, 0.0, 1.0));
}

#[test]
fn site_shapes_survive_a_save_and_load() {
    let mut ed = Editor::default();
    let a = add(
        &mut ed,
        site(&[(0.0, 0.0, 0.5), (10.0, 0.0, 1.5), (0.0, 10.0, 2.5)]),
    )
    .unwrap();
    let b = add(
        &mut ed,
        Shape::SiteModifier {
            modifier: ModifierKind::Pad {
                elevation: 7.0,
                slope: Some(PadSlope {
                    direction: 1.0,
                    percent: 3.0,
                }),
            },
            boundary: corners(&[DVec2::ZERO, DVec2::X, DVec2::ONE]),
        },
    )
    .unwrap();
    let g = add(
        &mut ed,
        Shape::SiteModifier {
            modifier: ModifierKind::GradeLimit,
            boundary: corners(&[DVec2::ZERO, DVec2::X, DVec2::ONE]),
        },
    )
    .unwrap();
    let json = serde_json::to_string(ed.doc()).unwrap();
    assert!(json.contains(r#""kind":"site_model""#) && json.contains(r#""type":"grade_limit""#));
    let back = Document::from_json(&json).unwrap();
    assert_eq!(&back, ed.doc());
    for id in [a, b, g] {
        assert_eq!(
            back.object(id).unwrap().shape,
            ed.doc().object(id).unwrap().shape
        );
    }
    // Settings may be missing in older or hand-written files.
    let doc: Shape =
        serde_json::from_str(r#"{"kind": "site_model", "points": [[0, 0, 1]]}"#).unwrap();
    assert!(
        matches!(doc, Shape::SiteModel { settings, .. } if settings == SiteSettings::default())
    );
}

#[test]
fn hull_and_outline() {
    let sq = [
        DVec2::ZERO,
        DVec2::new(10.0, 0.0),
        DVec2::new(10.0, 10.0),
        DVec2::new(0.0, 10.0),
        DVec2::new(5.0, 5.0),
        DVec2::new(5.0, 0.0),
    ];
    let h = svg::convex_hull(&sq);
    assert_eq!(h.len(), 4);
    assert!(kadakas_core::geom::polygon_area(&h) == 100.0);
    assert!(svg::convex_hull(&[DVec2::ZERO, DVec2::X]).len() == 2);
    assert!(svg::convex_hull(&[]).is_empty());
    assert_eq!(
        svg::site_outline(&site(&[
            (0.0, 0.0, 0.0),
            (10.0, 0.0, 0.0),
            (0.0, 10.0, 0.0)
        ]))
        .len(),
        4
    );
    assert_eq!(svg::site_outline(&pad()).len(), 4);
    assert!(
        svg::site_outline(&Shape::Circle {
            center: DVec2::ZERO,
            radius: 1.0
        })
        .is_empty()
    );
}

#[test]
fn svg_draws_outlines_and_calls_the_hook() {
    let mut ed = Editor::default();
    add(
        &mut ed,
        site(&[(0.0, 0.0, 0.0), (100.0, 0.0, 0.0), (0.0, 100.0, 0.0)]),
    )
    .unwrap();
    add(&mut ed, pad()).unwrap();
    let plain = svg::render(ed.doc(), 400);
    assert!(
        plain.contains(r#"data-kind="site_model""#)
            && plain.contains(r#"data-kind="site_modifier""#)
    );
    let hook = |_: &Document, o: &Object, style: &str, out: &mut String| {
        assert!(style.contains("stroke-width"));
        out.push_str(&format!("<g data-hook=\"{}\"/>\n", o.id));
    };
    let with = svg::render_with(ed.doc(), 400, Some(&hook));
    assert_eq!(with.matches("data-hook").count(), 2);
}

#[test]
fn mesh_hook_adds_geometry_and_flips_triangles_upward() {
    let mut ed = Editor::default();
    let id = add(
        &mut ed,
        site(&[(0.0, 0.0, 0.0), (100.0, 0.0, 0.0), (0.0, 100.0, 0.0)]),
    )
    .unwrap();
    let pad_id = add(&mut ed, pad()).unwrap();
    // Without a hook: only the outline of the modifier.
    let bare = mesh::build_at(ed.doc(), DVec3::ZERO);
    assert_eq!(bare.mesh.triangle_count(), 0);
    assert_eq!(bare.lines.segment_count(), 4);
    let hook = |_: &Document, o: &Object, sink: &mut mesh::SceneSink<'_>| {
        if o.id == pad_id {
            // One triangle given clockwise and one with a bad index.
            let p = [
                DVec3::new(0.0, 0.0, 1.0),
                DVec3::new(0.0, 10.0, 1.0),
                DVec3::new(10.0, 0.0, 1.0),
            ];
            sink.mesh(&p, &[0, 1, 2, 0, 1, 9], [1, 2, 3, 255]);
            sink.polyline(&p, true, [9, 9, 9, 255]);
        }
    };
    let layer_z = ed.doc().layers[0].elevation;
    let scene = mesh::build_with(ed.doc(), DVec3::ZERO, Some(&hook));
    assert_eq!(scene.mesh.triangle_count(), 1);
    assert!(scene.mesh.normals.iter().all(|n| n.z > 0.99));
    assert_eq!(scene.lines.segment_count(), 3);
    assert!(
        scene
            .mesh
            .tri_object
            .iter()
            .all(|o| *o == id || *o == pad_id)
    );
    assert_eq!(layer_z, 0.0);
    // The cache passes the hook through.
    let mut cache = mesh::MeshCache::default();
    let s = cache.get_with(5, ed.doc(), Some(&hook));
    assert_eq!(s.mesh.triangle_count(), 1);
    assert_eq!(
        cache.get(5, ed.doc()).mesh.triangle_count(),
        1,
        "same revision: cached"
    );
}
