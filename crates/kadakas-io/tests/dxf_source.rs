//! Source tracking of the DXF import (IOP-22): the tags in the object data, the layers of an
//! import, blocks, hostile names, undo, save and load, and copies.

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Document, Layer, Object};
use kadakas_core::provenance::{self, ImportTag, LayerSource};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::dxf::revision::{entity_hash, hex};
use kadakas_io::{Context, ImportOptions, Importer, LayerMode, Source};

fn file(header: &str, tables: &str, blocks: &str, entities: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n{tables}0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

fn import(ed: &mut Editor, name: &str, text: &str, opts: &ImportOptions) -> kadakas_io::Report {
    let src = Source {
        name: name.into(),
        data: text.as_bytes().to_vec(),
        extra: Default::default(),
    };
    let r = DxfFormat
        .import(&src, &Context::of(ed.doc()), opts)
        .unwrap();
    ed.execute(&r.command).unwrap();
    r.report
}

fn fresh() -> Editor {
    Editor::new(Document::with_sequential_ids())
}

fn tags(doc: &Document) -> Vec<(ImportTag, &Object)> {
    let mut v: Vec<_> = doc
        .objects
        .values()
        .filter_map(|o| provenance::tag(&o.data).map(|t| (t, o)))
        .collect();
    v.sort_by(|a, b| {
        a.0.handle
            .cmp(&b.0.handle)
            .then(a.0.via.len().cmp(&b.0.via.len()))
    });
    v
}

fn layer_named<'a>(doc: &'a Document, name: &str) -> &'a Layer {
    doc.layers.iter().find(|l| l.name == name).unwrap()
}

const LINES: &str = "0\nLINE\n5\n2f\n8\nWalls\n10\n0\n20\n0\n11\n1000\n21\n0\n\
0\nCIRCLE\n5\n30\n8\nTrees\n10\n50\n20\n50\n40\n5\n\
0\nLINE\n8\nWalls\n10\n0\n20\n0\n11\n0\n21\n7\n";

#[test]
fn every_object_gets_its_handle_source_and_hash() {
    let mut ed = fresh();
    let text = file("9\n$ACADVER\n1\nAC1027\n", "", "", LINES);
    let report = import(&mut ed, "plan", &text, &ImportOptions::default());
    let source = report.source.clone().unwrap();
    assert!(source.starts_with("plan#"), "{source}");
    assert_eq!(ed.doc().objects.len(), 3);
    let by_handle = |h: Option<&str>| {
        tags(ed.doc())
            .into_iter()
            .find(|(t, _)| t.handle.as_deref() == h && (h.is_some() || t.kind == "LINE"))
            .unwrap()
            .0
    };
    let line = by_handle(Some("2F"));
    assert_eq!(
        (line.source.as_str(), line.kind.as_str()),
        (source.as_str(), "LINE")
    );
    // The hash is the one of the revision module, so an update can compute it again.
    let parsed = kadakas_io::dxf::parse::parse(text.as_bytes()).unwrap();
    assert_eq!(line.hash, hex(entity_hash(&parsed.entities[0])));
    assert_eq!(by_handle(Some("30")).kind, "CIRCLE");
    // An entity without a handle still has a hash.
    let none = by_handle(None);
    assert_eq!(none.handle, None);
    assert_eq!(none.hash, hex(entity_hash(&parsed.entities[2])));
    assert!(tags(ed.doc()).iter().all(|(_, o)| o.data.len() == 1));
}

#[test]
fn two_files_with_the_same_name_have_different_sources() {
    let a = file("9\n$ACADVER\n1\nAC1027\n", "", "", LINES);
    let b = file("9\n$ACADVER\n1\nAC1032\n", "", "", LINES);
    let (sa, sb) = (
        import(&mut fresh(), "plan", &a, &ImportOptions::default()).source,
        import(&mut fresh(), "plan", &b, &ImportOptions::default()).source,
    );
    assert_ne!(sa, sb);
    assert_eq!(
        sa,
        import(&mut fresh(), "plan", &a, &ImportOptions::default()).source
    );
}

