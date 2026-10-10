//! Every kind of object has a plan drawing on the page: survey points, site models, massings,
//! meshes, irrigation, symbols, curved outlines and the annotation types.

use cadektor_core::command::{Command, Editor};
use cadektor_core::graphics::{GraphicResource, SymbolDef, SymbolItem};
use cadektor_core::model::*;
use cadektor_core::plants::style::PlantStyle;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::sheets::Viewport;
use cadektor_print::compose_page;
use cadektor_print::display::*;
use glam::DVec2;
use serde_json::{Value, json};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn run(ed: &mut Editor, c: Command) -> Vec<u64> {
    ed.execute(&c).unwrap().created
}

fn add_json(ed: &mut Editor, shape: Value) -> ObjectId {
    let shape: Shape = serde_json::from_value(shape).unwrap();
    ObjectId(
        run(
            ed,
            Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    )
}

/// A sheet with a plan viewport on layer 1, and the display list of its page.
fn page_of(ed: &mut Editor, scale: f64) -> Vec<Prim> {
    let sheet = LayerId(
        run(
            ed,
            Command::AddSheet {
                name: "S".into(),
                paper: None,
                orientation: None,
                number: None,
                title: None,
            },
        )[0],
    );
    let vp = Viewport::new(
        v(10.0, 10.0),
        v(400.0, 280.0),
        v(10_000.0, 10_000.0),
        scale,
        vec![FIRST_LAYER],
    );
    run(
        ed,
        Command::AddObject {
            shape: Shape::Viewport(Box::new(vp)),
            layer: Some(sheet),
            class: None,
            attrs: Attributes::default(),
        },
    );
    compose_page(ed.doc(), sheet).unwrap().prims
}

fn paths(p: &[Prim]) -> usize {
    p.iter().filter(|x| matches!(x, Prim::Path(_))).count()
}

#[test]
fn survey_points_lines_and_terrain_have_plan_symbols() {
    let mut ed = Editor::default();
    let base = paths(&page_of(&mut ed, 100.0));
    add_json(&mut ed, json!({"kind":"locus3d","p":[1000.0,1000.0,5.0]}));
    add_json(
        &mut ed,
        json!({"kind":"polyline3d","points":[[0.0,0.0,1.0],[4000.0,0.0,2.0],[4000.0,3000.0,3.0]],"closed":true}),
    );
    add_json(
        &mut ed,
        json!({"kind":"site_model","points":[[0.0,0.0,0.0],[5000.0,0.0,1.0],[0.0,5000.0,2.0],[5000.0,5000.0,3.0]]}),
    );
    add_json(
        &mut ed,
        json!({"kind":"site_modifier","modifier":{"type":"pad","elevation":1.0},
               "boundary":[[1000.0,1000.0],[2000.0,1000.0],[2000.0,2000.0]]}),
    );
    // A site model without an outline draws nothing.
    add_json(&mut ed, json!({"kind":"site_model"}));
    let p = paths(&page_of(&mut ed, 100.0));
    // Two marker strokes, one polygon, one outline and one pad outline.
    assert!(p >= base + 5, "{base} {p}");
}

#[test]
fn meshes_massings_and_irrigation_are_drawn_in_plan() {
    let mut ed = Editor::default();
    let before = paths(&page_of(&mut ed, 100.0));
    add_json(
        &mut ed,
        json!({"kind":"mesh3d","vertices":[[0.0,0.0,0.0],[3000.0,0.0,0.0],[0.0,2000.0,500.0]],"triangles":[[0,1,2]]}),
    );
    // A straight footprint with a gable roof draws the outline and the ridge. A curved one uses curve segments.
    add_json(
        &mut ed,
        json!({"kind":"massing","footprint":[[0.0,5000.0],[6000.0,5000.0],[6000.0,9000.0],[0.0,9000.0]],
               "roof":"gable"}),
    );
    add_json(
        &mut ed,
        json!({"kind":"massing","footprint":[[7000.0,5000.0],
               {"p":[10000.0,5000.0],"kind":"arc","radius":800.0},[10000.0,9000.0],[7000.0,9000.0]]}),
    );
    for shape in [
        json!({"kind":"irrigation","type":"outlet","at":[12000.0,0.0],"model":"x","arc":90.0,"radius":3000.0,"zone":2}),
        json!({"kind":"irrigation","type":"outlet","at":[12000.0,1000.0],"model":"x","arc":360.0}),
        json!({"kind":"irrigation","type":"line","points":[[12000.0,0.0],[14000.0,0.0]],"size":"25","material":"PE"}),
        json!({"kind":"irrigation","type":"poc","at":[15000.0,0.0],"static_pressure":400.0,"available_flow":50.0}),
        json!({"kind":"irrigation","type":"valve","at":[15000.0,1000.0],"zone":3,"kv":4.0}),
        json!({"kind":"irrigation","type":"controller","at":[15000.0,2000.0],"name":"C","stations":4}),
    ] {
        add_json(&mut ed, shape);
    }
    let after = paths(&page_of(&mut ed, 100.0));
    assert!(after > before + 8, "{before} {after}");
}

#[test]
fn symbols_and_plants_with_symbols_expand_to_their_parts() {
    let mut ed = Editor::default();
    let sym = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Dot".into(),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                    items: vec![SymbolItem {
                        shape: Shape::Circle {
                            center: DVec2::ZERO,
                            radius: 100.0,
                        },
                        attrs: Attributes::default(),
                        class: None,
                    }],
                    ..Default::default()
                }))),
            },
        )[0],
    );
    add_json(
        &mut ed,
        json!({"kind":"symbol","symbol":sym.0,"at":[1000.0,1000.0]}),
    );
    let before = paths(&page_of(&mut ed, 100.0));
    let style = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Birch".into(),
                body: ResourceBody::plant_style(PlantStyle {
                    spread: 2000.0,
                    plan_symbol: Some(sym),
                    ..Default::default()
                }),
            },
        )[0],
    );
    let plant = add_json(
        &mut ed,
        json!({"kind":"plant","at":[3000.0,3000.0],"species":"Birch","spread":2000.0,"height":5000.0}),
    );
    run(
        &mut ed,
        Command::SetPlantStyle {
            ids: vec![plant],
            style: Some(style),
            overrides: None,
            unlink: false,
        },
    );
    let after = paths(&page_of(&mut ed, 100.0));
    assert!(after > before, "{before} {after}");
}

