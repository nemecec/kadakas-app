//! Sheets, viewports, overrides, title blocks and saved views in the core.

use cadektor_core::command::{Command, CommandError, Editor};
use cadektor_core::model::*;
use cadektor_core::sheets::*;
use glam::DVec2;
use proptest::prelude::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn exec(ed: &mut Editor, c: Command) -> Result<Vec<u64>, CommandError> {
    ed.execute(&c).map(|o| o.created)
}

fn add_sheet(ed: &mut Editor, name: &str) -> LayerId {
    LayerId(exec(ed, Command::AddSheet { name: name.into(), paper: None, orientation: None, number: None, title: None }).unwrap()[0])
}

fn add_on(ed: &mut Editor, shape: Shape, layer: LayerId) -> Result<ObjectId, CommandError> {
    exec(ed, Command::AddObject { shape, layer: Some(layer), class: None, attrs: Attributes::default() }).map(|c| ObjectId(c[0]))
}

fn viewport(layers: Vec<LayerId>) -> Viewport {
    Viewport::new(v(20.0, 30.0), v(220.0, 180.0), v(5000.0, 3000.0), 50.0, layers)
}

proptest! {
    #[test]
    fn viewport_transform_round_trips(
        cx in -1.0e6..1.0e6f64, cy in -1.0e6..1.0e6f64,
        scale in 0.5..5000.0f64,
        px in -500.0..500.0f64, py in -500.0..500.0f64,
        mx in -1.0e7..1.0e7f64, my in -1.0e7..1.0e7f64,
    ) {
        let mut vp = viewport(vec![]);
        vp.center = v(cx, cy);
        vp.scale = scale;
        let paper = v(px, py);
        let back = vp.to_paper(vp.to_model(paper));
        prop_assert!(back.distance(paper) < 1e-6, "{paper:?} -> {back:?}");
        let model = v(mx, my);
        let back = vp.to_model(vp.to_paper(model));
        prop_assert!(back.distance(model) <= 1e-9 * (1.0 + model.length()) * scale.max(1.0));
        // A length of 1 mm on paper is `scale` mm in the model.
        let d = vp.to_model(paper + v(1.0, 0.0)) - vp.to_model(paper);
        prop_assert!((d.x - scale).abs() < 1e-6 * scale && d.y.abs() < 1e-6);
    }

    #[test]
    fn crop_polygon_stays_inside_frame_and_never_grows(
        pts in prop::collection::vec((-100.0..400.0f64, -100.0..300.0f64), 3..8),
        qx in 0.0..300.0f64, qy in 0.0..250.0f64,
    ) {
        let mut vp = viewport(vec![]);
        vp.crop = Some(Crop::Polygon { points: pts.iter().map(|(x, y)| v(*x, *y)).collect() });
        let poly = vp.clip_polygon();
        for p in &poly {
            prop_assert!(p.x >= vp.min.x - 1e-9 && p.x <= vp.max.x + 1e-9 && p.y >= vp.min.y - 1e-9 && p.y <= vp.max.y + 1e-9);
        }
        // Random polygons may cross themselves, so the area test uses a convex crop (below).
        let tri = [v(30.0, 40.0), v(120.0, 40.0), v(30.0, 140.0)];
        vp.crop = Some(Crop::Polygon { points: tri.to_vec() });
        let inside = vp.clip_polygon();
        prop_assert!((signed_area(&inside).abs() - signed_area(&tri)).abs() < 1e-6, "a crop inside the frame is kept whole");
        // A point is shown only inside the frame.
        let q = v(qx, qy);
        if vp.shows_paper(q) {
            prop_assert!(q.x >= vp.min.x && q.x <= vp.max.x && q.y >= vp.min.y && q.y <= vp.max.y);
        }
    }

    #[test]
    fn rect_crop_shows_exactly_the_intersection(
        ax in 0.0..300.0f64, ay in 0.0..250.0f64, bx in 0.0..300.0f64, by in 0.0..250.0f64,
        qx in 0.0..300.0f64, qy in 0.0..250.0f64,
    ) {
        let mut vp = viewport(vec![]);
        vp.crop = Some(Crop::Rect { min: v(ax.min(bx), ay.min(by)), max: v(ax.max(bx), ay.max(by)) });
        let q = v(qx, qy);
        let in_frame = q.x >= vp.min.x && q.x <= vp.max.x && q.y >= vp.min.y && q.y <= vp.max.y;
        let in_crop = q.x >= ax.min(bx) && q.x <= ax.max(bx) && q.y >= ay.min(by) && q.y <= ay.max(by);
        // Points on an edge may go either way, so stay away from the edges.
        let near_edge = [vp.min.x, vp.max.x, ax, bx].iter().any(|e| (q.x - e).abs() < 1e-6) || [vp.min.y, vp.max.y, ay, by].iter().any(|e| (q.y - e).abs() < 1e-6);
        prop_assume!(!near_edge);
        prop_assert_eq!(vp.shows_paper(q), in_frame && in_crop);
    }

    #[test]
    fn override_resolution_follows_the_documented_order(
        obj_pen in prop::option::of(any::<[u8; 4]>()),
        class_pen in prop::option::of(any::<[u8; 4]>()),
        ov_pen in prop::option::of(any::<[u8; 4]>()),
        obj_w in prop::option::of(0.0..2.0f64),
        class_w in prop::option::of(0.0..2.0f64),
        ov_w in prop::option::of(0.0..2.0f64),
    ) {
        let mut doc = Document::default();
        doc.classes[0].attrs = Attributes { pen: class_pen.map(Color), fill: None, line_weight: class_w };
        let mut o = Object::new(ObjectId(40), FIRST_LAYER, NONE_CLASS, Shape::Circle { center: DVec2::ZERO, radius: 1.0 });
        o.attrs = Attributes { pen: obj_pen.map(Color), fill: None, line_weight: obj_w };
        let mut vp = viewport(vec![FIRST_LAYER]);
        vp.class_overrides.insert(NONE_CLASS, ClassOverride { pen: ov_pen.map(Color), line_weight: ov_w, ..Default::default() });
        let a = vp.resolve(&doc, &o);
        // The object's own attributes, then the viewport override (by class only), then the class, then the defaults.
        let pen = obj_pen.or(ov_pen).or(class_pen).map(Color).unwrap_or(Color::BLACK);
        let w = obj_w.or(ov_w).or(class_w).unwrap_or(DEFAULT_WEIGHT);
        prop_assert_eq!(a.pen, Some(pen));
        prop_assert_eq!(a.line_weight, Some(w));
        prop_assert_eq!(a.fill, Some(Color::NONE));
    }
}

