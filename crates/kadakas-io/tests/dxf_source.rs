//! Source tracking of the DXF import (IOP-22): the tags in the object data, the identity of a
//! drawing and its revisions, the layers of an import, blocks, the stored base side, hostile
//! names, undo, save and load, and copies.

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Document, Layer, Object, ObjectId};
use kadakas_core::provenance::{self, ImportTag, LayerSource};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::dxf::hash::{entity_hash, hex};
use kadakas_io::dxf::revision::{self, Options, Rec, Status};
use kadakas_io::{Context, ImportOptions, Importer, IoError, LayerMode, Source};

fn file(header: &str, tables: &str, blocks: &str, entities: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n{tables}0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

/// A header with a fingerprint (the drawing) and a version GUID (the revision).
fn header(fingerprint: &str, version: &str) -> String {
    format!("9\n$FINGERPRINTGUID\n2\n{{{fingerprint}}}\n9\n$VERSIONGUID\n2\n{{{version}}}\n")
}

fn import(ed: &mut Editor, name: &str, text: &str, opts: &ImportOptions) -> kadakas_io::Report {
    try_import(ed, name, text, opts).unwrap()
}

fn try_import(
    ed: &mut Editor,
    name: &str,
    text: &str,
    opts: &ImportOptions,
) -> Result<kadakas_io::Report, IoError> {
    let src = Source {
        name: name.into(),
        data: text.as_bytes().to_vec(),
        extra: Default::default(),
    };
    let r = DxfFormat.import(&src, &Context::of(ed.doc()), opts)?;
    ed.execute(&r.command).unwrap();
    Ok(r.report)
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

fn source_of(l: &Layer) -> LayerSource {
    LayerSource::of(&l.extra[provenance::LAYER_KEY]).unwrap()
}

const LINES: &str = "0\nLINE\n5\n2f\n8\nWalls\n10\n0\n20\n0\n11\n1000\n21\n0\n\
0\nCIRCLE\n5\n30\n8\nTrees\n10\n50\n20\n50\n40\n5\n\
0\nLINE\n8\nWalls\n10\n0\n20\n0\n11\n0\n21\n7\n";

#[test]
fn every_object_gets_its_handle_drawing_revision_and_hashes() {
    let mut ed = fresh();
    let text = file(&header("AAA-1", "BBB-1"), "", "", LINES);
    let report = import(&mut ed, "plan.dxf", &text, &ImportOptions::default());
    assert_eq!(report.drawing.as_deref(), Some("guid:{AAA-1}"));
    assert_eq!(report.revision.as_deref(), Some("guid:{BBB-1}"));
    assert_eq!(ed.doc().objects.len(), 3);
    let all = tags(ed.doc());
    let find = |h: Option<&str>, kind: &str| {
        all.iter()
            .find(|(t, _)| t.handle.as_deref() == h && t.kind == kind)
            .unwrap()
            .0
            .clone()
    };
    let line = find(Some("2F"), "LINE");
    assert_eq!(line.drawing, "guid:{AAA-1}");
    assert_eq!(line.revision, "guid:{BBB-1}");
    // The hash is the one of the hash module, so an update can compute it again.
    let parsed = kadakas_io::dxf::parse::parse(text.as_bytes()).unwrap();
    assert_eq!(line.hash, hex(entity_hash(&parsed.entities[0])));
    assert_eq!(find(Some("30"), "CIRCLE").kind, "CIRCLE");
    let none = find(None, "LINE");
    assert_eq!(none.hash, hex(entity_hash(&parsed.entities[2])));
    assert!(all.iter().all(|(_, o)| o.data.len() == 1));
    // Right after the import no object is edited; a move is an edit.
    assert!(
        all.iter()
            .all(|(_, o)| provenance::is_edited(o) == Some(false))
    );
    let moved = all[0].1.id;
    ed.execute(&Command::MoveObjects {
        ids: vec![moved],
        delta: kadakas_core::DVec2::new(5.0, 0.0),
    })
    .unwrap();
    assert_eq!(
        provenance::is_edited(ed.doc().object(moved).unwrap()),
        Some(true)
    );
    let others = ed.doc().objects.values().filter(|o| o.id != moved);
    assert!(
        others
            .into_iter()
            .all(|o| provenance::is_edited(o) == Some(false))
    );
}

#[test]
fn a_rename_and_a_resave_keep_the_drawing_and_reuse_its_layer() {
    let mut ed = fresh();
    let v1 = file(&header("AAA-1", "BBB-1"), "", "", LINES);
    let r1 = import(&mut ed, "plan.dxf", &v1, &ImportOptions::default());
    assert!(!r1.notes.iter().any(|n| n.contains("imported before")));
    assert_eq!(ed.doc().layers.len(), 2);
    // The same drawing saved again (a new version GUID) under another name.
    let v2 = file(&header("AAA-1", "BBB-2"), "", "", LINES);
    let r2 = import(&mut ed, "plan_v2.dxf", &v2, &ImportOptions::default());
    assert_eq!(r2.drawing, r1.drawing, "the fingerprint is the drawing");
    assert_ne!(r2.revision, r1.revision);
    assert_eq!(ed.doc().layers.len(), 2, "the layer is reused");
    assert!(r2.layers_created.is_empty());
    assert!(r2.notes.iter().any(|n| n.contains("use Update")));
    let layer = ed
        .doc()
        .layers
        .iter()
        .find(|l| l.extra.contains_key(provenance::LAYER_KEY))
        .unwrap();
    assert_eq!(
        source_of(layer).revision,
        "guid:{BBB-2}",
        "the layer records the latest revision"
    );
    assert!(ed.doc().objects.values().all(|o| o.layer == layer.id));
    assert_eq!(ed.doc().objects.len(), 6);
    // Another fingerprint with the same file name is another drawing.
    let other = file(&header("CCC-9", "DDD-1"), "", "", LINES);
    let r3 = import(&mut ed, "plan.dxf", &other, &ImportOptions::default());
    assert_ne!(r3.drawing, r1.drawing);
    assert_eq!(ed.doc().layers.len(), 3);
    assert!(
        ed.doc()
            .layers
            .iter()
            .any(|l| l.name == "plan.dxf (plan.dxf)")
    );
}

#[test]
fn files_without_a_header_with_the_same_name_are_two_drawings() {
    let mut ed = fresh();
    let a = file("", "", "", LINES);
    let b = file("", "", "", "0\nLINE\n8\nX\n10\n0\n20\n0\n11\n9\n21\n9\n");
    let ra = import(&mut ed, "old.dxf", &a, &ImportOptions::default());
    let rb = import(&mut ed, "old.dxf", &b, &ImportOptions::default());
    assert_ne!(ra.drawing, rb.drawing);
    assert_eq!(ed.doc().layers.len(), 3, "each has a layer");
    assert!(
        ra.notes
            .iter()
            .any(|n| n.contains("no header") && n.contains("confirm"))
    );
    // The same file again is the same drawing and revision.
    let again = import(&mut ed, "old.dxf", &a, &ImportOptions::default());
    assert_eq!((again.drawing, again.revision), (ra.drawing, ra.revision));
    assert_eq!(ed.doc().layers.len(), 3);
}

#[test]
fn imported_objects_have_a_layer_of_their_own_with_the_transform() {
    let mut ed = fresh();
    let design = ed.doc().active_layer;
    let text = file(
        &format!("{}9\n$INSUNITS\n70\n6\n", header("A-1", "B-1")),
        "",
        "",
        LINES,
    );
    let report = import(&mut ed, "plan.dxf", &text, &ImportOptions::default());
    let doc = ed.doc();
    assert_eq!(doc.layers.len(), 2);
    let own = layer_named(doc, "plan.dxf");
    assert_eq!(report.layers_created, vec!["plan.dxf"]);
    assert!(
        doc.objects
            .values()
            .all(|o| o.layer == own.id && o.layer != design)
    );
    let src = source_of(own);
    assert_eq!(Some(&src.drawing), report.drawing.as_ref());
    assert_eq!(
        (src.dxf_layer.as_str(), src.unit_scale, src.shift),
        ("", 1000.0, [0.0, 0.0])
    );
    assert!(doc.classes.iter().any(|c| c.name == "Walls"));
}

#[test]
fn a_far_drawing_records_its_shift_and_its_tags_once() {
    // A survey 5 km from the origin is centered; the shift is on the layer.
    let far = "0\nLINE\n5\n2f\n8\nWalls\n10\n5000000\n20\n6000000\n11\n5000010\n21\n6000000\n\
0\nLINE\n5\n30\n8\nWalls\n10\n5000100\n20\n6000000\n11\n5000110\n21\n6000000\n";
    let mut ed = fresh();
    let text = file(
        &format!("{}9\n$INSUNITS\n70\n4\n", header("A-1", "B-1")),
        "",
        "",
        far,
    );
    let report = import(&mut ed, "survey.dxf", &text, &ImportOptions::default());
    assert_eq!(
        report.layers_created,
        vec!["survey.dxf"],
        "the probe pass made no layer"
    );
    assert_eq!(tags(ed.doc()).len(), 2);
    let src = source_of(layer_named(ed.doc(), "survey.dxf"));
    assert!(
        src.shift[0].abs() > 1.0e6 && src.shift[1].abs() > 1.0e6,
        "{:?}",
        src.shift
    );
    assert_eq!(src.unit_scale, 1.0);
}

#[test]
fn a_locked_layer_of_the_drawing_is_reused_and_locked_again() {
    let mut ed = fresh();
    let text = file(&header("A-1", "B-1"), "", "", LINES);
    import(&mut ed, "plan.dxf", &text, &ImportOptions::default());
    let id = layer_named(ed.doc(), "plan.dxf").id;
    ed.execute(&Command::ModifyLayer {
        id,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    let v2 = file(&header("A-1", "B-2"), "", "", LINES);
    let report = import(&mut ed, "plan.dxf", &v2, &ImportOptions::default());
    assert!(report.layers_created.is_empty());
    assert_eq!(ed.doc().layers.len(), 2);
    assert_eq!(
        ed.doc().objects.values().filter(|o| o.layer == id).count(),
        6
    );
    assert!(ed.doc().layer(id).unwrap().locked, "locked again");
    assert!(ed.undo());
    assert_eq!(ed.doc().objects.len(), 3);
    assert!(ed.doc().layer(id).unwrap().locked);
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
    let report = import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), "", "", LINES),
        &opts,
    );
    let doc = ed.doc();
    let walls = layer_named(doc, "Walls (plan.dxf)");
    assert_ne!(walls.id, design);
    assert!(doc.objects.values().all(|o| o.layer != design));
    assert_eq!(source_of(walls).dxf_layer, "Walls");
    assert!(
        layer_named(doc, "Trees")
            .extra
            .contains_key(provenance::LAYER_KEY)
    );
    assert!(report.layers_created.contains(&"Trees".to_string()));
    assert!(!doc.layers.iter().any(|l| l.name == "plan.dxf"));
}

#[test]
fn the_options_choose_the_layer_turn_tracking_off_and_limit_the_size() {
    let text = file(&header("A-1", "B-1"), "", "", LINES);
    let mut ed = fresh();
    let design = ed.doc().active_layer;
    let off = ImportOptions {
        track_source: false,
        ..ImportOptions::default()
    };
    let report = import(&mut ed, "plan", &text, &off);
    assert!(report.drawing.is_none() && report.revision.is_none());
    assert_eq!(ed.doc().layers.len(), 1);
    assert!(ed.doc().resources.is_empty(), "no base without tracking");
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
    import(&mut ed, "plan", &text, &named);
    let survey = layer_named(ed.doc(), "Survey").id;
    assert!(ed.doc().objects.values().all(|o| o.layer == survey));
    assert_eq!(tags(ed.doc()).len(), 3);
    // The size limit is checked before anything is parsed.
    let small = ImportOptions {
        max_bytes: 100,
        ..ImportOptions::default()
    };
    let e = try_import(&mut fresh(), "plan", &text, &small).unwrap_err();
    assert!(
        matches!(&e, IoError::Format(m) if m.contains("max_bytes")),
        "{e}"
    );
}

const BLOCKS: &str = "0\nBLOCK\n5\nB1\n330\nB0\n2\nTREE\n10\n0\n20\n0\n0\nLINE\n5\nB2\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n\
0\nCIRCLE\n5\nB3\n8\n0\n10\n0\n20\n0\n40\n3\n0\nENDBLK\n\
0\nBLOCK\n5\nC1\n2\nGROVE\n10\n0\n20\n0\n0\nINSERT\n5\nC2\n8\n0\n2\nTREE\n10\n100\n20\n0\n0\nENDBLK\n";

const INSERTS: &str = "0\nINSERT\n5\nA1\n8\nTrees\n2\nTREE\n10\n500\n20\n0\n\
0\nINSERT\n5\nA2\n8\nTrees\n2\nTREE\n10\n0\n20\n500\n70\n2\n44\n20\n\
0\nINSERT\n5\nA3\n8\nTrees\n2\nGROVE\n10\n0\n20\n900\n";

#[test]
fn block_content_keeps_the_handles_and_keys_of_inserts_and_definitions() {
    let mut ed = fresh();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), "", BLOCKS, INSERTS),
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
            (
                block.name.as_str(),
                block.key.as_str(),
                block.handle.as_deref()
            ),
            ("TREE", "rec:B0", Some("B1"))
        );
        assert_eq!((t.via.len(), t.via[0].copy), (1, None));
    }
    let handles: Vec<_> = a1.iter().map(|t| t.handle.clone().unwrap()).collect();
    assert_eq!(handles, vec!["B2", "B3"]);
    let mut copies: Vec<_> = of("A2").iter().map(|t| t.via[0].copy.unwrap()).collect();
    copies.sort();
    assert_eq!(copies, vec![0, 0, 1, 1]);
    for t in of("A3") {
        assert_eq!(t.via.len(), 2);
        assert_eq!(t.via[1].handle.as_deref(), Some("C2"));
        assert_eq!(t.block.as_ref().unwrap().name, "TREE");
    }
    let mut keys: Vec<String> = all
        .iter()
        .map(|(t, _)| format!("{:?}{:?}{:?}", t.handle, t.via, t.part))
        .collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 8, "every tag is different");
}

