//! The tools of the second part of MOD-08 in the window, through the real input path: the hole
//! through a slab, Offset (`F`), Polygon, Follow Me, and the Soften Edges panel, the context
//! menu commands and Entity Info of an edge.

use super::*;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{ATTR_SOFT, Edit, flagged};
use kadakas_ui_core::modeling::{EdgeStyle, Item};

use crate::tray::TrayAction;

fn new_model(app: &mut ModelerApp) -> ObjectId {
    app.session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap()
}

fn edit(app: &mut ModelerApp, id: ObjectId, edit: Edit) {
    app.session
        .editor
        .execute(&Command::EditSurface { id, edit })
        .unwrap();
}

fn ring(x0: f64, w: f64, z: f64) -> Edit {
    let p = [
        [x0, x0, z],
        [x0 + w, x0, z],
        [x0 + w, x0 + w, z],
        [x0, x0 + w, z],
    ];
    Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    }
}

/// A box 1000 x 1000 x 500 around the origin with a square of 400 in the middle of its top.
fn slab_with_square(app: &mut ModelerApp) -> ObjectId {
    let id = new_model(app);
    edit(app, id, ring(-500.0, 1000.0, 0.0));
    let face = surface(app).faces().next().unwrap().0;
    edit(
        app,
        id,
        Edit::PushPull {
            face,
            distance: 500.0,
        },
    );
    edit(app, id, ring(-200.0, 400.0, 500.0));
    id
}

fn minus(ctx: &egui::Context, app: &mut ModelerApp, t: &mut f64) {
    let mut ev = tap(egui::Key::Minus, egui::Modifiers::NONE);
    ev.push(egui::Event::Text("-".into()));
    *t += 0.1;
    let _ = frame(ctx, app, ev, *t);
}

#[test]
fn a_square_on_a_slab_pushed_to_the_bottom_cuts_a_hole_through() {
    let (ctx, mut app, mut t) = top_view();
    slab_with_square(&mut app);
    let c = center(&app);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    choose(&ctx, &mut app, egui::Key::P, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(app.edit.modeling.in_progress(), "the square was picked");
    let depth = app.session.editor.undo_depth();
    minus(&ctx, &mut app, &mut t);
    type_keys(&ctx, &mut app, "500", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "{}", app.status);
    let m = surface(&app);
    assert!(m.check().is_closed(), "{:?}", m.check());
    let holes: usize = m.faces().map(|(_, f)| f.holes.len()).sum();
    assert_eq!(holes, 2, "the top and the bottom have the hole");
}

#[test]
fn offset_on_the_f_key_draws_a_loop_inside_the_face() {
    let (ctx, mut app, mut t) = top_view();
    slab_with_square(&mut app);
    let c = center(&app);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    choose(&ctx, &mut app, egui::Key::F, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::Offset);
    let faces = surface(&app).face_count();
    let depth = app.session.editor.undo_depth();
    click_at(&ctx, &mut app, c, &mut t);
    assert!(app.edit.modeling.in_progress(), "{}", app.status);
    // The Measurements box shows the distance while the pointer moves.
    let _ = frame(&ctx, &mut app, vec![moved(c + egui::vec2(5.0, 0.0))], t);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        shown.contains(&tr!("uic-modeler-measure-distance")),
        "{shown}"
    );
    type_keys(&ctx, &mut app, "50", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "{}", app.status);
    assert_eq!(surface(&app).face_count(), faces + 1);
    assert!(
        surface(&app)
            .vertices()
            .any(|(_, p)| p.distance(glam::DVec3::new(-150.0, -150.0, 500.0)) < 1e-6)
    );
}

#[test]
fn polygon_from_the_tool_bar_draws_a_hexagon_with_a_typed_radius() {
    let (ctx, mut app, mut t) = top_view();
    app.run(Cmd::Tool(ModelerTool::Polygon), size(&app));
    assert_eq!(app.tools.active(), ModelerTool::Polygon);
    let c = center(&app);
    click_at(&ctx, &mut app, c, &mut t);
    type_keys(&ctx, &mut app, "300", &mut t);
    enter(&ctx, &mut app, &mut t);
    let m = surface(&app);
    assert_eq!((m.edge_count(), m.face_count()), (6, 1));
    assert_eq!(m.curves().count(), 1, "one curve, like a circle");
}