#[test]
fn sheet_layers_have_pages_and_the_model_does_not_show_them() {
    let mut ed = Editor::default();
    let s = add_sheet(&mut ed, "Plan");
    assert_eq!(ed.doc().layer(s).unwrap().kind, LayerKind::Sheet);
    assert_eq!(ed.doc().layer(s).unwrap().scale, 1.0);
    let sheet = ed.doc().sheet(s).unwrap();
    assert_eq!((sheet.paper, sheet.number.as_str(), sheet.title.as_str()), (PaperSize::A3, "1", "Plan"));
    assert!(ed.doc().sheet(FIRST_LAYER).is_none(), "a design layer has no page");

    // Objects on a sheet layer are paper space: the model views leave them out.
    let line = add_on(&mut ed, Shape::Line { a: v(0.0, 0.0), b: v(100.0, 0.0) }, s).unwrap();
    let doc = ed.doc();
    let o = doc.object(line).unwrap();
    assert!(!doc.is_visible(o) && doc.is_shown(o));
    assert!(doc.is_pickable(o) == (doc.active_layer == s));
    assert!(cadektor_core::svg::drawing_bounds(doc).is_none());
    assert!(!cadektor_core::svg::render(doc, 200).contains("<line"));
    assert!(cadektor_core::mesh::build(doc).is_empty());
    exec(&mut ed, Command::SetActive { layer: Some(s), class: None }).unwrap();
    assert!(ed.doc().is_pickable(ed.doc().object(line).unwrap()));

    // The page settings change and undo.
    exec(&mut ed, Command::SetSheet { layer: s, paper: Some(PaperSize::A1), orientation: Some(Orientation::Portrait), number: Some("L-101".into()), title: None }).unwrap();
    let sheet = ed.doc().sheet(s).unwrap();
    assert_eq!(sheet.size_mm(), v(594.0, 841.0));
    assert_eq!(sheet.number, "L-101");
    assert!(ed.undo());
    assert_eq!(ed.doc().sheet(s).unwrap().paper, PaperSize::A3);

    // Errors.
    let bad = Command::SetSheet { layer: FIRST_LAYER, paper: None, orientation: None, number: None, title: None };
    assert!(matches!(ed.execute(&bad), Err(CommandError::Invalid(_))));
    let tiny = Command::SetSheet { layer: s, paper: Some(PaperSize::Custom { width_mm: 1.0, height_mm: 1.0 }), orientation: None, number: None, title: None };
    assert!(ed.execute(&tiny).is_err());
    let nan = Command::SetSheet { layer: s, paper: Some(PaperSize::Custom { width_mm: f64::NAN, height_mm: 100.0 }), orientation: None, number: None, title: None };
    assert!(ed.execute(&nan).is_err());
    let custom = Command::AddSheet { name: "x".into(), paper: Some(PaperSize::Custom { width_mm: 5.0, height_mm: 5.0 }), orientation: None, number: Some("9".into()), title: None };
    assert!(ed.execute(&custom).is_err());
    assert_eq!(ed.doc().sheet_layers().count(), 1, "a refused sheet leaves nothing behind");
}

