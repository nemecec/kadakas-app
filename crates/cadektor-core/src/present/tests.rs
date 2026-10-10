use super::*;
use crate::command::{Command, CommandError, Editor};
use crate::hardscape::HardscapePatch;
use crate::hardscape::style::{Component, HardscapeStyle};
use crate::mesh::build;
use crate::model::{Attributes, NONE_CLASS, Vertex};
use crate::plants::style::PlantStyle;
use glam::DVec2;

fn add_material(ed: &mut Editor, name: &str, m: Material) -> ResourceId {
    let out = ed
        .execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::Render(Box::new(RenderResource::Material(m))),
        })
        .unwrap();
    ResourceId(out.created[0])
}

fn add_shape(ed: &mut Editor, shape: Shape) -> ObjectId {
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

fn plant(at: f64) -> Shape {
    Shape::Plant {
        at: DVec2::new(at, 0.0),
        species: "Oak".into(),
        spread: 4000.0,
        height: 8000.0,
    }
}

fn mapping(kind: MappingKind) -> TextureMapping {
    TextureMapping {
        kind,
        scale: [1000.0, 500.0],
        rotation: 0.0,
        offset: [0.0, 0.0],
    }
}

#[test]
fn planar_uv_uses_world_units() {
    let m = mapping(MappingKind::Planar);
    let uv = texture_uv(&m, [500.0, 250.0, 9999.0], [0.0, 0.0, 1.0], [0.0; 3], 1.0);
    assert_eq!(uv, [0.5, 0.5]);
    // Planar ignores the normal and the height.
    let side = texture_uv(&m, [500.0, 250.0, 0.0], [1.0, 0.0, 0.0], [0.0; 3], 1.0);
    assert_eq!(uv, side);
    // Two points one scale apart land one texture apart.
    let a = texture_uv(&m, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3], 1.0);
    let b = texture_uv(&m, [1000.0, 500.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3], 1.0);
    assert_eq!([b[0] - a[0], b[1] - a[1]], [1.0, 1.0]);
}

#[test]
fn rotation_and_offset_turn_and_shift_the_texture() {
    let m = TextureMapping {
        kind: MappingKind::Planar,
        scale: [1000.0, 1000.0],
        rotation: 90.0,
        offset: [0.0, 0.0],
    };
    // Turned counterclockwise by 90 degrees, the texture U axis points along world +Y.
    let uv = texture_uv(&m, [0.0, 500.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3], 1.0);
    assert!((uv[0] - 0.5).abs() < 1e-6 && uv[1].abs() < 1e-6, "{uv:?}");
    let shifted = TextureMapping {
        offset: [250.0, 0.0],
        rotation: 0.0,
        ..m
    };
    let uv = texture_uv(&shifted, [250.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3], 1.0);
    assert_eq!(uv, [0.0, 0.0]);
}

#[test]
fn box_uv_follows_the_dominant_normal() {
    let m = mapping(MappingKind::Box);
    let p = [300.0, 200.0, 100.0];
    let top = texture_uv(&m, p, [0.1, 0.0, 0.9], [0.0; 3], 1.0);
    assert_eq!(top, [0.3, 0.4]);
    let east = texture_uv(&m, p, [1.0, 0.1, 0.0], [0.0; 3], 1.0);
    assert_eq!(east, [0.2, 0.2]);
    let north = texture_uv(&m, p, [0.0, -1.0, 0.0], [0.0; 3], 1.0);
    assert_eq!(north, [0.3, 0.2]);
}

#[test]
fn cylindrical_uv_runs_around_the_axis() {
    let m = TextureMapping {
        kind: MappingKind::Cylindrical,
        scale: [1000.0, 1000.0],
        ..Default::default()
    };
    let c = [100.0, 100.0, 0.0];
    let r = 1000.0;
    // A quarter turn is a quarter of the circumference at the radius of the object.
    let east = texture_uv(&m, [1100.0, 100.0, 0.0], [1.0, 0.0, 0.0], c, r);
    let north = texture_uv(&m, [100.0, 1100.0, 0.0], [0.0, 1.0, 0.0], c, r);
    let quarter = std::f64::consts::FRAC_PI_2 as f32 * 1.0;
    assert!(east[0].abs() < 1e-6);
    assert!((north[0] - quarter).abs() < 1e-5, "{north:?}");
    // Height maps to V.
    let up = texture_uv(&m, [1100.0, 100.0, 2000.0], [1.0, 0.0, 0.0], c, r);
    assert_eq!(up[1], 2.0);
    // A radius below one mm does not divide by zero or collapse.
    assert!(texture_uv(&m, [5.0, 5.0, 0.0], [0.0; 3], c, 0.0)[0].is_finite());
}

