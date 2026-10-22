//! Deleting a class (a tag) moves every reference to it (MOD-05): the objects, the parts and
//! components of definitions, the overrides and annotations of viewports, the saved views and the
//! material slots. One undo step brings all of it back. Nothing is dropped without a word.

use glam::DVec2;
use kadakas_core::classes;
use kadakas_core::command::{Command, Editor};
use kadakas_core::component::Part;
use kadakas_core::graphics::{GraphicResource, SymbolDef, SymbolItem};
use kadakas_core::model::*;
use kadakas_core::present::{self, Material, SlotTarget};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::sheets::*;

fn tag(ed: &mut Editor, name: &str) -> ClassId {
    ClassId(
        ed.execute(&Command::AddClass {
            name: name.into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn line(ed: &mut Editor, class: ClassId) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
            layer: None,
            class: Some(class),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn symbol(ed: &mut Editor, class: ClassId) -> ResourceId {
    let def = SymbolDef {
        items: vec![SymbolItem {
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
            attrs: Attributes::default(),
            class: Some(class),
        }],
        parts: vec![Part {
            class: Some(class),
            ..Part::new(Shape::SurfaceModel(Box::default()))
        }],
        insert_class: Some(class),
        ..SymbolDef::default()
    };
    ResourceId(
        ed.execute(&Command::AddResource {
            name: "Tree".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
        })
        .unwrap()
        .created[0],
    )
}

fn symbol_def(ed: &Editor, id: ResourceId) -> SymbolDef {
    match ed.doc().resources.graphic(id) {
        Some(GraphicResource::Symbol(s)) => s.clone(),
        other => panic!("{other:?}"),
    }
}

fn delete(ed: &mut Editor, id: ClassId) {
    ed.execute(&Command::DeleteClass {
        id,
        move_to: None,
    })
    .unwrap();
}

#[test]
fn the_parts_and_components_of_a_definition_move_to_untagged_and_undo_brings_them_back() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    let sym = symbol(&mut ed, t);
    let uses = classes::uses(ed.doc(), t);
    assert_eq!(uses.definition_parts, 3, "a component, a part and the insert class");
    let before = ed.doc().clone();
    let depth = ed.undo_depth();
    delete(&mut ed, t);
    assert_eq!(ed.undo_depth(), depth + 1, "one undo step");
    let d = symbol_def(&ed, sym);
    assert_eq!(d.items[0].class, Some(NONE_CLASS));
    assert_eq!(d.parts[0].class, Some(NONE_CLASS));
    assert_eq!(d.insert_class, Some(NONE_CLASS));
    assert_eq!(classes::uses(ed.doc(), t).total(), 0, "no id is left");
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
    assert_eq!(symbol_def(&ed, sym).parts[0].class, Some(t));
}

#[test]
fn viewport_overrides_and_annotations_move_to_the_other_class() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    let other = tag(&mut ed, "Walls");
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "A1".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap()
        .created[0],
    );
    let mut vp = Viewport::new(
        DVec2::new(20.0, 30.0),
        DVec2::new(220.0, 180.0),
        DVec2::new(5000.0, 3000.0),
        50.0,
        vec![FIRST_LAYER],
    );
    let ov = ClassOverride {
        visible: Some(false),
        ..ClassOverride::default()
    };
    vp.class_overrides.insert(t, ov.clone());
    vp.annotations.push(Annotation {
        id: 1,
        class: t,
        attrs: Attributes::default(),
        item: AnnotationItem::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    });
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Viewport(Box::new(vp)),
            layer: Some(sheet),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let u = classes::uses(ed.doc(), t);
    assert_eq!((u.viewport_overrides, u.annotations), (1, 1));
    let before = ed.doc().clone();
    ed.execute(&Command::DeleteClass {
        id: t,
        move_to: Some(other),
    })
    .unwrap();
    let Shape::Viewport(v) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    assert!(!v.class_overrides.contains_key(&t));
    assert_eq!(v.class_overrides.get(&other), Some(&ov));
    assert_eq!(v.annotations[0].class, other);
    assert_eq!(classes::uses(ed.doc(), t).total(), 0);
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
}

#[test]
fn an_override_that_the_target_class_has_stays() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "A1".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap()
        .created[0],
    );
    let mut vp = Viewport::new(
        DVec2::ZERO,
        DVec2::new(100.0, 100.0),
        DVec2::ZERO,
        50.0,
        vec![FIRST_LAYER],
    );
    let keep = ClassOverride {
        visible: Some(true),
        ..ClassOverride::default()
    };
    vp.class_overrides.insert(NONE_CLASS, keep.clone());
    vp.class_overrides.insert(
        t,
        ClassOverride {
            visible: Some(false),
            ..ClassOverride::default()
        },
    );
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Viewport(Box::new(vp)),
            layer: Some(sheet),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    delete(&mut ed, t);
    let Shape::Viewport(v) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!(v.class_overrides.get(&NONE_CLASS), Some(&keep));
    assert_eq!(v.class_overrides.len(), 1);
}

