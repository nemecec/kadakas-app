//! Tests of the IFC import. The fixtures are small files in `tests/fixtures/ifc`.

use cadektor_core::command::Editor;
use cadektor_core::model::{Document, Object, Shape};
use cadektor_core::record;
use cadektor_io::Report;
use glam::{DVec2, DVec3};
use serde_json::json;

use super::*;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/ifc/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn import_into(doc: Document, name: &str, opts: &IfcImportOptions) -> (Editor, Report) {
    let result = import_bytes(fixture(name), &doc, opts).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&result.command)
        .unwrap_or_else(|e| panic!("{name}: the batch failed: {e}"));
    (ed, result.report)
}

fn import(name: &str) -> (Editor, Report) {
    import_into(Document::default(), name, &IfcImportOptions::default())
}

fn objects(ed: &Editor) -> Vec<&Object> {
    ed.doc().objects.values().collect()
}

fn only(ed: &Editor) -> &Object {
    let o = objects(ed);
    assert_eq!(
        o.len(),
        1,
        "{:?}",
        o.iter().map(|o| o.shape.kind()).collect::<Vec<_>>()
    );
    o[0]
}

fn mesh(o: &Object) -> &cadektor_core::mesh3d::Mesh3d {
    match &o.shape {
        Shape::Mesh3d(m) => m,
        other => panic!("expected a mesh, got {}", other.kind()),
    }
}

#[test]
fn rectangle_slab_is_a_mesh_with_class_data_and_records() {
    let (ed, report) = import("extruded_rect.ifc");
    let o = only(&ed);
    let m = mesh(o);
    // 4 m x 2 m x 0.2 m.
    assert!(
        (m.volume() - 4000.0 * 2000.0 * 200.0).abs() < 1.0,
        "{}",
        m.volume()
    );
    let (lo, hi) = m.bounds3();
    // Placed at (1000, 500, 300) and centered on the profile.
    assert!(
        (lo - DVec3::new(-1000.0, -500.0, 300.0)).length() < 1e-6,
        "{lo}"
    );
    assert!(
        (hi - DVec3::new(3000.0, 1500.0, 500.0)).length() < 1e-6,
        "{hi}"
    );
    assert_eq!(m.name, "Terrace slab");
    assert_eq!(o.data["ifc_class"], "IfcSlab");
    assert_eq!(o.data["ifc"]["global_id"], "0000000000000000000107");
    assert_eq!(o.data["ifc"]["predefined_type"], "BASESLAB");
    assert_eq!(o.data["ifc"]["tag"], "S-1");
    // Nothing becomes a hardscape.
    assert_eq!(report.objects["mesh3d"], 1);
    assert!(!report.objects.contains_key("hardscape"));
    // Property sets and quantities are records.
    let pset = record::values(o, "Pset_SlabCommon").expect("record");
    assert_eq!(pset["Material"], "Granite");
    assert_eq!(pset["IsExternal"], true);
    assert_eq!(pset["Thickness"], 200.0);
    assert!((pset["ThermalTransmittance"].as_f64().unwrap() - 1.25).abs() < 1e-9);
    let qto = record::values(o, "Qto_SlabBaseQuantities").expect("record");
    assert_eq!(qto["NetArea"], 8.0);
    assert_eq!(qto["Width"], 2000.0);
    let format = ed
        .doc()
        .resources
        .iter()
        .find(|r| r.name == "Pset_SlabCommon")
        .unwrap();
    let ResourceBody::RecordFormat(f) = &format.body else {
        panic!()
    };
    assert_eq!(
        f.field("Thickness").unwrap().ty,
        record::FieldType::Dimension
    );
    assert_eq!(
        f.field("IsExternal").unwrap().ty,
        record::FieldType::Boolean
    );
    // The slab sits on the layer "Site", which has no elevation.
    assert_eq!(ed.doc().layer(o.layer).unwrap().name, "Site");
}

#[test]
fn import_is_one_undo_step() {
    let doc = Document::default();
    let before = doc.clone();
    let (mut ed, _) = import_into(doc, "extruded_rect.ifc", &IfcImportOptions::default());
    assert!(!ed.doc().objects.is_empty());
    assert!(ed.undo());
    assert_eq!(ed.doc().objects, before.objects);
    assert_eq!(ed.doc().resources.iter().count(), 0);
    assert_eq!(ed.doc().layers.len(), before.layers.len());
    assert!(!ed.undo());
    assert!(ed.redo());
    assert_eq!(ed.doc().objects.len(), 1);
}

