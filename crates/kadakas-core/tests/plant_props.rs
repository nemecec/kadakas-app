//! Property tests for plant styles: any mix of style and plant commands, undos and redos
//! keeps every plant consistent with its style, and the document can be saved and reopened.

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::plants::link;
use kadakas_core::plants::style::{PlantForm, PlantGraphic, PlantStyle};
use kadakas_core::resource::{ResourceBody, ResourceId};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

#[derive(Clone, Debug)]
enum Op {
    AddStyle {
        name: u8,
        spread: f64,
    },
    Place {
        style: Option<u8>,
        n: usize,
        spread: Option<f64>,
    },
    ModifyStyle {
        style: u8,
        spread: f64,
        rename: Option<u8>,
    },
    SetStyle {
        plant: u8,
        style: u8,
        spread: Option<f64>,
    },
    Unlink {
        plant: u8,
    },
    Delete {
        style: u8,
        reassign: Option<u8>,
    },
    EditShape {
        plant: u8,
        spread: f64,
    },
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    let spread = || 100.0..9000.0f64;
    prop_oneof![
        3 => (0u8..6, spread()).prop_map(|(name, spread)| Op::AddStyle { name, spread }),
        4 => (prop::option::of(0u8..6), 1usize..6, prop::option::of(spread())).prop_map(|(style, n, spread)| Op::Place { style, n, spread }),
        2 => (0u8..6, spread(), prop::option::of(0u8..6)).prop_map(|(style, spread, rename)| Op::ModifyStyle { style, spread, rename }),
        2 => (0u8..8, 0u8..6, prop::option::of(spread())).prop_map(|(plant, style, spread)| Op::SetStyle { plant, style, spread }),
        1 => (0u8..8).prop_map(|plant| Op::Unlink { plant }),
        2 => (0u8..6, prop::option::of(0u8..6)).prop_map(|(style, reassign)| Op::Delete { style, reassign }),
        1 => (0u8..8, spread()).prop_map(|(plant, spread)| Op::EditShape { plant, spread }),
        2 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

fn style_ids(ed: &Editor) -> Vec<ResourceId> {
    ed.doc().resources.iter().map(|r| r.id).collect()
}

fn plant_ids(ed: &Editor) -> Vec<ObjectId> {
    ed.doc()
        .objects
        .values()
        .filter(|o| matches!(o.shape, Shape::Plant { .. }))
        .map(|o| o.id)
        .collect()
}

fn pick<T: Copy>(list: &[T], i: u8) -> Option<T> {
    if list.is_empty() {
        None
    } else {
        Some(list[i as usize % list.len()])
    }
}

fn overrides(spread: Option<f64>) -> Map<String, Value> {
    spread
        .map(|s| json!({"spread": s}).as_object().unwrap().clone())
        .unwrap_or_default()
}

fn run(ed: &mut Editor, op: &Op) {
    let styles = style_ids(ed);
    let plants = plant_ids(ed);
    let cmd = match op {
        Op::AddStyle { name, spread } => Some(Command::AddResource {
            name: format!("Style {name}"),
            body: ResourceBody::plant_style(PlantStyle {
                spread: *spread,
                form: PlantForm::Conical,
                graphic: PlantGraphic::Star,
                ..PlantStyle::default()
            }),
        }),
        Op::Place { style, n, spread } => Some(Command::AddPlants {
            style: style.and_then(|i| pick(&styles, i)),
            overrides: overrides(*spread),
            positions: (0..*n).map(|i| DVec2::new(i as f64 * 500.0, 0.0)).collect(),
            layer: None,
            class: None,
            variation: None,
        }),
        Op::ModifyStyle {
            style,
            spread,
            rename,
        } => pick(&styles, *style).map(|id| Command::ModifyResource {
            id,
            name: rename.map(|r| format!("Style {r}")),
            body: Some(ResourceBody::plant_style(PlantStyle {
                spread: *spread,
                ..PlantStyle::default()
            })),
        }),
        Op::SetStyle {
            plant,
            style,
            spread,
        } => pick(&plants, *plant)
            .zip(pick(&styles, *style))
            .map(|(p, s)| Command::SetPlantStyle {
                ids: vec![p],
                style: Some(s),
                overrides: spread.map(|x| overrides(Some(x))),
                unlink: false,
            }),
        Op::Unlink { plant } => pick(&plants, *plant).map(|p| Command::SetPlantStyle {
            ids: vec![p],
            style: None,
            overrides: None,
            unlink: true,
        }),
        Op::Delete { style, reassign } => pick(&styles, *style).map(|id| Command::DeleteResource {
            id,
            reassign: reassign.and_then(|r| pick(&styles, r)),
        }),
        Op::EditShape { plant, spread } => pick(&plants, *plant).map(|p| {
            let mut shape = ed.doc().object(p).unwrap().shape.clone();
            if let Shape::Plant { spread: s, .. } = &mut shape {
                *s = *spread;
            }
            Command::ModifyObject {
                id: p,
                shape: Some(shape),
                layer: None,
                class: None,
                attrs: None,
            }
        }),
        Op::Undo => {
            ed.undo();
            None
        }
        Op::Redo => {
            ed.redo();
            None
        }
    };
    if let Some(cmd) = cmd {
        // Commands may fail (a name in use, a style in use). A failed command changes nothing.
        let before = ed.doc().clone();
        if ed.execute(&cmd).is_err() {
            assert_eq!(ed.doc(), &before);
        }
    }
}

fn check(doc: &Document) {
    for o in doc.objects.values() {
        if let Shape::PlantTag { style, .. } = &o.shape {
            assert!(
                doc.resources.get(*style).is_some(),
                "tag refers to a missing style"
            );
        }
        let Some(l) = link::link(o) else { continue };
        // A link always refers to a style that exists, and the shape copy is current.
        assert!(
            doc.resources.plant_style(l.style).is_some(),
            "plant {} refers to a missing style",
            o.id
        );
        let e = link::effective(doc, o).unwrap();
        assert_eq!(e.style_id, Some(l.style));
        let Shape::Plant {
            species,
            spread,
            height,
            ..
        } = &o.shape
        else {
            panic!("not a plant")
        };
        assert_eq!(*spread, e.style.spread);
        assert_eq!(*height, e.style.height);
        assert_eq!(species, &e.style.species(&e.style_name));
    }
    assert!(doc.resources.validate().is_ok());
    let text = serde_json::to_string(doc).unwrap();
    assert_eq!(&Document::from_json(&text).unwrap(), doc);
}

proptest! {
    #[test]
    fn plants_stay_consistent_with_their_styles(ops in prop::collection::vec(op(), 1..40)) {
        let mut ed = Editor::default();
        for o in &ops {
            run(&mut ed, o);
            check(ed.doc());
        }
        while ed.can_undo() {
            ed.undo();
            check(ed.doc());
        }
        prop_assert!(ed.doc().objects.is_empty());
        prop_assert!(ed.doc().resources.is_empty());
        while ed.can_redo() {
            ed.redo();
            check(ed.doc());
        }
    }
}
