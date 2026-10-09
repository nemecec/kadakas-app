use super::*;
use crate::curve;
use crate::hardscape::{Component, HardscapeStyle, Mode, Slope};
use crate::landscape::{LandscapeAreaStyle, MixEntry};
use crate::plants::style::PlantStyle;

fn sq(w: f64) -> Vec<Vertex> {
    curve::corners(&[DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, w), DVec2::new(0.0, w)])
}

fn line_center() -> Vec<Vertex> {
    curve::corners(&[DVec2::ZERO, DVec2::new(5000.0, 0.0)])
}

fn add_hardscape(patch: HardscapePatch, boundary: Option<Vec<Vertex>>) -> Command {
    Command::AddHardscape { boundary, patch: Box::new(patch), layer: None, class: None, attrs: Attributes::default() }
}

fn hs_style(ed: &mut Editor, name: &str, thickness: f64) -> ResourceId {
    let s = HardscapeStyle { thickness, components: vec![Component { thickness: 100.0, ..Default::default() }], ..Default::default() };
    ResourceId(ed.execute(&Command::AddResource { name: name.into(), body: ResourceBody::HardscapeStyle(Box::new(s)) }).unwrap().created[0])
}

fn id_of(out: Outcome) -> ObjectId {
    ObjectId(out.created[0])
}

#[test]
fn add_hardscape_makes_slabs_and_paths_in_one_step() {
    let mut ed = Editor::default();
    let before = ed.doc().clone();
    let slab = id_of(ed.execute(&add_hardscape(HardscapePatch { elevation: Some(40.0), material: Some("Slate".into()), ..Default::default() }, Some(sq(2000.0)))).unwrap());
    let Shape::Hardscape { material, elevation, .. } = &ed.doc().object(slab).unwrap().shape else { panic!() };
    assert_eq!((material.as_str(), *elevation), ("Slate", Some(40.0)));
    let path = id_of(ed.execute(&add_hardscape(HardscapePatch { center: Some(line_center()), width: Some(1000.0), ..Default::default() }, None)).unwrap());
    assert_eq!(crate::hardscape::data(ed.doc().object(path).unwrap()).unwrap().mode, Mode::Path);
    assert!((ed.doc().object(path).unwrap().shape.area().unwrap() - 5.0e6).abs() < 1e-6);
    ed.undo();
    ed.undo();
    assert_eq!(ed.doc().objects, before.objects);
    ed.redo();
    ed.redo();
    assert_eq!(ed.doc().objects.len(), 2);
}

#[test]
fn add_hardscape_checks_its_input() {
    let mut ed = Editor::default();
    let sid = hs_style(&mut ed, "S", 50.0);
    let plant = ResourceId(ed.execute(&Command::AddResource { name: "P".into(), body: ResourceBody::plant_style(PlantStyle::default()) }).unwrap().created[0]);
    let none = HardscapePatch::default();
    let bad = |ed: &mut Editor, c: Command| ed.execute(&c).unwrap_err();
    assert!(bad(&mut ed, add_hardscape(none.clone(), None)).to_string().contains("boundary"));
    let both = HardscapePatch { center: Some(line_center()), ..Default::default() };
    assert!(bad(&mut ed, add_hardscape(both, Some(sq(10.0)))).to_string().contains("not both"));
    assert!(matches!(bad(&mut ed, add_hardscape(HardscapePatch { style: Some(ResourceId(999)), ..Default::default() }, Some(sq(10.0)))), CommandError::NoResource(_)));
    assert!(bad(&mut ed, add_hardscape(HardscapePatch { style: Some(plant), ..Default::default() }, Some(sq(10.0)))).to_string().contains("not a hardscape style"));
    assert!(bad(&mut ed, add_hardscape(none.clone(), Some(curve::corners(&[DVec2::ZERO, DVec2::X])))).to_string().contains("3 or more"));
    assert!(bad(&mut ed, add_hardscape(HardscapePatch { center: Some(curve::corners(&[DVec2::ZERO, DVec2::ZERO])), ..Default::default() }, None)).to_string().contains("no length"));
    let nan = Command::AddHardscape { boundary: Some(sq(10.0)), patch: Box::new(none.clone()), layer: None, class: None, attrs: Attributes { line_weight: Some(f64::NAN), ..Default::default() } };
    assert!(bad(&mut ed, nan).to_string().contains("finite"));
    let missing_layer = Command::AddHardscape { boundary: Some(sq(10.0)), patch: Box::new(none.clone()), layer: Some(LayerId(99)), class: None, attrs: Attributes::default() };
    assert!(matches!(bad(&mut ed, missing_layer), CommandError::NoLayer(_)));
    let missing_class = Command::AddHardscape { boundary: Some(sq(10.0)), patch: Box::new(none), layer: None, class: Some(ClassId(99)), attrs: Attributes::default() };
    assert!(matches!(bad(&mut ed, missing_class), CommandError::NoClass(_)));
    assert!(ed.doc().objects.is_empty());
    // A good style works.
    assert!(ed.execute(&add_hardscape(HardscapePatch { style: Some(sid), ..Default::default() }, Some(sq(1000.0)))).is_ok());
}