#[test]
fn property_set_modes() {
    let opts = IfcImportOptions {
        property_sets: PropertyMode::Data,
        ..IfcImportOptions::default()
    };
    let (ed, _) = import_into(Document::default(), "extruded_rect.ifc", &opts);
    let o = only(&ed);
    assert_eq!(
        o.data["ifc_psets"]["Pset_SlabCommon"]["Material"],
        "Granite"
    );
    assert!(ed.doc().resources.iter().count() == 0);
    let opts = IfcImportOptions {
        property_sets: PropertyMode::None,
        ..IfcImportOptions::default()
    };
    let (ed, _) = import_into(Document::default(), "extruded_rect.ifc", &opts);
    assert!(!only(&ed).data.contains_key("ifc_psets"));
    assert!(ed.doc().resources.iter().count() == 0);
}

#[test]
fn circle_column() {
    let (ed, _) = import("extruded_circle.ifc");
    let m = mesh(only(&ed));
    let exact = std::f64::consts::PI * 150.0 * 150.0 * 3000.0;
    // The circle is flattened with 2 mm tolerance, so the volume is a little smaller.
    assert!(
        m.volume() < exact && m.volume() > exact * 0.98,
        "{}",
        m.volume()
    );
}

#[test]
fn profile_with_voids_keeps_the_holes() {
    let (ed, _) = import("extruded_voids.ifc");
    let m = mesh(only(&ed));
    let area = 10000.0 * 6000.0 - 2.0 * 2000.0 * 2000.0;
    assert!((m.volume() - area * 100.0).abs() < 1e3, "{}", m.volume());
}

#[test]
fn composite_curve_with_arcs_gives_a_landscape_area_with_exact_arcs() {
    let (ed, report) = import("composite_arcs.ifc");
    let o = only(&ed);
    let Shape::LandscapeArea { boundary, name } = &o.shape else {
        panic!("expected a landscape area, got {}", o.shape.kind())
    };
    assert_eq!(name, "Round bed");
    assert!(
        boundary
            .iter()
            .any(|v| v.kind == cadektor_core::curve::VertexKind::PointOnArc)
    );
    // A half disc of radius 2 m: the exact area is half of pi r^2.
    let area = cadektor_core::curve::area(boundary);
    let exact = std::f64::consts::PI * 2000.0 * 2000.0 / 2.0;
    assert!((area - exact).abs() < 1e3, "{area} {exact}");
    assert_eq!(report.objects["landscape_area"], 1);
}

#[test]
fn terrain_becomes_a_site_model() {
    let (ed, report) = import("terrain_triangulated.ifc");
    let o = only(&ed);
    let Shape::SiteModel { points, .. } = &o.shape else {
        panic!("expected a site model, got {}", o.shape.kind())
    };
    assert_eq!(points.len(), 9);
    assert_eq!(points.iter().map(|p| p.z).fold(0.0, f64::max), 1000.0);
    assert_eq!(report.objects["site_model"], 1);
    // The terrain can be turned into a mesh.
    let opts = IfcImportOptions {
        terrain: TerrainMode::Mesh,
        ..IfcImportOptions::default()
    };
    let (ed, _) = import_into(Document::default(), "terrain_triangulated.ifc", &opts);
    let m = mesh(only(&ed));
    assert_eq!(m.triangles.len(), 8);
}

#[test]
fn polygonal_face_set_covering() {
    let (ed, _) = import("polygonal_faceset.ifc");
    let o = only(&ed);
    assert_eq!(o.data["ifc_class"], "IfcCovering");
    assert!((mesh(o).volume() - 2000.0 * 1000.0 * 100.0).abs() < 1.0);
}

#[test]
fn faceted_brep_in_ifc2x3() {
    let (ed, _) = import("faceted_brep_ifc2x3.ifc");
    let o = only(&ed);
    assert_eq!(o.data["ifc_class"], "IfcWallStandardCase");
    assert!((mesh(o).volume() - 3000.0 * 200.0 * 2500.0).abs() < 1.0);
    assert_eq!(o.data["ifc"]["schema"], "IFC2X3");
}

