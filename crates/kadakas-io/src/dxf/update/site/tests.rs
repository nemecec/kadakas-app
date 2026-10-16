use super::*;
use kadakas_core::command::Editor;
use kadakas_core::model::{Attributes, SiteSettings};

fn tag(handle: &str, obj: &str) -> ImportTag {
    ImportTag {
        drawing: "d".into(),
        revision: "r1".into(),
        handle: Some(handle.into()),
        kind: "POINT".into(),
        hash: "0123456789abcdef".into(),
        obj: obj.into(),
        block: None,
        via: vec![],
        part: None,
    }
}

/// A site model with two points (one tagged) and one line (tagged, and edited since).
fn doc() -> (Editor, ObjectId) {
    let mut ed = Editor::default();
    let p0 = DVec3::new(1000.0, 2000.0, 3000.0);
    let line = SiteLine {
        points: vec![DVec3::new(0.0, 0.0, 1.0), DVec3::new(10.0, 4.0, 1.0)],
        closed: false,
    };
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SiteModel {
                points: vec![p0, DVec3::new(5.0, 5.0, 5.0)],
                lines: vec![line],
                settings: SiteSettings::default(),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let src = SiteSources {
        points: vec![
            Some(SourceRef {
                tag: tag("A", &provenance::point_state(p0)),
                dxf_layer: "SPOT".into(),
            }),
            None,
        ],
        lines: vec![Some(SourceRef {
            tag: tag("B", "0000000000000001"),
            dxf_layer: "CONTOUR".into(),
        })],
    };
    ed.execute(&Command::SetData {
        id,
        set: [(provenance::SITE_SOURCES_KEY.to_string(), src.to_value())].into(),
        remove: vec![],
    })
    .unwrap();
    (ed, id)
}

#[test]
fn tagged_sources_become_virtual_objects_with_a_place_and_an_edit_flag() {
    let (ed, id) = doc();
    let found = collect(ed.doc(), "d");
    assert_eq!(found.len(), 2);
    let point = found.values().find(|s| s.kind == Kind::Point).unwrap();
    let line = found.values().find(|s| s.kind == Kind::Line).unwrap();
    assert_eq!(
        (point.site, point.index, point.center),
        (id, 0, [1000.0, 2000.0])
    );
    assert!(!point.edited, "the state is the one of the import");
    assert!(line.edited, "the state of the line differs from the tag");
    assert_eq!(line.center, [5.0, 2.0]);
    assert_eq!(point.dxf_layer, "SPOT");
    // The IDs are not those of objects, and they are the same each time.
    assert!(found.keys().all(|k| ed.doc().object(*k).is_none()));
    assert_eq!(
        collect(ed.doc(), "d").keys().collect::<Vec<_>>(),
        found.keys().collect::<Vec<_>>()
    );
    assert!(collect(ed.doc(), "other").is_empty());
    assert_eq!(count(ed.doc(), "d"), 2);
    assert_eq!(count(ed.doc(), "other"), 0);
    // A model joins by the DXF layer of its sources, without regard to the case.
    let h = homes(ed.doc(), &found);
    assert_eq!(h.get("spot"), Some(&id));
    assert_eq!(h.get("contour"), Some(&id));
    assert!(!h.contains_key("trees"));
}

#[test]
fn sources_of_a_model_that_does_not_fit_its_tags_are_not_read() {
    let (mut ed, id) = doc();
    // A point is removed without its tag: the lists no longer fit.
    let mut shape = ed.doc().object(id).unwrap().shape.clone();
    if let Shape::SiteModel { points, .. } = &mut shape {
        points.pop();
    }
    ed.execute(&Command::ModifyObject {
        id,
        shape: Some(shape),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    assert!(collect(ed.doc(), "d").is_empty());
}

#[test]
fn work_replaces_removes_adds_and_refreshes_the_sources() {
    let (mut ed, id) = doc();
    let new_point = NewSource {
        shape: Shape::Locus3d {
            p: DVec3::new(7.0, 8.0, 9.0),
        },
        tag: tag("C", "0000000000000000"),
        dxf_layer: "SPOT".into(),
    };
    let new_line = NewSource {
        shape: Shape::Polyline3d {
            points: vec![DVec3::ZERO, DVec3::new(1.0, 1.0, 1.0)],
            closed: true,
        },
        tag: tag("D", "0000000000000000"),
        dxf_layer: "CONTOUR".into(),
    };
    assert_eq!(new_point.kind(), Some(Kind::Point));
    assert_eq!(new_line.kind(), Some(Kind::Line));
    let not_a_source = NewSource {
        shape: Shape::Circle {
            center: kadakas_core::DVec2::ZERO,
            radius: 1.0,
        },
        tag: tag("E", "0000000000000000"),
        dxf_layer: String::new(),
    };
    assert_eq!(not_a_source.kind(), None);
    assert_eq!(
        not_a_source.entry("r2").tag.obj,
        provenance::EDITED_OBJ,
        "a shape that is no source never matches"
    );
    let mut work = Work::default();
    assert!(work.is_empty());
    work.replace.push((Kind::Point, 0, new_point));
    work.delete.insert((Kind::Point, 1));
    work.add.push(new_line);
    work.refresh
        .push((Kind::Line, 0, tag("Z", "0000000000000000")));
    assert!(!work.is_empty());
    for c in commands(ed.doc(), id, &work, "r2") {
        ed.execute(&c).unwrap();
    }
    let o = ed.doc().object(id).unwrap();
    let Shape::SiteModel { points, lines, .. } = &o.shape else {
        panic!()
    };
    assert_eq!(points, &vec![DVec3::new(7.0, 8.0, 9.0)]);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].closed);
    let src = SiteSources::read(o).unwrap();
    assert_eq!(src.points.len(), 1);
    let r = src.points[0].as_ref().unwrap();
    assert_eq!(r.tag.revision, "r2");
    assert_eq!(r.tag.handle.as_deref(), Some("C"));
    assert_eq!(
        r.tag.obj,
        provenance::point_state(DVec3::new(7.0, 8.0, 9.0))
    );
    // The line that stayed has the identity of its entity in the new file and its old revision.
    let kept = src.lines[0].as_ref().unwrap();
    assert_eq!(kept.tag.handle.as_deref(), Some("Z"));
    assert_eq!(kept.tag.revision, "r1");
    assert_eq!(
        src.lines[1].as_ref().unwrap().tag.handle.as_deref(),
        Some("D")
    );
    // Taking the last tagged sources away clears the key.
    let mut work = Work::default();
    work.delete.insert((Kind::Point, 0));
    work.delete.insert((Kind::Line, 0));
    work.delete.insert((Kind::Line, 1));
    for c in commands(ed.doc(), id, &work, "r3") {
        ed.execute(&c).unwrap();
    }
    let o = ed.doc().object(id).unwrap();
    assert!(!o.data.contains_key(provenance::SITE_SOURCES_KEY));
    // An ID that is no site model gives no command.
    assert!(commands(ed.doc(), ObjectId::from_num(999), &work, "r3").is_empty());
}