#[test]
fn set_hardscape_changes_many_or_none() {
    let mut ed = Editor::default();
    let sid = hs_style(&mut ed, "S", 50.0);
    let a = id_of(ed.execute(&add_hardscape(HardscapePatch::default(), Some(sq(1000.0)))).unwrap());
    let b = id_of(ed.execute(&add_hardscape(HardscapePatch::default(), Some(sq(2000.0)))).unwrap());
    let line = id_of(ed.execute(&Command::AddObject { shape: Shape::Line { a: DVec2::ZERO, b: DVec2::X }, layer: None, class: None, attrs: Attributes::default() }).unwrap());
    let before = ed.doc().clone();
    let set = |ids: Vec<ObjectId>, patch: HardscapePatch| Command::SetHardscape { ids, patch };
    // One bad object: nothing changes.
    let p = HardscapePatch { style: Some(sid), ..Default::default() };
    assert!(ed.execute(&set(vec![a, b, line], p.clone())).unwrap_err().to_string().contains("not a hardscape"));
    assert_eq!(ed.doc().objects, before.objects);
    assert!(ed.execute(&set(vec![a], HardscapePatch::default())).is_err());
    assert!(matches!(ed.execute(&set(vec![ObjectId(999)], p.clone())), Err(CommandError::NoObject(_))));
    assert!(matches!(ed.execute(&set(vec![a], HardscapePatch { style: Some(ResourceId(999)), ..Default::default() })), Err(CommandError::NoResource(_))));
    // Both change in one step, a duplicate ID counts once.
    ed.execute(&set(vec![a, b, a], p)).unwrap();
    for id in [a, b] {
        assert_eq!(crate::hardscape::data(ed.doc().object(id).unwrap()).unwrap().style, Some(sid));
    }
    ed.undo();
    assert_eq!(ed.doc().objects, before.objects);
    // A locked layer refuses.
    ed.execute(&Command::ModifyLayer { id: FIRST_LAYER, name: None, visible: None, locked: Some(true), elevation: None, scale: None }).unwrap();
    assert!(matches!(ed.execute(&set(vec![a], HardscapePatch { drape: Some(true), ..Default::default() })), Err(CommandError::LayerLocked(_))));
}

