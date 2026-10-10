//! The plant list: single plants and the plants of landscape areas in one report.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{self, Vertex};
use kadakas_core::landscape::LandscapePatch;
use kadakas_core::landscape::style::{LandscapeAreaStyle, MixEntry};
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::plants::style::{PlantStyle, QuantityType};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_plants::plant_list::{PlantList, plant_list, plant_list_csv};

fn rect(w: f64, h: f64) -> Vec<Vertex> {
    curve::corners(&[
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, h),
        DVec2::new(0.0, h),
    ])
}

fn plant(ed: &mut Editor, name: &str, f: impl FnOnce(&mut PlantStyle)) -> ResourceId {
    let mut s = PlantStyle {
        id_code: name[..2].to_uppercase(),
        spread: 500.0,
        ..PlantStyle::default()
    };
    f(&mut s);
    ResourceId(
        ed.execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::plant_style(s),
        })
        .unwrap()
        .created[0],
    )
}

fn area_style(ed: &mut Editor, name: &str, mix: Vec<MixEntry>) -> ResourceId {
    let s = LandscapeAreaStyle {
        mix,
        ..Default::default()
    };
    ResourceId(
        ed.execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::LandscapeAreaStyle(Box::new(s)),
        })
        .unwrap()
        .created[0],
    )
}

fn area(ed: &mut Editor, w: f64, h: f64) -> ObjectId {
    let shape = Shape::LandscapeArea {
        boundary: rect(w, h),
        name: "Bed".into(),
    };
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn set(ed: &mut Editor, id: ObjectId, patch: LandscapePatch) {
    ed.execute(&Command::SetLandscapeArea {
        ids: vec![id],
        patch,
    })
    .unwrap();
}

fn entry(plant: ResourceId, percent: f64) -> MixEntry {
    MixEntry {
        plant,
        percent,
        ..Default::default()
    }
}

/// A 10 m by 10 m bed with two plants: 60 percent at 4 plants per m2 and 40 percent at 0.5 m spacing.
fn setup() -> (Editor, ObjectId, ResourceId, ResourceId, ResourceId) {
    let mut ed = Editor::default();
    let a = plant(&mut ed, "Lavender", |_| {});
    let b = plant(&mut ed, "Sedge", |s| s.price = 3.0);
    let st = area_style(
        &mut ed,
        "Meadow",
        vec![
            MixEntry {
                density: Some(4.0),
                ..entry(a, 60.0)
            },
            MixEntry {
                spacing: Some(500.0),
                ..entry(b, 40.0)
            },
        ],
    );
    let id = area(&mut ed, 10_000.0, 10_000.0);
    set(
        &mut ed,
        id,
        LandscapePatch {
            style: Some(st),
            ..Default::default()
        },
    );
    (ed, id, st, a, b)
}

#[test]
fn plant_list_adds_areas_to_single_plants() {
    let (mut ed, _, _, a, b) = setup();
    // Two single lavenders (one with an override) and an unstyled plant.
    ed.execute(&Command::AddPlants {
        style: Some(a),
        overrides: Default::default(),
        positions: vec![DVec2::new(100.0, 100.0)],
        layer: None,
        class: None,
        variation: None,
    })
    .unwrap();
    let mut over = serde_json::Map::new();
    over.insert("price".into(), 9.0.into());
    ed.execute(&Command::AddPlants {
        style: Some(a),
        overrides: over,
        positions: vec![DVec2::new(200.0, 100.0)],
        layer: None,
        class: None,
        variation: None,
    })
    .unwrap();
    ed.execute(&Command::AddObject {
        shape: Shape::Plant {
            at: DVec2::ZERO,
            species: "Loose".into(),
            spread: 500.0,
            height: 500.0,
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let list = plant_list(ed.doc());
    let row = |n: &str| list.rows.iter().find(|r| r.name == n).unwrap();
    let lav = row("Lavender");
    assert_eq!(
        (lav.plants, lav.in_areas, lav.count),
        (2, 240, 242),
        "overrides share one row"
    );
    assert_eq!(lav.areas.len(), 1);
    assert_eq!(lav.areas[0].name, "Bed");
    assert_eq!(lav.areas[0].count, 240);
    assert!((lav.total_price - 9.0).abs() < 1e-9);
    assert_eq!(
        (row("Sedge").plants, row("Sedge").count),
        (0, 160),
        "a style that only areas use"
    );
    assert!(
        (row("Sedge").total_price - 480.0).abs() < 1e-9
            && (row("Sedge").unit_price - 3.0).abs() < 1e-9
    );
    assert_eq!(row("Loose").count, 1);
    assert_eq!(
        (list.total_count, list.total_plants, list.total_in_areas),
        (403, 3, 400)
    );
    assert!(
        list.rows
            .windows(2)
            .all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase())
    );
    let csv = plant_list_csv(&list);
    assert!(csv.starts_with("Name,ID,Botanical name"));
    assert!(
        csv.contains("Lavender") && csv.trim_end().ends_with("489.00,,"),
        "{csv}"
    );
    assert_eq!(b, row("Sedge").style.unwrap());
}

#[test]
fn plant_list_keeps_area_quantity_and_skips_missing_styles() {
    let mut ed = Editor::default();
    let cover = plant(&mut ed, "Thyme", |s| {
        s.quantity = QuantityType::Area;
        s.price = 10.0;
    });
    let st = area_style(&mut ed, "Cover", vec![entry(cover, 100.0)]);
    let id = area(&mut ed, 4000.0, 2500.0);
    set(
        &mut ed,
        id,
        LandscapePatch {
            style: Some(st),
            ..Default::default()
        },
    );
    // A single thyme plant with a canopy of 500 mm adds to the covered area.
    ed.execute(&Command::AddPlants {
        style: Some(cover),
        overrides: Default::default(),
        positions: vec![DVec2::ZERO],
        layer: None,
        class: None,
        variation: None,
    })
    .unwrap();
    let list = plant_list(ed.doc());
    let r = &list.rows[0];
    let single = std::f64::consts::PI * 0.25 * 0.25;
    assert!((r.area_m2 - (10.0 + single)).abs() < 1e-9);
    assert!((list.total_area_m2 - r.area_m2).abs() < 1e-12);
    assert!((r.total_price - 10.0 * (10.0 + single)).abs() < 1e-9);
    assert!(plant_list_csv(&list).contains("area"));
    // An empty document has an empty list.
    assert_eq!(plant_list(&Document::default()), PlantList::default());
    // Unnamed areas get a name from their ID.
    let shape = Shape::LandscapeArea {
        boundary: rect(1000.0, 1000.0),
        name: " ".into(),
    };
    let id2 = ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    set(
        &mut ed,
        id2,
        LandscapePatch {
            style: Some(st),
            ..Default::default()
        },
    );
    assert!(
        plant_list(ed.doc()).rows[0]
            .areas
            .iter()
            .any(|a| a.name.starts_with("Landscape area"))
    );
}
