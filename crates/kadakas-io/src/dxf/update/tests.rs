//! Tests of the update from DXF.

use super::*;
use crate::{Format, Source, importer};
use kadakas_core::DVec2;
use kadakas_core::model::Shape;
use serde_json::Value;

const GUID: &str = "A1B2C3D4-0001";

/// A DXF file of one drawing (`GUID`) in the revision `version`, with handles.
fn file(version: &str, blocks: &str, ents: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$FINGERPRINTGUID\n2\n{GUID}\n9\n$VERSIONGUID\n2\n{version}\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(handle: u32, layer: &str, y: i64) -> String {
    format!("0\nLINE\n5\n{handle:X}\n8\n{layer}\n10\n0\n20\n{y}\n11\n1000\n21\n{y}\n")
}

fn text(handle: u32, y: i64, t: &str) -> String {
    format!("0\nTEXT\n5\n{handle:X}\n8\nA\n10\n0\n20\n{y}\n40\n100\n1\n{t}\n")
}

fn block(name: &str, record: u32, body: &str) -> String {
    format!(
        "0\nBLOCK\n5\n{:X}\n2\n{name}\n10\n0\n20\n0\n{body}0\nENDBLK\n",
        record + 1
    )
}

fn insert(handle: u32, name: &str, x: i64) -> String {
    format!("0\nINSERT\n5\n{handle:X}\n8\nA\n2\n{name}\n10\n{x}\n20\n0\n")
}

fn import_text(ed: &mut Editor, name: &str, data: &str, opts: &ImportOptions) {
    let src = Source {
        name: name.into(),
        data: data.as_bytes().to_vec(),
        extra: Default::default(),
    };
    let r = importer(Format::Dxf)
        .import(&src, &Context::of(ed.doc()), opts)
        .unwrap();
    ed.execute(&r.command).unwrap();
}

fn editor_with(data: &str) -> Editor {
    let mut ed = Editor::new(Document::with_sequential_ids());
    import_text(&mut ed, "plan.dxf", data, &ImportOptions::default());
    ed
}

fn no_link(_: &FileLink) -> Result<Vec<Rec>, LinkError> {
    Err(LinkError::Gone("old.dxf".into()))
}

fn opts() -> ImportOptions {
    ImportOptions::default()
}

fn make(doc: &Document, data: &str) -> Result<Plan, UpdateError> {
    make_with(doc, data, None, &opts())
}

fn make_with(
    doc: &Document,
    data: &str,
    old: Option<&str>,
    options: &ImportOptions,
) -> Result<Plan, UpdateError> {
    plan(&Request {
        doc,
        drawing: None,
        name: "plan_rev2.dxf",
        data: data.as_bytes(),
        new_link: None,
        old_file: old.map(|o| (o.as_bytes(), "old.dxf")),
        read_link: &no_link,
        options,
        tolerance: None,
        layers: &[],
        max_bytes: 1 << 30,
    })
}

fn apply_all(ed: &mut Editor, plan: &Plan) -> Built {
    let built = plan
        .build(ed.doc(), &vec![Take::All; plan.changes.len()])
        .unwrap();
    ed.execute(&built.command).unwrap();
    built
}

fn object_at(doc: &Document, handle: u32) -> Option<&Object> {
    let h = format!("{handle:X}");
    doc.objects.values().find(|o| {
        provenance::tag(&o.data)
            .and_then(|t| t.handle)
            .is_some_and(|x| x == h)
            && provenance::tag(&o.data).is_some_and(|t| t.block.is_none())
    })
}

fn y_of(o: &Object) -> f64 {
    match &o.shape {
        Shape::Line { a, .. } => a.y,
        other => panic!("not a line: {other:?}"),
    }
}

fn base_text() -> String {
    file(
        "AAAA0001",
        "",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 100),
            line(0x12, "A", 200),
            line(0x13, "A", 300),
            line(0x14, "A", 400),
        ]
        .concat(),
    )
}