#[test]
fn add_layer_of_kind_sheet_gets_page_settings_and_delete_removes_them() {
    let mut ed = Editor::default();
    let id = LayerId(exec(&mut ed, Command::AddLayer { name: "S".into(), elevation: 0.0, scale: None, kind: Some(LayerKind::Sheet) }).unwrap()[0]);
    assert!(ed.doc().drawing.sheets.contains_key(&id));
    assert_eq!(ed.doc().layer(id).unwrap().scale, 1.0);
    exec(&mut ed, Command::DeleteLayer { id }).unwrap();
    assert!(ed.doc().drawing.sheets.is_empty());
    assert!(ed.undo());
    assert!(ed.doc().drawing.sheets.contains_key(&id), "undo brings the page settings back");
}

#[test]
fn viewports_and_title_blocks_go_on_sheet_layers_only() {
    let mut ed = Editor::default();
    let s = add_sheet(&mut ed, "S");
    let vp = Shape::Viewport(Box::new(viewport(vec![FIRST_LAYER])));
    let tb = Shape::TitleBlock(TitleBlock::default());
    assert!(matches!(add_on(&mut ed, vp.clone(), FIRST_LAYER), Err(CommandError::Invalid(m)) if m.contains("sheet layer")));
    assert!(add_on(&mut ed, tb.clone(), FIRST_LAYER).is_err());
    let id = add_on(&mut ed, vp, s).unwrap();
    // A sheet layer refuses model shapes.
    let plant = Shape::Plant { at: DVec2::ZERO, species: "x".into(), spread: 10.0, height: 10.0 };
    assert!(matches!(add_on(&mut ed, plant, s), Err(CommandError::Invalid(m)) if m.contains("design layer")));
    // A viewport shows design layers that exist.
    assert!(matches!(add_on(&mut ed, Shape::Viewport(Box::new(viewport(vec![LayerId(999)]))), s), Err(CommandError::NoLayer(_))));
    assert!(add_on(&mut ed, Shape::Viewport(Box::new(viewport(vec![s]))), s).is_err());
    let mut with_class = viewport(vec![]);
    with_class.class_overrides.insert(ClassId(77), ClassOverride::default());
    assert!(matches!(add_on(&mut ed, Shape::Viewport(Box::new(with_class)), s), Err(CommandError::NoClass(_))));
    let mut with_layer_override = viewport(vec![]);
    with_layer_override.layer_overrides.insert(LayerId(999), true);
    assert!(add_on(&mut ed, Shape::Viewport(Box::new(with_layer_override)), s).is_err());
    let mut bad_scale = viewport(vec![]);
    bad_scale.scale = 0.0;
    assert!(matches!(add_on(&mut ed, Shape::Viewport(Box::new(bad_scale)), s), Err(CommandError::Invalid(m)) if m.contains("scale")));
    // Moving an object between layers is checked too.
    let mv = Command::ModifyObject { id, shape: None, layer: Some(FIRST_LAYER), class: None, attrs: None };
    assert!(ed.execute(&mv).is_err());
    // A move shifts the frame and the crop, and undo puts it back.
    let before = ed.doc().object(id).unwrap().shape.clone();
    exec(&mut ed, Command::MoveObjects { ids: vec![id], delta: v(10.0, 5.0) }).unwrap();
    let Shape::Viewport(m) = &ed.doc().object(id).unwrap().shape else { panic!() };
    assert_eq!(m.min, v(30.0, 35.0));
    assert!(ed.undo());
    assert_eq!(ed.doc().object(id).unwrap().shape, before);
}