#[test]
fn the_stored_base_maps_back_to_objects_and_a_changed_definition_fans_out() {
    let mut ed = fresh();
    let v1 = file(&header("A-1", "B-1"), "", BLOCKS, INSERTS);
    import(&mut ed, "plan.dxf", &v1, &ImportOptions::default());
    // One base resource, keyed by drawing and revision, that the document keeps.
    assert_eq!(ed.doc().resources.len(), 1);
    let stored = provenance::base_of(ed.doc(), "guid:{A-1}", "guid:{B-1}").unwrap();
    let old: Vec<Rec> = stored["recs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| Rec::from_value(v).unwrap())
        .collect();
    let parsed = kadakas_io::dxf::parse::parse(v1.as_bytes()).unwrap();
    let (mut made, _) = revision::records(&parsed, 1000);
    made.iter_mut().for_each(Rec::compact);
    assert_eq!(
        old, made,
        "the stored summaries are the records of the file"
    );
    // A new file: the circle in the block is bigger, and one INSERT is moved.
    let v2 = file(
        &header("A-1", "B-2"),
        "",
        &BLOCKS.replace("\n40\n3\n", "\n40\n4\n"),
        &INSERTS.replace("500\n20\n0\n", "505\n20\n0\n"),
    );
    let new_dxf = kadakas_io::dxf::parse::parse(v2.as_bytes()).unwrap();
    let (new, _) = revision::records(&new_dxf, 1000);
    let d = revision::diff(&old, &new, &Options::default());
    let changed: Vec<&Rec> = d
        .entries
        .iter()
        .filter(|e| e.status == Status::Changed)
        .map(|e| &old[e.base.unwrap()])
        .collect();
    assert_eq!(changed.len(), 2, "{:?}", d.entries);
    let all = tags(ed.doc());
    let tagged = || all.iter().map(|(t, o)| (o.id, t));
    let circle = changed.iter().find(|r| r.kind == "CIRCLE").unwrap();
    let hit = revision::objects_of(circle, tagged());
    // The circle is in TREE, which is placed by A1, twice by A2 and once through GROVE.
    assert_eq!(hit.len(), 4);
    let insert = changed.iter().find(|r| r.kind == "INSERT").unwrap();
    assert_eq!(insert.handle.as_deref(), Some("A1"));
    let hit = revision::objects_of(insert, tagged());
    assert_eq!(hit.len(), 2, "the line and the circle of that INSERT");
    // None of them was edited by the user, so an update may change them.
    assert!(
        hit.iter()
            .all(|id| provenance::is_edited(ed.doc().object(*id).unwrap()) == Some(false))
    );
    // The same revision is not stored twice; a new one is.
    import(&mut ed, "plan.dxf", &v1, &ImportOptions::default());
    assert_eq!(ed.doc().resources.len(), 1);
    import(&mut ed, "plan.dxf", &v2, &ImportOptions::default());
    assert_eq!(ed.doc().resources.len(), 2);
}

#[test]
fn one_entity_with_several_objects_numbers_its_parts() {
    let hatch = "0\nHATCH\n5\nH1\n8\nH\n70\n0\n91\n2\n\
92\n7\n72\n0\n73\n1\n93\n3\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n97\n0\n\
92\n7\n72\n0\n73\n1\n93\n3\n10\n20\n20\n0\n10\n30\n20\n0\n10\n30\n20\n10\n97\n0\n";
    let mut ed = fresh();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), "", "", hatch),
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
        assert!(report.drawing.unwrap().chars().count() <= provenance::MAX_TEXT);
    }
}