fn revision_two() -> String {
    // 0x10 same; 0x11 moved; 0x12 on another layer; 0x13 removed; 0x14 same; 0x15 added.
    file(
        "AAAA0002",
        "",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 150),
            line(0x12, "B", 200),
            line(0x14, "A", 400),
            line(0x15, "A", 500),
        ]
        .concat(),
    )
}

fn kinds(plan: &Plan) -> Vec<(Kind, String)> {
    plan.changes
        .iter()
        .map(|c| (c.status, c.handle.clone().unwrap_or_default()))
        .collect()
}

#[test]
fn moved_changed_added_and_removed_entities_are_found_and_applied() {
    let mut ed = editor_with(&base_text());
    let before = ed.doc().clone();
    let plan = make(ed.doc(), &revision_two()).unwrap();
    assert_eq!(plan.old_from, "base");
    assert_eq!(plan.counts.unchanged, 2);
    assert_eq!(plan.counts.changed, 2);
    assert_eq!(plan.counts.added, 1);
    assert_eq!(plan.counts.removed, 1);
    assert_eq!(plan.counts.conflicts, 0);
    let k = kinds(&plan);
    assert!(k.contains(&(Kind::Added, "15".into())));
    assert!(k.contains(&(Kind::Removed, "13".into())));
    let moved = plan
        .changes
        .iter()
        .find(|c| c.handle.as_deref() == Some("11"));
    assert_eq!(moved.unwrap().what, vec![Prop::Geometry]);
    let layer = plan
        .changes
        .iter()
        .find(|c| c.handle.as_deref() == Some("12"));
    assert_eq!(layer.unwrap().what, vec![Prop::Layer]);
    // Planning changes nothing.
    assert_eq!(*ed.doc(), before);

    let built = apply_all(&mut ed, &plan);
    assert_eq!(built.applied.added, 1);
    assert_eq!(built.applied.replaced, 2);
    assert_eq!(built.applied.removed, 1);
    // The update data of the old revision goes, and that of the new one comes.
    assert_eq!(built.applied.dropped_base, 1);
    assert!(provenance::base_recs(ed.doc(), &plan.drawing, &plan.old_revision).is_none());
    let doc = ed.doc();
    assert!((y_of(object_at(doc, 0x11).unwrap()) - 150.0).abs() < 1e-9);
    assert!(object_at(doc, 0x13).is_none());
    assert!(object_at(doc, 0x15).is_some());
    // The objects have the new revision, the layer too, and the base of the new revision is there.
    let src_rev: Vec<String> = doc
        .layers
        .iter()
        .filter_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .map(|s| s.revision)
        .collect();
    assert_eq!(src_rev, vec![plan.new_revision.clone()]);
    for o in doc.objects.values() {
        let t = provenance::tag(&o.data).unwrap();
        // The objects that the update touched carry the new revision, the others the old one.
        let touched = matches!(t.handle.as_deref(), Some("11" | "12" | "15"));
        let want = if touched {
            &plan.new_revision
        } else {
            &plan.old_revision
        };
        assert_eq!(&t.revision, want, "{:?}", t.handle);
        // Nothing the update made counts as an edit by the person.
        assert_eq!(provenance::is_edited(o), Some(false), "{:?}", o.id);
    }
    assert!(provenance::base_recs(doc, &plan.drawing, &plan.new_revision).is_some());
    // The class of the DXF layer B came with the changed object.
    assert!(doc.classes.iter().any(|c| c.name == "B"));

    // One undo step brings the old drawing back.
    assert!(ed.undo());
    assert_eq!(ed.doc().objects, before.objects);
    assert_eq!(ed.doc().layers, before.layers);
    assert_eq!(ed.doc().resources, before.resources);
    assert!(provenance::base_recs(ed.doc(), &plan.drawing, &plan.old_revision).is_some());
}