#[test]
fn mapping_and_mode_names() {
    for k in [
        MappingKind::Planar,
        MappingKind::Box,
        MappingKind::Cylindrical,
    ] {
        assert_eq!(MappingKind::from_name(k.id()), Some(k));
    }
    assert_eq!(
        MappingKind::from_name("Cylinder"),
        Some(MappingKind::Cylindrical)
    );
    assert_eq!(MappingKind::from_name("cube"), None);
    for p in Procedural::ALL {
        assert_eq!(Procedural::from_name(p.id()), Some(p));
    }
    assert_eq!(Procedural::from_name("marble"), None);
    for m in RenderMode::ALL {
        assert_eq!(RenderMode::from_name(m.id()), Some(m));
        assert_eq!(RenderMode::from_name(m.label()), Some(m));
    }
    assert_eq!(RenderMode::from_name("OpenGL"), Some(RenderMode::Shaded));
    assert_eq!(
        RenderMode::from_name("dashed-hidden-line"),
        Some(RenderMode::DashedHiddenLine)
    );
    assert_eq!(RenderMode::from_name("sketch"), None);
    assert_eq!(RenderMode::Shaded.toggled(), RenderMode::Wireframe);
    assert_eq!(RenderMode::HiddenLine.toggled(), RenderMode::Shaded);
    assert!(RenderMode::HiddenLine.is_hidden_line() && !RenderMode::Shaded.is_hidden_line());
}

#[test]
fn material_validation() {
    assert!(Material::default().validate().is_ok());
    for bad in [
        Material {
            roughness: 1.5,
            ..Default::default()
        },
        Material {
            metallic: -0.1,
            ..Default::default()
        },
        Material {
            opacity: f32::NAN,
            ..Default::default()
        },
        Material {
            mapping: TextureMapping {
                scale: [0.0, 1.0],
                ..Default::default()
            },
            ..Default::default()
        },
        Material {
            mapping: TextureMapping {
                rotation: f64::INFINITY,
                ..Default::default()
            },
            ..Default::default()
        },
        Material {
            texture: Some(Texture::Image {
                png_base64: "x".into(),
                width: 0,
                height: 4,
            }),
            ..Default::default()
        },
        Material {
            texture: Some(Texture::Image {
                png_base64: "x".repeat(MAX_TEXTURE_BASE64 + 1),
                width: 4,
                height: 4,
            }),
            ..Default::default()
        },
    ] {
        assert!(bad.validate().is_err(), "{bad:?}");
    }
    assert!(
        Material {
            opacity: 0.5,
            ..Default::default()
        }
        .is_transparent()
    );
    assert!(!Material::default().is_transparent());
}

