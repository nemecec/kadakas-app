//! Tests of the review of MOD-05a in the window: the copy key, undo while a tool runs, the
//! selection after a copy.

use super::*;

fn copy_mods() -> egui::Modifiers {
    if cfg!(target_os = "macos") {
        egui::Modifiers::ALT
    } else {
        egui::Modifiers::CTRL
    }
}

#[test]
fn the_copy_key_with_another_key_does_not_toggle_copy() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    // Ctrl+Z (a Z with the copy key held), then the key is let go.
    t += 0.1;
    let _ = frame_with(
        &ctx,
        &mut app,
        tap(egui::Key::X, copy_mods()),
        t,
        copy_mods(),
    );
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    assert!(!app.edit.modeling.copying(), "a combination is no toggle");
    // The key alone, pressed and let go, toggles; once more toggles back.
    toggle_copy(&ctx, &mut app, &mut t);
    assert!(app.edit.modeling.copying());
    toggle_copy(&ctx, &mut app, &mut t);
    assert!(!app.edit.modeling.copying());
}

#[test]
fn holding_the_copy_key_with_a_click_is_no_toggle_either() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    let b = egui::PointerButton::Primary;
    let m = copy_mods();
    t += 0.1;
    let _ = frame_with(&ctx, &mut app, vec![moved(c)], t, m);
    t += 0.05;
    let _ = frame_with(&ctx, &mut app, vec![button(c, b, true, m)], t, m);
    t += 0.05;
    let _ = frame_with(&ctx, &mut app, vec![button(c, b, false, m)], t, m);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    assert!(!app.edit.modeling.copying());
}

#[test]
fn undo_while_a_move_is_in_progress_ends_it_and_changes_nothing_else() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(app.edit.modeling.in_progress());
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::Undo, size(&app));
    assert!(!app.edit.modeling.in_progress());
    assert_eq!(app.edit.modeling.typed(), "");
    assert!(app.session.editor.undo_depth() < depth, "the undo ran");
    // The tool is still Move and starts again with the next click.
    assert_eq!(app.tools.active(), ModelerTool::Move);
}

#[test]
fn after_a_copy_the_next_move_moves_the_copy() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    toggle_copy(&ctx, &mut app, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    let _ = frame(&ctx, &mut app, vec![moved(c + egui::vec2(60.0, 0.0))], t);
    t += 0.1;
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::ArrowRight, egui::Modifiers::NONE),
        t,
    );
    type_keys(&ctx, &mut app, "2000", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.selection.len(), 1);
    let copy = app.session.selection[0];
    assert_ne!(copy, id, "the copy is selected");
    assert!(bounds(&app, copy).0.x > 1000.0);
    assert!(bounds(&app, id).0.x < 0.0);
}

#[test]
fn scale_about_the_center_while_the_copy_key_is_held() {
    let (ctx, mut app, mut t, id) = ready();
    app.session.selection = vec![id];
    choose(&ctx, &mut app, egui::Key::S, &mut t);
    let corner = screen_of(&mut app, DVec3::new(500.0, 500.0, 500.0));
    let _ = frame(&ctx, &mut app, vec![moved(corner)], t);
    click_at(&ctx, &mut app, corner, &mut t);
    assert!(app.edit.modeling.in_progress());
    // The key is held while the value is entered.
    type_keys(&ctx, &mut app, "2", &mut t);
    t += 0.1;
    let _ = frame_with(
        &ctx,
        &mut app,
        tap(egui::Key::Enter, copy_mods()),
        t,
        copy_mods(),
    );
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    let (lo, hi) = bounds(&app, id);
    assert!(
        near(lo, DVec3::new(-1000.0, -1000.0, -250.0), 0.5),
        "{lo:?}"
    );
    assert!(near(hi, DVec3::new(1000.0, 1000.0, 750.0), 0.5), "{hi:?}");
    assert!(!app.edit.modeling.copying(), "Scale has no copy toggle");
}
