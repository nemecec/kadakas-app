use super::*;
use crate::command::{Command, Editor};
use crate::ground::NoTerrain;
use crate::model::Attributes;
use crate::plants::style::{PlantCategory, PlantStyle};
use crate::resource::ResourceBody;

fn rect(w: f64, h: f64) -> Vec<Vertex> {
    curve::corners(&[
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, h),
        DVec2::new(0.0, h),
    ])
}

fn plant(
    ed: &mut Editor,
    name: &str,
    f: impl FnOnce(&mut crate::plants::style::PlantStyle),
) -> ResourceId {
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
fn quantities_follow_percent_density_and_holes() {
    let (mut ed, id, _, a, b) = setup();
    let q = quantities(ed.doc(), ed.doc().object(id).unwrap()).unwrap();
    assert_eq!(
        (q.gross_area_m2, q.holes_area_m2, q.net_area_m2),
        (100.0, 0.0, 100.0)
    );
    assert_eq!(q.rows.len(), 2);
    assert_eq!(
        (q.rows[0].plant, q.rows[0].count),
        (a, 240),
        "60 m2 at 4 per m2"
    );
    assert_eq!(
        (q.rows[1].plant, q.rows[1].count),
        (b, 160),
        "40 m2 at 0.5 m spacing"
    );
    assert!(
        (q.rows[1].density_per_m2 - 4.0).abs() < 1e-9
            && (q.rows[1].spacing_mm - 500.0).abs() < 1e-9
    );
    assert!((q.rows[1].total_price - 480.0).abs() < 1e-9);
    // A hole of 2 m by 5 m takes 10 m2 away from every plant.
    let hole = vec![curve::corners(&[
        DVec2::new(1000.0, 1000.0),
        DVec2::new(3000.0, 1000.0),
        DVec2::new(3000.0, 6000.0),
        DVec2::new(1000.0, 6000.0),
    ])];
    set(
        &mut ed,
        id,
        LandscapePatch {
            holes: Some(hole),
            ..Default::default()
        },
    );
    let q = quantities(ed.doc(), ed.doc().object(id).unwrap()).unwrap();
    assert_eq!((q.holes_area_m2, q.net_area_m2), (10.0, 90.0));
    assert_eq!(q.rows[0].count, 216);
    assert_eq!(q.rows[1].count, 144);
}

#[test]
fn minimum_counts_default_spacing_and_area_quantity() {
    let mut ed = Editor::default();
    let tree = plant(&mut ed, "Birch", |s| {
        s.category = PlantCategory::Tree;
        s.spread = 4000.0;
    });
    let cover = plant(&mut ed, "Thyme", |s| {
        s.quantity = QuantityType::Area;
        s.price = 12.0;
        s.spread = 300.0;
    });
    let st = area_style(
        &mut ed,
        "Mixed",
        vec![
            MixEntry {
                min_count: 3,
                percent: 10.0,
                ..entry(tree, 10.0)
            },
            entry(cover, 90.0),
        ],
    );
    let id = area(&mut ed, 2000.0, 2000.0);
    set(
        &mut ed,
        id,
        LandscapePatch {
            style: Some(st),
            ..Default::default()
        },
    );
    let q = quantities(ed.doc(), ed.doc().object(id).unwrap()).unwrap();
    // 0.4 m2 of tree at one per 16 m2 is below one plant: the minimum applies.
    assert_eq!(q.rows[0].count, 3);
    // Thyme: 3.6 m2 at the default spacing of its canopy (300 mm): 11.1 plants per m2.
    assert_eq!(q.rows[1].count, 40);
    assert_eq!(q.rows[1].quantity, QuantityType::Area);
    assert!(
        (q.rows[1].total_price - 12.0 * 3.6).abs() < 1e-9,
        "the price is per m2 covered"
    );
    assert!(
        quantities(
            ed.doc(),
            &Object::new(
                ObjectId::from_num(99),
                crate::model::LayerId::from_num(1),
                crate::model::ClassId::from_num(0),
                Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::X
                }
            )
        )
        .is_none()
    );
    // An area without a style has no rows.
    let plain = area(&mut ed, 1000.0, 1000.0);
    assert!(
        quantities(ed.doc(), ed.doc().object(plain).unwrap())
            .unwrap()
            .rows
            .is_empty()
    );
}