#[test]
fn default_library_is_valid_and_complete() {
    let lib = default_library();
    let names: Vec<_> = lib.iter().map(|(n, _)| *n).collect();
    for n in [
        "Grass",
        "Gravel",
        "Concrete pavers",
        "Bark mulch",
        "Water",
        "Brick",
        "Timber",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    for (n, m) in &lib {
        assert!(m.validate().is_ok(), "{n}");
        assert!(matches!(m.texture, Some(Texture::Procedural { .. })), "{n}");
    }
    let water = &lib.iter().find(|(n, _)| *n == "Water").unwrap().1;
    assert!(water.is_transparent() && water.roughness < 0.2);
}

#[test]
fn render_styles_validate_and_presets_cover_the_modes() {
    assert!(RenderStyle::default().validate().is_ok());
    assert!(
        RenderStyle {
            line_weight: -1.0,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        RenderStyle {
            line_weight: f64::NAN,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    let presets = RenderStyle::presets();
    for mode in RenderMode::ALL {
        assert!(presets.iter().any(|(_, s)| s.mode == mode), "{mode:?}");
    }
    assert!(presets.iter().any(|(_, s)| s.shadows && s.sun));
}

#[test]
fn resources_round_trip_through_json_and_unknown_content_is_kept() {
    let mut ed = Editor::default();
    let m = Material {
        texture: Some(Texture::Procedural {
            kind: Procedural::Brick,
            seed: 3,
        }),
        ..Default::default()
    };
    add_material(&mut ed, "Wall", m.clone());
    ed.execute(&Command::AddResource {
        name: "Print".into(),
        body: ResourceBody::Render(Box::new(
            RenderResource::RenderStyle(RenderStyle::default()),
        )),
    })
    .unwrap();
    let text = serde_json::to_string(ed.doc()).unwrap();
    let back = crate::model::Document::from_json(&text).unwrap();
    assert_eq!(&back, ed.doc());
    assert!(text.contains("\"type\":\"material\"") && text.contains("\"type\":\"render_style\""));
    // A material with a field of the wrong type is kept as raw JSON.
    let bad = serde_json::json!({"type": "material", "opacity": "lots"});
    let body: ResourceBody = serde_json::from_value(bad.clone()).unwrap();
    assert_eq!(body, ResourceBody::Unknown(bad));
    assert!(RenderResource::from_json(&serde_json::json!({"type": "hatch"})).is_none());
    assert!(RenderResource::from_json(&serde_json::json!({"x": 1})).is_none());
}

#[test]
fn commands_set_slots_the_sun_and_the_style_with_undo() {
    let mut ed = Editor::default();
    let grass = add_material(&mut ed, "Grass", Material::default());
    let id = add_shape(&mut ed, plant(0.0));
    let target = SlotTarget::Object {
        object: id,
        slot: Some("trunk".into()),
    };
    ed.execute(&Command::SetMaterialSlot {
        target: target.clone(),
        material: Some(grass),
    })
    .unwrap();
    assert_eq!(ed.doc().drawing.presentation.slot(&target), Some(grass));
    // Setting the same target again replaces it. `None` clears it.
    ed.execute(&Command::SetMaterialSlot {
        target: target.clone(),
        material: None,
    })
    .unwrap();
    assert!(ed.doc().drawing.presentation.slots.is_empty());
    ed.undo();
    assert_eq!(ed.doc().drawing.presentation.slot(&target), Some(grass));

    let sun = SunSettings {
        hour: 15.0,
        ..Default::default()
    };
    ed.execute(&Command::SetSun { sun: Some(sun) }).unwrap();
    assert_eq!(ed.doc().drawing.presentation.sun, Some(sun));
    ed.undo();
    assert_eq!(ed.doc().drawing.presentation.sun, None);
    assert!(
        ed.execute(&Command::SetSun {
            sun: Some(SunSettings {
                month: 13,
                ..Default::default()
            })
        })
        .is_err()
    );
    assert!(
        ed.execute(&Command::SetSun {
            sun: Some(SunSettings {
                north_angle: f64::NAN,
                ..Default::default()
            })
        })
        .is_err()
    );

    // Render style: must exist and be a render style.
    assert_eq!(
        ed.execute(&Command::SetRenderStyle {
            style: Some(ResourceId(999))
        }),
        Err(CommandError::NoResource(ResourceId(999)))
    );
    assert!(
        ed.execute(&Command::SetRenderStyle { style: Some(grass) })
            .is_err()
    );
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Plan".into(),
            body: ResourceBody::Render(Box::new(RenderResource::RenderStyle(
                RenderStyle::default(),
            ))),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetRenderStyle { style: Some(style) })
        .unwrap();
    assert_eq!(ed.doc().drawing.presentation.active_style, Some(style));
    // Deleting the style forgets it, and undo brings it back.
    ed.execute(&Command::DeleteResource {
        id: style,
        reassign: None,
    })
    .unwrap();
    assert_eq!(ed.doc().drawing.presentation.active_style, None);
    ed.undo();
    assert_eq!(ed.doc().drawing.presentation.active_style, Some(style));
}

#[test]
fn slot_commands_check_their_targets() {
    let mut ed = Editor::default();
    let grass = add_material(&mut ed, "Grass", Material::default());
    let id = add_shape(&mut ed, plant(0.0));
    let set = |ed: &mut Editor, target: SlotTarget, m: Option<ResourceId>| {
        ed.execute(&Command::SetMaterialSlot {
            target,
            material: m,
        })
    };
    assert_eq!(
        set(
            &mut ed,
            SlotTarget::Object {
                object: ObjectId(777),
                slot: None
            },
            Some(grass)
        ),
        Err(CommandError::NoObject(ObjectId(777)))
    );
    assert!(
        set(
            &mut ed,
            SlotTarget::Object {
                object: id,
                slot: Some(" ".into())
            },
            Some(grass)
        )
        .is_err()
    );
    assert_eq!(
        set(
            &mut ed,
            SlotTarget::Class { class: ClassId(55) },
            Some(grass)
        ),
        Err(CommandError::NoClass(ClassId(55)))
    );
    assert_eq!(
        set(
            &mut ed,
            SlotTarget::Class { class: NONE_CLASS },
            Some(ResourceId(888))
        ),
        Err(CommandError::NoResource(ResourceId(888)))
    );
    // A resource of another kind is not a material.
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Oak".into(),
            body: ResourceBody::plant_style(PlantStyle::default()),
        })
        .unwrap()
        .created[0],
    );
    assert!(
        set(
            &mut ed,
            SlotTarget::Class { class: NONE_CLASS },
            Some(style)
        )
        .is_err()
    );
    assert_eq!(
        set(
            &mut ed,
            SlotTarget::Style {
                style: ResourceId(4242),
                slot: "trunk".into()
            },
            Some(grass)
        ),
        Err(CommandError::NoResource(ResourceId(4242)))
    );
    assert!(
        set(
            &mut ed,
            SlotTarget::Style {
                style,
                slot: "".into()
            },
            Some(grass)
        )
        .is_err()
    );
    set(
        &mut ed,
        SlotTarget::Style {
            style,
            slot: "canopy".into(),
        },
        Some(grass),
    )
    .unwrap();
    // Deleting the material removes its slots.
    ed.execute(&Command::DeleteResource {
        id: grass,
        reassign: None,
    })
    .unwrap();
    assert!(ed.doc().drawing.presentation.slots.is_empty());
    // Deleting the plant style removes the slots of the style.
    let m2 = add_material(&mut ed, "M2", Material::default());
    set(
        &mut ed,
        SlotTarget::Style {
            style,
            slot: "canopy".into(),
        },
        Some(m2),
    )
    .unwrap();
    let mut p = ed.doc().drawing.presentation.clone();
    p.forget(style);
    assert!(p.slots.is_empty());
    p.forget_object(id);
}

