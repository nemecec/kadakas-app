//! Fillet and Chamfer (MOD-21) in the window: the panel of the tray, the commands of the Edit menu
//! menu, the refusals in the language of the window, and undo.

use super::*;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{Edit, FaceId};
use kadakas_ui_core::modeling::{FilletKind, Item};

use crate::tray::TrayAction;

fn rounded_box(app: &mut ModelerApp) -> ObjectId {
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
    let p = [
        [-500.0, -300.0, 0.0],
        [500.0, -300.0, 0.0],
        [500.0, 300.0, 0.0],
        [-500.0, 300.0, 0.0],
    ];
    for e in [Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    }] {
        app.session
            .editor
            .execute(&Command::EditSurface { id, edit: e })
            .unwrap();
    }
    let face = surface(app).faces().next().unwrap().0;
    app.session
        .editor
        .execute(&Command::EditSurface {
            id,
            edit: Edit::PushPull {
                face,
                distance: 400.0,
            },
        })
        .unwrap();
    id
}

fn top(app: &ModelerApp) -> FaceId {
    surface(app)
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.99 && f.plane.offset > 399.0)
        .unwrap()
        .0
}

#[test]
fn the_panel_rounds_the_edges_of_the_selected_face_in_one_step() {
    let (ctx, mut app, mut t) = top_view();
    let id = rounded_box(&mut app);
    // Nothing selected: the panel says what to do.
    app.tray_action(TrayAction::Fillet(FilletKind::Fillet));
    assert_eq!(app.status, tr!("modeler-fillet-select"));
    app.edit.sub.set([Item::Face(id, top(&app))]);
    let depth = app.session.editor.undo_depth();
    let faces = surface(&app).face_count();
    app.tray_ui.fillet.size = 40.0;
    app.tray_action(TrayAction::Fillet(FilletKind::Fillet));
    assert_eq!(
        app.status,
        tr!("modeler-fillet-done", kind = "fillet", count = 4)
    );
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert!(surface(&app).face_count() > faces);
    assert!(surface(&app).solid_report().is_closed());
    // Undo gives the box back.
    app.run(Cmd::Undo, size(&app));
    assert_eq!(surface(&app).face_count(), faces);
    // A radius that does not fit says so without ids, with the largest that fits.
    app.edit.sub.set([Item::Face(id, top(&app))]);
    app.tray_ui.fillet.size = 450.0;
    app.run(Cmd::Fillet(FilletKind::Chamfer), size(&app));
    assert!(app.status.contains("299.99"), "{}", app.status);
    assert_eq!(app.session.editor.undo_depth(), depth);
    // The Edit menu item with a fitting size.
    app.tray_ui.fillet.size = 30.0;
    app.run(Cmd::Fillet(FilletKind::Chamfer), size(&app));
    assert_eq!(
        app.status,
        tr!("modeler-fillet-done", kind = "chamfer", count = 4)
    );
    // The panel shows in the tray.
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(shown.contains(&tr!("modeler-tray-fillet")), "{shown}");
}