#[test]
fn a_second_update_goes_from_the_new_base() {
    let mut ed = editor_with(&base_text());
    let plan = make(ed.doc(), &revision_two()).unwrap();
    apply_all(&mut ed, &plan);
    // The same file again: nothing changes.
    let again = make(ed.doc(), &revision_two()).unwrap();
    assert!(again.changes.is_empty());
    assert_eq!(again.counts.unchanged, 5);
    // A third revision changes one entity against the second.
    let third = file(
        "AAAA0003",
        "",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 175),
            line(0x12, "B", 200),
            line(0x14, "A", 400),
            line(0x15, "A", 500),
        ]
        .concat(),
    );
    let plan3 = make(ed.doc(), &third).unwrap();
    assert_eq!(plan3.old_revision, plan.new_revision);
    assert_eq!(kinds(&plan3), vec![(Kind::Changed, "11".into())]);
}

#[test]
fn renumbered_handles_are_matched_by_content() {
    let mut ed = editor_with(&base_text());
    // The same drawing written by another program: every handle is new, one line moved.
    let renumbered = file(
        "AAAA0002",
        "",
        &[
            line(0x11, "A", 0),
            line(0x12, "A", 103),
            line(0x13, "A", 200),
            line(0x14, "A", 300),
            line(0x10, "A", 400),
        ]
        .concat(),
    );
    let mut req = request_for(ed.doc(), &renumbered);
    req.tolerance = Some(10.0);
    let plan = super::plan(&req).unwrap();
    assert!(plan.renumbered);
    assert_eq!(plan.counts.changed, 1, "{:?}", plan.changes);
    assert_eq!(plan.counts.added, 0);
    assert_eq!(plan.counts.removed, 0);
    assert_eq!(plan.counts.unchanged, 4);
    assert_eq!(plan.changes[0].via, Some(Via::Proximity));
    apply_all(&mut ed, &plan);
    assert_eq!(ed.doc().objects.len(), 5);
    let ys: Vec<i64> = {
        let mut v: Vec<i64> = ed.doc().objects.values().map(|o| y_of(o) as i64).collect();
        v.sort();
        v
    };
    assert_eq!(ys, vec![0, 103, 200, 300, 400]);
}

#[test]
fn a_changed_block_definition_fans_out_to_its_instances() {
    let body = |y: i64| line(0x30, "A", y);
    let base = file(
        "AAAA0001",
        &block("TREE", 0x20, &body(0)),
        &[
            insert(0x40, "TREE", 0),
            insert(0x41, "TREE", 5000),
            insert(0x42, "TREE", 9000),
        ]
        .concat(),
    );
    let new = file(
        "AAAA0002",
        &block("TREE", 0x20, &body(70)),
        &[
            insert(0x40, "TREE", 0),
            insert(0x41, "TREE", 5000),
            insert(0x42, "TREE", 9000),
        ]
        .concat(),
    );
    let mut ed = editor_with(&base);
    assert_eq!(ed.doc().objects.len(), 3);
    let plan = make(ed.doc(), &new).unwrap();
    assert_eq!(plan.counts.changed, 1, "{:?}", plan.changes);
    assert_eq!(plan.changes[0].objects, 3);
    assert_eq!(plan.counts.objects_replaced, 3);
    apply_all(&mut ed, &plan);
    let mut ys: Vec<i64> = ed.doc().objects.values().map(|o| y_of(o) as i64).collect();
    ys.sort();
    assert_eq!(ys, vec![70, 70, 70]);
    // One more instance and a moved one.
    let more = file(
        "AAAA0003",
        &block("TREE", 0x20, &body(70)),
        &[
            insert(0x40, "TREE", 100),
            insert(0x41, "TREE", 5000),
            insert(0x42, "TREE", 9000),
            insert(0x43, "TREE", 12000),
        ]
        .concat(),
    );
    let plan = make(ed.doc(), &more).unwrap();
    assert_eq!(plan.counts.added, 1);
    assert_eq!(plan.counts.changed, 1);
    apply_all(&mut ed, &plan);
    assert_eq!(ed.doc().objects.len(), 4);
}