#[test]
fn material_resolution_order_is_part_object_style_class() {
    let mut ed = Editor::default();
    let [m_part, m_obj, m_style, m_class] =
        ["part", "object", "style", "class"].map(|n| add_material(&mut ed, n, Material::default()));
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Oak".into(),
            body: ResourceBody::plant_style(PlantStyle::default()),
        })
        .unwrap()
        .created[0],
    );
    let id = add_shape(&mut ed, plant(0.0));
    ed.execute(&Command::SetPlantStyle {
        ids: vec![id],
        style: Some(style),
        overrides: None,
        unlink: false,
    })
    .unwrap();
    let set = |ed: &mut Editor, target, m| {
        ed.execute(&Command::SetMaterialSlot {
            target,
            material: Some(m),
        })
        .unwrap();
    };
    let resolve = |ed: &Editor, part| {
        let doc = ed.doc();
        SlotIndex::new(doc).resolve(doc.object(id).unwrap(), part)
    };
    assert!(SlotIndex::new(ed.doc()).is_empty());
    assert_eq!(resolve(&ed, PART_TRUNK), None);
    // 5. The class.
    set(&mut ed, SlotTarget::Class { class: NONE_CLASS }, m_class);
    assert_eq!(resolve(&ed, PART_TRUNK), Some(m_class));
    // 4. The style slot beats the class, and only for its own part.
    set(
        &mut ed,
        SlotTarget::Style {
            style,
            slot: "trunk".into(),
        },
        m_style,
    );
    assert_eq!(resolve(&ed, PART_TRUNK), Some(m_style));
    assert_eq!(resolve(&ed, PART_CANOPY), Some(m_class));
    // 3. The whole object beats the style.
    set(
        &mut ed,
        SlotTarget::Object {
            object: id,
            slot: None,
        },
        m_obj,
    );
    assert_eq!(resolve(&ed, PART_TRUNK), Some(m_obj));
    assert_eq!(resolve(&ed, PART_CANOPY), Some(m_obj));
    // 1. The part of the object beats everything.
    set(
        &mut ed,
        SlotTarget::Object {
            object: id,
            slot: Some("trunk".into()),
        },
        m_part,
    );
    assert_eq!(resolve(&ed, PART_TRUNK), Some(m_part));
    assert_eq!(resolve(&ed, PART_CANOPY), Some(m_obj));
    // A slot whose material is gone (not a material resource) is skipped.
    let mut doc = ed.doc().clone();
    doc.drawing.presentation.set_slot(
        SlotTarget::Object {
            object: id,
            slot: Some("canopy".into()),
        },
        Some(ResourceId(31337)),
    );
    assert_eq!(
        SlotIndex::new(&doc).resolve(doc.object(id).unwrap(), PART_CANOPY),
        Some(m_obj)
    );
}