#[test]
fn title_block_page_follows_the_sheet() {
    let mut ed = Editor::default();
    let s = add_sheet(&mut ed, "S");
    let id = add_on(&mut ed, Shape::TitleBlock(TitleBlock { page: v(10.0, 10.0), ..Default::default() }), s).unwrap();
    let page = |ed: &Editor| match &ed.doc().object(id).unwrap().shape {
        Shape::TitleBlock(t) => t.page,
        _ => unreachable!(),
    };
    assert_eq!(page(&ed), v(420.0, 297.0), "set when the title block is added");
    exec(&mut ed, Command::SetSheet { layer: s, paper: Some(PaperSize::A1), orientation: None, number: None, title: None }).unwrap();
    assert_eq!(page(&ed), v(841.0, 594.0));
    assert!(ed.undo());
    assert_eq!(page(&ed), v(420.0, 297.0), "undo restores the title block too");
    let (lo, hi) = ed.doc().object(id).unwrap().shape.bounds();
    assert_eq!((lo, hi), (v(10.0, 10.0), v(410.0, 287.0)));
}

#[test]
fn project_and_saved_views() {
    let mut ed = Editor::default();
    exec(&mut ed, Command::SetProject { name: Some("Garden".into()), client: None, address: None, drawn_by: Some("NM".into()), date: None, revision: None }).unwrap();
    assert_eq!((ed.doc().drawing.project.name.as_str(), ed.doc().drawing.project.drawn_by.as_str()), ("Garden", "NM"));
    exec(&mut ed, Command::SetProject { name: None, client: Some("C".into()), address: Some("A".into()), drawn_by: None, date: Some("2026-10-10".into()), revision: Some("B".into()) }).unwrap();
    assert_eq!(ed.doc().drawing.project.name, "Garden");
    assert_eq!(ed.doc().drawing.project.revision, "B");

    let l2 = LayerId(exec(&mut ed, Command::AddLayer { name: "Planting".into(), elevation: 0.0, scale: None, kind: None }).unwrap()[0]);
    let c2 = ClassId(exec(&mut ed, Command::AddClass { name: "Trees".into(), attrs: Attributes::default() }).unwrap()[0]);
    let all = |ed: &Editor| SavedView {
        name: " Both ".into(),
        layers: ed.doc().layers.iter().map(|l| (l.id, l.visible)).collect(),
        classes: ed.doc().classes.iter().map(|c| (c.id, c.visible)).collect(),
        active_layer: Some(ed.doc().active_layer),
        active_class: Some(ed.doc().active_class),
        zoom: Some(0.2),
        pan: Some(v(1.0, 2.0)),
        view: Some("plan".into()),
        ..Default::default()
    };
    let first = all(&ed);
    exec(&mut ed, Command::SaveView { view: Box::new(first) }).unwrap();
    assert_eq!(ed.doc().drawing.saved_views[0].name, "Both", "names are trimmed");
    // Change the state, then restore it.
    exec(&mut ed, Command::Batch { commands: vec![
        Command::ModifyLayer { id: l2, name: None, visible: Some(false), locked: None, elevation: None, scale: None },
        Command::ModifyClass { id: c2, name: None, visible: Some(false), attrs: None },
        Command::SetActive { layer: Some(l2), class: Some(c2) },
    ] }).unwrap();
    exec(&mut ed, Command::RestoreView { name: "both".into() }).unwrap();
    let d = ed.doc();
    assert!(d.layer(l2).unwrap().visible && d.class(c2).unwrap().visible);
    assert_eq!((d.active_layer, d.active_class), (FIRST_LAYER, NONE_CLASS));
    // Restore is one undo step.
    assert!(ed.undo());
    assert!(!ed.doc().layer(l2).unwrap().visible);
    assert!(ed.redo());

    // Saving under the same name replaces the view.
    let mut again = all(&ed);
    again.zoom = Some(5.0);
    exec(&mut ed, Command::SaveView { view: Box::new(again) }).unwrap();
    assert_eq!(ed.doc().drawing.saved_views.len(), 1);
    assert_eq!(ed.doc().drawing.saved_views[0].zoom, Some(5.0));

    // Errors.
    let base = all(&ed);
    let named = |f: &dyn Fn(&mut SavedView)| {
        let mut v = base.clone();
        f(&mut v);
        Command::SaveView { view: Box::new(v) }
    };
    assert!(ed.execute(&named(&|v| v.name = "  ".into())).is_err());
    assert!(ed.execute(&named(&|v| v.zoom = Some(-1.0))).is_err());
    assert!(matches!(ed.execute(&named(&|v| { v.layers.insert(LayerId(500), true); })), Err(CommandError::NoLayer(_))));
    assert!(matches!(ed.execute(&named(&|v| { v.classes.insert(ClassId(500), true); })), Err(CommandError::NoClass(_))));
    assert!(matches!(ed.execute(&named(&|v| v.active_layer = Some(LayerId(500)))), Err(CommandError::NoLayer(_))));
    assert!(matches!(ed.execute(&named(&|v| v.active_class = Some(ClassId(500)))), Err(CommandError::NoClass(_))));
    assert!(ed.execute(&Command::RestoreView { name: "nope".into() }).is_err());
    assert!(ed.execute(&Command::DeleteView { name: "nope".into() }).is_err());
    exec(&mut ed, Command::DeleteView { name: "BOTH".into() }).unwrap();
    assert!(ed.doc().drawing.saved_views.is_empty());
    assert!(ed.undo());
    assert_eq!(ed.doc().drawing.saved_views.len(), 1);

    // A view that names a layer or class that was deleted later still restores the rest.
    ed.execute(&Command::SaveView { view: Box::new(SavedView { layers: [(l2, true)].into(), name: "Stale".into(), ..Default::default() }) }).unwrap();
    exec(&mut ed, Command::DeleteLayer { id: l2 }).unwrap();
    exec(&mut ed, Command::RestoreView { name: "Stale".into() }).unwrap();
    exec(&mut ed, Command::SaveView { view: Box::new(SavedView { name: "Gone".into(), active_layer: Some(FIRST_LAYER), ..Default::default() }) }).unwrap();
}