#[test]
fn curved_outlines_draw_arcs_and_curves() {
    let mut ed = Editor::default();
    let before = paths(&page_of(&mut ed, 100.0));
    // Arc, Bezier and Cubic vertices all give curve pieces.
    add_json(
        &mut ed,
        json!({"kind":"polyline","closed":true,"points":[
            [0.0,0.0],{"p":[2000.0,0.0],"kind":"arc","radius":500.0},[2000.0,2000.0],[0.0,2000.0]]}),
    );
    add_json(
        &mut ed,
        json!({"kind":"polyline","closed":false,"points":[
            [0.0,0.0],{"p":[1000.0,2000.0],"kind":"bezier"},[3000.0,0.0]]}),
    );
    add_json(
        &mut ed,
        json!({"kind":"polyline","closed":false,"points":[
            [0.0,0.0],{"p":[1000.0,1000.0],"kind":"cubic"},{"p":[2000.0,0.0],"kind":"cubic"},[3000.0,1000.0]]}),
    );
    add_json(
        &mut ed,
        json!({"kind":"massing","footprint":[[0.0,0.0],
            {"p":[3000.0,0.0],"kind":"bezier"},[3000.0,3000.0],[0.0,3000.0]]}),
    );
    add_json(
        &mut ed,
        json!({"kind":"hardscape","material":"m","boundary":[[0.0,5000.0],
            {"p":[3000.0,5000.0],"kind":"point_on_arc"},[3000.0,8000.0],[0.0,8000.0]]}),
    );
    let after = paths(&page_of(&mut ed, 100.0));
    assert!(after >= before + 5, "{before} {after}");
}

#[test]
fn every_annotation_type_draws_lines_and_text() {
    let mut ed = Editor::default();
    let ann = [
        json!({"kind":"annotation","type":"dimension","mode":{"type":"linear","axis":"horizontal"},
               "points":[[0.0,0.0],[4000.0,0.0]],"offset":8.0}),
        json!({"kind":"annotation","type":"scale_bar","at":[2000.0,-3000.0],"segment":5.0,"segments":2}),
        json!({"kind":"annotation","type":"north_arrow","at":[2000.0,5000.0]}),
        json!({"kind":"annotation","type":"revision_cloud","revision":"A",
               "points":[[0.0,0.0],[2000.0,0.0],[2000.0,2000.0]]}),
        json!({"kind":"annotation","type":"text","at":[0.0,4000.0],"text":"Hello"}),
    ];
    for a in ann {
        add_json(&mut ed, a);
    }
    let p = page_of(&mut ed, 100.0);
    assert!(paths(&p) >= 4, "{}", paths(&p));
    assert!(
        p.iter()
            .any(|x| matches!(x, Prim::Text(t) if t.text == "Hello"))
    );
}

#[test]
fn annotation_objects_inside_a_viewport_use_paper_millimeters() {
    use cadektor_core::sheets::{Annotation, AnnotationItem};
    let mut ed = Editor::default();
    let sheet = LayerId(
        run(
            &mut ed,
            Command::AddSheet {
                name: "S".into(),
                paper: None,
                orientation: None,
                number: None,
                title: None,
            },
        )[0],
    );
    let mut vp = Viewport::new(
        v(10.0, 10.0),
        v(300.0, 200.0),
        v(0.0, 0.0),
        100.0,
        vec![FIRST_LAYER],
    );
    let an: cadektor_core::annot::Annot = serde_json::from_value(
        json!({"type":"scale_bar","at":[20.0,20.0],"segment":5.0,"segments":2}),
    )
    .unwrap();
    vp.annotations = vec![Annotation {
        id: 1,
        class: Default::default(),
        attrs: Attributes::default(),
        item: AnnotationItem::Shape {
            shape: Box::new(Shape::Annotation(Box::new(an))),
        },
    }];
    run(
        &mut ed,
        Command::AddObject {
            shape: Shape::Viewport(Box::new(vp)),
            layer: Some(sheet),
            class: None,
            attrs: Attributes::default(),
        },
    );
    let p = compose_page(ed.doc(), sheet).unwrap().prims;
    assert!(paths(&p) >= 2, "{}", paths(&p));
}