#[test]
fn patch_checks_holes_names_and_styles() {
    let (mut ed, id, st, ..) = setup();
    let mut o = ed.doc().object(id).unwrap().clone();
    let p = |h: Vec<Vec<Vertex>>| LandscapePatch {
        holes: Some(h),
        ..Default::default()
    };
    assert!(
        apply_patch(&mut o, &p(vec![curve::corners(&[DVec2::ZERO, DVec2::X])]))
            .unwrap_err()
            .contains("3 or more")
    );
    let outside = curve::corners(&[
        DVec2::new(-50.0, 0.0),
        DVec2::new(100.0, 0.0),
        DVec2::new(100.0, 100.0),
    ]);
    assert!(
        apply_patch(&mut o, &p(vec![outside]))
            .unwrap_err()
            .contains("not inside")
    );
    let inf = curve::corners(&[
        DVec2::new(f64::INFINITY, 0.0),
        DVec2::new(100.0, 0.0),
        DVec2::new(100.0, 100.0),
    ]);
    assert!(
        apply_patch(&mut o, &p(vec![inf]))
            .unwrap_err()
            .contains("finite")
    );
    assert!(
        apply_patch(&mut o, &p(vec![rect(10.0, 10.0); MAX_HOLES + 1]))
            .unwrap_err()
            .contains("at most")
    );
    // A hole on the edge is fine.
    assert!(apply_patch(&mut o, &p(vec![rect(1000.0, 1000.0)])).is_ok());
    assert!(
        apply_patch(
            &mut o,
            &LandscapePatch {
                style: Some(st),
                unstyle: true,
                ..Default::default()
            }
        )
        .is_err()
    );
    apply_patch(
        &mut o,
        &LandscapePatch {
            unstyle: true,
            name: Some("Beta".into()),
            show_plants: Some(true),
            drape: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        data(&o).unwrap(),
        LandscapeData {
            style: None,
            show_plants: Some(true),
            holes: vec![rect(1000.0, 1000.0)],
            drape: true
        }
    );
    let Shape::LandscapeArea { name, .. } = &o.shape else {
        panic!()
    };
    assert_eq!(name, "Beta");
    // Not an area, or unreadable data.
    let mut line = Object::new(
        ObjectId::from_num(99),
        crate::model::LayerId::from_num(1),
        crate::model::ClassId::from_num(0),
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
    assert!(apply_patch(&mut line, &LandscapePatch::default()).is_err());
    assert!(data(&line).is_none());
    o.data.insert(KEY.into(), serde_json::json!({"holes": 5}));
    assert!(
        apply_patch(&mut o, &LandscapePatch::default())
            .unwrap_err()
            .contains("cannot be read")
    );
    // Commands: unknown and wrong style types are refused.
    let bad = ed.execute(&Command::SetLandscapeArea {
        ids: vec![id],
        patch: LandscapePatch {
            style: Some(ResourceId::from_num(999)),
            ..Default::default()
        },
    });
    assert!(matches!(
        bad,
        Err(crate::command::CommandError::NoResource(_))
    ));
    let plant_id = ed.doc().resources.find_plant_style("Sedge").unwrap();
    assert!(
        ed.execute(&Command::SetLandscapeArea {
            ids: vec![id],
            patch: LandscapePatch {
                style: Some(plant_id),
                ..Default::default()
            }
        })
        .is_err()
    );
    assert!(
        ed.execute(&Command::SetLandscapeArea {
            ids: vec![id],
            patch: LandscapePatch::default()
        })
        .is_err()
    );
}

#[test]
fn holes_move_with_the_area_and_undo_works() {
    let (mut ed, id, ..) = setup();
    let before = ed.doc().clone();
    set(
        &mut ed,
        id,
        LandscapePatch {
            holes: Some(vec![rect(1000.0, 1000.0)]),
            ..Default::default()
        },
    );
    ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta: DVec2::new(500.0, 0.0),
    })
    .unwrap();
    assert_eq!(
        data(ed.doc().object(id).unwrap()).unwrap().holes[0][0].p,
        DVec2::new(500.0, 0.0)
    );
    // The hole is still inside, so the net area did not change.
    assert_eq!(
        quantities(ed.doc(), ed.doc().object(id).unwrap())
            .unwrap()
            .net_area_m2,
        99.0
    );
    ed.undo();
    ed.undo();
    assert_eq!(ed.doc().objects, before.objects);
    // An area without data ignores a move.
    let plain = area(&mut ed, 1000.0, 1000.0);
    ed.execute(&Command::MoveObjects {
        ids: vec![plain],
        delta: DVec2::X,
    })
    .unwrap();
    assert!(data(ed.doc().object(plain).unwrap()).is_none());
}

