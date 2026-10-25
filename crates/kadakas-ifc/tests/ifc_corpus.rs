//! The public IFC corpus (IOP-29): CC BY 4.0 files fetched at run time. See `support/corpus.rs`.
//!
//! Every test skips (and says why) when the files are not there. The expected numbers come from the
//! files themselves (counted by reading the STEP text, not with this importer), see
//! `docs/research/ifc-real-world.md` section 12.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use kadakas_core::command::Editor;
use kadakas_core::model::{Document, Object, Shape};
use kadakas_ifc::{IfcImportError, IfcImportOptions, import_file};
use kadakas_io::Report;
use support::corpus;

/// A debug build of a 2 MB file takes well under a second; a minute means a loop.
const TIME_BOUND: Duration = Duration::from_secs(60);

fn load(path: &Path) -> (Editor, Report) {
    load_with(path, &IfcImportOptions::default())
}

fn load_with(path: &Path, opts: &IfcImportOptions) -> (Editor, Report) {
    let doc = Document::with_sequential_ids();
    let r = import_file(path, &doc, opts).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut ed = Editor::new(doc);
    ed.execute(&r.command)
        .unwrap_or_else(|e| panic!("{}: the batch failed: {e}", path.display()));
    (ed, r.report)
}

fn kinds(ed: &Editor) -> std::collections::BTreeMap<&'static str, usize> {
    let mut m = std::collections::BTreeMap::new();
    for o in ed.doc().objects.values() {
        *m.entry(o.shape.kind()).or_default() += 1;
    }
    m
}

fn objects(ed: &Editor) -> Vec<&Object> {
    ed.doc().objects.values().collect()
}

fn mesh_volume(o: &Object) -> f64 {
    match &o.shape {
        Shape::Mesh3d(m) => m.volume(),
        other => panic!("not a mesh: {}", other.kind()),
    }
}

#[test]
fn every_file_imports_without_a_panic_in_time_and_the_accounting_holds() {
    corpus::prefetch(&corpus::all_ifc());
    let mut seen = 0;
    for (repo, path) in corpus::all_ifc() {
        let Some(file) = corpus::need(&repo, &path) else {
            continue;
        };
        seen += 1;
        let start = Instant::now();
        let doc = Document::with_sequential_ids();
        let r = import_file(&file, &doc, &IfcImportOptions::default())
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(
            start.elapsed() < TIME_BOUND,
            "{path} took {:?}",
            start.elapsed()
        );
        let mut ed = Editor::new(doc);
        ed.execute(&r.command)
            .unwrap_or_else(|e| panic!("{path}: the batch failed: {e}"));
        let rep = &r.report;
        assert_eq!(
            rep.entities_read,
            rep.entities_imported + rep.entities_skipped,
            "{path}: every product read is imported or skipped"
        );
        assert!(
            rep.skipped_total() >= rep.entities_skipped,
            "{path}: every skipped product has a line in the report"
        );
        assert_eq!(rep.total(), ed.doc().objects.len(), "{path}");
    }
    if seen == 0 {
        eprintln!("SKIP: no file of the IFC corpus is available");
    }
}

#[test]
fn landscaping_scene_of_ifc4_has_the_counts_of_the_file() {
    let Some(file) = corpus::scene("4", "Infra-Landscaping") else {
        return;
    };
    let (ed, rep) = load(&file);
    // The file (counted in the STEP text): 8 IfcSite (1 COMPLEX, 7 PARTIAL), 76 IfcGeographicElement
    // (14 terrain, 62 vegetation, all with an IfcTriangulatedFaceSet), 15 proxies, 10 members and
    // 10 element assemblies without a shape. 101 products have a shape.
    assert_eq!(rep.entities_read, 101);
    assert_eq!(rep.entities_skipped, 0);
    let k = kinds(&ed);
    assert_eq!(k["site_model"], 14, "the 14 terrain layers are site models");
    assert_eq!(k["plant"], 8, "the 8 trees with a size are plants");
    assert_eq!(
        k["mesh3d"], 79,
        "62 - 8 vegetation parts, 15 proxies, 10 members"
    );
    // Every product is on a layer and has its class.
    let geo: Vec<&Object> = objects(&ed)
        .into_iter()
        .filter(|o| o.data["ifc_class"] == "IfcGeographicElement")
        .collect();
    assert_eq!(geo.len(), 76);
    assert!(geo.iter().all(|o| ed.doc().layer(o.layer).is_some()));
    let terrain = geo
        .iter()
        .filter(|o| o.data["ifc"]["object_type"] == "terrain")
        .count();
    let vegetation = geo
        .iter()
        .filter(|o| o.data["ifc"]["object_type"] == "vegetation")
        .count();
    assert_eq!((terrain, vegetation), (14, 62));
}

