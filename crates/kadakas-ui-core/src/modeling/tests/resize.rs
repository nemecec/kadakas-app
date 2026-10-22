//! Resize the model by a typed length after a measure (MOD-05): the default flow with guides, the
//! life of a measure, and the plan that is found before the question.

use glam::{DVec2, DVec3};
use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, FIRST_LAYER, ObjectId, Shape};

use super::*;

#[test]
fn the_default_flow_measure_then_type_resizes_without_ctrl() {
    let mut r = rig(ModelerTool::TapeMeasure);
    assert!(
        r.tools.tape_creates_guides(),
        "guides are on: no Ctrl was pressed"
    );
    r.click(DVec3::new(1000.0, 0.0, 0.0));
    r.click(DVec3::new(3000.0, 0.0, 0.0));
    assert_eq!(r.tools.phase(), crate::measure_box::Phase::Done);
    let out = r.type_text("4000");
    let resize = out
        .resize
        .expect("a question, though the click made guides");
    assert!((resize.factor - 2.0).abs() < 1e-12);
    assert_eq!(
        resize.about,
        DVec3::new(1000.0, 0.0, 0.0),
        "about the first point"
    );
}

#[test]
fn an_offset_from_an_edge_can_resize_too() {
    let mut r = rig(ModelerTool::TapeMeasure);
    let e = super::tape::with_edge(&mut r);
    r.click_inf(
        super::tape::on_edge(e, DVec3::new(200.0, 0.0, 0.0)),
        down_ray(DVec3::ZERO),
    );
    r.click(DVec3::new(700.0, 400.0, 0.0));
    let out = r.type_text("800");
    let resize = out.resize.unwrap();
    assert_eq!(
        (resize.measured, resize.typed),
        (400.0, 800.0),
        "the offset is the measure"
    );
}

#[test]
fn a_measure_is_forgotten_when_the_document_changes_in_another_way() {
    let mut r = rig(ModelerTool::TapeMeasure);
    r.click(DVec3::ZERO);
    r.click(DVec3::new(1000.0, 0.0, 0.0));
    let rev = r.ed.revision();
    // The window tells the tool the revision after the click: the guides it made are that one.
    r.tools.seen_revision(rev);
    r.tools.seen_revision(rev);
    assert!(
        r.type_text("2000").resize.is_some(),
        "still the same document"
    );
    // Another edit (the person drew a line) changes the revision.
    r.ed.execute(&Command::AddObject {
        shape: Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    r.tools.seen_revision(r.ed.revision());
    let out = r.type_text("2000");
    assert!(
        out.resize.is_none() && out.message.is_some(),
        "an old measure resizes nothing"
    );
}

#[test]
fn escape_and_choosing_the_tool_again_forget_the_measure() {
    let mut r = rig(ModelerTool::TapeMeasure);
    r.click(DVec3::ZERO);
    r.click(DVec3::new(1000.0, 0.0, 0.0));
    r.tools.cancel();
    assert!(r.type_text("2000").resize.is_none());
    r.click(DVec3::ZERO);
    r.click(DVec3::new(1000.0, 0.0, 0.0));
    r.tools.choose(ModelerTool::TapeMeasure);
    assert!(r.type_text("2000").resize.is_none());
}

fn add(r: &mut Rig, shape: Shape, layer: Option<kadakas_core::model::LayerId>) -> ObjectId {
    ObjectId(
        r.ed.execute(&Command::AddObject {
            shape,
            layer,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

#[test]
fn the_plan_counts_what_scales_and_what_keeps_its_height() {
    let mut r = rig(ModelerTool::Select);
    add(&mut r, Shape::SurfaceModel(Box::default()), None);
    add(
        &mut r,
        Shape::Plant {
            at: DVec2::ZERO,
            species: "Oak".into(),
            spread: 3000.0,
            height: 9000.0,
        },
        None,
    );
    add(
        &mut r,
        Shape::Guide(Box::new(kadakas_core::guide::Guide::point(DVec3::ZERO))),
        None,
    );
    let plan = resize_plan(r.ed.doc(), None);
    assert_eq!((plan.scaled, plan.plan_objects), (3, 1));
    assert!(plan.allowed() && plan.locked_layers.is_empty());
    assert!(!resize_plan(&kadakas_core::model::Document::default(), None).allowed());
}

#[test]
fn a_locked_layer_refuses_the_resize_by_name_before_the_question() {
    let mut r = rig(ModelerTool::Select);
    let upper = kadakas_core::model::LayerId(
        r.ed.execute(&Command::AddLayer {
            name: "Walls".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0],
    );
    add(&mut r, Shape::SurfaceModel(Box::default()), None);
    add(&mut r, Shape::SurfaceModel(Box::default()), Some(upper));
    r.ed.execute(&Command::ModifyLayer {
        id: upper,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    let plan = resize_plan(r.ed.doc(), None);
    assert_eq!(plan.locked_layers, vec!["Walls".to_string()]);
    assert!(!plan.allowed());
    // The command would refuse the whole step: that is why the plan says so first.
    let cmd = resize_command(
        &plan,
        &Resize {
            factor: 2.0,
            about: DVec3::ZERO,
            measured: 1.0,
            typed: 2.0,
        },
    );
    assert!(r.ed.execute(&cmd).is_err());
    let _ = FIRST_LAYER;
}

#[test]
fn a_grey_tag_refuses_the_resize_too() {
    let mut r = rig(ModelerTool::Select);
    add(&mut r, Shape::SurfaceModel(Box::default()), None);
    r.ed.execute(&Command::SetVisibility {
        layers: vec![],
        classes: vec![kadakas_core::model::NONE_CLASS],
        mode: kadakas_core::visibility::VisMode::Grey,
    })
    .unwrap();
    let plan = resize_plan(r.ed.doc(), None);
    assert_eq!(plan.grey_objects, 1);
    assert!(!plan.allowed());
}

#[test]
fn the_command_scales_about_the_first_point_in_one_step() {
    let mut r = rig(ModelerTool::Select);
    let id = add(&mut r, Shape::SurfaceModel(Box::default()), None);
    let p = [
        [1000.0, 0.0, 0.0],
        [2000.0, 0.0, 0.0],
        [2000.0, 1000.0, 0.0],
        [1000.0, 1000.0, 0.0],
    ];
    r.ed.execute(&Command::EditSurface {
        id,
        edit: kadakas_core::surface::Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let plan = resize_plan(r.ed.doc(), None);
    let depth = r.ed.undo_depth();
    let cmd = resize_command(
        &plan,
        &Resize {
            factor: 2.0,
            about: DVec3::new(1000.0, 0.0, 0.0),
            measured: 1000.0,
            typed: 2000.0,
        },
    );
    r.ed.execute(&cmd).unwrap();
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let (lo, hi) = model_of(r.ed.doc(), id).unwrap().bounds().unwrap();
    assert!(
        lo.distance(DVec3::new(1000.0, 0.0, 0.0)) < 1e-6,
        "the first point stays"
    );
    assert!(hi.distance(DVec3::new(3000.0, 2000.0, 0.0)) < 1e-6);
}