#[test]
fn scatter_is_deterministic_inside_and_proportional() {
    let (mut ed, id, ..) = setup();
    let hole = curve::corners(&[
        DVec2::new(2000.0, 2000.0),
        DVec2::new(5000.0, 2000.0),
        DVec2::new(5000.0, 5000.0),
        DVec2::new(2000.0, 5000.0),
    ]);
    set(
        &mut ed,
        id,
        LandscapePatch {
            holes: Some(vec![hole.clone()]),
            ..Default::default()
        },
    );
    let o = ed.doc().object(id).unwrap();
    let s1 = scatter(ed.doc(), o);
    let s2 = scatter(ed.doc(), o);
    assert_eq!(s1, s2, "same document, same symbols");
    assert!(!s1.is_empty() && s1.len() <= MAX_SCATTER);
    let ring = curve::positions(&rect(10_000.0, 10_000.0));
    let hole_ring = curve::positions(&hole);
    assert!(
        s1.iter()
            .all(|p| geom::point_in_polygon(p.at, &ring)
                && !geom::point_in_polygon(p.at, &hole_ring))
    );
    let a = s1
        .iter()
        .filter(|p| p.plant == ed.doc().resources.find_plant_style("Lavender").unwrap())
        .count();
    let b = s1.len() - a;
    assert!(
        (a as f64 / b as f64 - 1.5).abs() < 0.15,
        "60 to 40: {a} to {b}"
    );
    // The symbols are the plants when they are few.
    let few = {
        let mut small = Editor::default();
        let p = plant(&mut small, "Rose", |_| {});
        let st = area_style(
            &mut small,
            "Roses",
            vec![MixEntry {
                density: Some(1.0),
                ..entry(p, 100.0)
            }],
        );
        let id = area(&mut small, 3000.0, 3000.0);
        set(
            &mut small,
            id,
            LandscapePatch {
                style: Some(st),
                ..Default::default()
            },
        );
        scatter(small.doc(), small.doc().object(id).unwrap()).len()
    };
    assert!(
        (7..=9).contains(&few),
        "9 m2 at 1 per m2 gives about 9 symbols, got {few}"
    );
    // Another object ID gives another layout.
    let other = area(&mut ed, 10_000.0, 10_000.0);
    let style = data(ed.doc().object(id).unwrap()).unwrap().style;
    set(
        &mut ed,
        other,
        LandscapePatch {
            style,
            ..Default::default()
        },
    );
    assert_ne!(
        scatter(ed.doc(), ed.doc().object(other).unwrap())
            .iter()
            .map(|p| p.at)
            .collect::<Vec<_>>(),
        s1.iter().map(|p| p.at).collect::<Vec<_>>()
    );
}