#[test]
fn imported_objects_have_a_layer_of_their_own() {
    let mut ed = fresh();
    let design = ed.doc().active_layer;
    let text = file("", "", "", LINES);
    let report = import(&mut ed, "plan", &text, &ImportOptions::default());
    let doc = ed.doc();
    assert_eq!(doc.layers.len(), 2);
    let own = layer_named(doc, "plan");
    assert_eq!(report.layers_created, vec!["plan"]);
    assert!(
        doc.objects
            .values()
            .all(|o| o.layer == own.id && o.layer != design)
    );
    let src = LayerSource::of(&own.extra[provenance::LAYER_KEY]).unwrap();
    assert_eq!(Some(&src.source), report.source.as_ref());
    assert_eq!(src.dxf_layer, "");
    // The DXF layers are classes, as before.
    assert!(doc.classes.iter().any(|c| c.name == "Walls"));
    // The same source again goes to the same layer.
    let before = doc.layers.len();
    import(&mut ed, "plan", &text, &ImportOptions::default());
    assert_eq!(ed.doc().layers.len(), before);
    assert_eq!(ed.doc().objects.len(), 6);
    // Another file with the same name does not take the layer: it gets a name that tells them apart.
    let other = file("9\n$ACADVER\n1\nAC1032\n", "", "", LINES);
    import(&mut ed, "plan", &other, &ImportOptions::default());
    assert_eq!(ed.doc().layers.len(), before + 1);
    assert!(ed.doc().layers.iter().any(|l| l.name == "plan (plan)"));
}