#[test]
fn an_edited_object_that_the_revision_changes_is_a_conflict() {
    let mut ed = editor_with(&base_text());
    let id = object_at(ed.doc(), 0x11).unwrap().id;
    let removed = object_at(ed.doc(), 0x13).unwrap().id;
    let untouched = object_at(ed.doc(), 0x12).unwrap().id;
    // The person moves 0x11 and 0x13, and leaves 0x12.
    ed.execute(&Command::MoveObjects {
        ids: vec![id, removed],
        delta: DVec2::new(0.0, 7.0),
    })
    .unwrap();
    // 0x14 is edited too, but the revision does not change it.
    let calm = object_at(ed.doc(), 0x14).unwrap().id;
    ed.execute(&Command::MoveObjects {
        ids: vec![calm],
        delta: DVec2::new(0.0, 3.0),
    })
    .unwrap();
    let plan = make(ed.doc(), &revision_two()).unwrap();
    assert_eq!(plan.counts.conflicts, 2);
    let conflicts: BTreeSet<String> = plan
        .changes
        .iter()
        .filter(|c| c.conflict)
        .map(|c| c.handle.clone().unwrap())
        .collect();
    assert_eq!(
        conflicts,
        BTreeSet::from(["11".to_string(), "13".to_string()])
    );
    let edited = &plan.changes.iter().find(|c| c.conflict).unwrap().edited_ids;
    assert_eq!(edited.len(), 1);

    // Everything but the conflicts.
    let chosen: Vec<Take> = plan.changes.iter().map(|c| (!c.conflict).into()).collect();
    let built = plan.build(ed.doc(), &chosen).unwrap();
    let mut partial = Editor::new(ed.doc().clone());
    partial.execute(&built.command).unwrap();
    assert!((y_of(partial.doc().objects.get(&id).unwrap()) - 107.0).abs() < 1e-9);
    assert!(partial.doc().objects.contains_key(&removed));
    assert_eq!(built.applied.removed, 0);
    assert_eq!(built.applied.replaced, 1);
    // The calm edit stays, and the object keeps its tag.
    assert!((y_of(partial.doc().objects.get(&calm).unwrap()) - 403.0).abs() < 1e-9);
    let t = provenance::tag(&partial.doc().objects.get(&calm).unwrap().data).unwrap();
    assert_eq!(t.revision, plan.old_revision);
    assert_eq!(
        provenance::is_edited(partial.doc().objects.get(&untouched).unwrap()),
        Some(false)
    );

    // Everything, the conflicts too.
    let built = plan
        .build(ed.doc(), &vec![Take::All; plan.changes.len()])
        .unwrap();
    ed.execute(&built.command).unwrap();
    assert!((y_of(ed.doc().objects.get(&id).unwrap()) - 150.0).abs() < 1e-9);
    assert!(!ed.doc().objects.contains_key(&removed));
}

#[test]
fn a_conflict_can_keep_the_edited_objects_and_update_the_others() {
    let body = |y: i64| line(0x30, "A", y);
    let inserts = [
        insert(0x40, "TREE", 0),
        insert(0x41, "TREE", 5000),
        insert(0x42, "TREE", 9000),
    ]
    .concat();
    let mut ed = editor_with(&file("AAAA0001", &block("TREE", 0x20, &body(0)), &inserts));
    // The person moves one of the three trees.
    let edited = ed.doc().objects.keys().copied().next().unwrap();
    ed.execute(&Command::MoveObjects {
        ids: vec![edited],
        delta: DVec2::new(0.0, 5.0),
    })
    .unwrap();
    let plan = make(
        ed.doc(),
        &file("AAAA0002", &block("TREE", 0x20, &body(70)), &inserts),
    )
    .unwrap();
    assert_eq!(plan.counts.conflicts, 1);
    assert_eq!(plan.changes[0].objects, 3);
    let built = plan
        .build(ed.doc(), &vec![Take::KeepEdits; plan.changes.len()])
        .unwrap();
    assert_eq!((built.applied.replaced, built.applied.kept_edits), (2, 1));
    ed.execute(&built.command).unwrap();
    let mut ys: Vec<i64> = ed.doc().objects.values().map(|o| y_of(o) as i64).collect();
    ys.sort();
    assert_eq!(
        ys,
        vec![5, 70, 70],
        "the edited tree stays where the person put it"
    );
    // Take::All replaces the edit as well.
    let mut again = editor_with(&file("AAAA0001", &block("TREE", 0x20, &body(0)), &inserts));
    let first = again.doc().objects.keys().copied().next().unwrap();
    again
        .execute(&Command::MoveObjects {
            ids: vec![first],
            delta: DVec2::new(0.0, 5.0),
        })
        .unwrap();
    let built = plan
        .build(again.doc(), &vec![Take::All; plan.changes.len()])
        .unwrap();
    assert_eq!((built.applied.replaced, built.applied.kept_edits), (3, 0));
}