#[test]
fn moving_an_area_moves_its_symbols_and_does_not_shuffle_them() {
    let (mut ed, id, ..) = setup();
    let before: Vec<DVec2> = scatter(ed.doc(), ed.doc().object(id).unwrap())
        .iter()
        .map(|p| p.at)
        .collect();
    let delta = DVec2::new(123_456.0, -78_000.0);
    ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta,
    })
    .unwrap();
    let after: Vec<DVec2> = scatter(ed.doc(), ed.doc().object(id).unwrap())
        .iter()
        .map(|p| p.at)
        .collect();
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert!((*a + delta - *b).length() < 1e-6, "{a} {b}");
    }
}

#[test]
fn scatter_edge_cases() {
    let (mut ed, id, ..) = setup();
    let line = Object::new(
        ObjectId::from_num(99),
        crate::model::LayerId::from_num(1),
        crate::model::ClassId::from_num(0),
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
    assert!(scatter(ed.doc(), &line).is_empty());
    let plain = area(&mut ed, 1000.0, 1000.0);
    assert!(scatter(ed.doc(), ed.doc().object(plain).unwrap()).is_empty());
    // The whole area is a hole.
    set(
        &mut ed,
        id,
        LandscapePatch {
            holes: Some(vec![rect(10_000.0, 10_000.0)]),
            ..Default::default()
        },
    );
    assert!(scatter(ed.doc(), ed.doc().object(id).unwrap()).is_empty());
    // A huge, thin area draws a few symbols (the count is capped, so the work is too).
    let thin = area(&mut ed, 1.0e9, 1.0);
    let style = ed.doc().resources.find_landscape_area_style("Meadow");
    set(
        &mut ed,
        thin,
        LandscapePatch {
            style,
            ..Default::default()
        },
    );
    let few = scatter(ed.doc(), ed.doc().object(thin).unwrap());
    assert!(!few.is_empty() && few.len() <= MAX_SCATTER);
}

#[test]
fn a_large_l_shaped_area_still_draws_symbols() {
    let (mut ed, ..) = setup();
    let style = ed.doc().resources.find_landscape_area_style("Meadow");
    // An L with arms of 200 m by 2 m: the box is 200 m by 200 m, and the area only 800 m2.
    let l = curve::corners(&[
        DVec2::ZERO,
        DVec2::new(200_000.0, 0.0),
        DVec2::new(200_000.0, 2000.0),
        DVec2::new(2000.0, 2000.0),
        DVec2::new(2000.0, 200_000.0),
        DVec2::new(0.0, 200_000.0),
    ]);
    let shape = Shape::LandscapeArea {
        boundary: l,
        name: "L".into(),
    };
    let id = ObjectId(
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
        id,
        LandscapePatch {
            style,
            ..Default::default()
        },
    );
    let placed = scatter(ed.doc(), ed.doc().object(id).unwrap());
    assert!(placed.len() > 50, "{} symbols", placed.len());
    let (a, b) = (
        ed.doc().resources.find_plant_style("Lavender").unwrap(),
        ed.doc().resources.find_plant_style("Sedge").unwrap(),
    );
    assert!(
        placed.iter().any(|p| p.plant == a) && placed.iter().any(|p| p.plant == b),
        "every plant of the mix is drawn"
    );
}

#[test]
fn holes_count_as_the_union_cut_to_the_boundary() {
    let (mut ed, id, ..) = setup();
    let hole = |x0: f64, y0: f64, x1: f64, y1: f64| {
        curve::corners(&[
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ])
    };
    // Two holes of 4 m2 that overlap in 1 m2 take 7 m2 away. The area is 100 m2.
    set(
        &mut ed,
        id,
        LandscapePatch {
            holes: Some(vec![
                hole(1000.0, 1000.0, 3000.0, 3000.0),
                hole(2000.0, 2000.0, 4000.0, 4000.0),
            ]),
            ..Default::default()
        },
    );
    let q = quantities(ed.doc(), ed.doc().object(id).unwrap()).unwrap();
    assert!(
        (q.holes_area_m2 - 7.0).abs() < 1e-6 && (q.net_area_m2 - 93.0).abs() < 1e-6,
        "{q:?}"
    );
    // The boundary can shrink only while it holds the holes.
    let small = Shape::LandscapeArea {
        boundary: rect(2500.0, 2500.0),
        name: "Bed".into(),
    };
    let err = ed
        .execute(&Command::ModifyObject {
            id,
            shape: Some(small),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("holes"), "{err}");
    let err = ed
        .execute(&Command::SetVertex {
            id,
            index: 2,
            kind: None,
            p: Some(DVec2::new(2500.0, 2500.0)),
        })
        .unwrap_err();
    assert!(err.to_string().contains("holes"), "{err}");
    // A hole that sticks out of the boundary (a file edited by hand) counts only inside.
    let o = ed.doc().object(id).unwrap();
    let Shape::LandscapeArea { boundary, .. } = &o.shape else {
        panic!()
    };
    let sticking = vec![hole(9000.0, 9000.0, 11_000.0, 11_000.0)];
    assert!(
        (holes_area(boundary, &sticking) - 1.0e6).abs() < 1.0,
        "1 m2 of the 4 m2 hole is inside"
    );
    assert!(!holes_fit(boundary, &sticking));
    assert_eq!(holes_area(boundary, &[]), 0.0);
}

#[test]
fn svg_shows_holes_and_plants() {
    let (mut ed, id, ..) = setup();
    let o = ed.doc().object(id).unwrap();
    assert!(
        svg_extras(ed.doc(), o, 100.0, "black").is_empty(),
        "plants are off by default"
    );
    assert!(!shows_plants(ed.doc(), o));
    set(
        &mut ed,
        id,
        LandscapePatch {
            show_plants: Some(true),
            holes: Some(vec![rect(1000.0, 1000.0)]),
            ..Default::default()
        },
    );
    let svg = svg_extras(ed.doc(), ed.doc().object(id).unwrap(), 100.0, "black");
    assert!(svg.contains(r#"data-part="hole""#) && svg.contains(r#"data-part="plants""#));
    assert!(
        svg.matches("<circle").count() > 100,
        "large areas draw circles"
    );
    // The style can switch plants on, and the object can switch them off.
    let st = ed
        .doc()
        .resources
        .find_landscape_area_style("Meadow")
        .unwrap();
    let mut style = ed.doc().resources.landscape_area_style(st).unwrap().clone();
    style.show_plants = true;
    ed.execute(&Command::ModifyResource {
        id: st,
        name: None,
        body: Some(ResourceBody::LandscapeAreaStyle(Box::new(style))),
    })
    .unwrap();
    set(
        &mut ed,
        id,
        LandscapePatch {
            show_plants: Some(false),
            ..Default::default()
        },
    );
    assert!(!svg_extras(ed.doc(), ed.doc().object(id).unwrap(), 100.0, "black").contains("plants"));
    let line = Object::new(
        ObjectId::from_num(99),
        crate::model::LayerId::from_num(1),
        crate::model::ClassId::from_num(0),
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
    assert!(svg_extras(ed.doc(), &line, 100.0, "black").is_empty());
}

#[test]
fn small_areas_draw_graphics_and_protection_zones() {
    let mut ed = Editor::default();
    let tree = plant(&mut ed, "Oak", |s| {
        s.category = PlantCategory::Tree;
        s.spread = 3000.0;
        s.trunk_diameter = 400.0;
        s.protection_zone = true;
        s.graphic = PlantGraphic::Star;
    });
    let st = area_style(
        &mut ed,
        "Grove",
        vec![MixEntry {
            density: Some(0.1),
            ..entry(tree, 100.0)
        }],
    );
    let id = area(&mut ed, 12_000.0, 12_000.0);
    set(
        &mut ed,
        id,
        LandscapePatch {
            style: Some(st),
            show_plants: Some(true),
            ..Default::default()
        },
    );
    let svg = svg_extras(ed.doc(), ed.doc().object(id).unwrap(), 100.0, "black");
    assert!(svg.contains("<path d=\"M"), "the star graphic");
    assert!(
        svg.contains(r#"data-part="rpz""#),
        "tree protection zone of 4800 mm"
    );
    assert!(svg.contains("r=\"4800\""));
}

#[test]
fn deleting_a_plant_style_checks_the_mixes() {
    let (mut ed, id, st, a, b) = setup();
    let err = ed
        .execute(&Command::DeleteResource {
            id: a,
            reassign: None,
        })
        .unwrap_err();
    assert!(
        matches!(err, crate::command::CommandError::ResourceInUse(_, 1)),
        "{err:?}"
    );
    // Reassign to a plant that is in the mix: the percentages join.
    ed.execute(&Command::DeleteResource {
        id: a,
        reassign: Some(b),
    })
    .unwrap();
    let s = ed.doc().resources.landscape_area_style(st).unwrap();
    assert_eq!(s.mix.len(), 1);
    assert_eq!((s.mix[0].plant, s.mix[0].percent), (b, 100.0));
    // Reassign to a plant that is not in the mix.
    let c = plant(&mut ed, "Fescue", |_| {});
    ed.execute(&Command::DeleteResource {
        id: b,
        reassign: Some(c),
    })
    .unwrap();
    assert_eq!(
        ed.doc().resources.landscape_area_style(st).unwrap().mix[0].plant,
        c
    );
    assert!(
        quantities(ed.doc(), ed.doc().object(id).unwrap())
            .unwrap()
            .rows[0]
            .count
            > 0
    );
    // Undo restores the mix.
    ed.undo();
    assert_eq!(
        ed.doc().resources.landscape_area_style(st).unwrap().mix[0].plant,
        b
    );
    // A style cannot use a plant that does not exist, or a resource of another type.
    let bad = LandscapeAreaStyle {
        mix: vec![entry(ResourceId::from_num(999), 100.0)],
        ..Default::default()
    };
    assert!(
        ed.execute(&Command::AddResource {
            name: "Bad".into(),
            body: ResourceBody::LandscapeAreaStyle(Box::new(bad))
        })
        .is_err()
    );
    let bad = LandscapeAreaStyle {
        mix: vec![entry(st, 100.0)],
        ..Default::default()
    };
    assert!(
        ed.execute(&Command::AddResource {
            name: "Bad".into(),
            body: ResourceBody::LandscapeAreaStyle(Box::new(bad.clone()))
        })
        .unwrap_err()
        .to_string()
        .contains("not a plant style")
    );
    assert!(
        ed.execute(&Command::ModifyResource {
            id: st,
            name: None,
            body: Some(ResourceBody::LandscapeAreaStyle(Box::new(bad)))
        })
        .is_err()
    );
    // A landscape area style in use cannot be deleted without reassign.
    assert!(
        ed.execute(&Command::DeleteResource {
            id: st,
            reassign: None
        })
        .is_err()
    );
}

#[test]
fn report_lists_areas_with_fields() {
    let (mut ed, id, st, ..) = setup();
    let mut style = ed.doc().resources.landscape_area_style(st).unwrap().clone();
    style.fields.insert("hydrozone".into(), "low".into());
    ed.execute(&Command::ModifyResource {
        id: st,
        name: None,
        body: Some(ResourceBody::LandscapeAreaStyle(Box::new(style))),
    })
    .unwrap();
    let r = report(ed.doc(), &NoTerrain);
    assert_eq!(r.rows.len(), 1);
    assert_eq!(r.rows[0].id, id);
    assert_eq!(r.rows[0].fields["hydrozone"], "low");
    assert_eq!(r.rows[0].style_name.as_deref(), Some("Meadow"));
    assert_eq!(
        (r.total_count, r.total_gross_area_m2, r.total_net_area_m2),
        (400, 100.0, 100.0)
    );
    assert!((r.total_price - 480.0).abs() < 1e-9);
    assert!(r.rows[0].surface_area_m2.is_none());
    // Draped: a ramp of 100 percent.
    set(
        &mut ed,
        id,
        LandscapePatch {
            drape: Some(true),
            ..Default::default()
        },
    );
    struct Ramp;
    impl crate::ground::GroundSource for Ramp {
        fn exact(&self, _: Option<&Object>, p: DVec2, _: crate::ground::Surface) -> Option<f64> {
            Some(p.x)
        }
    }
    let r = report(ed.doc(), &Ramp);
    assert!((r.rows[0].surface_area_m2.unwrap() - 100.0 * 2.0f64.sqrt()).abs() < 1e-6);
    assert!(
        draped_area_m2(
            &Object::new(
                ObjectId::from_num(99),
                crate::model::LayerId::from_num(1),
                crate::model::ClassId::from_num(0),
                Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::X
                }
            ),
            &|_| None
        )
        .is_none()
    );
}

#[test]
fn old_files_load_and_styles_round_trip() {
    let (ed, ..) = setup();
    let text = serde_json::to_string(ed.doc()).unwrap();
    let back = Document::from_json(&text).unwrap();
    assert_eq!(&back, ed.doc());
    // A landscape area from an old file has no data and no rows.
    let old = r#"{"format_version":3,"layers":[{"id":"00000000-0000-0000-0000-000000000001","name":"L","visible":true,"locked":false,"elevation":0,"scale":100}],"classes":[{"id":"00000000-0000-0000-0000-000000000000","name":"None","visible":true,"attrs":{}}],"objects":[{"id":"00000000-0000-0000-0000-000000000002","layer":"00000000-0000-0000-0000-000000000001","class":"00000000-0000-0000-0000-000000000000","shape":{"kind":"landscape_area","boundary":[[0,0],[4,0],[4,3]],"name":"Lawn"}}],"active_layer":"00000000-0000-0000-0000-000000000001","active_class":"00000000-0000-0000-0000-000000000000"}"#;
    let doc = Document::from_json(old).unwrap();
    let o = doc.object(ObjectId::from_num(2)).unwrap();
    assert!(data(o).is_none());
    assert!(quantities(&doc, o).unwrap().rows.is_empty());
    assert_eq!(quantities(&doc, o).unwrap().gross_area_m2, 6.0e-6);
}

#[test]
fn draped_face_follows_the_ground() {
    use crate::mesh;
    let (mut ed, id, ..) = setup();
    set(
        &mut ed,
        id,
        LandscapePatch {
            drape: Some(true),
            ..Default::default()
        },
    );
    let ground = |p: DVec2| Some(p.x / 10.0);
    let ext = |doc: &Document, obj: &Object, sink: &mut crate::mesh::SceneSink<'_>| {
        emit_object(doc, obj, Some(&ground), sink)
    };
    let scene = mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext));
    let (lo, hi) = scene.object_bounds(id).unwrap();
    assert!(
        (lo.z - 5.0).abs() < 1e-3 && (hi.z - 1005.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    // Without the extension the area is flat.
    let flat = mesh::build_with(ed.doc(), glam::DVec3::ZERO, None);
    assert_eq!(flat.object_bounds(id).unwrap().1.z, 0.0);
    // No ground at all: a flat face.
    let ext = |doc: &Document, obj: &Object, sink: &mut crate::mesh::SceneSink<'_>| {
        emit_object(doc, obj, None, sink)
    };
    let scene = mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext));
    assert_eq!(scene.object_bounds(id).unwrap().1.z, 0.0);
}