#[test]
fn follow_me_sweeps_the_clicked_face_along_the_selected_path() {
    let (ctx, mut app, mut t) = top_view();
    let id = new_model(&mut app);
    edit(&mut app, id, ring(-200.0, 400.0, 0.0));
    edit(
        &mut app,
        id,
        Edit::AddEdges {
            edges: vec![[[2000.0, 0.0, 0.0], [2000.0, 0.0, 1000.0]]],
        },
    );
    let path = surface(&app)
        .edges()
        .find(|(_, e)| {
            let m = surface(&app);
            m.vertex(e.v[0]).unwrap().z != m.vertex(e.v[1]).unwrap().z
        })
        .unwrap()
        .0;
    app.edit.sub.set([Item::Edge(id, path)]);
    app.run(Cmd::Tool(ModelerTool::FollowMe), size(&app));
    let c = center(&app);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    let depth = app.session.editor.undo_depth();
    click_at(&ctx, &mut app, c, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "{}", app.status);
    // A box 400 x 400 x 1000 (the path stays as a line).
    let m = surface(&app);
    assert_eq!(m.face_count(), 6);
    let (lo, hi) = m.local_bounds().unwrap();
    assert!((hi.z - lo.z - 1000.0).abs() < 1e-6);
}

#[test]
fn the_soften_panel_and_the_context_commands_change_the_selected_edges() {
    let (ctx, mut app, mut t) = top_view();
    let id = new_model(&mut app);
    edit(&mut app, id, ring(-500.0, 1000.0, 0.0));
    let face = surface(&app).faces().next().unwrap().0;
    edit(
        &mut app,
        id,
        Edit::PushPull {
            face,
            distance: 500.0,
        },
    );
    let edges: Vec<Item> = surface(&app)
        .edges()
        .map(|(e, _)| Item::Edge(id, e))
        .collect();
    let soft = |app: &ModelerApp| {
        let m = surface(app);
        m.edges()
            .filter(|(e, _)| flagged(m.edge_attrs(*e), ATTR_SOFT))
            .count()
    };
    // Nothing selected: the panel says what to do.
    app.tray_action(TrayAction::Soften);
    assert_eq!(app.status, tr!("modeler-soften-select"));
    app.edit.sub.set(edges.clone());
    let depth = app.session.editor.undo_depth();
    // The slider over 90 degrees: all edges of the box soften; then back under 90: none.
    app.tray_ui.soften.angle = 100.0;
    app.tray_action(TrayAction::Soften);
    assert_eq!(soft(&app), 12);
    app.tray_ui.soften.angle = 95.0;
    app.tray_action(TrayAction::Soften);
    app.tray_ui.soften.angle = 30.0;
    app.tray_action(TrayAction::Soften);
    assert_eq!(soft(&app), 0);
    assert_eq!(
        app.session.editor.undo_depth(),
        depth,
        "back to the start: no step"
    );
    // The context menu commands.
    app.run(Cmd::EdgeStyle(EdgeStyle::Soften), size(&app));
    assert_eq!(soft(&app), 12);
    app.run(Cmd::EdgeStyle(EdgeStyle::Unsoften), size(&app));
    assert_eq!(soft(&app), 0);
    // Entity Info of one edge shows its flags.
    app.run(Cmd::EdgeStyle(EdgeStyle::Soften), size(&app));
    app.edit.sub.set([edges[0]]);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(shown.contains(&tr!("modeler-entity-soft")), "{shown}");
    assert!(shown.contains(&tr!("modeler-tray-soften")), "{shown}");
}

#[test]
fn the_soften_slider_previews_while_dragged_and_applies_once_when_let_go() {
    let (_ctx, mut app, _t) = top_view();
    let id = new_model(&mut app);
    edit(&mut app, id, ring(-500.0, 1000.0, 0.0));
    let face = surface(&app).faces().next().unwrap().0;
    edit(
        &mut app,
        id,
        Edit::PushPull {
            face,
            distance: 500.0,
        },
    );
    let edges: Vec<Item> = surface(&app)
        .edges()
        .map(|(e, _)| Item::Edge(id, e))
        .collect();
    app.edit.sub.set(edges);
    let (rev, depth) = (
        app.session.editor.revision(),
        app.session.editor.undo_depth(),
    );
    // Dragging over 90 degrees: the twelve edges are shown, nothing changes yet.
    app.tray_ui.soften.angle = 100.0;
    app.tray_action(TrayAction::SoftenPreview);
    assert_eq!(app.edit.soften_preview.len(), 12);
    assert_eq!(app.session.editor.revision(), rev);
    // Let go: one edit, one undo step, and the preview goes.
    app.tray_action(TrayAction::Soften);
    assert!(app.edit.soften_preview.is_empty());
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let m = surface(&app);
    assert_eq!(m.edges().filter(|(e, _)| m.edge_soft(*e)).count(), 12);
}