#[test]
fn shell_based_surface_model() {
    let (ed, _) = import("shell_surface_model.ifc");
    let m = mesh(only(&ed));
    assert!((m.volume() - 1000.0f64.powi(3) / 6.0).abs() < 1.0);
}

#[test]
fn mapped_items_become_symbols_with_instances() {
    let (ed, report) = import("mapped_items.ifc");
    assert_eq!(report.objects["symbol"], 3);
    let defs: Vec<_> = ed
        .doc()
        .resources
        .iter()
        .filter(|r| matches!(r.body, ResourceBody::Graphic(_)))
        .collect();
    assert_eq!(defs.len(), 1);
    let ResourceBody::Graphic(g) = &defs[0].body else {
        panic!()
    };
    let GraphicResource::Symbol(def) = &**g else {
        panic!()
    };
    assert_eq!(def.solids.len(), 1);
    match &def.solids[0] {
        Solid3d::Extrusion {
            height, outline, ..
        } => {
            assert_eq!(*height, 900.0);
            assert_eq!(outline.len(), 4);
        }
        other => panic!("{other:?}"),
    }
    let mut at: Vec<DVec2> = objects(&ed)
        .iter()
        .filter_map(|o| match &o.shape {
            Shape::Symbol { at, scale, .. } => {
                assert_eq!(*scale, 1.0);
                Some(*at)
            }
            _ => None,
        })
        .collect();
    at.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(
        at,
        vec![
            DVec2::new(1000.0, 1000.0),
            DVec2::new(3000.0, 1000.0),
            DVec2::new(5000.0, 2000.0)
        ]
    );
    // Without instancing the same file gives three meshes.
    let opts = IfcImportOptions {
        instances: false,
        ..IfcImportOptions::default()
    };
    let (ed, report) = import_into(Document::default(), "mapped_items.ifc", &opts);
    assert_eq!(report.objects["mesh3d"], 3);
    assert!(ed.doc().resources.iter().count() == 0);
    for o in objects(&ed) {
        assert!((mesh(o).volume() - 200.0 * 200.0 * 900.0).abs() < 1.0);
    }
}

#[test]
fn clipping_by_a_half_space() {
    let (ed, _) = import("boolean_clipping.ifc");
    let m = mesh(only(&ed));
    // The wall is 3 m high; everything above 2 m is removed.
    assert!(
        (m.volume() - 4000.0 * 200.0 * 2000.0).abs() < 1.0,
        "{}",
        m.volume()
    );
    assert!((m.bounds3().1.z - 2000.0).abs() < 1e-6);
}

#[test]
fn unsupported_geometry_becomes_a_bounding_box_or_is_skipped() {
    let (ed, report) = import("unsupported_bbox.ifc");
    let o = only(&ed);
    assert_eq!(o.data["ifc"]["name"], "Sculpture");
    assert!((mesh(o).volume() - 1000.0 * 2000.0 * 3000.0).abs() < 1.0);
    let text = serde_json::to_string(&report).unwrap();
    assert!(text.contains("imported as a bounding box"), "{text}");
    assert!(text.contains("IFCADVANCEDBREP"), "{text}");
    // The second element has no box: it is skipped, with the reason.
    assert!(report.skipped.iter().any(
        |s| s.entity == "IfcBuildingElementProxy" && s.reason.contains("unsupported geometry")
    ));
}

#[test]
fn georeference_storeys_classes_and_colors() {
    let (ed, report) = import("georef_storeys.ifc");
    let g = ed.doc().georef.clone().expect("georeference");
    assert_eq!(g.crs, "EPSG:3067");
    assert_eq!(g.origin, DVec2::new(385000.0, 6672000.0));
    assert_eq!(g.rotation, 0.0);
    assert!(report.georeferenced);
    // One layer per storey, with its elevation.
    let layer = |name: &str| {
        ed.doc()
            .layers
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no layer {name}"))
    };
    assert_eq!(layer("Ground floor").elevation, 0.0);
    assert_eq!(layer("First floor").elevation, 3200.0);
    // Presentation layers are classes.
    let class = |n: &str| ed.doc().classes.iter().find(|c| c.name == n).unwrap().id;
    let by_name = |n: &str| {
        objects(&ed)
            .into_iter()
            .find(|o| o.data["ifc"]["name"] == n)
            .unwrap()
    };
    let ground = by_name("Ground slab");
    let upper = by_name("Upper slab");
    assert_eq!(ground.class, class("A-FLOR"));
    assert_eq!(upper.class, class("A-FLOR-UPPER"));
    assert_eq!(ground.layer, layer("Ground floor").id);
    assert_eq!(upper.layer, layer("First floor").id);
    // The slab on the first floor is 3.2 m higher.
    assert!((mesh(upper).bounds3().0.z - 3200.0).abs() < 1e-6);
    assert!((mesh(ground).bounds3().0.z - 0.0).abs() < 1e-6);
    // The color of the styled item.
    assert_eq!(mesh(ground).color, Some(Color([51, 102, 153, 255])));
}