#[test]
fn undo_removes_the_layers_objects_and_base_in_one_step() {
    let mut ed = fresh();
    let before = ed.doc().clone();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), "", "", LINES),
        &ImportOptions::default(),
    );
    assert_eq!(ed.doc().resources.len(), 1);
    assert!(ed.undo());
    assert_eq!(ed.doc().layers, before.layers);
    assert!(ed.doc().objects.is_empty() && ed.doc().resources.is_empty());
    assert!(ed.redo());
    assert!(
        ed.doc()
            .layers
            .iter()
            .any(|l| l.extra.contains_key(provenance::LAYER_KEY))
    );
}

#[test]
fn tags_and_the_base_survive_a_save_and_a_copy_drops_the_tag() {
    let mut ed = fresh();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), "", "", LINES),
        &ImportOptions::default(),
    );
    let text = serde_json::to_string(ed.doc()).unwrap();
    let loaded = Document::from_json(&text).unwrap();
    assert_eq!(&loaded, ed.doc());
    assert!(loaded.load_notes().is_empty());
    assert_eq!(tags(&loaded).len(), 3);
    assert!(provenance::base_of(&loaded, "guid:{A-1}", "guid:{B-1}").is_some());

    let id = ed.doc().objects.values().next().unwrap().id;
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![id],
            delta: Default::default(),
        })
        .unwrap();
    let copy = ed.doc().object(ObjectId(out.created[0])).unwrap();
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
fn a_bad_tag_or_base_in_a_file_is_dropped_on_load() {
    let mut ed = fresh();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), "", "", LINES),
        &ImportOptions::default(),
    );
    let text = serde_json::to_string(ed.doc()).unwrap();
    let broken = text.replace("\"kind\":\"CIRCLE\"", "\"kind\":\"\"");
    assert_ne!(text, broken);
    let loaded = Document::from_json(&broken).unwrap();
    assert_eq!(tags(&loaded).len(), 2);
    assert_eq!(loaded.load_notes().len(), 1);
    // A base that is not valid does not stop the file from opening.
    let broken = text.replace("\"recs\":[", "\"recs\":[5,");
    assert_ne!(text, broken);
    let loaded = Document::from_json(&broken).unwrap();
    assert!(loaded.resources.is_empty());
    assert!(loaded.load_notes().iter().any(|n| n.contains("removed")));
}

