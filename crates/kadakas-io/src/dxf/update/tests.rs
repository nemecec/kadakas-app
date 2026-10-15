//! Tests of the update from DXF.

use super::super::revision::Status;
use super::*;
use crate::{Format, Source, importer};
use kadakas_core::DVec2;
use kadakas_core::model::Shape;
use kadakas_core::provenance::hex;
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
        base_given: options.base != BaseMode::Auto,
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
        base_given: false,
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

fn paper_text(handle: u32, t: &str) -> String {
    format!("0\nTEXT\n5\n{handle:X}\n8\nA\n67\n1\n10\n0\n20\n0\n40\n100\n1\n{t}\n")
}

#[test]
fn changes_that_make_no_object_are_counted_and_the_skipped_entities_are_told() {
    let base = file(
        "AAAA0001",
        "",
        &[line(0x10, "A", 0), paper_text(0x30, "one")].concat(),
    );
    let new = file(
        "AAAA0002",
        "",
        &[line(0x10, "A", 0), paper_text(0x30, "two")].concat(),
    );
    let ed = editor_with(&base);
    let plan = make(ed.doc(), &new).unwrap();
    assert!(plan.changes.is_empty());
    assert_eq!(plan.counts.ignored, 1);
    assert!(plan.notes.iter().any(|n| n.contains("make no object")));
    assert!(plan.notes.iter().any(|n| n.contains("are not imported")));
}

#[test]
fn the_unit_and_the_update_data_of_the_new_revision() {
    let ed = editor_with(&base_text());
    // Another unit in the new file is told.
    let meters = revision_two().replace("$INSUNITS\n70\n4", "$INSUNITS\n70\n6");
    let plan = make(ed.doc(), &meters).unwrap();
    assert!(
        plan.notes.iter().any(|n| n.contains("another unit")),
        "{:?}",
        plan.notes
    );
    // No update data for the new revision: told, and none is stored.
    let none = ImportOptions {
        base: BaseMode::Off,
        ..ImportOptions::default()
    };
    let plan = make_with(ed.doc(), &revision_two(), None, &none).unwrap();
    assert!(plan.notes.iter().any(|n| n.contains("base: none")));
    let mut again = Editor::new(ed.doc().clone());
    apply_all(&mut again, &plan);
    assert!(provenance::base_recs(again.doc(), &plan.drawing, &plan.new_revision).is_none());
    assert!(provenance::base_recs(again.doc(), &plan.drawing, &plan.old_revision).is_none());
    // A link needs the file on disk: without one the plan is refused, with one it is kept.
    let link_opts = ImportOptions {
        base: BaseMode::Link,
        ..ImportOptions::default()
    };
    let err = make_with(ed.doc(), &revision_two(), None, &link_opts)
        .err()
        .unwrap();
    assert!(matches!(err, UpdateError::NewFile(_)), "{err}");
    let data = revision_two();
    let mut req = request_for(ed.doc(), &data);
    req.options = &link_opts;
    req.base_given = true;
    req.new_link = Some(FileLink::of_bytes("/data/plan_rev2.dxf", data.as_bytes()));
    let plan = super::plan(&req).unwrap();
    let mut linked = Editor::new(ed.doc().clone());
    apply_all(&mut linked, &plan);
    let src = linked
        .doc()
        .layers
        .iter()
        .find_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .unwrap();
    assert!(src.link.is_some());
}