#[test]
fn saved_views_name_the_other_class_after_the_delete() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    ed.execute(&Command::SaveView {
        view: Box::new(SavedView {
            name: "Trees only".into(),
            classes: [(t, true), (NONE_CLASS, false)].into(),
            active_class: Some(t),
            ..SavedView::default()
        }),
    })
    .unwrap();
    assert_eq!(classes::uses(ed.doc(), t).saved_views, 1);
    let before = ed.doc().clone();
    delete(&mut ed, t);
    let v = ed.doc().drawing.saved_view("Trees only").unwrap();
    assert!(!v.classes.contains_key(&t));
    assert_eq!(v.classes.get(&NONE_CLASS), Some(&false), "the entry of Untagged stays");
    assert_eq!(v.active_class, Some(NONE_CLASS));
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
    // Restoring a view that names a deleted class no longer fails: it names Untagged now.
    delete(&mut ed, t);
    ed.execute(&Command::RestoreView {
        name: "Trees only".into(),
    })
    .unwrap();
}

#[test]
fn a_class_material_becomes_a_material_of_each_moved_object() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    let a = line(&mut ed, t);
    let b = line(&mut ed, t);
    let c = line(&mut ed, NONE_CLASS);
    let m = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Bark".into(),
            body: ResourceBody::Render(Box::new(present::RenderResource::Material(
                Material::default(),
            ))),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Class { class: t },
        material: Some(m),
    })
    .unwrap();
    // The object b has a material of its own already: it keeps it.
    let own = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Own".into(),
            body: ResourceBody::Render(Box::new(present::RenderResource::Material(
                Material::default(),
            ))),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object {
            object: b,
            slot: None,
        },
        material: Some(own),
    })
    .unwrap();
    assert_eq!(classes::uses(ed.doc(), t).material_slots, 1);
    let before = ed.doc().clone();
    delete(&mut ed, t);
    let slots = &ed.doc().drawing.presentation;
    assert_eq!(slots.slot(&SlotTarget::Class { class: t }), None);
    let obj = |o| SlotTarget::Object { object: o, slot: None };
    assert_eq!(slots.slot(&obj(a)), Some(m), "the moved object looks the same");
    assert_eq!(slots.slot(&obj(b)), Some(own));
    assert_eq!(
        slots.slot(&SlotTarget::Class { class: NONE_CLASS }),
        None,
        "Untagged objects do not get the material"
    );
    assert_eq!(slots.slot(&obj(c)), None);
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
}

#[test]
fn the_uses_count_everything_that_a_question_must_say() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    line(&mut ed, t);
    symbol(&mut ed, t);
    ed.execute(&Command::SaveView {
        view: Box::new(SavedView {
            name: "v".into(),
            classes: [(t, true)].into(),
            ..SavedView::default()
        }),
    })
    .unwrap();
    let u = classes::uses(ed.doc(), t);
    assert_eq!(u.objects, 1);
    assert_eq!(u.definition_parts, 3);
    assert_eq!(u.saved_views, 1);
    assert_eq!(u.others(), 4);
    assert_eq!(u.total(), 5);
}