#[test]
fn documents_with_sheets_save_and_load() {
    let mut ed = Editor::default();
    let s = add_sheet(&mut ed, "Plan");
    add_on(&mut ed, Shape::Viewport(Box::new(viewport(vec![FIRST_LAYER]))), s).unwrap();
    add_on(&mut ed, Shape::TitleBlock(TitleBlock { grid_columns: 4, ..Default::default() }), s).unwrap();
    exec(&mut ed, Command::SetProject { name: Some("P".into()), client: None, address: None, drawn_by: None, date: None, revision: None }).unwrap();
    exec(&mut ed, Command::SaveView { view: Box::new(SavedView { name: "A".into(), layers: [(FIRST_LAYER, true)].into(), ..Default::default() }) }).unwrap();
    let json = serde_json::to_string(ed.doc()).unwrap();
    let loaded = Document::from_json(&json).unwrap();
    assert_eq!(&loaded, ed.doc());
    // A document without sheets writes no drawing data, and old files load.
    let plain = serde_json::to_value(Document::default()).unwrap();
    assert!(plain.get("drawing").is_none());
    assert_eq!(Document::from_value(plain).unwrap().drawing, Drawing::default());
    // A viewport with a scale of zero does not load.
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    for o in value["objects"].as_array_mut().unwrap() {
        if o["shape"]["kind"] == "viewport" {
            o["shape"]["scale"] = serde_json::json!(0.0);
        }
    }
    assert!(Document::from_value(value).is_err());
}