#[test]
fn the_escape_x27_in_the_text_of_the_scene_decodes_to_an_apostrophe() {
    for schema in ["4", "4x3", "2x3"] {
        let Some(file) = corpus::scene(schema, "Infra-Landscaping") else {
            continue;
        };
        let (ed, _) = load(&file);
        let described: Vec<String> = objects(&ed)
            .iter()
            .filter_map(|o| o.data["ifc"]["description"].as_str().map(str::to_string))
            .collect();
        assert!(!described.is_empty(), "{schema}: no description");
        assert!(
            described
                .iter()
                .any(|d| d.contains("earth's best-kept secrets")),
            "{schema}: {described:?}"
        );
        assert!(
            described
                .iter()
                .all(|d| !d.contains("\\X\\") && !d.contains('\\')),
            "{schema}: an escape is left in a text"
        );
    }
}

#[test]
fn the_map_conversion_of_the_scene_is_applied_with_its_scale() {
    let Some(file) = corpus::scene("4", "Infra-Landscaping") else {
        return;
    };
    let (ed, rep) = load(&file);
    let g = ed.doc().georef.as_ref().expect("a georeference");
    assert_eq!(g.crs, "EPSG:32760");
    // IfcMapConversion(Eastings, Northings, ..., scale 0.001): the model is in millimeters and the
    // CRS in meters, so the origin is the map origin and the geometry is not scaled.
    assert!(
        (g.origin.x - 729011.2258823584).abs() < 1e-6,
        "{:?}",
        g.origin
    );
    assert!(
        (g.origin.y - 9063960.607644705).abs() < 1e-6,
        "{:?}",
        g.origin
    );
    assert!(rep.georeferenced);
    assert!(
        !rep.notes.iter().any(|n| n.contains("scale")),
        "the scale 0.001 of a millimeter model is the normal case: {:?}",
        rep.notes
    );
    // The scene is a few hundred meters wide: the coordinates are in millimeters near the origin.
    let mut hi = 0.0f64;
    for o in objects(&ed) {
        if let Shape::Mesh3d(m) = &o.shape {
            let (a, b) = m.bounds3();
            hi = hi.max(a.abs().max_element()).max(b.abs().max_element());
        }
    }
    assert!(hi > 1e3 && hi < 2e6, "extent {hi} mm");
    // The same scene in IFC4X3_ADD2 gives the same result.
    let Some(f3) = corpus::scene("4x3", "Infra-Landscaping") else {
        return;
    };
    let (ed3, _) = load(&f3);
    let g3 = ed3.doc().georef.as_ref().unwrap();
    assert_eq!(g3.crs, g.crs);
    assert_eq!(g3.origin, g.origin);
}

#[test]
fn the_ifc2x3_scene_is_all_proxies_and_has_no_georeference() {
    let Some(file) = corpus::scene("2x3", "Infra-Landscaping") else {
        return;
    };
    let (ed, rep) = load(&file);
    assert_eq!(rep.entities_read, 101);
    assert_eq!(kinds(&ed)["mesh3d"], 101);
    assert!(ed.doc().georef.is_none());
    assert!(rep.notes.iter().any(|n| n.contains("no georeference")));
}

#[test]
fn the_small_landscaping_scene_gives_site_models_plants_and_meshes() {
    for schema in ["4", "4x3"] {
        let Some(file) = corpus::scene(schema, "Building-Landscaping") else {
            continue;
        };
        let (ed, _) = load(&file);
        let k = kinds(&ed);
        assert_eq!(k["site_model"], 2, "{schema}");
        assert_eq!(k["plant"], 2, "{schema}");
        assert_eq!(k["mesh3d"], 2, "{schema}");
    }
}

#[test]
fn reference_view_samples_give_one_closed_body_each() {
    for name in [
        "basin-tessellation",
        "column-straight-rectangle-tessellation",
        "tessellated-item",
        "tessellation-with-individual-colors",
    ] {
        let Some(file) = corpus::reference_view(name) else {
            continue;
        };
        let (ed, rep) = load(&file);
        assert_eq!(rep.entities_read, 1, "{name}");
        let o = objects(&ed);
        assert_eq!(o.len(), 1, "{name}");
        assert!(mesh_volume(o[0]) > 0.0, "{name}");
    }
}

#[test]
fn xml_and_json_twins_are_refused_by_name() {
    let Some(xml) = corpus::iso("geographic-referencing.xml") else {
        return;
    };
    let e = import_file(
        &xml,
        &Document::with_sequential_ids(),
        &IfcImportOptions::default(),
    )
    .unwrap_err();
    assert_eq!(
        e,
        IfcImportError::Format("IFC XML is not supported; export STEP (.ifc) or ifcZIP".into())
    );
    let Some(json) = corpus::iso("geographic-referencing.json") else {
        return;
    };
    let e = import_file(
        &json,
        &Document::with_sequential_ids(),
        &IfcImportOptions::default(),
    )
    .unwrap_err();
    assert!(
        e.to_string().starts_with("IFC JSON is not supported"),
        "{e}"
    );
}
