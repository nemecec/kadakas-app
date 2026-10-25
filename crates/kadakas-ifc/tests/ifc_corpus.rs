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

/// Only a guard against a hang. The work of an import is bounded by counted budgets, not by the
/// clock: the tests below give a small triangle limit and boolean budget and assert that no file
/// passes them, so the result does not depend on the speed of the machine.
const HANG_GUARD: Duration = Duration::from_secs(600);

/// The counted budgets of the corpus tests (a scene of 2 MB makes well under 200,000 triangles).
fn bounded() -> IfcImportOptions {
    IfcImportOptions {
        max_triangles: 2_000_000,
        boolean_work: 100_000_000,
        ..Default::default()
    }
}

fn load(path: &Path) -> (Editor, Report) {
    load_with(path, &bounded())
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
        let r = import_file(&file, &doc, &bounded()).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(
            start.elapsed() < HANG_GUARD,
            "{path} took {:?}",
            start.elapsed()
        );
        // No counted budget was used up: the work is bounded by counts, not by the clock.
        assert!(
            !r.report
                .notes
                .iter()
                .any(|n| n.contains("limit was reached") || n.contains("work budget")),
            "{path}: {:?}",
            r.report.notes
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

#[test]
fn the_wall_with_an_opening_and_a_window_is_cut() {
    let Some(file) = corpus::reference_view("wall-with-opening-and-window") else {
        return;
    };
    let (ed, rep) = load(&file);
    // The wall is 3000 x 300 x 2000. The opening is 1000 x 300 x 1000 and goes through the wall.
    // The window (1000 x 200 x 1000) sits in the opening and is an object of its own.
    let gross = 3000.0 * 300.0 * 2000.0;
    let opening = 1000.0 * 300.0 * 1000.0;
    let mut volumes: Vec<f64> = objects(&ed).iter().map(|o| mesh_volume(o)).collect();
    volumes.sort_by(f64::total_cmp);
    assert_eq!(volumes.len(), 2, "{rep:?}");
    assert!(
        (volumes[0] - 1000.0 * 200.0 * 1000.0).abs() < 1.0,
        "{volumes:?}"
    );
    assert!((volumes[1] - (gross - opening)).abs() < 1.0, "{volumes:?}");
    assert!(
        rep.notes
            .iter()
            .any(|n| n == "1 openings (IfcRelVoidsElement) were cut from their walls and slabs"),
        "{:?}",
        rep.notes
    );
    assert_eq!(rep.entities_read, 3);
}

#[test]
fn the_slab_with_a_round_hole_and_a_recess_has_the_analytic_volume() {
    let Some(file) = corpus::iso("slab-openings.ifc") else {
        return;
    };
    let (ed, rep) = load(&file);
    let o = objects(&ed);
    assert_eq!(o.len(), 1, "{rep:?}");
    // The slab is 200 thick. Its outline: a 1000 x 4000 rectangle with two circular segments
    // (chord 4000, rise 400) on the long sides. A round hole of radius 50 goes through it and a
    // recess of 1000 x 500 x 50 is cut from the top.
    let (chord, rise) = (4000.0f64, 400.0f64);
    let r = (chord * chord / 4.0 + rise * rise) / (2.0 * rise);
    let segment =
        r * r * ((r - rise) / r).acos() - (r - rise) * (2.0 * r * rise - rise * rise).sqrt();
    let area = 1000.0 * 4000.0 + 2.0 * segment;
    let want = area * 200.0 - std::f64::consts::PI * 50.0 * 50.0 * 200.0 - 1000.0 * 500.0 * 50.0;
    let got = mesh_volume(o[0]);
    // The arcs and the circle are cut into short lines (2 mm), so the volume is a little less.
    assert!((got - want).abs() < want * 0.003, "{got} {want}");
    assert!(got < want, "{got} {want}");
    assert!(
        rep.notes.iter().any(|n| n.starts_with("2 openings")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn advanced_breps_with_spline_faces_have_a_volume_and_keep_their_exact_surfaces() {
    // cube-advanced-brep: a unit cube turned by 75 degrees from the bottom to the top, with four
    // B-spline side faces (a ruled surface). The file is in meters. Its volume is 5/6 of a cubic
    // meter: the section at the height z is a square of side 1 rotated by 75 degrees * z and
    // shrunk to the chord of the rotation.
    let Some(file) = corpus::iso("cube-advanced-brep.ifc") else {
        return;
    };
    let (ed, rep) = load(&file);
    let o = objects(&ed);
    assert_eq!(o.len(), 1, "{rep:?}");
    let want = 5.0 / 6.0 * 1e9;
    let got = mesh_volume(o[0]);
    assert!((got - want).abs() < want * 0.01, "{got} {want}");
    let Shape::Mesh3d(m) = &o[0].shape else {
        panic!()
    };
    assert_eq!(m.exact_state(), Some(kadakas_core::exact::State::Fresh));
    let exact = m.exact.as_ref().unwrap();
    assert_eq!(exact.faces.len(), 6);
    // The payload is in millimeters like the triangles.
    let (lo, hi) = m.bounds3();
    let ev = exact
        .vertices
        .iter()
        .fold(glam::DVec3::NEG_INFINITY, |a, v| a.max(*v));
    assert!(
        (ev.z - hi.z).abs() < 1e-6 && hi.z > 999.0 && lo.z.abs() < 1e-6,
        "{ev} {hi}"
    );
    assert!(rep.notes.is_empty() && rep.skipped.is_empty(), "{rep:?}");
}

#[test]
fn the_revolved_beam_and_the_csg_samples_have_their_analytic_volumes() {
    if let Some(file) = corpus::iso("beam-revolved-solid.ifc") {
        // IPE 600 turned by 1.522 radians about an axis 7.25 m from the centre of the profile
        // (the theorem of Pappus). The 2 mm tolerance cuts the 24 mm fillets into few lines.
        let (w, d, tw, tf, fr) = (0.22, 0.6, 0.012, 0.019, 0.024);
        let area = 2.0 * w * tf + (d - 2.0 * tf) * tw + (4.0 - std::f64::consts::PI) * fr * fr;
        let want = area * 7.25 * 1.52202550844946 * 1e9;
        let (ed, _) = load(&file);
        let got = mesh_volume(objects(&ed)[0]);
        assert!((got - want).abs() < want * 0.015, "{got} {want}");
    }
    if let Some(file) = corpus::iso("csg-primitive.ifc") {
        let (ed, _) = load(&file);
        assert!((mesh_volume(objects(&ed)[0]) - 1000.0 * 1000.0 * 2000.0).abs() < 1.0);
    }
    if let Some(file) = corpus::iso("bath-csg-solid.ifc") {
        // A block 2000 x 800 x 800 less a rounded rectangle 1800 x 600 (corner radius 200), 700 deep.
        let hole = 1800.0 * 600.0 - (4.0 - std::f64::consts::PI) * 200.0 * 200.0;
        let want = 2000.0 * 800.0 * 800.0 - hole * 700.0;
        let (ed, _) = load(&file);
        let got = mesh_volume(objects(&ed)[0]);
        assert!((got - want).abs() < want * 0.01, "{got} {want}");
    }
}

#[test]
fn nurbs_samples_of_bentley_match_their_facet_twins_or_say_what_is_missing() {
    // Each sample has a twin written as facets. Where the surfaces can be cut, the volumes agree.
    for (nurbs, facet) in [
        ("Faces/6.RectWithHole.ifc", "Faces/6.RectWithHole.facet.ifc"),
        (
            "SolidsAndSheets/WithPlaneSurface.ifc",
            "SolidsAndSheets/WithPlaneSurface.facet.ifc",
        ),
    ] {
        let (Some(a), Some(b)) = (corpus::bentley(nurbs), corpus::bentley(facet)) else {
            continue;
        };
        let (ea, ra) = load(&a);
        let (eb, _) = load(&b);
        let (va, vb) = (mesh_volume(objects(&ea)[0]), mesh_volume(objects(&eb)[0]));
        assert!((va - vb).abs() < vb * 0.01, "{nurbs}: {va} {vb}");
        assert!(ra.skipped.is_empty(), "{nurbs}: {ra:?}");
    }
    // Faces that the STEP reader cannot cut (a cone or a torus without seam edges) are named. The
    // product is still there, as its bounding box.
    if let Some(a) = corpus::bentley("Faces/5.Cone.ifc") {
        let (ed, rep) = load(&a);
        assert_eq!(objects(&ed).len(), 1);
        assert!(
            rep.skipped
                .iter()
                .any(|s| s.reason.contains("imported as a bounding box")),
            "{rep:?}"
        );
    }
}

#[test]
fn tin_scenarios_of_12d_keep_their_holes_and_land_in_their_crs() {
    // UT_Tin_1: a network whose triangles carry the flags 7 (breaklines), -2 (void) and -1 (hole).
    if let Some(file) = corpus::rc("Tin_1 (RC1)/UT_Tin_1.ifc") {
        let text = std::fs::read_to_string(&file).unwrap();
        let at = text.find("IFCTRIANGULATEDIRREGULARNETWORK").unwrap();
        let statement = &text[at..at + text[at..].find(");").unwrap()];
        // The flags are the last list of the statement.
        let flags = &statement[statement.rfind('(').unwrap()..];
        let list: Vec<i64> = flags
            .split(|c: char| !(c.is_ascii_digit() || c == '-'))
            .filter_map(|s| s.parse().ok())
            .collect();
        let hidden = list.iter().filter(|f| **f < 0).count();
        assert!(
            hidden > 0 && hidden < list.len(),
            "{hidden} of {}",
            list.len()
        );
        let (ed, rep) = load(&file);
        let o = objects(&ed);
        assert_eq!(o.len(), 1, "{rep:?}");
        // Holes would be filled by a site model, so the network is a mesh with the visible triangles.
        let Shape::Mesh3d(m) = &o[0].shape else {
            panic!("{}", o[0].shape.kind())
        };
        assert_eq!(m.triangles.len(), list.len() - hidden);
        assert!(
            rep.notes
                .iter()
                .any(|n| n.contains("27 holes and 172 voids")),
            "{:?}",
            rep.notes
        );
    }
    // UT_Tin_in_MGA_56: the coordinates are map coordinates (northing 7 million meters) with an
    // identity map conversion. They are moved to the origin and the shift goes into the georeference.
    if let Some(file) = corpus::rc("Georeferencing_Tin_1 (RC1)/UT_Tin_in_MGA_56.ifc") {
        let (ed, rep) = load(&file);
        assert_eq!(objects(&ed).len(), 1, "{rep:?}");
        let g = ed.doc().georef.as_ref().expect("a georeference");
        assert_eq!(g.crs, "EPSG:28356");
        assert!(
            g.origin.x > 200_000.0 && g.origin.y > 7_000_000.0,
            "{:?}",
            g.origin
        );
        let Shape::Mesh3d(m) = &objects(&ed)[0].shape else {
            panic!()
        };
        let (lo, hi) = m.bounds3();
        assert!(hi.x.abs() < 1.0e6 && lo.y.abs() < 1.0e6, "{lo} {hi}");
        assert!(rep.notes.iter().any(|n| n.contains("far from the origin")));
    }
}

#[test]
fn partial_sites_of_the_scene_are_layers_and_assembly_parts_follow_their_site() {
    let Some(file) = corpus::scene("4", "Infra-Landscaping") else {
        return;
    };
    let (ed, rep) = load(&file);
    let layer_name = |o: &Object| ed.doc().layer(o.layer).unwrap().name.clone();
    let mut used: std::collections::BTreeMap<String, usize> = Default::default();
    for o in objects(&ed) {
        *used.entry(layer_name(o)).or_default() += 1;
    }
    // 1 complex site (it holds one element, the underground) and 7 partial sites.
    assert!(used.contains_key("park - site"), "{used:?}");
    assert!(used.contains_key("meadow - site"), "{used:?}");
    assert!(
        !used.contains_key("Site"),
        "every element is in a partial site: {used:?}"
    );
    assert_eq!(used.values().sum::<usize>(), 101);
    assert_eq!(used.len(), 8, "{used:?}");
    assert_eq!(used["park - site"], 57, "{used:?}");
    assert_eq!(rep.layers_created.len(), 8);
    // The posts of a signal assembly are on the layer of the site of the assembly.
    let posts: Vec<&Object> = objects(&ed)
        .into_iter()
        .filter(|o| o.data["ifc_class"] == "IfcMember")
        .collect();
    assert_eq!(posts.len(), 10);
    assert!(posts.iter().all(|o| layer_name(o) != "Site"));
}

#[test]
fn columns_placed_on_a_grid_land_at_the_crossings() {
    let Some(file) = corpus::iso("grid-placement.ifc") else {
        return;
    };
    let (ed, rep) = load(&file);
    let columns: Vec<&Object> = objects(&ed)
        .into_iter()
        .filter(|o| o.data["ifc_class"] == "IfcColumn")
        .collect();
    assert!(columns.len() >= 20, "{rep:?}");
    // Each column stands at a crossing of two axes: no two are in one place unless the grid says so.
    let mut at: Vec<(i64, i64)> = columns
        .iter()
        .map(|o| {
            let Shape::Mesh3d(m) = &o.shape else { panic!() };
            let (lo, hi) = m.bounds3();
            let c = (lo + hi) / 2.0;
            ((c.x / 10.0).round() as i64, (c.y / 10.0).round() as i64)
        })
        .collect();
    at.sort();
    at.dedup();
    assert_eq!(
        at.len(),
        columns.len(),
        "columns on top of each other: {at:?}"
    );
    assert!(
        !rep.notes.iter().any(|n| n.contains("placements")),
        "{:?}",
        rep.notes
    );
}