#[test]
fn file_in_a_document_with_the_same_crs_is_moved_into_its_georeference() {
    let mut doc = Document::default();
    doc.georef = Some(cadektor_core::georef::Georeference {
        crs: "EPSG:3067".into(),
        origin: DVec2::new(384_990.0, 6_671_995.0),
        rotation: 0.0,
        user_origin: DVec2::ZERO,
    });
    let (ed, report) = import_into(
        doc.clone(),
        "georef_storeys.ifc",
        &IfcImportOptions::default(),
    );
    // The file origin is 10 m east and 5 m north of the document origin.
    let o = objects(&ed)
        .into_iter()
        .find(|o| o.data["ifc"]["name"] == "Ground slab")
        .unwrap();
    let (lo, hi) = mesh(o).bounds3();
    assert!((lo.x - (10_000.0 - 1500.0)).abs() < 1e-3, "{lo}");
    assert!((hi.y - (5_000.0 + 1500.0)).abs() < 1e-3, "{hi}");
    assert_eq!(ed.doc().georef, doc.georef);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("moved into the georeference"))
    );
    // A different CRS keeps the local coordinates and says so.
    doc.georef.as_mut().unwrap().crs = "EPSG:25832".into();
    let (ed, report) = import_into(doc, "georef_storeys.ifc", &IfcImportOptions::default());
    let o = objects(&ed)
        .into_iter()
        .find(|o| o.data["ifc"]["name"] == "Ground slab")
        .unwrap();
    assert!((mesh(o).bounds3().0.x + 1500.0).abs() < 1e-3);
    assert!(report.notes.iter().any(|n| n.contains("differs")));
    // The option can turn the georeference off.
    let opts = IfcImportOptions {
        georef: false,
        ..IfcImportOptions::default()
    };
    let (ed, _) = import_into(Document::default(), "georef_storeys.ifc", &opts);
    assert!(ed.doc().georef.is_none());
}

#[test]
fn units_in_feet() {
    let (ed, _) = import("units_feet.ifc");
    let m = mesh(only(&ed));
    // 10 ft x 10 ft x 1 ft.
    assert!(
        (m.volume() - (3048.0f64).powi(2) * 304.8).abs() < 1.0,
        "{}",
        m.volume()
    );
}

#[test]
fn vegetation_with_a_size_becomes_a_plant() {
    let (ed, report) = import("vegetation_plant.ifc");
    let o = only(&ed);
    let Shape::Plant {
        at,
        species,
        spread,
        height,
    } = &o.shape
    else {
        panic!("expected a plant, got {}", o.shape.kind())
    };
    assert_eq!(species, "Quercus robur");
    // The tree stands at (3, 4) m. The canopy has a radius of 2 m (flattened a little) and the tree is 5 m high.
    assert!(
        (at.x - 3000.0).abs() < 1.0 && (at.y - 4000.0).abs() < 1.0,
        "{at}"
    );
    assert!((*spread - 4000.0).abs() < 20.0, "{spread}");
    assert!((*height - 5000.0).abs() < 1e-6);
    assert_eq!(report.objects["plant"], 1);
    // The option turns it off.
    let opts = IfcImportOptions {
        vegetation: false,
        ..IfcImportOptions::default()
    };
    let (ed, _) = import_into(Document::default(), "vegetation_plant.ifc", &opts);
    assert_eq!(only(&ed).shape.kind(), "mesh3d");
}