#[test]
fn plants_have_trunk_and_canopy_parts_in_the_mesh() {
    let mut ed = Editor::default();
    let id = add_shape(&mut ed, plant(0.0));
    let rect = add_shape(
        &mut ed,
        Shape::Rect {
            min: DVec2::new(9000.0, 0.0),
            max: DVec2::new(10000.0, 500.0),
        },
    );
    let scene = build(ed.doc());
    let m = &scene.mesh;
    assert_eq!(m.tri_part.len(), m.triangle_count());
    let parts_of = |obj: ObjectId| -> std::collections::BTreeSet<u8> {
        (0..m.triangle_count())
            .filter(|t| m.tri_object[*t] == obj)
            .map(|t| m.tri_part[t])
            .collect()
    };
    assert_eq!(
        parts_of(id),
        [PART_TRUNK, PART_CANOPY].into_iter().collect()
    );
    assert_eq!(parts_of(rect), [PART_MAIN].into_iter().collect());
    let obj = ed.doc().object(id).unwrap();
    assert_eq!(part_slot(obj, PART_TRUNK).as_deref(), Some("trunk"));
    assert_eq!(part_slot(obj, PART_CANOPY).as_deref(), Some("canopy"));
    assert_eq!(part_slot(obj, PART_MAIN), None);
    assert_eq!(part_slot(ed.doc().object(rect).unwrap(), PART_TRUNK), None);
}

