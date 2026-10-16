use super::*;
use crate::command::{Arrange, Command, Editor};
use crate::curve::VertexKind;
use crate::model::{Attributes, Document, NONE_CLASS};
use crate::present::{
    Material, PART_MAIN, Procedural, RenderResource, SlotIndex, SlotTarget, Texture,
};
use crate::resource::ResourceBody;
use crate::site::SurfaceLook;

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vertex> {
    curve::corners(&[
        DVec2::new(x0, y0),
        DVec2::new(x1, y0),
        DVec2::new(x1, y1),
        DVec2::new(x0, y1),
    ])
}

fn bed_shape(texture: Option<ResourceId>, mapping: Option<BedMapping>, x0: f64, x1: f64) -> Shape {
    Shape::SiteModifier {
        modifier: ModifierKind::TextureBed { texture, mapping },
        boundary: square(x0, 0.0, x1, 1000.0),
    }
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    let out = ed
        .execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    ObjectId(out.created[0])
}

fn site(ed: &mut Editor, settings: SiteSettings) -> ObjectId {
    add(
        ed,
        Shape::SiteModel {
            points: vec![
                glam::DVec3::new(0.0, 0.0, 0.0),
                glam::DVec3::new(10_000.0, 0.0, 0.0),
                glam::DVec3::new(0.0, 10_000.0, 0.0),
            ],
            lines: vec![],
            settings,
        },
    )
}

fn material(ed: &mut Editor, name: &str) -> ResourceId {
    let m = Material {
        texture: Some(Texture::Procedural {
            kind: Procedural::Grass,
            seed: 1,
        }),
        ..Default::default()
    };
    let out = ed
        .execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::Render(Box::new(RenderResource::Material(m))),
        })
        .unwrap();
    ResourceId(out.created[0])
}

#[test]
fn the_mapping_is_checked_and_turns_into_a_planar_projection() {
    let m = BedMapping::default();
    assert_eq!(m.problem(), None);
    assert_eq!((m.tile, m.offset, m.rotation), (2000.0, [0.0; 2], 0.0));
    let planar = BedMapping {
        tile: 750.0,
        offset: [3.0, 4.0],
        rotation: -90.0,
    }
    .mapping();
    assert_eq!(planar.kind, MappingKind::Planar);
    assert_eq!(
        (planar.scale, planar.rotation, planar.offset),
        ([750.0; 2], 270.0, [3.0, 4.0])
    );
    let bad = |f: &dyn Fn(&mut BedMapping), word: &str| {
        let mut m = BedMapping::default();
        f(&mut m);
        assert!(m.problem().expect(word).contains(word), "{word}");
    };
    bad(&|m| m.tile = 1.0, "tile");
    bad(&|m| m.tile = 1e12, "tile");
    bad(&|m| m.tile = f64::NAN, "finite");
    bad(&|m| m.offset = [0.0, f64::INFINITY], "finite");
    bad(&|m| m.offset = [2e9, 0.0], "offset");
    bad(&|m| m.rotation = 2e6, "rotation");
    let s = SiteSettings {
        texture_tile: 99.0,
        texture_offset: [1.0, 2.0],
        texture_rotation: 5.0,
        ..Default::default()
    };
    assert_eq!(BedMapping::of_site(&s).tile, 99.0);
    assert_eq!(BedMapping::of_site(&s).numbers(), vec![99.0, 1.0, 2.0, 5.0]);
}