#[test]
fn a_partial_apply_changes_only_the_chosen_changes() {
    let mut ed = editor_with(&base_text());
    let plan = make(ed.doc(), &revision_two()).unwrap();
    let chosen: Vec<Take> = plan
        .changes
        .iter()
        .map(|c| (c.status == Kind::Added).into())
        .collect();
    let built = plan.build(ed.doc(), &chosen).unwrap();
    ed.execute(&built.command).unwrap();
    assert_eq!(built.applied.added, 1);
    assert_eq!(built.applied.replaced + built.applied.removed, 0);
    assert!(object_at(ed.doc(), 0x15).is_some());
    assert!(object_at(ed.doc(), 0x13).is_some());
    assert!((y_of(object_at(ed.doc(), 0x11).unwrap()) - 100.0).abs() < 1e-9);
    // The ID of an added object in the document is known.
    assert_eq!(built.added.len(), 1);
    assert!(ed.doc().objects.contains_key(&built.added[0].1));
    // An empty choice only moves the layers and the update data to the new revision.
    let mut ed2 = editor_with(&base_text());
    let built = plan
        .build(ed2.doc(), &vec![Take::No; plan.changes.len()])
        .unwrap();
    ed2.execute(&built.command).unwrap();
    assert_eq!(ed2.doc().objects.len(), 5);
    assert!(
        ed2.doc()
            .objects
            .values()
            .all(|o| provenance::tag(&o.data).unwrap().revision == plan.old_revision)
    );
    assert!(provenance::base_recs(ed2.doc(), &plan.drawing, &plan.new_revision).is_some());
}