#[test]
fn hardscape_layers_have_parts_and_slots() {
    let mut ed = Editor::default();
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Paving".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                thickness: 60.0,
                components: vec![Component {
                    thickness: 100.0,
                    ..Default::default()
                }],
                ..Default::default()
            })),
        })
        .unwrap()
        .created[0],
    );
    let ring = [
        DVec2::ZERO,
        DVec2::new(2000.0, 0.0),
        DVec2::new(2000.0, 1000.0),
        DVec2::new(0.0, 1000.0),
    ];
    let out = ed
        .execute(&Command::AddHardscape {
            boundary: Some(ring.iter().map(|p| Vertex::corner(*p)).collect()),
            patch: Box::new(HardscapePatch {
                style: Some(style),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let id = ObjectId(out.created[0]);
    let scene = build(ed.doc());
    let parts: std::collections::BTreeSet<u8> = (0..scene.mesh.triangle_count())
        .filter(|t| scene.mesh.tri_object[*t] == id)
        .map(|t| scene.mesh.tri_part[t])
        .collect();
    assert_eq!(parts, [PART_LAYER, PART_LAYER + 1].into_iter().collect());
    let obj = ed.doc().object(id).unwrap();
    assert_eq!(part_slot(obj, PART_LAYER).as_deref(), Some("surface"));
    assert_eq!(
        part_slot(obj, PART_LAYER + 1).as_deref(),
        Some("component:0")
    );
    // A material on the style's `surface` slot reaches the object.
    let m = add_material(&mut ed, "Pavers", Material::default());
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Style {
            style,
            slot: "surface".into(),
        },
        material: Some(m),
    })
    .unwrap();
    let doc = ed.doc();
    let ix = SlotIndex::new(doc);
    assert_eq!(ix.resolve(doc.object(id).unwrap(), PART_LAYER), Some(m));
    assert_eq!(ix.resolve(doc.object(id).unwrap(), PART_LAYER + 1), None);
}

#[test]
fn sun_settings_give_a_light_direction_only_when_the_sun_is_up() {
    let noon = SunSettings::default();
    let d = noon.light_direction().expect("midsummer noon");
    assert!(d[2] > 0.5, "{d:?}");
    assert!(noon.validate().is_ok());
    let night = SunSettings { hour: 0.0, ..noon };
    assert!(night.light_direction().is_none());
    assert!(night.position().elevation < 0.0);
    assert!(
        SunSettings {
            latitude: 100.0,
            ..noon
        }
        .validate()
        .is_err()
    );
    // The north angle turns the light around the vertical axis.
    let turned = SunSettings {
        north_angle: 90.0,
        ..noon
    }
    .light_direction()
    .unwrap();
    assert!((turned[2] - d[2]).abs() < 1e-9);
    assert!((turned[0] - d[0]).abs() > 0.1);
}

#[test]
fn slots_follow_copies_and_deletes_and_undo_restores_them() {
    let mut ed = Editor::default();
    let m = add_material(&mut ed, "M", Material::default());
    let id = add_shape(&mut ed, plant(0.0));
    for slot in [None, Some("canopy".to_string())] {
        ed.execute(&Command::SetMaterialSlot {
            target: SlotTarget::Object { object: id, slot },
            material: Some(m),
        })
        .unwrap();
    }
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![id],
            delta: DVec2::new(100.0, 0.0),
        })
        .unwrap();
    let copy = ObjectId(out.created[0]);
    let p = &ed.doc().drawing.presentation;
    assert_eq!(p.slots.len(), 4);
    assert_eq!(
        p.slot(&SlotTarget::Object {
            object: copy,
            slot: None
        }),
        Some(m)
    );
    assert_eq!(
        p.slot(&SlotTarget::Object {
            object: copy,
            slot: Some("canopy".into())
        }),
        Some(m)
    );
    // Deleting the original removes only its slots.
    ed.execute(&Command::DeleteObjects { ids: vec![id] })
        .unwrap();
    let p = &ed.doc().drawing.presentation;
    assert_eq!(p.slots.len(), 2);
    assert!(
        p.slots
            .iter()
            .all(|e| matches!(&e.target, SlotTarget::Object { object, .. } if *object == copy))
    );
    // Undo brings the object and its slots back; undoing the copy removes the copy's.
    assert!(ed.undo());
    assert_eq!(ed.doc().drawing.presentation.slots.len(), 4);
    assert!(ed.undo());
    assert_eq!(ed.doc().drawing.presentation.slots.len(), 2);
}

#[test]
fn every_plant_form_has_a_trunk_part_and_a_canopy_part() {
    use crate::plants::style::PlantForm;
    for form in [
        PlantForm::Tree,
        PlantForm::Columnar,
        PlantForm::Conical,
        PlantForm::Spreading,
        PlantForm::ShrubMound,
    ] {
        let mut ed = Editor::default();
        let style = ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: ResourceBody::plant_style(PlantStyle {
                    form,
                    ..PlantStyle::default()
                }),
            })
            .unwrap()
            .created[0],
        );
        let out = ed
            .execute(&Command::AddPlants {
                style: Some(style),
                overrides: Default::default(),
                positions: vec![DVec2::ZERO],
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        let id = ObjectId(out.created[0]);
        let m = &build(ed.doc()).mesh;
        let parts: std::collections::BTreeSet<u8> = (0..m.triangle_count())
            .filter(|t| m.tri_object[*t] == id)
            .map(|t| m.tri_part[t])
            .collect();
        assert_eq!(
            parts,
            [PART_TRUNK, PART_CANOPY].into_iter().collect(),
            "{form:?}"
        );
    }
}

#[test]
fn summer_time_moves_the_sun_one_hour_on_the_clock() {
    let winter = SunSettings {
        latitude: 60.17,
        longitude: 24.94,
        tz_offset: 2.0,
        hour: 12.0,
        ..Default::default()
    };
    let summer = SunSettings {
        dst: true,
        hour: 13.0,
        ..winter
    };
    let (a, b) = (winter.position(), summer.position());
    assert!((a.azimuth - b.azimuth).abs() < 1e-9 && (a.elevation - b.elevation).abs() < 1e-9);
    // The same clock time is one hour earlier in the sun's day with summer time.
    let same_clock = SunSettings {
        dst: true,
        hour: 12.0,
        ..winter
    };
    assert!(same_clock.position().azimuth < a.azimuth);
}