#[test]
fn a_bed_is_part_of_the_shape_json_and_old_values_read() {
    let kind = ModifierKind::TextureBed {
        texture: Some(ResourceId::from_num(9)),
        mapping: Some(BedMapping {
            tile: 500.0,
            ..Default::default()
        }),
    };
    let json = serde_json::to_value(kind).unwrap();
    assert_eq!(json["type"], "texture_bed");
    assert_eq!(serde_json::from_value::<ModifierKind>(json).unwrap(), kind);
    // Missing fields take their defaults: the bed is by class, with the mapping of the site.
    let bare: ModifierKind = serde_json::from_str(r#"{"type": "texture_bed"}"#).unwrap();
    assert_eq!(
        bare,
        ModifierKind::TextureBed {
            texture: None,
            mapping: None
        }
    );
    assert_eq!(
        serde_json::to_value(bare).unwrap(),
        serde_json::json!({"type": "texture_bed"})
    );
    // A mapping with only some fields reads with defaults for the rest.
    let part: ModifierKind =
        serde_json::from_str(r#"{"type": "texture_bed", "mapping": {"tile": 300.0}}"#).unwrap();
    assert!(
        matches!(part, ModifierKind::TextureBed { mapping: Some(m), .. } if m.tile == 300.0 && m.rotation == 0.0)
    );
    assert!(!bare.changes_grade() && ModifierKind::GradeLimit.changes_grade());
    assert!(kind.is_finite() && bare.is_finite());
    assert_eq!(kind.numbers().len(), 4);
    assert!(bare.numbers().is_empty());
    let nan = ModifierKind::TextureBed {
        texture: None,
        mapping: Some(BedMapping {
            tile: f64::NAN,
            ..Default::default()
        }),
    };
    assert!(!nan.is_finite());
}

#[test]
fn problems_are_found_and_repaired() {
    let ok = bed_shape(None, None, 0.0, 1000.0);
    assert_eq!(problem(&ok), None);
    assert_eq!(ok.problem(), None);
    let bad_map = bed_shape(
        None,
        Some(BedMapping {
            tile: 1.0,
            ..Default::default()
        }),
        0.0,
        1.0,
    );
    assert!(problem(&bad_map).unwrap().contains("tile"));
    assert!(bad_map.problem().is_some());
    let many = Shape::SiteModifier {
        modifier: ModifierKind::TextureBed {
            texture: None,
            mapping: None,
        },
        boundary: (0..=MAX_BED_VERTICES)
            .map(|i| Vertex::corner(DVec2::new(i as f64, (i * i % 7) as f64)))
            .collect(),
    };
    assert!(problem(&many).unwrap().contains("4096"));
    // Other shapes have no bed problem.
    assert_eq!(
        problem(&Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X
        }),
        None
    );
    let mut fix = bad_map.clone();
    assert!(repair(&mut fix).is_some());
    assert_eq!(fix.problem(), None);
    assert!(matches!(
        fix,
        Shape::SiteModifier {
            modifier: ModifierKind::TextureBed { mapping: None, .. },
            ..
        }
    ));
    assert!(repair(&mut fix).is_none() && repair(&mut ok.clone()).is_none());
}

#[test]
fn a_file_with_a_bad_mapping_loads_with_the_mapping_reset() {
    let mut ed = Editor::with_sequential_ids();
    let id = add(&mut ed, bed_shape(None, None, 0.0, 1000.0));
    let mut doc = ed.doc().clone();
    if let Shape::SiteModifier { modifier, .. } = &mut doc.object_mut(id).unwrap().shape {
        *modifier = ModifierKind::TextureBed {
            texture: None,
            mapping: Some(BedMapping {
                tile: 0.0,
                ..Default::default()
            }),
        };
    }
    let loaded = Document::from_json(&serde_json::to_string(&doc).unwrap()).expect("opens");
    assert_eq!(loaded.object(id).unwrap().shape.problem(), None);
    assert_eq!(loaded.load_notes().len(), 1);
    assert!(loaded.load_notes()[0].contains("texture bed"));
}

#[test]
fn a_moved_bed_takes_its_own_mapping_along() {
    let mut own = bed_shape(
        None,
        Some(BedMapping {
            offset: [10.0, 20.0],
            ..Default::default()
        }),
        0.0,
        1000.0,
    );
    own.translate(DVec2::new(100.0, -5.0));
    let Shape::SiteModifier {
        modifier: ModifierKind::TextureBed { mapping, .. },
        boundary,
    } = &own
    else {
        panic!()
    };
    assert_eq!(mapping.unwrap().offset, [110.0, 15.0]);
    assert_eq!(boundary[0].p.x, 100.0);
    // A bed with the mapping of the site model has nothing to move.
    let mut shared = bed_shape(None, None, 0.0, 1000.0);
    shared.translate(DVec2::new(100.0, 0.0));
    assert!(matches!(
        shared,
        Shape::SiteModifier {
            modifier: ModifierKind::TextureBed { mapping: None, .. },
            ..
        }
    ));
}

#[test]
fn beds_are_found_in_stacking_order_with_the_material_rules() {
    let mut ed = Editor::with_sequential_ids();
    let site_id = site(&mut ed, SiteSettings::default());
    let grass = material(&mut ed, "Grass");
    let gravel = material(&mut ed, "Gravel");
    let a = add(&mut ed, bed_shape(Some(grass), None, 0.0, 2000.0));
    let b = add(&mut ed, bed_shape(None, None, 1000.0, 3000.0));
    let c = add(&mut ed, bed_shape(Some(gravel), None, 2000.0, 4000.0));
    let doc = ed.doc();
    let site_obj = doc.object(site_id).unwrap();
    // The middle bed has no material: it has no effect.
    let list = beds_for(doc, site_obj);
    assert_eq!(
        list.iter().map(|b| (b.object, b.part)).collect::<Vec<_>>(),
        vec![(a, PART_BED), (c, PART_BED + 1)]
    );
    assert_eq!(list[0].material, grass);
    assert_eq!(list[1].material, gravel);
    // By class: the material of the class of the bed.
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Class { class: NONE_CLASS },
        material: Some(gravel),
    })
    .unwrap();
    let doc = ed.doc();
    let by_class = beds_for(doc, doc.object(site_id).unwrap());
    assert_eq!(by_class.len(), 3);
    assert_eq!(by_class[1].object, b);
    assert_eq!(by_class[1].material, gravel);
    // A material on the bed object itself wins over its class.
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object {
            object: b,
            slot: None,
        },
        material: Some(grass),
    })
    .unwrap();
    let doc = ed.doc();
    assert_eq!(
        crate::present::bed_material(doc, doc.object(b).unwrap()),
        Some(grass)
    );
    // Arrange moves the front bed to the back: the order follows the document.
    ed.execute(&Command::Arrange {
        ids: vec![c],
        how: Arrange::Back,
    })
    .unwrap();
    let doc = ed.doc();
    let order: Vec<ObjectId> = beds_for(doc, doc.object(site_id).unwrap())
        .iter()
        .map(|b| b.object)
        .collect();
    assert_eq!(order, vec![c, a, b]);
}