#[test]
fn a_layer_of_the_design_work_is_never_reused() {
    let mut ed = fresh();
    ed.execute(&Command::AddLayer {
        name: "Walls".into(),
        elevation: 0.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let design = layer_named(ed.doc(), "Walls").id;
    let opts = ImportOptions {
        layer_mode: LayerMode::Layers,
        ..ImportOptions::default()
    };
    let report = import(&mut ed, "plan", &file("", "", "", LINES), &opts);
    let doc = ed.doc();
    let walls = layer_named(doc, "Walls (plan)");
    assert_ne!(walls.id, design);
    assert!(doc.objects.values().all(|o| o.layer != design));
    let src = LayerSource::of(&walls.extra[provenance::LAYER_KEY]).unwrap();
    assert_eq!(src.dxf_layer, "Walls");
    assert!(
        layer_named(doc, "Trees")
            .extra
            .contains_key(provenance::LAYER_KEY)
    );
    assert!(report.layers_created.contains(&"Trees".to_string()));
    // No extra layer for the whole file in this mode.
    assert!(!doc.layers.iter().any(|l| l.name == "plan"));
}

#[test]
fn the_options_choose_the_layer_or_turn_tracking_off() {
    let text = file("", "", "", LINES);
    let mut ed = fresh();
    let design = ed.doc().active_layer;
    let off = ImportOptions {
        track_source: false,
        ..ImportOptions::default()
    };
    let report = import(&mut ed, "plan", &text, &off);
    assert!(report.source.is_none());
    assert_eq!(ed.doc().layers.len(), 1);
    assert!(
        ed.doc()
            .objects
            .values()
            .all(|o| o.layer == design && !o.data.contains_key(provenance::KEY))
    );
    // A named layer wins, and the tags stay.
    let mut ed = fresh();
    let named = ImportOptions {
        layer: Some("Survey".into()),
        source_name: Some("survey.dxf".into()),
        ..ImportOptions::default()
    };
    let report = import(&mut ed, "plan", &text, &named);
    let survey = layer_named(ed.doc(), "Survey").id;
    assert!(ed.doc().objects.values().all(|o| o.layer == survey));
    assert_eq!(tags(ed.doc()).len(), 3);
    assert!(report.source.unwrap().starts_with("survey.dxf#"));
}

const BLOCKS: &str = "0\nBLOCK\n5\nB1\n2\nTREE\n10\n0\n20\n0\n0\nLINE\n5\nB2\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n\
0\nCIRCLE\n5\nB3\n8\n0\n10\n0\n20\n0\n40\n3\n0\nENDBLK\n\
0\nBLOCK\n5\nC1\n2\nGROVE\n10\n0\n20\n0\n0\nINSERT\n5\nC2\n8\n0\n2\nTREE\n10\n100\n20\n0\n0\nENDBLK\n";

#[test]
fn block_content_keeps_the_handles_of_the_inserts_and_the_definition() {
    let mut ed = fresh();
    let ents = "0\nINSERT\n5\nA1\n8\nTrees\n2\nTREE\n10\n500\n20\n0\n\
0\nINSERT\n5\nA2\n8\nTrees\n2\nTREE\n10\n0\n20\n500\n70\n2\n44\n20\n\
0\nINSERT\n5\nA3\n8\nTrees\n2\nGROVE\n10\n0\n20\n900\n";
    import(
        &mut ed,
        "plan",
        &file("", "", BLOCKS, ents),
        &ImportOptions::default(),
    );
    let all = tags(ed.doc());
    // A1: 2 objects. A2: 2 copies of 2 objects. A3: GROVE holds one INSERT of TREE: 2 objects.
    assert_eq!(all.len(), 8);
    let of = |insert: &str| -> Vec<&ImportTag> {
        all.iter()
            .map(|(t, _)| t)
            .filter(|t| t.via.first().and_then(|v| v.handle.as_deref()) == Some(insert))
            .collect()
    };
    let a1 = of("A1");
    assert_eq!(a1.len(), 2);
    for t in &a1 {
        let block = t.block.as_ref().unwrap();
        assert_eq!(
            (block.name.as_str(), block.handle.as_deref()),
            ("TREE", Some("B1"))
        );
        assert_eq!(t.via.len(), 1);
        assert_eq!(t.via[0].copy, None);
    }
    let handles: Vec<_> = a1.iter().map(|t| t.handle.clone().unwrap()).collect();
    assert_eq!(handles, vec!["B2", "B3"]);
    // The copies of an array are told apart.
    let a2 = of("A2");
    let mut copies: Vec<_> = a2.iter().map(|t| t.via[0].copy.unwrap()).collect();
    copies.sort();
    assert_eq!(copies, vec![0, 0, 1, 1]);
    // Blocks inside blocks: the outer INSERT first.
    let a3 = of("A3");
    assert_eq!(a3.len(), 2);
    for t in a3 {
        assert_eq!(t.via.len(), 2);
        assert_eq!(t.via[1].handle.as_deref(), Some("C2"));
        let block = t.block.as_ref().unwrap();
        assert_eq!(block.name, "TREE", "the innermost block");
    }
    // Every tag is different, so an update can find each object again.
    let mut keys: Vec<String> = all
        .iter()
        .map(|(t, _)| format!("{:?}{:?}{:?}", t.handle, t.via, t.part))
        .collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 8);
}

#[test]
fn one_entity_with_several_objects_numbers_its_parts() {
    let hatch = "0\nHATCH\n5\nH1\n8\nH\n70\n0\n91\n2\n\
92\n7\n72\n0\n73\n1\n93\n3\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n97\n0\n\
92\n7\n72\n0\n73\n1\n93\n3\n10\n20\n20\n0\n10\n30\n20\n0\n10\n30\n20\n10\n97\n0\n";
    let mut ed = fresh();
    import(
        &mut ed,
        "plan",
        &file("", "", "", hatch),
        &ImportOptions::default(),
    );
    let all = tags(ed.doc());
    let mut parts: Vec<_> = all
        .iter()
        .map(|(t, _)| (t.handle.clone(), t.part))
        .collect();
    parts.sort();
    assert_eq!(
        parts,
        vec![(Some("H1".into()), Some(0)), (Some("H1".into()), Some(1))]
    );
}

#[test]
fn hostile_names_and_handles_never_break_the_import() {
    let long = "N".repeat(5000);
    let blocks = format!(
        "0\nBLOCK\n5\nB1\n2\n{long}\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n"
    );
    let ents = format!(
        "0\nINSERT\n5\nnot a handle!\n2\n{long}\n10\n0\n20\n0\n\
0\nLINE\n5\n{}\n8\nLay\x07er\n10\n0\n20\n0\n11\n5\n21\n5\n\
0\nINSERT\n5\n\n2\n\n10\n0\n20\n0\n",
        "F".repeat(500)
    );
    for mode in [LayerMode::Classes, LayerMode::Layers] {
        let mut ed = fresh();
        let opts = ImportOptions {
            layer_mode: mode,
            source_name: Some(format!("a\nb{long}")),
            ..ImportOptions::default()
        };
        let report = import(&mut ed, "x", &file("", "", &blocks, &ents), &opts);
        assert_eq!(report.total(), 2);
        let all = tags(ed.doc());
        assert_eq!(all.len(), 2, "{:?}", report.skipped);
        assert!(all.iter().all(|(t, _)| t.problem().is_none()));
        assert!(all.iter().any(|(t, _)| t.handle.is_none()));
        assert!(report.source.unwrap().chars().count() <= provenance::MAX_TEXT);
    }
}

#[test]
fn undo_removes_the_layers_and_objects_in_one_step() {
    let mut ed = fresh();
    let before = ed.doc().clone();
    import(
        &mut ed,
        "plan",
        &file("", "", "", LINES),
        &ImportOptions::default(),
    );
    assert!(ed.undo());
    assert_eq!(ed.doc().layers, before.layers);
    assert!(ed.doc().objects.is_empty());
    assert!(ed.redo());
    assert!(
        ed.doc()
            .layers
            .iter()
            .any(|l| l.extra.contains_key(provenance::LAYER_KEY))
    );
}

#[test]
fn tags_survive_a_save_and_a_copy_drops_them() {
    let mut ed = fresh();
    import(
        &mut ed,
        "plan",
        &file("", "", "", LINES),
        &ImportOptions::default(),
    );
    let text = serde_json::to_string(ed.doc()).unwrap();
    let loaded = Document::from_json(&text).unwrap();
    assert_eq!(&loaded, ed.doc());
    assert!(loaded.load_notes().is_empty());
    assert_eq!(tags(&loaded).len(), 3);
    assert!(
        loaded
            .layers
            .iter()
            .any(|l| l.extra.contains_key(provenance::LAYER_KEY))
    );

    // A copy is new work: it keeps the layer but not the origin.
    let id = ed.doc().objects.values().next().unwrap().id;
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![id],
            delta: Default::default(),
        })
        .unwrap();
    let copy = ed
        .doc()
        .object(kadakas_core::model::ObjectId(out.created[0]))
        .unwrap();
    assert!(!copy.data.contains_key(provenance::KEY));
    assert_eq!(copy.layer, ed.doc().object(id).unwrap().layer);
    assert!(
        ed.doc()
            .object(id)
            .unwrap()
            .data
            .contains_key(provenance::KEY)
    );
}

#[test]
fn a_bad_tag_in_a_file_is_dropped_on_load() {
    let mut ed = fresh();
    import(
        &mut ed,
        "plan",
        &file("", "", "", LINES),
        &ImportOptions::default(),
    );
    let text = serde_json::to_string(ed.doc()).unwrap();
    let broken = text.replace("\"kind\":\"CIRCLE\"", "\"kind\":\"\"");
    assert_ne!(text, broken);
    let loaded = Document::from_json(&broken).unwrap();
    assert_eq!(tags(&loaded).len(), 2);
    assert_eq!(loaded.load_notes().len(), 1);
}

#[test]
fn tags_do_not_change_the_geometry_or_the_report_counts() {
    let text = file("", "", "", LINES);
    let (mut a, mut b) = (fresh(), fresh());
    let off = ImportOptions {
        track_source: false,
        ..ImportOptions::default()
    };
    let (ra, rb) = (
        import(&mut a, "plan", &text, &ImportOptions::default()),
        import(&mut b, "plan", &text, &off),
    );
    assert_eq!(ra.objects, rb.objects);
    let shapes = |d: &Document| {
        d.objects
            .values()
            .map(|o| o.shape.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(shapes(a.doc()), shapes(b.doc()));
}