#[test]
fn a_path_outline_cannot_be_edited_directly() {
    let mut ed = Editor::default();
    let path = id_of(ed.execute(&add_hardscape(HardscapePatch { center: Some(line_center()), ..Default::default() }, None)).unwrap());
    let slab = id_of(ed.execute(&add_hardscape(HardscapePatch { slope: Some(Slope::None), ..Default::default() }, Some(sq(1000.0)))).unwrap());
    let err = ed.execute(&Command::SetVertex { id: path, index: 0, kind: None, p: Some(DVec2::new(5.0, 5.0)) }).unwrap_err();
    assert!(err.to_string().contains("center line"));
    let shape = Shape::Hardscape { boundary: sq(10.0), material: "x".into(), thickness: None, elevation: None };
    assert!(ed.execute(&Command::ModifyObject { id: path, shape: Some(shape), layer: None, class: None, attrs: None }).is_err());
    let other = Shape::Circle { center: DVec2::ZERO, radius: 5.0 };
    assert!(ed.execute(&Command::ModifyObject { id: path, shape: Some(other), layer: None, class: None, attrs: None }).is_err());
    // The same shape is fine (for example a change of attributes with the shape repeated).
    let same = ed.doc().object(path).unwrap().shape.clone();
    assert!(ed.execute(&Command::ModifyObject { id: path, shape: Some(same), layer: None, class: None, attrs: None }).is_ok());
    // A slab with settings can be edited as usual.
    assert!(ed.execute(&Command::SetVertex { id: slab, index: 0, kind: None, p: Some(DVec2::new(5.0, 5.0)) }).is_ok());
    // The reserved key cannot be written with set_data.
    let mut set = BTreeMap::new();
    set.insert("hardscape".to_string(), Value::Null);
    assert!(ed.execute(&Command::SetData { id: slab, set, remove: vec![] }).unwrap_err().to_string().contains("reserved"));
    assert!(ed.execute(&Command::SetData { id: slab, set: BTreeMap::new(), remove: vec!["landscape_area".into()] }).is_err());
}

#[test]
fn styles_follow_edits_and_reassign() {
    let mut ed = Editor::default();
    let a = hs_style(&mut ed, "A", 50.0);
    let b = hs_style(&mut ed, "B", 80.0);
    let h = id_of(ed.execute(&add_hardscape(HardscapePatch { style: Some(a), ..Default::default() }, Some(sq(1000.0)))).unwrap());
    let thickness = |ed: &Editor| match &ed.doc().object(h).unwrap().shape {
        Shape::Hardscape { thickness, material, .. } => (*thickness, material.clone()),
        _ => panic!(),
    };
    assert_eq!(thickness(&ed), (Some(150.0), "A".into()));
    // Editing the style updates the object in the same step.
    let before = ed.doc().clone();
    let new = HardscapeStyle { material: "Gneiss".into(), thickness: 70.0, ..Default::default() };
    ed.execute(&Command::ModifyResource { id: a, name: None, body: Some(ResourceBody::HardscapeStyle(Box::new(new))) }).unwrap();
    assert_eq!(thickness(&ed), (Some(70.0), "Gneiss".into()));
    ed.undo();
    assert_eq!(ed.doc().objects, before.objects);
    // In use: refused. Reassign moves the object.
    assert!(matches!(ed.execute(&Command::DeleteResource { id: a, reassign: None }), Err(CommandError::ResourceInUse(_, 1))));
    ed.execute(&Command::DeleteResource { id: a, reassign: Some(b) }).unwrap();
    assert_eq!(crate::hardscape::data(ed.doc().object(h).unwrap()).unwrap().style, Some(b));
    assert_eq!(thickness(&ed), (Some(180.0), "B".into()));
    // A style whose settings are invalid is refused.
    let bad = HardscapeStyle { thickness: -1.0, ..Default::default() };
    assert!(ed.execute(&Command::ModifyResource { id: b, name: None, body: Some(ResourceBody::HardscapeStyle(Box::new(bad))) }).is_err());
}