#[test]
fn beds_must_be_on_the_layer_visible_and_near_the_site() {
    let mut ed = Editor::with_sequential_ids();
    let site_id = site(&mut ed, SiteSettings::default());
    let grass = material(&mut ed, "Grass");
    let near = add(&mut ed, bed_shape(Some(grass), None, 0.0, 2000.0));
    // Far away: its box does not touch the site.
    add(&mut ed, bed_shape(Some(grass), None, 1.0e6, 1.1e6));
    // A bed that is not a texture bed is not found.
    add(
        &mut ed,
        Shape::SiteModifier {
            modifier: ModifierKind::GradeLimit,
            boundary: square(0.0, 0.0, 1000.0, 1000.0),
        },
    );
    let doc = ed.doc();
    let find = |doc: &Document| beds_for(doc, doc.object(site_id).unwrap());
    assert_eq!(
        find(doc).iter().map(|b| b.object).collect::<Vec<_>>(),
        vec![near]
    );
    // A bed on a layer that has no site model acts on the model under it, on any layer.
    ed.execute(&Command::AddLayer {
        name: "Other".into(),
        elevation: 0.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let other = ed.doc().layers.last().unwrap().id;
    ed.execute(&Command::ModifyObject {
        id: near,
        shape: None,
        layer: Some(other),
        class: None,
        attrs: None,
    })
    .unwrap();
    assert_eq!(
        find(ed.doc()).iter().map(|b| b.object).collect::<Vec<_>>(),
        vec![near]
    );
    // A layer with a site model of its own keeps its beds for it.
    ed.execute(&Command::AddObject {
        shape: Shape::SiteModel {
            points: vec![
                glam::DVec3::new(5.0e6, 0.0, 0.0),
                glam::DVec3::new(5.0e6 + 1000.0, 0.0, 0.0),
                glam::DVec3::new(5.0e6, 1000.0, 0.0),
            ],
            lines: vec![],
            settings: SiteSettings::default(),
        },
        layer: Some(other),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    assert!(find(ed.doc()).is_empty());
    // The site model gives none for another shape.
    let doc = ed.doc();
    assert!(beds_for(doc, doc.object(near).unwrap()).is_empty());
}

#[test]
fn only_the_frontmost_64_beds_act() {
    let mut ed = Editor::with_sequential_ids();
    let site_id = site(&mut ed, SiteSettings::default());
    let grass = material(&mut ed, "Grass");
    let ids: Vec<ObjectId> = (0..70)
        .map(|i| {
            add(
                &mut ed,
                bed_shape(Some(grass), None, i as f64 * 10.0, i as f64 * 10.0 + 500.0),
            )
        })
        .collect();
    let doc = ed.doc();
    let list = beds_for(doc, doc.object(site_id).unwrap());
    assert_eq!(list.len(), MAX_TEXTURE_BEDS);
    assert_eq!(list[0].object, ids[6]);
    assert_eq!(list[63].object, ids[69]);
    assert_eq!(list[63].part, PART_BED + 63);
}

#[test]
fn the_mapping_of_a_bed_is_its_own_or_that_of_the_site() {
    let mut ed = Editor::with_sequential_ids();
    let settings = SiteSettings {
        texture_tile: 1234.0,
        texture_offset: [5.0, 6.0],
        texture_rotation: 30.0,
        ..Default::default()
    };
    let site_id = site(&mut ed, settings);
    let grass = material(&mut ed, "Grass");
    add(&mut ed, bed_shape(Some(grass), None, 0.0, 1000.0));
    add(
        &mut ed,
        bed_shape(
            Some(grass),
            Some(BedMapping {
                tile: 400.0,
                ..Default::default()
            }),
            0.0,
            1000.0,
        ),
    );
    let doc = ed.doc();
    let list = beds_for(doc, doc.object(site_id).unwrap());
    assert_eq!(list[0].mapping.scale, [1234.0; 2]);
    assert_eq!(list[0].mapping.rotation, 30.0);
    assert_eq!(list[0].mapping.offset, [5.0, 6.0]);
    assert_eq!(list[1].mapping.scale, [400.0; 2]);
}

#[test]
fn the_outline_of_a_curved_bed_stays_under_the_edge_cap() {
    // A circle of 200 m radius as four arcs: flattened at 2 mm it would have far more edges.
    let round = vec![
        Vertex {
            p: DVec2::new(200_000.0, 0.0),
            kind: VertexKind::Arc { radius: 200_000.0 },
        },
        Vertex {
            p: DVec2::new(0.0, 200_000.0),
            kind: VertexKind::Arc { radius: 200_000.0 },
        },
        Vertex {
            p: DVec2::new(-200_000.0, 0.0),
            kind: VertexKind::Arc { radius: 200_000.0 },
        },
        Vertex {
            p: DVec2::new(0.0, -200_000.0),
            kind: VertexKind::Arc { radius: 200_000.0 },
        },
    ];
    let o = outline(&round);
    assert!(o.len() >= 8 && o.len() <= MAX_BED_EDGES, "{}", o.len());
    // A plain square has four corners.
    assert_eq!(outline(&square(0.0, 0.0, 10.0, 10.0)).len(), 4);
    // Numbers that are not finite give no outline.
    let mut nan = square(0.0, 0.0, 10.0, 10.0);
    nan[1].p.x = f64::NAN;
    assert!(outline(&nan).is_empty());
    // Many corners are thinned to the cap.
    let many: Vec<Vertex> = (0..3 * MAX_BED_EDGES)
        .map(|i| Vertex::corner(DVec2::new(i as f64, (i % 2) as f64)))
        .collect();
    assert!(outline(&many).len() <= MAX_BED_EDGES);
}

#[test]
fn the_slot_index_gives_the_bed_parts_their_material_and_mapping() {
    let mut ed = Editor::with_sequential_ids();
    let site_id = site(&mut ed, SiteSettings::default());
    let grass = material(&mut ed, "Grass");
    let own = BedMapping {
        tile: 300.0,
        ..Default::default()
    };
    add(&mut ed, bed_shape(Some(grass), Some(own), 0.0, 1000.0));
    let doc = ed.doc();
    let ix = SlotIndex::new(doc);
    let obj = doc.object(site_id).unwrap();
    assert!(!ix.is_empty());
    assert_eq!(ix.beds_of(site_id).len(), 1);
    assert_eq!(ix.resolve(obj, PART_BED), Some(grass));
    assert_eq!(ix.resolve(obj, PART_MAIN), None);
    assert_eq!(
        ix.mapping_override(obj, PART_BED).unwrap().scale,
        [300.0; 2]
    );
    assert_eq!(ix.mapping_override(obj, PART_BED + 1), None);
    assert_eq!(ix.mapping_override(obj, PART_MAIN), None);
    assert!(ix.beds_of(ObjectId::from_num(999)).is_empty());
    // The elevation look has vertex colors, but a bed still has its material.
    let mut ed2 = Editor::with_sequential_ids();
    let site2 = site(
        &mut ed2,
        SiteSettings {
            surface_look: SurfaceLook::Elevation,
            ..Default::default()
        },
    );
    let g2 = material(&mut ed2, "Grass");
    add(&mut ed2, bed_shape(Some(g2), None, 0.0, 1000.0));
    let doc2 = ed2.doc();
    let ix2 = SlotIndex::new(doc2);
    let o2 = doc2.object(site2).unwrap();
    assert_eq!(ix2.resolve(o2, PART_MAIN), None);
    assert_eq!(ix2.resolve(o2, PART_BED), Some(g2));
    // The texture look of the site model: base part uses the site mapping, the bed uses its own.
    assert_eq!(
        crate::present::part_slot(o2, PART_BED + 3).as_deref(),
        Some("bed:3")
    );
    assert_eq!(crate::present::part_slot(o2, PART_MAIN), None);
}

#[test]
fn a_moved_mapping_stays_small_and_shows_the_same_texture() {
    use crate::present::texture_uv_f64;
    let m = BedMapping {
        tile: 700.0,
        offset: [100.0, 50.0],
        rotation: 30.0,
    };
    let d = DVec2::new(123_456.0, -98_765.0);
    let moved = m.translated(d);
    assert!(
        moved.offset.iter().all(|v| v.abs() < 1000.0),
        "{:?}",
        moved.offset
    );
    // The texture of the moved ground is the texture of the old ground: the same fraction of a
    // repeat at every point.
    for q in [DVec2::new(5.0, 9.0), DVec2::new(-4000.0, 777.0)] {
        let old = texture_uv_f64(
            &m.mapping(),
            [q.x, q.y, 0.0],
            [0.0, 0.0, 1.0],
            [0.0; 3],
            1.0,
        );
        let new = texture_uv_f64(
            &moved.mapping(),
            [q.x + d.x, q.y + d.y, 0.0],
            [0.0, 0.0, 1.0],
            [0.0; 3],
            1.0,
        );
        for k in 0..2 {
            let diff = (new[k] - old[k]).rem_euclid(1.0);
            assert!(diff < 1e-6 || diff > 1.0 - 1e-6, "{diff}");
        }
    }
    // Moving again and again keeps the offset in range.
    let mut again = m;
    for _ in 0..1000 {
        again = again.translated(DVec2::new(9.0e5, 1.1e5));
    }
    assert!(again.offset.iter().all(|v| v.abs() < 1000.0));
    // A mapping that cannot wrap is only shifted.
    let odd = BedMapping { tile: 0.0, ..m }.translated(DVec2::X);
    assert_eq!(odd.offset, [101.0, 50.0]);
}

#[test]
fn rotate_mirror_and_scale_are_refused_for_beds_and_a_move_is_not() {
    let mut ed = Editor::with_sequential_ids();
    for mapping in [None, Some(BedMapping::default())] {
        let id = add(&mut ed, bed_shape(None, mapping, 0.0, 1000.0));
        let before = ed.doc().object(id).unwrap().shape.clone();
        for transform in [
            serde_json::json!({"type": "rotate", "center": [0, 0], "angle": 0.5}),
            serde_json::json!({"type": "scale", "center": [0, 0], "sx": 2.0}),
            serde_json::json!({"type": "mirror", "a": [0, 0], "b": [1, 0]}),
        ] {
            let cmd: Command = serde_json::from_value(
                serde_json::json!({"op": "transform_objects", "ids": [id], "transform": transform}),
            )
            .unwrap();
            let err = ed.execute(&cmd).unwrap_err().to_string();
            assert!(err.contains("site_modifier"), "{err}");
            assert_eq!(ed.doc().object(id).unwrap().shape, before);
        }
        let cmd: Command = serde_json::from_value(serde_json::json!(
            {"op": "transform_objects", "ids": [id], "transform": {"type": "move", "delta": [5, 6]}}
        ))
        .unwrap();
        ed.execute(&cmd).unwrap();
        assert_ne!(ed.doc().object(id).unwrap().shape, before);
    }
}

#[test]
fn a_file_with_too_many_boundary_points_is_thinned_and_opens() {
    let mut ed = Editor::with_sequential_ids();
    let id = add(&mut ed, bed_shape(None, None, 0.0, 1000.0));
    let mut doc = ed.doc().clone();
    if let Shape::SiteModifier { boundary, .. } = &mut doc.object_mut(id).unwrap().shape {
        *boundary = (0..3 * MAX_BED_VERTICES)
            .map(|i| {
                let a = i as f64 / (3 * MAX_BED_VERTICES) as f64 * std::f64::consts::TAU;
                Vertex::corner(DVec2::new(1000.0 * a.cos(), 1000.0 * a.sin()))
            })
            .collect();
    }
    let loaded = Document::from_json(&serde_json::to_string(&doc).unwrap()).expect("opens");
    let Shape::SiteModifier { boundary, .. } = &loaded.object(id).unwrap().shape else {
        panic!()
    };
    assert!(boundary.len() <= MAX_BED_VERTICES && boundary.len() > MAX_BED_VERTICES / 2);
    assert_eq!(loaded.object(id).unwrap().shape.problem(), None);
    assert!(loaded.load_notes()[0].contains("thinned"));
}

#[test]
fn a_bed_of_a_kind_that_this_version_does_not_know_is_kept_as_unknown() {
    let mut ed = Editor::with_sequential_ids();
    let id = add(&mut ed, bed_shape(None, None, 0.0, 1000.0));
    let mut json = serde_json::to_value(ed.doc()).unwrap();
    for o in json["objects"].as_array_mut().unwrap() {
        if o["id"] == serde_json::json!(id) {
            o["shape"]["modifier"] = serde_json::json!({"type": "shiny_bed", "gloss": 3});
        }
    }
    let loaded = Document::from_json(&json.to_string()).expect("opens");
    assert!(matches!(
        &loaded.object(id).unwrap().shape,
        Shape::Unknown { kind, .. } if kind == "site_modifier"
    ));
    // It saves back unchanged.
    let again = serde_json::to_value(&loaded).unwrap();
    let saved = again["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == serde_json::json!(id))
        .unwrap();
    assert_eq!(saved["shape"]["modifier"]["type"], "shiny_bed");
    // A known kind with bad fields is still an error.
    let mut bad = serde_json::to_value(ed.doc()).unwrap();
    for o in bad["objects"].as_array_mut().unwrap() {
        if o["id"] == serde_json::json!(id) {
            o["shape"]["modifier"]["mapping"] = serde_json::json!("x");
        }
    }
    assert!(Document::from_json(&bad.to_string()).is_err());
}

#[test]
fn a_purge_keeps_the_material_of_a_bed_and_of_a_textured_site() {
    use crate::library::{purge_plan, usage};
    let mut ed = Editor::with_sequential_ids();
    let used_by_bed = material(&mut ed, "Bed");
    let used_by_site = material(&mut ed, "Site");
    let spare = material(&mut ed, "Spare");
    add(&mut ed, bed_shape(Some(used_by_bed), None, 0.0, 1000.0));
    site(
        &mut ed,
        SiteSettings {
            surface_look: SurfaceLook::Texture,
            texture: Some(used_by_site),
            ..Default::default()
        },
    );
    assert_eq!(usage(ed.doc(), used_by_bed).objects.len(), 1);
    assert_eq!(usage(ed.doc(), used_by_site).objects.len(), 1);
    assert_eq!(purge_plan(ed.doc(), &["material"]), vec![spare]);
    let listed = crate::library::list(ed.doc(), None, None);
    let used = |id| listed.iter().find(|r| r.id == id).unwrap().used_by;
    assert_eq!(
        (used(used_by_bed), used(used_by_site), used(spare)),
        (1, 1, 0)
    );
}

#[test]
fn many_arc_heavy_beds_are_cut_to_64_and_flattened_within_bounds() {
    let mut ed = Editor::with_sequential_ids();
    let site_id = site(&mut ed, SiteSettings::default());
    let grass = material(&mut ed, "Grass");
    // 70 beds of 4000 large arcs: flattened at 2 mm each would be millions of points.
    let big_arcs = |k: usize| -> Vec<Vertex> {
        (0..4000)
            .map(|i| {
                let a = i as f64 / 4000.0 * std::f64::consts::TAU;
                Vertex {
                    p: DVec2::new(
                        5000.0 + 4000.0 * a.cos() + k as f64,
                        5000.0 + 4000.0 * a.sin(),
                    ),
                    kind: VertexKind::Arc { radius: 1.0e7 },
                }
            })
            .collect()
    };
    for k in 0..70 {
        add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::TextureBed {
                    texture: Some(grass),
                    mapping: None,
                },
                boundary: big_arcs(k),
            },
        );
    }
    let doc = ed.doc();
    let list = beds_for(doc, doc.object(site_id).unwrap());
    assert_eq!(list.len(), MAX_TEXTURE_BEDS);
    assert!(
        list.iter()
            .all(|b| b.outline.len() >= 3 && b.outline.len() <= MAX_BED_EDGES)
    );
    // The outline itself: the limit holds for one huge boundary, whatever the arcs ask for.
    let o = outline(&big_arcs(0));
    assert!(!o.is_empty() && o.len() <= MAX_BED_EDGES, "{}", o.len());
    // Beds that are not displayed are not flattened and do not count toward the 64.
    assert!(
        beds_for(doc, doc.object(site_id).unwrap())
            .iter()
            .all(|b| doc.is_displayed(doc.object(b.object).unwrap()))
    );
}