#[test]
fn ifc4x3_classes() {
    let (ed, _) = import("ifc4x3_pavement.ifc");
    let mut classes: Vec<String> = objects(&ed)
        .iter()
        .map(|o| o.data["ifc_class"].as_str().unwrap().to_string())
        .collect();
    classes.sort();
    assert_eq!(classes, vec!["IfcCourse", "IfcPavement"]);
    assert!(objects(&ed).iter().all(|o| o.shape.kind() == "mesh3d"));
}

#[test]
fn annotation_curves_keep_their_arcs() {
    let (ed, report) = import("annotation.ifc");
    let mut kinds: Vec<&str> = objects(&ed).iter().map(|o| o.shape.kind()).collect();
    kinds.sort();
    assert_eq!(kinds, vec!["locus3d", "polyline"]);
    let line = objects(&ed)
        .into_iter()
        .find_map(|o| match &o.shape {
            Shape::Polyline { points, closed } => Some((points.clone(), *closed)),
            _ => None,
        })
        .unwrap();
    assert!(!line.1);
    assert!(
        line.0
            .iter()
            .any(|v| v.kind == cadektor_core::curve::VertexKind::PointOnArc)
    );
    assert_eq!(report.total(), 2);
}

#[test]
fn truncated_file_imports_what_is_complete() {
    let (ed, report) = import("truncated.ifc");
    assert_eq!(only(&ed).data["ifc_class"], "IfcSlab");
    assert!(report.notes.iter().any(|n| n.contains("cut off")));
}

#[test]
fn bad_files_are_refused() {
    let doc = Document::default();
    let opts = IfcImportOptions::default();
    let err = import_bytes(b"hello".to_vec(), &doc, &opts).unwrap_err();
    assert!(matches!(err, IfcImportError::Format(_)));
    let step = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n#1=X();\nENDSEC;\nEND-ISO-10303-21;\n";
    let err = import_bytes(step.as_bytes().to_vec(), &doc, &opts).unwrap_err();
    assert!(err.to_string().contains("not an IFC file"), "{err}");
    let empty = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";
    assert!(import_bytes(empty.as_bytes().to_vec(), &doc, &opts).is_err());
    let err = import_file(Path::new("/nonexistent/file.ifc"), &doc, &opts).unwrap_err();
    assert!(matches!(err, IfcImportError::Io(_)));
    let small = IfcImportOptions {
        limits: Limits {
            max_bytes: 100,
            ..Limits::default()
        },
        ..IfcImportOptions::default()
    };
    let err = import_bytes(fixture("extruded_rect.ifc"), &doc, &small).unwrap_err();
    assert!(err.to_string().contains("larger than"), "{err}");
}

#[test]
fn an_ifc_file_with_no_products_says_so() {
    let text = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCPROJECT('g',$,'p',$,$,$,$,$,$);\nENDSEC;\nEND-ISO-10303-21;\n";
    let r = import_bytes(
        text.as_bytes().to_vec(),
        &Document::default(),
        &IfcImportOptions::default(),
    )
    .unwrap();
    assert_eq!(r.report.total(), 0);
    assert!(r.report.notes.iter().any(|n| n.contains("no geometry")));
}

#[test]
fn the_object_limit_stops_the_import() {
    let opts = IfcImportOptions {
        max_objects: 1,
        ..IfcImportOptions::default()
    };
    let (ed, report) = import_into(Document::default(), "mapped_items.ifc", &opts);
    assert_eq!(ed.doc().objects.len(), 1);
    assert!(report.notes.iter().any(|n| n.contains("object limit")));
}

#[test]
fn reimport_into_the_same_document_does_not_clash() {
    let (mut ed, _) = import("extruded_rect.ifc");
    let result = import_bytes(
        fixture("extruded_rect.ifc"),
        ed.doc(),
        &IfcImportOptions::default(),
    )
    .unwrap();
    ed.execute(&result.command).unwrap();
    assert_eq!(ed.doc().objects.len(), 2);
    let names: Vec<_> = ed.doc().resources.iter().map(|r| r.name.clone()).collect();
    assert!(names.contains(&"Pset_SlabCommon".to_string()));
    assert!(
        names.contains(&"Pset_SlabCommon (2)".to_string()),
        "{names:?}"
    );
}