#[test]
fn a_bad_tolerance_a_broken_stored_base_and_a_plan_over_the_limit_are_refused() {
    let ed = editor_with(&base_text());
    let data = revision_two();
    let mut req = request_for(ed.doc(), &data);
    req.tolerance = Some(-1.0);
    assert!(matches!(
        super::plan(&req).err().unwrap(),
        UpdateError::NewFile(_)
    ));
    req.tolerance = Some(f64::NAN);
    assert!(super::plan(&req).is_err());

    // The stored summaries are not valid (a hand-made document).
    let mut doc = ed.doc().clone();
    let ids: Vec<_> = doc
        .resources
        .iter()
        .filter(|r| matches!(r.body, kadakas_core::resource::ResourceBody::ImportBase(_)))
        .map(|r| r.id)
        .collect();
    for id in ids {
        let r = doc.resources.get_mut(id).unwrap();
        if let kadakas_core::resource::ResourceBody::ImportBase(v) = &r.body {
            let mut body = (**v).clone();
            body["recs"] = serde_json::json!(["not a record"]);
            r.body = kadakas_core::resource::ResourceBody::ImportBase(std::sync::Arc::new(body));
        }
    }
    let err = make(&doc, &data).err().unwrap();
    assert!(matches!(err, UpdateError::NeedsOldFile(_)), "{err}");

    // Some limits hold the plan back with the second message (the bytes of the whole plan), some
    // with the first (the new objects).
    let mut messages = BTreeSet::new();
    for max in (200..6000).step_by(100) {
        let mut req = request_for(ed.doc(), &data);
        req.max_bytes = max;
        if let Err(UpdateError::Limit(m)) = super::plan(&req) {
            messages.insert(m.contains("bytes, the limit is"));
        }
    }
    assert_eq!(messages.len(), 2, "both limits are reached: {messages:?}");
}

#[test]
fn the_pairing_and_the_classification_of_objects() {
    fn tag(handle: &str, part: Option<u32>) -> ImportTag {
        ImportTag {
            drawing: "d".into(),
            revision: "r".into(),
            handle: Some(handle.into()),
            kind: "HATCH".into(),
            hash: hex(1),
            obj: hex(0),
            block: None,
            via: vec![],
            part,
        }
    }
    let id = |n: u64| ObjectId(kadakas_core::uid::Uid::from_num(n));
    let old_tags = [tag("A", Some(0)), tag("A", Some(1)), tag("A", Some(2))];
    let new_tags = [tag("A", Some(0)), tag("A", Some(5))];
    let old: TagMap = old_tags
        .iter()
        .enumerate()
        .map(|(i, t)| (id(i as u64 + 1), t))
        .collect();
    let new: TagMap = new_tags
        .iter()
        .enumerate()
        .map(|(i, t)| (id(i as u64 + 10), t))
        .collect();
    // Part 0 pairs by place; part 5 has no partner, but one old object is left: they pair in
    // order; the last old object goes.
    let (pairs, gone, fresh) = pair_objects(&old, &new, &[id(1), id(2), id(3)], &[id(10), id(11)]);
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(id(1), id(10))));
    assert_eq!((gone.len(), fresh.len()), (1, 0));
    // A changed entity whose objects are all gone is a removal.
    let (kind, ops) = classify(Status::Changed, &[id(1)], &[], &old, &new);
    assert_eq!(kind, Kind::Removed);
    assert_eq!(ops.delete, vec![id(1)]);
    let (kind, _) = classify(Status::Changed, &[id(1)], &[id(10)], &old, &new);
    assert_eq!(kind, Kind::Changed);
    assert_eq!(
        classify(Status::Added, &[], &[id(10)], &old, &new).0,
        Kind::Added
    );
    assert_eq!(
        classify(Status::Removed, &[id(1)], &[], &old, &new).0,
        Kind::Removed
    );
    assert_eq!(Take::from(true), Take::All);
    assert_eq!(Take::from(false), Take::No);
}