#[test]
fn objects_of_other_sources_are_never_touched() {
    let mut ed = editor_with(&base_text());
    // A second drawing with the same handles, another lineage.
    let other = base_text().replace(GUID, "FFFF0002-OTHER");
    import_text(&mut ed, "other.dxf", &other, &opts());
    // And a hand drawn object.
    ed.execute(&Command::AddObject {
        shape: Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::new(5.0, 5.0),
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let others: Vec<Object> = ed
        .doc()
        .objects
        .values()
        .filter(|o| provenance::tag(&o.data).is_none_or(|t| t.drawing != format!("guid:{GUID}")))
        .cloned()
        .collect();
    assert_eq!(others.len(), 6);
    let layers_before: Vec<_> = ed
        .doc()
        .layers
        .iter()
        .filter(|l| {
            LayerSource::of(l.extra.get(provenance::LAYER_KEY).unwrap_or(&Value::Null))
                .is_some_and(|s| s.drawing != format!("guid:{GUID}"))
        })
        .cloned()
        .collect();
    // Two drawings: the new file names the one to update.
    let plan = make(ed.doc(), &revision_two()).unwrap();
    assert_eq!(plan.drawing, format!("guid:{GUID}"));
    apply_all(&mut ed, &plan);
    for o in &others {
        assert_eq!(ed.doc().objects.get(&o.id), Some(o));
    }
    for l in &layers_before {
        assert!(ed.doc().layers.contains(l));
    }
}

#[test]
fn the_drawing_is_found_or_the_caller_must_name_it() {
    let mut ed = editor_with(&base_text());
    let other = base_text().replace(GUID, "FFFF0002-OTHER");
    import_text(&mut ed, "other.dxf", &other, &opts());
    // A new file that says nothing about its lineage and has another name.
    let anon = revision_two().replace(GUID, "00000000");
    let err = make(ed.doc(), &anon).err().unwrap();
    assert!(matches!(err, UpdateError::Ambiguous(_)), "{err}");
    assert!(err.to_string().contains("guid:FFFF0002-OTHER"));
    // Named.
    let named = plan(&Request {
        drawing: Some(&format!("guid:{GUID}")),
        ..request_for(ed.doc(), &anon)
    })
    .unwrap();
    assert_eq!(named.drawing, format!("guid:{GUID}"));
    assert!(named.notes.iter().any(|n| n.contains("you chose it")));
    let unknown = plan(&Request {
        drawing: Some("nothing"),
        ..request_for(ed.doc(), &anon)
    })
    .err()
    .unwrap();
    assert!(matches!(unknown, UpdateError::Unknown(..)));
    // No imported drawing at all.
    let empty = Document::with_sequential_ids();
    assert!(matches!(
        make(&empty, &revision_two()).err().unwrap(),
        UpdateError::NoDrawing
    ));
}

fn request_for<'a>(doc: &'a Document, data: &'a str) -> Request<'a> {
    static OPTS: std::sync::OnceLock<ImportOptions> = std::sync::OnceLock::new();
    Request {
        doc,
        drawing: None,
        name: "plan_rev2.dxf",
        data: data.as_bytes(),
        new_link: None,
        old_file: None,
        read_link: &no_link,
        options: OPTS.get_or_init(ImportOptions::default),
        tolerance: None,
        layers: &[],
        max_bytes: 1 << 30,
    }
}

#[test]
fn without_update_data_the_old_file_is_needed() {
    let none = ImportOptions {
        base: BaseMode::Off,
        ..ImportOptions::default()
    };
    let mut ed = Editor::new(Document::with_sequential_ids());
    import_text(&mut ed, "plan.dxf", &base_text(), &none);
    let err = make(ed.doc(), &revision_two()).err().unwrap();
    assert!(matches!(err, UpdateError::NeedsOldFile(_)), "{err}");
    assert!(err.to_string().contains("old file"));
    // With the old file the update works as with a stored base.
    let plan = make_with(ed.doc(), &revision_two(), Some(&base_text()), &opts()).unwrap();
    assert_eq!(plan.old_from, "file");
    assert_eq!(plan.counts.changed, 2);
    apply_all(&mut ed, &plan);
    assert!(object_at(ed.doc(), 0x15).is_some());
    // A bad old file is told.
    let bad = make_with(ed.doc(), &revision_two(), Some("not a dxf"), &opts());
    assert!(matches!(bad.err().unwrap(), UpdateError::OldFile(_)));
}

#[test]
fn a_linked_base_is_read_through_the_link() {
    let link_opts = ImportOptions {
        base: BaseMode::Link,
        ..ImportOptions::default()
    };
    let link = FileLink::of_bytes("/data/plan.dxf", base_text().as_bytes());
    let mut ed = Editor::new(Document::with_sequential_ids());
    let src = Source {
        name: "plan.dxf".into(),
        data: base_text().into_bytes(),
        extra: Default::default(),
    };
    let r =
        super::super::import_linked(&src, &Context::of(ed.doc()), &link_opts, Some(&link)).unwrap();
    ed.execute(&r.command).unwrap();
    assert!(
        ed.doc()
            .resources
            .iter()
            .all(|r| !matches!(r.body, kadakas_core::resource::ResourceBody::ImportBase(_)))
    );
    // The file is there: the reader gives the records.
    let old =
        super::super::link::records_of_bytes(base_text().as_bytes(), &link, "/data/plan.dxf", 1000)
            .unwrap();
    let ok = |l: &FileLink| -> Result<Vec<Rec>, LinkError> {
        assert_eq!(l, &link);
        Ok(old.clone())
    };
    let data = revision_two();
    let new_link = FileLink::of_bytes("/data/plan_rev2.dxf", data.as_bytes());
    let mut req = request_for(ed.doc(), &data);
    req.read_link = &ok;
    req.new_link = Some(new_link.clone());
    let plan = plan(&req).unwrap();
    assert_eq!(plan.old_from, "link");
    assert_eq!(plan.counts.changed, 2);
    apply_all(&mut ed, &plan);
    // The link moves to the new file, and no base is stored.
    let src = ed
        .doc()
        .layers
        .iter()
        .find_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .unwrap();
    assert_eq!(src.link, Some(new_link));
    assert_eq!(src.revision, plan.new_revision);

    // The file is gone, or changed: the plan says so and asks for the old file.
    let gone = |_: &FileLink| -> Result<Vec<Rec>, LinkError> { Err(LinkError::Gone("x".into())) };
    let mut req = request_for(ed.doc(), &data);
    req.read_link = &gone;
    let err = super::plan(&req).err().unwrap();
    assert!(matches!(err, UpdateError::NeedsOldFile(_)));
    assert!(err.to_string().contains("gone"), "{err}");
    let changed =
        |_: &FileLink| -> Result<Vec<Rec>, LinkError> { Err(LinkError::Changed("x".into())) };
    req.read_link = &changed;
    let err = super::plan(&req).err().unwrap();
    assert!(err.to_string().contains("changed"), "{err}");
}

#[test]
fn text_changes_say_what_changed_and_layers_limit_the_update() {
    let base = file(
        "AAAA0001",
        "",
        &[
            text(0x10, 0, "Oak"),
            text(0x11, 500, "Elm"),
            line(0x12, "WALLS", 900),
        ]
        .concat(),
    );
    let new = file(
        "AAAA0002",
        "",
        &[
            text(0x10, 0, "Birch"),
            text(0x11, 500, "Elm"),
            line(0x12, "WALLS", 950),
        ]
        .concat(),
    );
    let ed = editor_with(&base);
    let plan = make(ed.doc(), &new).unwrap();
    assert_eq!(plan.counts.changed, 2);
    let t = plan
        .changes
        .iter()
        .find(|c| c.entity == "TEXT")
        .and_then(|c| c.text.clone())
        .unwrap();
    assert_eq!((t.old.as_str(), t.new.as_str()), ("Oak", "Birch"));
    let mut req = request_for(ed.doc(), &new);
    let only = ["walls".to_string()];
    req.layers = &only;
    let limited = super::plan(&req).unwrap();
    assert_eq!(limited.counts.changed, 1);
    assert_eq!(limited.changes[0].entity, "LINE");
}

#[test]
fn change_ids_are_stable_and_unique() {
    let ed = editor_with(&base_text());
    let a = make(ed.doc(), &revision_two()).unwrap();
    let b = make(ed.doc(), &revision_two()).unwrap();
    let ids: Vec<&String> = a.changes.iter().map(|c| &c.id).collect();
    assert_eq!(ids, b.changes.iter().map(|c| &c.id).collect::<Vec<_>>());
    let unique: BTreeSet<&&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
    assert_eq!(a.index_of(&a.changes[1].id), Some(1));
    assert!(a.index_of("nope").is_none());
    assert!(a.bytes() > 0);
    assert!(a.ops(0).is_some() && a.ops(99).is_none());
}

#[test]
fn a_plan_over_the_size_limit_is_refused() {
    let ed = editor_with(&base_text());
    let mut req = request_for(ed.doc(), "");
    let data = revision_two();
    req.data = data.as_bytes();
    req.max_bytes = 100;
    let err = super::plan(&req).err().unwrap();
    assert!(matches!(err, UpdateError::Limit(_)), "{err}");
}

#[test]
fn a_locked_layer_of_the_drawing_is_unlocked_and_locked_again() {
    let mut ed = editor_with(&base_text());
    let layer = ed
        .doc()
        .layers
        .iter()
        .find(|l| l.extra.contains_key(provenance::LAYER_KEY))
        .unwrap()
        .id;
    ed.execute(&Command::ModifyLayer {
        id: layer,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    let plan = make(ed.doc(), &revision_two()).unwrap();
    apply_all(&mut ed, &plan);
    assert!(ed.doc().layer(layer).unwrap().locked);
    assert!(object_at(ed.doc(), 0x15).is_some());
}

#[test]
fn drawings_lists_what_was_imported() {
    let ed = editor_with(&base_text());
    let d = drawings(ed.doc());
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].objects, 5);
    assert!(d[0].has_base);
    assert!(!d[0].linked);
    assert!(drawings(&Document::with_sequential_ids()).is_empty());
}