#[test]
fn tags_do_not_change_the_geometry_or_the_report_counts() {
    let text = file(&header("A-1", "B-1"), "", "", LINES);
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

#[test]
fn a_hidden_dxf_layer_makes_a_hidden_source_layer() {
    // A frozen DXF layer (flag 1) is hidden, and the layer of its source is too.
    let tables = "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nWalls\n70\n1\n62\n7\n0\nLAYER\n2\nTrees\n70\n0\n62\n3\n0\nENDTAB\n";
    let opts = ImportOptions {
        layer_mode: LayerMode::Layers,
        ..ImportOptions::default()
    };
    let mut ed = fresh();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), tables, "", LINES),
        &opts,
    );
    assert!(!layer_named(ed.doc(), "Walls").visible);
    assert!(layer_named(ed.doc(), "Trees").visible);
    assert_eq!(source_of(layer_named(ed.doc(), "Walls")).dxf_layer, "Walls");
    // The next revision finds the hidden layer again and does not make another.
    let before = ed.doc().layers.len();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-2"), tables, "", LINES),
        &opts,
    );
    assert_eq!(ed.doc().layers.len(), before);
    // The same with all DXF layers in classes: a class is hidden, the file layer is not.
    let mut ed = fresh();
    import(
        &mut ed,
        "plan.dxf",
        &file(&header("A-1", "B-1"), tables, "", LINES),
        &ImportOptions::default(),
    );
    assert!(layer_named(ed.doc(), "plan.dxf").visible);
    assert!(
        !ed.doc()
            .classes
            .iter()
            .find(|c| c.name == "Walls")
            .unwrap()
            .visible
    );
}