#[test]
fn add_landscape_area_with_settings() {
    let mut ed = Editor::default();
    let plant = ResourceId(ed.execute(&Command::AddResource { name: "P".into(), body: ResourceBody::plant_style(PlantStyle::default()) }).unwrap().created[0]);
    let mix = vec![MixEntry { plant, percent: 100.0, ..Default::default() }];
    let style = ResourceId(ed.execute(&Command::AddResource { name: "L".into(), body: ResourceBody::LandscapeAreaStyle(Box::new(LandscapeAreaStyle { mix, ..Default::default() })) }).unwrap().created[0]);
    let add = |boundary: Vec<Vertex>, patch: LandscapePatch| Command::AddLandscapeArea { boundary, name: "Bed".into(), patch: Box::new(patch), layer: None, class: None, attrs: Attributes::default() };
    let before = ed.doc().clone();
    // Without settings: a plain area.
    let plain = id_of(ed.execute(&add(sq(1000.0), LandscapePatch::default())).unwrap());
    assert!(crate::landscape::data(ed.doc().object(plain).unwrap()).is_none());
    let id = id_of(ed.execute(&add(sq(4000.0), LandscapePatch { style: Some(style), holes: Some(vec![sq(500.0)]), show_plants: Some(true), ..Default::default() })).unwrap());
    let d = crate::landscape::data(ed.doc().object(id).unwrap()).unwrap();
    assert_eq!((d.style, d.holes.len(), d.show_plants), (Some(style), 1, Some(true)));
    ed.undo();
    ed.undo();
    assert_eq!(ed.doc().objects, before.objects);
    // Errors.
    assert!(matches!(ed.execute(&add(sq(10.0), LandscapePatch { style: Some(ResourceId(999)), ..Default::default() })), Err(CommandError::NoResource(_))));
    assert!(ed.execute(&add(sq(10.0), LandscapePatch { style: Some(plant), ..Default::default() })).unwrap_err().to_string().contains("not a landscape area style"));
    assert!(ed.execute(&add(curve::corners(&[DVec2::ZERO, DVec2::X]), LandscapePatch::default())).is_err());
    assert!(ed.execute(&add(sq(10.0), LandscapePatch { holes: Some(vec![sq(500.0)]), ..Default::default() })).is_err(), "the hole is outside");
    let bad_attrs = Command::AddLandscapeArea { boundary: sq(10.0), name: "x".into(), patch: Box::default(), layer: None, class: None, attrs: Attributes { line_weight: Some(f64::NAN), ..Default::default() } };
    assert!(ed.execute(&bad_attrs).is_err());
    let bad_layer = Command::AddLandscapeArea { boundary: sq(10.0), name: "x".into(), patch: Box::default(), layer: Some(LayerId(99)), class: None, attrs: Attributes::default() };
    assert!(matches!(ed.execute(&bad_layer), Err(CommandError::NoLayer(_))));
    let bad_class = Command::AddLandscapeArea { boundary: sq(10.0), name: "x".into(), patch: Box::default(), layer: None, class: Some(ClassId(99)), attrs: Attributes::default() };
    assert!(matches!(ed.execute(&bad_class), Err(CommandError::NoClass(_))));
    assert!(ed.doc().objects.is_empty());
}

#[test]
fn relinking_plants_to_a_style_that_now_locks_a_field_drops_the_old_override() {
    let mut ed = Editor::default();
    let sid = ResourceId(ed.execute(&Command::AddResource { name: "P".into(), body: ResourceBody::plant_style(PlantStyle::default()) }).unwrap().created[0]);
    let mut overrides = serde_json::Map::new();
    overrides.insert("spread".into(), 1234.0.into());
    overrides.insert("price".into(), 7.0.into());
    let out = ed
        .execute(&Command::AddPlants { style: Some(sid), overrides, positions: vec![DVec2::ZERO], layer: None, class: None, variation: None })
        .unwrap();
    let plant = ObjectId(out.created[0]);
    // The style locks the spread afterwards.
    let locked = PlantStyle { locked: vec!["spread".into()], ..PlantStyle::default() };
    ed.execute(&Command::ModifyResource { id: sid, name: None, body: Some(ResourceBody::plant_style(locked)) }).unwrap();
    // Linking the plant to the same style again keeps the price override and drops the spread.
    ed.execute(&Command::SetPlantStyle { ids: vec![plant], style: Some(sid), overrides: None, unlink: false }).unwrap();
    let link = crate::plants::link::link(ed.doc().object(plant).unwrap()).unwrap();
    assert!(!link.overrides.contains_key("spread") && link.overrides.contains_key("price"));
    // Overrides that are given on purpose are still checked.
    let mut bad = serde_json::Map::new();
    bad.insert("spread".into(), 5.0.into());
    assert!(ed.execute(&Command::SetPlantStyle { ids: vec![plant], style: Some(sid), overrides: Some(bad), unlink: false }).is_err());
}