#[test]
fn edits_are_kept_for_removed_objects_and_locked_layers_of_other_work_are_respected() {
    let mut ed = editor_with(&base_text());
    let removed = object_at(ed.doc(), 0x13).unwrap().id;
    let moved = object_at(ed.doc(), 0x11).unwrap().id;
    let edit = |ed: &mut Editor, id| {
        ed.execute(&Command::MoveObjects {
            ids: vec![id],
            delta: DVec2::new(0.0, 2.0),
        })
        .unwrap();
    };
    edit(&mut ed, removed);
    // The person moves another object to a layer of their own work, which is locked.
    ed.execute(&Command::AddLayer {
        name: "Mine".into(),
        elevation: 0.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let mine = ed
        .doc()
        .layers
        .iter()
        .find(|l| l.name == "Mine")
        .unwrap()
        .id;
    ed.execute(&Command::ModifyObject {
        id: moved,
        shape: None,
        layer: Some(mine),
        class: None,
        attrs: None,
    })
    .unwrap();
    ed.execute(&Command::ModifyLayer {
        id: mine,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    let plan = make(ed.doc(), &revision_two()).unwrap();
    // The edited removed object is kept with KeepEdits.
    let keep = plan
        .build(ed.doc(), &vec![Take::KeepEdits; plan.changes.len()])
        .unwrap();
    assert_eq!(keep.applied.removed, 0);
    assert!(keep.applied.kept_edits >= 2, "{:?}", keep.applied);
    // With All, the object on the locked layer of the other work is left alone, and told.
    let all = plan
        .build(ed.doc(), &vec![Take::All; plan.changes.len()])
        .unwrap();
    assert_eq!(all.applied.skipped_locked, 1, "{:?}", all.applied);
    assert_eq!(all.applied.removed, 1);
    assert!(all.notes.iter().any(|n| n.contains("locked layer")));
    ed.execute(&all.command).unwrap();
    assert!(ed.doc().objects.contains_key(&moved));
    assert!(!ed.doc().objects.contains_key(&removed));
    // The new objects can be read from the plan for a window.
    let added = &plan
        .ops(
            plan.index_of(
                &plan
                    .changes
                    .iter()
                    .find(|c| c.status == Kind::Added)
                    .unwrap()
                    .id,
            )
            .unwrap(),
        )
        .unwrap()
        .add;
    assert!(plan.new_object(added[0]).is_some());
    assert!(
        plan.new_object(ObjectId(kadakas_core::uid::Uid::from_num(9999)))
            .is_none()
    );
}

fn line_nh(layer: &str, y: i64) -> String {
    format!("0\nLINE\n8\n{layer}\n10\n0\n20\n{y}\n11\n1000\n21\n{y}\n")
}

fn objects_at(doc: &Document, y: i64) -> Vec<&Object> {
    doc.objects
        .values()
        .filter(|o| matches!(o.shape, Shape::Line { .. }) && y_of(o) as i64 == y)
        .collect()
}

fn ys(doc: &Document) -> Vec<i64> {
    let mut v: Vec<i64> = doc.objects.values().map(|o| y_of(o) as i64).collect();
    v.sort();
    v
}

fn handle_at(doc: &Document, y: i64) -> String {
    provenance::tag(&objects_at(doc, y)[0].data)
        .unwrap()
        .handle
        .unwrap()
}

#[test]
fn objects_follow_the_handles_of_the_new_file_over_several_updates() {
    let mut ed = editor_with(&base_text());
    // Another program renumbers every handle and one line moves a little.
    let renumbered = |third: i64| {
        file(
            "AAAA0002",
            "",
            &[
                line(0x11, "A", 0),
                line(0x12, "A", 103),
                line(0x13, "A", 200),
                line(0x14, "A", 300),
                line(0x10, "A", third),
            ]
            .concat(),
        )
    };
    let v2 = renumbered(400);
    let mut req = request_for(ed.doc(), &v2);
    req.tolerance = Some(10.0);
    let plan = super::plan(&req).unwrap();
    assert!(plan.renumbered);
    apply_all(&mut ed, &plan);
    // The object at y = 0 is the entity 11 of the new file now, whatever it was called before.
    assert_eq!(handle_at(ed.doc(), 0), "11");
    assert_eq!(handle_at(ed.doc(), 400), "10");
    // The third revision moves the line 10 (at y = 400). It must not touch the line at y = 0.
    let third = renumbered(450).replace("AAAA0002", "AAAA0003");
    let plan3 = make(ed.doc(), &third).unwrap();
    assert_eq!(kinds(&plan3), vec![(Kind::Changed, "10".into())]);
    apply_all(&mut ed, &plan3);
    assert_eq!(ys(ed.doc()), vec![0, 103, 200, 300, 450]);
    // And a fourth one that moves the line 11 (at y = 0).
    let fourth = file(
        "AAAA0004",
        "",
        &[
            line(0x11, "A", 20),
            line(0x12, "A", 103),
            line(0x13, "A", 200),
            line(0x14, "A", 300),
            line(0x10, "A", 450),
        ]
        .concat(),
    );
    let plan4 = make(ed.doc(), &fourth).unwrap();
    assert_eq!(kinds(&plan4), vec![(Kind::Changed, "11".into())]);
    apply_all(&mut ed, &plan4);
    assert_eq!(ys(ed.doc()), vec![20, 103, 200, 300, 450]);
}

#[test]
fn a_rejected_change_does_not_make_the_next_update_duplicate_the_object() {
    // A file without handles: the objects are found by the hash of their entity.
    let nh = |version: &str, ys: &[i64]| {
        file(
            version,
            "",
            &ys.iter().map(|y| line_nh("A", *y)).collect::<String>(),
        )
    };
    let mut ed = editor_with(&nh("AAAA0001", &[0, 100, 200]));
    let v2 = nh("AAAA0002", &[0, 103, 200]);
    let mut req = request_for(ed.doc(), &v2);
    req.tolerance = Some(10.0);
    let plan = super::plan(&req).unwrap();
    assert_eq!(plan.counts.changed, 1, "{:?}", plan.changes);
    // The person rejects the change: the object stays where it was.
    let built = plan.build(ed.doc(), &[Take::No]).unwrap();
    ed.execute(&built.command).unwrap();
    assert_eq!(ys(ed.doc()), vec![0, 100, 200]);
    // The next revision moves that entity again. The object is replaced, not duplicated.
    let v3 = nh("AAAA0003", &[0, 108, 200]);
    let mut req = request_for(ed.doc(), &v3);
    req.tolerance = Some(10.0);
    let plan3 = super::plan(&req).unwrap();
    assert_eq!(plan3.counts.changed, 1, "{:?}", plan3.changes);
    assert_eq!(plan3.counts.added, 0);
    apply_all(&mut ed, &plan3);
    assert_eq!(ys(ed.doc()), vec![0, 108, 200]);
}

#[test]
fn objects_of_left_out_changes_follow_the_new_file_too() {
    let mut ed = editor_with(&base_text());
    let renumbered = |second: i64| {
        file(
            "AAAA0002",
            "",
            &[
                line(0x21, "A", 0),
                line(0x22, "A", second),
                line(0x23, "A", 200),
                line(0x24, "A", 300),
                line(0x20, "A", 400),
            ]
            .concat(),
        )
    };
    let v2 = renumbered(103);
    let mut req = request_for(ed.doc(), &v2);
    req.tolerance = Some(10.0);
    let plan = super::plan(&req).unwrap();
    assert_eq!(plan.counts.changed, 1);
    // The move of the second line is left out.
    let built = plan
        .build(ed.doc(), &vec![Take::No; plan.changes.len()])
        .unwrap();
    ed.execute(&built.command).unwrap();
    assert_eq!(ys(ed.doc()), vec![0, 100, 200, 300, 400]);
    assert_eq!(handle_at(ed.doc(), 0), "21");
    assert_eq!(
        handle_at(ed.doc(), 100),
        "22",
        "the left out change still gets the new handle"
    );
    // The next revision moves the entity 22: the object at y = 100 is found.
    let next = renumbered(140).replace("AAAA0002", "AAAA0003");
    let plan2 = make(ed.doc(), &next).unwrap();
    assert_eq!(kinds(&plan2), vec![(Kind::Changed, "22".into())]);
    assert_eq!(plan2.changes[0].objects, 1);
    apply_all(&mut ed, &plan2);
    assert_eq!(ys(ed.doc()), vec![0, 140, 200, 300, 400]);
}

fn layered(version: &str, a: i64, b: i64) -> String {
    file(
        version,
        "",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", a),
            line(0x12, "B", 200),
            line(0x13, "B", b),
        ]
        .concat(),
    )
}

#[test]
fn an_update_of_some_layers_leaves_the_other_layers_for_the_next_update() {
    let layers = ImportOptions {
        layer_mode: LayerMode::Layers,
        ..ImportOptions::default()
    };
    let mut ed = Editor::new(Document::with_sequential_ids());
    import_text(&mut ed, "plan.dxf", &layered("AAAA0001", 100, 300), &layers);
    let v2 = layered("AAAA0002", 150, 350);
    let only_a = ["A".to_string()];
    let mut req = request_for(ed.doc(), &v2);
    req.layers = &only_a;
    let plan = super::plan(&req).unwrap();
    assert_eq!(kinds(&plan), vec![(Kind::Changed, "11".into())]);
    apply_all(&mut ed, &plan);
    assert_eq!(ys(ed.doc()), vec![0, 150, 200, 300]);
    // The layer A is at the new revision, the layer B still at the old one.
    let revision_of = |ed: &Editor, name: &str| {
        let l = ed
            .doc()
            .layers
            .iter()
            .find(|l| l.name.ends_with(name))
            .unwrap();
        LayerSource::of(l.extra.get(provenance::LAYER_KEY).unwrap())
            .unwrap()
            .revision
    };
    assert_eq!(revision_of(&ed, "A"), plan.new_revision);
    assert_eq!(revision_of(&ed, "B"), plan.old_revision);
    // The full update still offers the change on the layer B, and not the one on A again.
    let full = make(ed.doc(), &v2).unwrap();
    assert_eq!(
        kinds(&full),
        vec![(Kind::Changed, "13".into())],
        "{:?}",
        full.changes
    );
    apply_all(&mut ed, &full);
    assert_eq!(ys(ed.doc()), vec![0, 150, 200, 350]);
    assert_eq!(revision_of(&ed, "B"), full.new_revision);
}

#[test]
fn no_update_data_stays_none_unless_the_caller_chooses() {
    let none = ImportOptions {
        base: BaseMode::Off,
        ..ImportOptions::default()
    };
    let mut ed = Editor::new(Document::with_sequential_ids());
    import_text(&mut ed, "plan.dxf", &base_text(), &none);
    // An import with no update data: the update with the old file keeps none.
    let plan = make_with(ed.doc(), &revision_two(), Some(&base_text()), &opts()).unwrap();
    let mut other = Editor::new(ed.doc().clone());
    apply_all(&mut other, &plan);
    assert!(provenance::base_recs(other.doc(), &plan.drawing, &plan.new_revision).is_none());
    assert!(plan.notes.iter().any(|n| n.contains("base: none")));
    // The caller can ask for update data.
    let data = revision_two();
    let old = base_text();
    let mut req = request_for(ed.doc(), &data);
    req.old_file = Some((old.as_bytes(), "old.dxf"));
    req.base_given = true;
    let plan = super::plan(&req).unwrap();
    apply_all(&mut ed, &plan);
    assert!(provenance::base_recs(ed.doc(), &plan.drawing, &plan.new_revision).is_some());
}

#[test]
fn a_file_of_another_lineage_must_be_confirmed() {
    let ed = editor_with(&base_text());
    let anon = revision_two().replace(GUID, "00000000");
    let err = make(ed.doc(), &anon).err().unwrap();
    assert!(
        matches!(&err, UpdateError::Mismatch { drawing, .. } if drawing == &format!("guid:{GUID}")),
        "{err}"
    );
    let ok = plan(&Request {
        drawing: Some(&format!("guid:{GUID}")),
        ..request_for(ed.doc(), &anon)
    })
    .unwrap();
    assert!(ok.notes.iter().any(|n| n.contains("you chose it")));
}

fn array_insert(handle: u32, name: &str, cols: u32) -> String {
    format!("0\nINSERT\n5\n{handle:X}\n8\nA\n2\n{name}\n10\n0\n20\n0\n70\n{cols}\n71\n1\n44\n500\n")
}

#[test]
fn identical_entities_without_handles_are_removed_and_moved_one_by_one() {
    let nh = |version: &str, ys: &[i64]| {
        file(
            version,
            "",
            &ys.iter().map(|y| line_nh("A", *y)).collect::<String>(),
        )
    };
    // Two identical lines: the new revision has one of them.
    let mut ed = editor_with(&nh("AAAA0001", &[0, 100, 100]));
    assert_eq!(ed.doc().objects.len(), 3);
    let plan = make(ed.doc(), &nh("AAAA0002", &[0, 100])).unwrap();
    assert_eq!(plan.counts.removed, 1, "{:?}", plan.changes);
    apply_all(&mut ed, &plan);
    assert_eq!(ys(ed.doc()), vec![0, 100], "only one of the twins goes");
    // Two identical lines again: one of them moves.
    let mut ed = editor_with(&nh("AAAA0001", &[0, 100, 100]));
    let v2 = nh("AAAA0002", &[0, 100, 104]);
    let mut req = request_for(ed.doc(), &v2);
    req.tolerance = Some(10.0);
    let plan = super::plan(&req).unwrap();
    assert_eq!(
        plan.counts.removed + plan.counts.added + plan.counts.changed,
        1
    );
    apply_all(&mut ed, &plan);
    assert_eq!(ys(ed.doc()), vec![0, 100, 104]);
    // And one more of them comes.
    let mut ed = editor_with(&nh("AAAA0001", &[0, 100]));
    let plan = make(ed.doc(), &nh("AAAA0002", &[0, 100, 100])).unwrap();
    assert_eq!(plan.counts.added, 1);
    apply_all(&mut ed, &plan);
    assert_eq!(ys(ed.doc()), vec![0, 100, 100]);
}

#[test]
fn edits_are_kept_when_a_change_brings_more_or_fewer_objects() {
    let body = line(0x30, "A", 0);
    let blocks = block("TREE", 0x20, &body);
    let three = file("AAAA0002", &blocks, &array_insert(0x40, "TREE", 3));
    let two = file("AAAA0001", &blocks, &array_insert(0x40, "TREE", 2));
    let mut ed = editor_with(&two);
    assert_eq!(ed.doc().objects.len(), 2);
    // The person moves one copy.
    let edited = *ed.doc().objects.keys().next().unwrap();
    ed.execute(&Command::MoveObjects {
        ids: vec![edited],
        delta: DVec2::new(0.0, 5.0),
    })
    .unwrap();
    let plan = make(ed.doc(), &three).unwrap();
    assert_eq!(plan.counts.conflicts, 1, "{:?}", plan.changes);
    let built = plan
        .build(ed.doc(), &vec![Take::KeepEdits; plan.changes.len()])
        .unwrap();
    assert_eq!(built.applied.kept_edits, 1);
    assert_eq!(built.applied.added, 1);
    ed.execute(&built.command).unwrap();
    assert_eq!(ed.doc().objects.len(), 3);
    assert!(ed.doc().objects.contains_key(&edited));
    // The same file again: nothing is left to change.
    let again = make(ed.doc(), &three).unwrap();
    assert!(again.changes.is_empty(), "{:?}", again.changes);
    // Back to two copies: the edited copy is kept when it would have gone, and no copy is added.
    let back = file("AAAA0003", &blocks, &array_insert(0x40, "TREE", 2));
    let plan = make(ed.doc(), &back).unwrap();
    let built = plan
        .build(ed.doc(), &vec![Take::KeepEdits; plan.changes.len()])
        .unwrap();
    assert_eq!(built.applied.added, 0);
    ed.execute(&built.command).unwrap();
    assert!(ed.doc().objects.contains_key(&edited));
    assert!(ed.doc().objects.len() >= 2);
}