#[test]
fn options_from_json() {
    let o = IfcImportOptions::from_json(&json!({
        "layers_from_storeys": false, "classes_from_layers": false, "property_sets": "data",
        "terrain": "mesh", "instances": false, "vegetation": false, "georef": false,
        "max_objects": 5, "max_triangles": 6, "max_file_mb": 7
    }))
    .unwrap();
    assert!(!o.layers_from_storeys && !o.classes_from_layers && !o.instances);
    assert_eq!(o.property_sets, PropertyMode::Data);
    assert_eq!(o.terrain, TerrainMode::Mesh);
    assert_eq!(
        (o.max_objects, o.max_triangles, o.limits.max_bytes),
        (5, 6, 7 << 20)
    );
    assert!(IfcImportOptions::from_json(&json!(null)).is_ok());
    assert!(IfcImportOptions::from_json(&json!([])).is_err());
    assert!(
        IfcImportOptions::from_json(&json!({"nope": 1}))
            .unwrap_err()
            .to_string()
            .contains("unknown option")
    );
    assert!(IfcImportOptions::from_json(&json!({"terrain": "x"})).is_err());
    assert!(IfcImportOptions::from_json(&json!({"property_sets": 1})).is_err());
    assert!(IfcImportOptions::from_json(&json!({"georef": 1})).is_err());
    assert!(IfcImportOptions::from_json(&json!({"max_objects": 0})).is_err());
}

#[test]
fn layers_off_puts_everything_on_the_active_layer() {
    let opts = IfcImportOptions {
        layers_from_storeys: false,
        classes_from_layers: false,
        ..IfcImportOptions::default()
    };
    let (ed, report) = import_into(Document::default(), "georef_storeys.ifc", &opts);
    assert!(report.layers_created.is_empty());
    assert!(report.classes_created.is_empty());
    for o in objects(&ed) {
        assert_eq!(o.layer, ed.doc().active_layer);
        assert_eq!(o.class, ed.doc().active_class);
    }
}

fn round_trip(opts: &crate::IfcOptions) -> (Document, Editor, Report) {
    let doc = crate::tests::sample();
    let (bytes, _) = crate::export_bytes(&doc, opts).unwrap();
    let result = import_bytes(bytes, &Document::default(), &IfcImportOptions::default()).unwrap();
    let mut ed = Editor::new(Document::default());
    ed.execute(&result.command).unwrap();
    (doc, ed, result.report)
}

