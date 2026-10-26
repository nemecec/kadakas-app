//! Circles and arcs with an exact description in the window (MOD-15): Entity Info shows the true
//! radius and segments of a selected curve, and the field for the number of segments rebuilds it.

use super::*;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{Analytic, CurveId, Edit};
use kadakas_ui_core::modeling::Item;

use crate::tray::TrayAction;

fn circle_model(app: &mut ModelerApp, n: usize) -> (ObjectId, CurveId) {
    let id = app
        .session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    let points = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [750.0 * a.cos(), 750.0 * a.sin(), 0.0]
        })
        .collect();
    app.session
        .editor
        .execute(&Command::EditSurface {
            id,
            edit: Edit::AddCurve {
                points,
                closed: true,
                faceted: false,
                analytic: Some(Analytic::circle(
                    glam::DVec3::ZERO,
                    glam::DVec3::Z,
                    glam::DVec3::X,
                    750.0,
                )),
            },
        })
        .unwrap();
    let curve = surface(app).curves().next().unwrap().0;
    (id, curve)
}

fn select_edge(app: &mut ModelerApp, id: ObjectId, curve: CurveId) {
    let e = surface(app).curve_edges(curve).unwrap()[0];
    app.edit.sub.set([Item::Edge(id, e)]);
}

#[test]
fn entity_info_shows_the_true_radius_and_the_segments_of_a_circle() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (id, curve) = circle_model(&mut app, 16);
    select_edge(&mut app, id, curve);
    let _ = frame(&ctx, &mut app, vec![], 0.0);
    let shown = texts(&frame(&ctx, &mut app, vec![], 0.1)).join("\n");
    assert!(shown.contains(&tr!("modeler-curve-radius")), "{shown}");
    assert!(shown.contains(&tr!("modeler-curve-kind-circle")), "{shown}");
    assert!(shown.contains(&tr!("modeler-curve-segments")), "{shown}");
    assert!(
        shown.contains(&tr!("modeler-curve-segments-apply")),
        "{shown}"
    );
    assert!(
        shown.contains(&tr!("modeler-curve-circumference")),
        "{shown}"
    );
    let u = kadakas_units::current();
    assert!(shown.contains(&u.length_text(750.0)), "{shown}");
}

#[test]
fn the_whole_curve_selected_shows_the_curve_rows() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (id, curve) = circle_model(&mut app, 12);
    let edges: Vec<Item> = surface(&app)
        .curve_edges(curve)
        .unwrap()
        .iter()
        .map(|e| Item::Edge(id, *e))
        .collect();
    app.edit.sub.set(edges);
    let _ = frame(&ctx, &mut app, vec![], 0.0);
    let shown = texts(&frame(&ctx, &mut app, vec![], 0.1)).join("\n");
    assert!(shown.contains(&tr!("modeler-curve-radius")), "{shown}");
}

#[test]
fn an_arc_shows_its_angles_and_length() {
    let ctx = egui::Context::default();
    let mut app = app();
    let id = app
        .session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    let points = (0..=6)
        .map(|i| {
            let a = 0.4 + 1.2 * f64::from(i) / 6.0;
            [500.0 * a.cos(), 500.0 * a.sin(), 0.0]
        })
        .collect();
    app.session
        .editor
        .execute(&Command::EditSurface {
            id,
            edit: Edit::AddCurve {
                points,
                closed: false,
                faceted: false,
                analytic: Some(Analytic::arc(
                    glam::DVec3::ZERO,
                    glam::DVec3::Z,
                    glam::DVec3::X,
                    500.0,
                    0.4,
                    1.6,
                )),
            },
        })
        .unwrap();
    let curve = surface(&app).curves().next().unwrap().0;
    select_edge(&mut app, id, curve);
    let _ = frame(&ctx, &mut app, vec![], 0.0);
    let shown = texts(&frame(&ctx, &mut app, vec![], 0.1)).join("\n");
    assert!(shown.contains(&tr!("modeler-curve-kind-arc")), "{shown}");
    assert!(shown.contains(&tr!("modeler-curve-sweep")), "{shown}");
    assert!(shown.contains(&tr!("modeler-curve-length")), "{shown}");
}

#[test]
fn the_segments_action_rebuilds_the_curve_and_a_refusal_goes_to_the_status_line() {
    let mut app = app();
    let (id, curve) = circle_model(&mut app, 12);
    let depth = app.session.editor.undo_depth();
    app.tray_action(TrayAction::CurveSegments {
        model: id,
        curve: curve.0,
        segments: 36,
    });
    assert_eq!(surface(&app).curve_segments(curve), Some(36));
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(app.status, tr!("modeler-curve-segments-set", segments = 36));
    app.tray_action(TrayAction::CurveSegments {
        model: id,
        curve: curve.0,
        segments: 2,
    });
    assert_eq!(surface(&app).curve_segments(curve), Some(36));
    assert!(
        app.status.contains(&tr!("uic-curve-refused-range")),
        "{}",
        app.status
    );
    assert!(app.session.editor.undo());
    assert_eq!(surface(&app).curve_segments(curve), Some(12));
}

#[test]
fn the_whole_new_chain_stays_selected_after_the_segments_change() {
    let mut app = app();
    let (id, curve) = circle_model(&mut app, 12);
    let edges: Vec<Item> = surface(&app)
        .curve_edges(curve)
        .unwrap()
        .iter()
        .map(|e| Item::Edge(id, *e))
        .collect();
    app.edit.sub.set(edges);
    for n in [30, 8] {
        app.tray_action(TrayAction::CurveSegments {
            model: id,
            curve: curve.0,
            segments: n,
        });
        let chosen: Vec<Item> = app.edit.sub.items().collect();
        let now = surface(&app).curve_edges(curve).unwrap().to_vec();
        assert_eq!(chosen.len(), n as usize);
        assert!(now.iter().all(|e| chosen.contains(&Item::Edge(id, *e))));
    }
}

#[test]
fn entity_info_of_a_model_with_unequal_scales_says_the_circle_is_not_true() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (id, curve) = circle_model(&mut app, 12);
    app.session
        .editor
        .execute(&Command::TransformObjects3d {
            ids: vec![id],
            matrix: glam::DAffine3::from_scale(glam::DVec3::new(2.0, 1.0, 1.0)).to_cols_array(),
            copy: false,
            count: 1,
        })
        .unwrap();
    select_edge(&mut app, id, curve);
    let _ = frame(&ctx, &mut app, vec![], 0.0);
    let shown = texts(&frame(&ctx, &mut app, vec![], 0.1)).join("\n");
    assert!(shown.contains(&tr!("modeler-curve-not-true")), "{shown}");
    assert!(
        !shown.contains(&tr!("modeler-curve-circumference")),
        "{shown}"
    );
}