#[test]
fn annotations_are_stored_in_the_viewport() {
    let mut ed = Editor::default();
    let s = add_sheet(&mut ed, "S");
    let mut vp = viewport(vec![FIRST_LAYER]);
    vp.annotations.push(Annotation { id: 1, class: NONE_CLASS, attrs: Attributes::default(), item: AnnotationItem::Text { at: v(5.0, 5.0), text: "N".into(), height: 4.0 } });
    let id = add_on(&mut ed, Shape::Viewport(Box::new(vp.clone())), s).unwrap();
    vp.annotations.push(Annotation { id: 2, class: NONE_CLASS, attrs: Attributes::default(), item: AnnotationItem::Dimension { a: v(0.0, 0.0), b: v(50.0, 0.0), offset: 8.0, text: None } });
    exec(&mut ed, Command::ModifyObject { id, shape: Some(Shape::Viewport(Box::new(vp.clone()))), layer: None, class: None, attrs: None }).unwrap();
    let Shape::Viewport(got) = &ed.doc().object(id).unwrap().shape else { panic!() };
    assert_eq!(got.annotations.len(), 2);
    assert!(ed.undo());
    let Shape::Viewport(got) = &ed.doc().object(id).unwrap().shape else { panic!() };
    assert_eq!(got.annotations.len(), 1);
    // Moving the viewport keeps annotation positions: they are relative to the frame.
    exec(&mut ed, Command::MoveObjects { ids: vec![id], delta: v(100.0, 0.0) }).unwrap();
    let Shape::Viewport(got) = &ed.doc().object(id).unwrap().shape else { panic!() };
    assert_eq!(got.annotations[0].item, AnnotationItem::Text { at: v(5.0, 5.0), text: "N".into(), height: 4.0 });
    assert_eq!(got.annotation_to_paper(v(5.0, 5.0)), v(125.0, 35.0));
}

#[test]
fn plants_and_3d_viewport_limits_on_sheet_layers() {
    let mut ed = Editor::default();
    let s = add_sheet(&mut ed, "S");
    let r = ed.execute(&Command::AddPlants { style: None, overrides: Default::default(), positions: vec![v(1.0, 1.0)], layer: Some(s), class: None, variation: None });
    assert!(matches!(r, Err(CommandError::Invalid(m)) if m.contains("design layer")), "plants never land on a sheet layer");
    assert!(ed.doc().objects.is_empty());
    for i in 0..MAX_3D_VIEWPORTS {
        let mut vp = viewport(vec![FIRST_LAYER]);
        vp.view = "top".into();
        vp.min = v(i as f64, 0.0);
        add_on(&mut ed, Shape::Viewport(Box::new(vp)), s).unwrap();
    }
    let mut vp = viewport(vec![FIRST_LAYER]);
    vp.view = "front".into();
    assert!(matches!(add_on(&mut ed, Shape::Viewport(Box::new(vp.clone())), s), Err(CommandError::Invalid(m)) if m.contains("3D viewports")));
    // Plan viewports are not limited, and an existing 3D viewport can still change.
    vp.view = "plan".into();
    let plan = add_on(&mut ed, Shape::Viewport(Box::new(vp.clone())), s).unwrap();
    let first = *ed.doc().objects.keys().next().unwrap();
    let Shape::Viewport(f) = ed.doc().object(first).unwrap().shape.clone() else { panic!() };
    let mut f = f;
    f.view = "back".into();
    exec(&mut ed, Command::ModifyObject { id: first, shape: Some(Shape::Viewport(f)), layer: None, class: None, attrs: None }).unwrap();
    vp.view = "left".into();
    let r = ed.execute(&Command::ModifyObject { id: plan, shape: Some(Shape::Viewport(Box::new(vp))), layer: None, class: None, attrs: None });
    assert!(r.is_err(), "turning a ninth viewport into 3D is refused");
}

#[test]
fn loaded_documents_keep_paper_and_model_apart() {
    let mut ed = Editor::default();
    let s = add_sheet(&mut ed, "S");
    add_on(&mut ed, Shape::Line { a: v(0.0, 0.0), b: v(1.0, 1.0) }, s).unwrap();
    let mut value = serde_json::to_value(ed.doc()).unwrap();
    value["objects"][0]["shape"] = serde_json::json!({"kind": "plant", "at": [0, 0], "species": "x", "spread": 5, "height": 5});
    assert!(Document::from_value(value).is_err(), "a plant on a sheet layer does not load");
}