#[test]
fn round_trip_of_our_own_export() {
    let (doc, ed, report) = round_trip(&crate::IfcOptions::default());
    let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
    for o in ed.doc().objects.values() {
        *kinds.entry(o.shape.kind()).or_default() += 1;
    }
    let _ = &report;
    // Hardscapes are generic meshes (the draped landscape area is a mesh too), the tree and the plants are plants.
    assert_eq!(kinds["mesh3d"], 6, "{kinds:?}");
    assert_eq!(kinds["plant"], 4, "{kinds:?}");
    assert_eq!(kinds["landscape_area"], 1);
    assert_eq!(kinds["site_model"], 1);
    assert_eq!(kinds.len(), 4, "{kinds:?}");
    // The georeference comes back, and the classes.
    let g = ed.doc().georef.clone().unwrap();
    assert_eq!(g.crs, "EPSG:3067");
    assert!((g.origin - DVec2::new(385000.0, 6672000.0)).length() < 1e-6);
    assert!((g.rotation - 0.05).abs() < 1e-9);
    for c in ["Planting", "Paving"] {
        assert!(ed.doc().classes.iter().any(|x| x.name == c), "{c}");
    }
    // The terrain keeps its 81 points and their heights.
    let site = |d: &Document| {
        d.objects
            .values()
            .find_map(|o| match &o.shape {
                Shape::SiteModel { points, .. } => Some(points.clone()),
                _ => None,
            })
            .unwrap()
    };
    let (a, b) = (site(&doc), site(ed.doc()));
    assert_eq!(a.len(), b.len());
    let max = |p: &[DVec3]| p.iter().map(|p| p.z).fold(f64::MIN, f64::max);
    assert!((max(&a) - max(&b)).abs() < 1e-3);
    // The slabs know their class.
    let slabs = ed
        .doc()
        .objects
        .values()
        .filter(|o| o.data.get("ifc_class") == Some(&json!("IfcSlab")))
        .count();
    assert_eq!(slabs, 5);
    // The area keeps its name.
    let names = |d: &Document| -> Vec<String> {
        d.objects
            .values()
            .filter_map(|o| match &o.shape {
                Shape::LandscapeArea { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(names(ed.doc()).len(), 1);
    assert!(names(&doc).contains(&names(ed.doc())[0]));
}

#[test]
fn round_trip_with_storeys_gives_layers_with_elevations() {
    let opts = crate::IfcOptions {
        layers_as_storeys: true,
        ..Default::default()
    };
    let (doc, ed, _) = round_trip(&opts);
    for l in &doc.layers {
        let back = ed.doc().layers.iter().find(|x| x.name == l.name);
        assert!(back.is_some(), "layer {}", l.name);
        // A layer without objects has no storey; the others keep their elevation.
        assert!(
            (back.unwrap().elevation - l.elevation).abs() < 1e-6,
            "{}",
            l.name
        );
    }
}

#[test]
fn imported_meshes_export_again_and_come_back_the_same() {
    for (file, class, volume) in [
        ("extruded_rect.ifc", "IfcSlab", 4000.0 * 2000.0 * 200.0),
        (
            "polygonal_faceset.ifc",
            "IfcCovering",
            2000.0 * 1000.0 * 100.0,
        ),
        (
            "faceted_brep_ifc2x3.ifc",
            "IfcBuildingElementProxy",
            3000.0 * 200.0 * 2500.0,
        ),
    ] {
        let (ed, _) = import(file);
        let (bytes, report) = crate::export_bytes(ed.doc(), &crate::IfcOptions::default()).unwrap();
        assert_eq!(report.total(), 1, "{file}: {report:?}");
        let parsed = crate::step::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert!(parsed.dangling().is_empty());
        assert_eq!(
            parsed
                .entities
                .iter()
                .filter(
                    |e| e.name == format!("IFC{}", class.to_uppercase().trim_start_matches("IFC"))
                )
                .count(),
            1,
            "{file}"
        );
        let result =
            import_bytes(bytes, &Document::default(), &IfcImportOptions::default()).unwrap();
        let mut again = Editor::new(Document::default());
        again.execute(&result.command).unwrap();
        let o = only(&again);
        assert!(
            (mesh(o).volume() - volume).abs() < 1.0,
            "{file}: {}",
            mesh(o).volume()
        );
        assert_eq!(o.data["ifc_class"], class, "{file}");
    }
}

mod fuzz {
    use super::*;
    use proptest::prelude::*;

    const FILES: [&str; 12] = [
        "extruded_rect.ifc",
        "extruded_voids.ifc",
        "composite_arcs.ifc",
        "terrain_triangulated.ifc",
        "polygonal_faceset.ifc",
        "faceted_brep_ifc2x3.ifc",
        "mapped_items.ifc",
        "boolean_clipping.ifc",
        "georef_storeys.ifc",
        "vegetation_plant.ifc",
        "annotation.ifc",
        "unsupported_bbox.ifc",
    ];

    /// Imports the bytes. Must not panic, and a batch that comes back must run.
    fn check(bytes: Vec<u8>) {
        let doc = Document::default();
        if let Ok(r) = import_bytes(bytes, &doc, &IfcImportOptions::default()) {
            let mut ed = Editor::new(doc);
            ed.execute(&r.command)
                .unwrap_or_else(|e| panic!("the batch failed: {e}"));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn truncated_fixtures(file in 0usize..FILES.len(), cut in 0.0f64..1.0) {
            let bytes = fixture(FILES[file]);
            let n = (bytes.len() as f64 * cut) as usize;
            check(bytes[..n].to_vec());
        }

        #[test]
        fn mutated_fixtures(file in 0usize..FILES.len(), at in 0.0f64..1.0, b in any::<u8>(), n in 1usize..6) {
            let mut bytes = fixture(FILES[file]);
            for k in 0..n {
                let p = ((bytes.len() as f64 * at) as usize + k * 13) % bytes.len();
                bytes[p] = b.wrapping_add(k as u8);
            }
            check(bytes);
        }

        #[test]
        fn numbers_swapped_for_odd_values(file in 0usize..FILES.len(), which in 0usize..40, odd in prop::sample::select(vec!["0.", "-1.", "1.E30", "-1.E30", "1.E-30", "0.00001", "99999999.", "2"])) {
            let text = String::from_utf8(fixture(FILES[file])).unwrap();
            // Replace the `which`-th real number of the data section.
            let data = text.find("DATA;").unwrap();
            let mut out = String::new();
            let mut count = 0usize;
            let mut rest = &text[data..];
            out.push_str(&text[..data]);
            while let Some(p) = rest.find(|c: char| c.is_ascii_digit()) {
                let end = rest[p..]
                    .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'E' || c == '-'))
                    .map_or(rest.len(), |e| e + p);
                out.push_str(&rest[..p]);
                let tok = &rest[p..end];
                if tok.contains('.') && count == which { out.push_str(odd); } else { out.push_str(tok); }
                if tok.contains('.') { count += 1; }
                rest = &rest[end..];
            }
            out.push_str(rest);
            check(out.into_bytes());
        }

        #[test]
        fn random_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            check(bytes);
        }

        #[test]
        fn random_entities_after_a_valid_header(tail in "[#=(),'.$*A-Z0-9 ;-]{0,300}") {
            let text = format!("ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n{tail}\nENDSEC;\n");
            check(text.into_bytes());
        }
    }
}

#[test]
fn a_big_file_imports() {
    // 1500 slabs on a grid, and a terrain of 120 x 120 points.
    let mut text = String::from(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCPROJECT('g',$,'big',$,$,$,$,$,#2);\n#2=IFCUNITASSIGNMENT((#3));\n#3=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n#4=IFCDIRECTION((0.,0.,1.));\n#5=IFCAXIS2PLACEMENT3D(#6,$,$);\n#6=IFCCARTESIANPOINT((0.,0.,0.));\n",
    );
    let mut id = 100u64;
    let mut elements = Vec::new();
    for k in 0..1500u64 {
        let (x, y) = ((k % 50) as f64 * 3000.0, (k / 50) as f64 * 3000.0);
        text += &format!(
            "#{a}=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,2000.,2000.);\n#{b}=IFCEXTRUDEDAREASOLID(#{a},#5,#4,100.);\n#{c}=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{b}));\n#{d}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{c}));\n#{e}=IFCCARTESIANPOINT(({x}.,{y}.,0.));\n#{f}=IFCAXIS2PLACEMENT3D(#{e},$,$);\n#{g}=IFCLOCALPLACEMENT($,#{f});\n#{h}=IFCSLAB('g{k}',$,'Slab {k}',$,$,#{g},#{d},$,.FLOOR.);\n",
            a = id,
            b = id + 1,
            c = id + 2,
            d = id + 3,
            e = id + 4,
            f = id + 5,
            g = id + 6,
            h = id + 7,
        );
        elements.push(id + 7);
        id += 8;
    }
    let n = 120usize;
    let mut coords = Vec::new();
    for j in 0..n {
        for i in 0..n {
            coords.push(format!("({}.,{}.,{}.)", i * 1000, j * 1000, (i * j) % 700));
        }
    }
    let mut tris = Vec::new();
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = j * n + i + 1;
            tris.push(format!("({},{},{})", a, a + 1, a + n + 1));
            tris.push(format!("({},{},{})", a, a + n + 1, a + n));
        }
    }
    text += &format!(
        "#{p}=IFCCARTESIANPOINTLIST3D(({}));\n#{t}=IFCTRIANGULATEDFACESET(#{p},$,.F.,({}),$);\n#{r}=IFCSHAPEREPRESENTATION($,'Body','Tessellation',(#{t}));\n#{s}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{r}));\n#{g}=IFCGEOGRAPHICELEMENT('terr',$,'Ground',$,$,#6,#{s},$,.TERRAIN.);\n",
        coords.join(","),
        tris.join(","),
        p = id,
        t = id + 1,
        r = id + 2,
        s = id + 3,
        g = id + 4,
    );
    text += "ENDSEC;\nEND-ISO-10303-21;\n";
    // The placement of the terrain refers to a point, which is not a placement: it is read as the origin.
    let r = import_bytes(
        text.into_bytes(),
        &Document::default(),
        &IfcImportOptions::default(),
    )
    .unwrap();
    assert_eq!(r.report.objects["mesh3d"], 1500);
    assert_eq!(r.report.objects["site_model"], 1);
    let mut ed = Editor::new(Document::default());
    ed.execute(&r.command).unwrap();
    assert_eq!(ed.doc().objects.len(), 1501);
}
