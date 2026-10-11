use super::style::MAX_NOTES;
use super::*;

fn polyline_shape(pts: &[(f64, f64)]) -> Shape {
    Shape::Polyline {
        points: pts.iter().map(|p| vtx(p.0, p.1)).collect(),
        closed: false,
    }
}

fn modify(ed: &mut Editor, id: ObjectId, shape: Shape) {
    ed.execute(&Command::ModifyObject {
        id,
        shape: Some(shape),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
}

#[test]
fn georef_turns_the_north_arrow_the_right_way() {
    // The model is turned by theta against the map (to_map = R(theta) * model), so north is at -theta.
    let mut ed = Editor::default();
    let theta = 30f64.to_radians();
    ed.execute(&Command::SetGeoreference {
        georef: Some(crate::georef::Georeference {
            crs: String::new(),
            origin: DVec2::ZERO,
            rotation: theta,
            user_origin: DVec2::ZERO,
        }),
    })
    .unwrap();
    let n = NorthArrow {
        at: v(0.0, 0.0),
        size_mm: 10.0,
        rotation_deg: 0.0,
        use_georef: true,
        style: NorthStyle::Simple,
    };
    assert!((draw::north_rotation_deg(ed.doc(), &n) + 30.0).abs() < 1e-9);
    let plain = NorthArrow {
        rotation_deg: -30.0,
        use_georef: false,
        ..n.clone()
    };
    let a = draw::prims(ed.doc(), &Annot::NorthArrow(n), &ctx100(), &PlainSource);
    let b = draw::prims(ed.doc(), &Annot::NorthArrow(plain), &ctx100(), &PlainSource);
    assert_eq!(a, b);
    // The tip of the arrow points to the map north: R(theta) applied to the model direction gives +Y.
    let Prim::Poly { pts, .. } = &a[0] else {
        panic!()
    };
    let dir = (pts[1] - pts[0]).normalize();
    let map_dir = DVec2::from_angle(theta).rotate(dir);
    assert!((map_dir - DVec2::Y).length() < 1e-9, "{map_dir:?}");
}

#[test]
fn anchors_survive_inserted_and_deleted_vertices() {
    let mut ed = Editor::default();
    let pl = add(
        &mut ed,
        polyline_shape(&[(0.0, 0.0), (1000.0, 0.0), (2000.0, 0.0), (3000.0, 0.0)]),
    );
    let d = add_annot(
        &mut ed,
        Annot::Dimension(Dimension {
            anchors: vec![vertex(pl, 1), vertex(pl, 3)],
            ..dim(
                DimMode::Linear {
                    axis: Axis::Aligned,
                },
                vec![v(1000.0, 0.0), v(3000.0, 0.0)],
            )
        }),
    );
    let value = |ed: &Editor| {
        let Annot::Dimension(x) = annot_of(ed, d) else {
            panic!()
        };
        (dim::values(&x)[0], x.detached, x.anchors.clone())
    };
    assert_eq!(value(&ed).0, 2000.0);
    // A vertex inserted before the anchors: the indices shift, the measure stays.
    modify(
        &mut ed,
        pl,
        polyline_shape(&[
            (0.0, 0.0),
            (500.0, 0.0),
            (1000.0, 0.0),
            (2000.0, 0.0),
            (3000.0, 0.0),
        ]),
    );
    let (val, detached, anchors) = value(&ed);
    assert_eq!((val, detached), (2000.0, false));
    assert_eq!(
        anchors[0].unwrap().target,
        AnchorTarget::Vertex { index: 2 }
    );
    assert_eq!(
        anchors[1].unwrap().target,
        AnchorTarget::Vertex { index: 4 }
    );
    // A vertex deleted that is not anchored: still bound.
    modify(
        &mut ed,
        pl,
        polyline_shape(&[(0.0, 0.0), (1000.0, 0.0), (2000.0, 0.0), (3000.0, 0.0)]),
    );
    assert_eq!(value(&ed).0, 2000.0);
    // An anchored vertex deleted: the dimension keeps its place and says it is detached.
    modify(
        &mut ed,
        pl,
        polyline_shape(&[(0.0, 0.0), (1000.0, 0.0), (2000.0, 0.0)]),
    );
    let (val, detached, anchors) = value(&ed);
    assert_eq!((val, detached), (2000.0, true));
    assert!(
        anchors.len() == 1 && anchors[0].is_some(),
        "the other anchor still holds"
    );
    // Undo brings the anchors back.
    assert!(ed.undo());
    let (val, detached, anchors) = value(&ed);
    assert_eq!((val, detached, anchors.len()), (2000.0, false, 2));
    // A vertex that moves (same count) drags the dimension along.
    ed.execute(&Command::SetVertex {
        id: pl,
        index: 3,
        kind: None,
        p: Some(v(5000.0, 0.0)),
    })
    .unwrap();
    assert_eq!(value(&ed).0, 4000.0);
}

#[test]
fn edge_anchors_and_callouts_detach_like_vertices() {
    let mut ed = Editor::default();
    let pl = add(
        &mut ed,
        polyline_shape(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0)]),
    );
    let d = add_annot(
        &mut ed,
        Annot::Dimension(Dimension {
            anchors: vec![Some(Anchor::new(
                pl,
                AnchorTarget::Edge { index: 1, t: 0.5 },
            ))],
            ..dim(horiz(), vec![v(1000.0, 500.0), v(0.0, 0.0)])
        }),
    );
    modify(
        &mut ed,
        pl,
        polyline_shape(&[(-500.0, 0.0), (0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0)]),
    );
    let Annot::Dimension(x) = annot_of(&ed, d) else {
        panic!()
    };
    assert_eq!(x.points[0], v(1000.0, 500.0));
    assert_eq!(
        x.anchors[0].unwrap().target,
        AnchorTarget::Edge { index: 2, t: 0.5 }
    );
    modify(
        &mut ed,
        pl,
        polyline_shape(&[(0.0, 0.0), (900.0, 0.0), (50.0, 50.0)]),
    );
    let Annot::Dimension(x) = annot_of(&ed, d) else {
        panic!()
    };
    assert!(x.detached && x.anchors.is_empty());
    let mut ed = Editor::default();
    let pl = add(
        &mut ed,
        polyline_shape(&[(0.0, 0.0), (1000.0, 0.0), (2000.0, 0.0)]),
    );
    let mut c = callout_default();
    c.leaders[0] = Leader {
        points: vec![v(1000.0, 0.0)],
        anchor: vertex(pl, 1),
    };
    let cid = add_annot(&mut ed, Annot::Callout(c));
    modify(&mut ed, pl, polyline_shape(&[(0.0, 0.0), (2000.0, 0.0)]));
    let Annot::Callout(c) = annot_of(&ed, cid) else {
        panic!()
    };
    assert!(c.detached && c.leaders[0].anchor.is_none());
    assert_eq!(c.leaders[0].points[0], v(1000.0, 0.0));
}

#[test]
fn tag_positions_follow_a_new_layer_scale() {
    let mut ed = Editor::default();
    let style = res(&mut ed, "T", AnnotResource::TagStyle(TagStyle::default()));
    let p = add(
        &mut ed,
        Shape::Plant {
            at: v(1000.0, 1000.0),
            species: "Oak".into(),
            spread: 2000.0,
            height: 5000.0,
        },
    );
    let t = add_annot(
        &mut ed,
        Annot::DataTag(DataTag {
            style,
            object: p,
            offset: v(10.0, 0.0),
            at: DVec2::ZERO,
            anchor: DVec2::ZERO,
        }),
    );
    ed.execute(&Command::ModifyLayer {
        id: LayerId::from_num(1),
        name: None,
        visible: None,
        locked: None,
        elevation: None,
        scale: Some(500.0),
    })
    .unwrap();
    let Annot::DataTag(x) = annot_of(&ed, t) else {
        panic!()
    };
    assert_eq!(x.at, v(6000.0, 1000.0), "10 paper mm at 1:500");
    assert!(ed.undo());
    let Annot::DataTag(x) = annot_of(&ed, t) else {
        panic!()
    };
    assert_eq!(x.at, v(2000.0, 1000.0));
}

#[test]
fn rounding_goes_half_away_from_zero_and_to_increments() {
    assert_eq!(dim::round_half_away(2.5, 0), 3.0);
    assert_eq!(dim::round_half_away(-2.5, 0), -3.0);
    assert_eq!(dim::round_half_away(0.125, 2), 0.13);
    assert_eq!(dim::round_half_away(f64::INFINITY, 2), f64::INFINITY);
    let mut s = DimensionStyle::default();
    assert_eq!(dim::format_length(2.5, &s), "3");
    assert_eq!(dim::format_length(-2.5, &s), "-3");
    assert_eq!(dim::format_length(3.5, &s), "4");
    s.round_to = Some(5.0);
    assert_eq!(dim::format_length(1237.0, &s), "1235");
    assert_eq!(dim::format_length(1238.0, &s), "1240");
    assert_eq!(dim::format_length(1237.5, &s), "1240");
    s.round_to = Some(0.5);
    s.precision = 1;
    assert_eq!(dim::format_length(10.26, &s), "10.5");
    assert_eq!(dim::format_length(10.24, &s), "10.0");
    assert!(
        DimensionStyle {
            round_to: Some(0.0),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        DimensionStyle {
            round_to: Some(f64::NAN),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert_eq!(
        dim::format_angle(
            0.25,
            &DimensionStyle {
                angle_precision: 1,
                ..Default::default()
            }
        ),
        "0.3\u{b0}"
    );
}

#[test]
fn clockwise_angular_points_are_put_in_order() {
    let o = DVec2::ZERO;
    let (a, b) = (v(1000.0, 0.0), v(0.0, 1000.0));
    assert_eq!(dim::angular_order(o, a, b), (a, b));
    assert_eq!(dim::angular_order(o, b, a), (a, b));
}

#[test]
fn a_resource_with_an_unknown_value_is_kept_as_it_is() {
    let json = serde_json::json!({"id": "00000000-0000-0000-0000-000000000009", "name": "Yards", "body": {"type": "dimension_style", "unit": "yd", "precision": 2}});
    let r: crate::resource::Resource = serde_json::from_value(json.clone()).unwrap();
    assert!(matches!(r.body, ResourceBody::Unknown(_)));
    assert_eq!(serde_json::to_value(&r).unwrap(), json);
    let mut doc = Document::default();
    doc.resources.push(r);
    let text = serde_json::to_string(&doc).unwrap();
    assert_eq!(Document::from_json(&text).unwrap().resources, doc.resources);
    // An annotation that refers to it is refused, since it is not a dimension style.
    let mut ed = Editor::new(doc);
    let mut d = dim(horiz(), vec![v(0.0, 0.0), v(1.0, 0.0)]);
    d.style = Some(ResourceId::from_num(9));
    assert!(
        ed.execute(&Command::AddObject {
            shape: Shape::Annotation(Box::new(Annot::Dimension(d))),
            layer: None,
            class: None,
            attrs: Attributes::default()
        })
        .is_err()
    );
}

#[test]
fn limits_for_agent_input() {
    let note = |k: &str, t: &str| Note {
        key: k.into(),
        text: t.into(),
    };
    let many = NotesDb {
        notes: (0..=MAX_NOTES).map(|i| note(&i.to_string(), "x")).collect(),
    };
    assert!(many.validate().unwrap_err().contains("at most"));
    let ok = NotesDb {
        notes: (0..MAX_NOTES).map(|i| note(&i.to_string(), "x")).collect(),
    };
    assert!(ok.validate().is_ok());
    assert!(
        NotesDb {
            notes: vec![note("a", &"x".repeat(MAX_TEXT_CHARS + 1))]
        }
        .validate()
        .unwrap_err()
        .contains("too long")
    );
    assert!(
        NotesDb {
            notes: vec![note(&"k".repeat(201), "x")]
        }
        .validate()
        .is_err()
    );
    assert!(
        DimensionStyle {
            prefix: "x".repeat(MAX_TEXT_CHARS + 1),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        TagStyle {
            lines: vec!["x".repeat(MAX_TEXT_CHARS + 1)],
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    let mut c = callout_default();
    c.note = Some("k".repeat(201));
    assert!(Annot::Callout(c).problem().is_some());
}

#[test]
fn objects_round_trip_with_every_option() {
    let full = serde_json::json!({
        "type": "dimension", "mode": {"type": "ordinate", "axis": "vertical"}, "points": [[0.0, 0.0], [1.0, 1.0]],
        "anchors": [{"object": "00000000-0000-0000-0000-000000000003", "target": {"at": "edge", "index": 1, "t": 0.25}, "count": 4}, null],
        "offset": -2.5, "style": "00000000-0000-0000-0000-000000000007", "text": "x", "detached": true
    });
    let d: Annot = serde_json::from_value(full.clone()).unwrap();
    assert_eq!(serde_json::to_value(&d).unwrap(), full);
    let minimal: Annot = serde_json::from_value(serde_json::json!({"type": "dimension", "mode": {"type": "angular"}, "points": [[0, 0], [1, 0], [0, 1]]})).unwrap();
    let Annot::Dimension(m) = &minimal else {
        panic!()
    };
    assert!(m.anchors.is_empty() && !m.detached && m.offset == 0.0);
    let callout: Annot = serde_json::from_value(serde_json::json!({"type": "callout", "at": [0, 0], "leaders": [{"points": [[1, 1]], "anchor": {"object": "00000000-0000-0000-0000-000000000001", "target": {"at": "center"}}}], "terminator": "tick", "detached": true})).unwrap();
    assert_eq!(serde_json::to_value(&callout).unwrap()["detached"], true);
    let Annot::Callout(c) = &callout else {
        panic!()
    };
    assert_eq!(c.shoulder_mm, 5.0);
    let tag: Annot =
        serde_json::from_value(serde_json::json!({"type": "data_tag", "style": "00000000-0000-0000-0000-000000000001", "object": "00000000-0000-0000-0000-000000000002"}))
            .unwrap();
    let Annot::DataTag(t) = &tag else { panic!() };
    assert_eq!(t.offset, DVec2::ZERO);
    let legend: Annot = serde_json::from_value(serde_json::json!({"type": "legend", "at": [0, 0], "title": "T", "criteria": "c", "group_by": "Class", "show_count": false, "swatch_mm": 8, "style": "00000000-0000-0000-0000-000000000002"})).unwrap();
    assert_eq!(serde_json::to_value(&legend).unwrap()["group_by"], "Class");
    let legend_min: Annot =
        serde_json::from_value(serde_json::json!({"type": "legend", "at": [0, 0]})).unwrap();
    let Annot::Legend(l) = &legend_min else {
        panic!()
    };
    assert!(l.show_count && l.group_by == "Style" && l.swatch_mm == 6.0);
    for style in ["arrow", "compass", "simple"] {
        let n: Annot = serde_json::from_value(serde_json::json!({"type": "north_arrow", "at": [0, 0], "style": style, "use_georef": true})).unwrap();
        assert_eq!(serde_json::to_value(&n).unwrap()["style"], style);
    }
    for unit in ["mm", "m", "ft"] {
        let b: Annot = serde_json::from_value(serde_json::json!({"type": "scale_bar", "at": [0, 0], "unit": unit, "scale": 50, "style": "00000000-0000-0000-0000-000000000003", "segments": 2, "segment": 1.5, "height_mm": 3})).unwrap();
        let Annot::ScaleBar(b) = &b else { panic!() };
        assert_eq!(b.segments, 2);
    }
    let rc: Annot = serde_json::from_value(
        serde_json::json!({"type": "revision_cloud", "points": [[0, 0], [1, 0], [0, 1]]}),
    )
    .unwrap();
    let Annot::RevisionCloud(r) = &rc else {
        panic!()
    };
    assert!(r.mark && r.bump_mm == 6.0);
    assert_eq!(
        Anchor::new(ObjectId::from_num(1), AnchorTarget::Center).count,
        None
    );
    for axis in ["horizontal", "vertical", "aligned"] {
        assert!(serde_json::from_value::<Axis>(serde_json::json!(axis)).is_ok());
    }
}

#[test]
fn defaults_and_the_dimension_arm_of_draw() {
    let b: Annot =
        serde_json::from_value(serde_json::json!({"type": "scale_bar", "at": [0, 0]})).unwrap();
    let Annot::ScaleBar(b) = &b else { panic!() };
    assert_eq!((b.segment, b.segments, b.height_mm), (5.0, 4, 2.0));
    let mut ed = Editor::default();
    let st = res(
        &mut ed,
        "D",
        AnnotResource::DimensionStyle(DimensionStyle {
            unit: LengthUnit::M,
            precision: 1,
            ..Default::default()
        }),
    );
    let d = Dimension {
        style: Some(st),
        ..dim(horiz(), vec![v(0.0, 0.0), v(5000.0, 0.0)])
    };
    assert_eq!(draw::dimension_style(ed.doc(), &d).unit, LengthUnit::M);
    let prims = draw::prims(
        ed.doc(),
        &Annot::Dimension(d),
        &Ctx {
            paper: 1.0,
            real: 1.0,
            view: None,
        },
        &PlainSource,
    );
    assert!(
        prims
            .iter()
            .any(|p| matches!(p, Prim::Text { text, .. } if text == "5.0"))
    );
    // A center anchor on an object that has no center is dropped without a flag.
    let l = line(&mut ed, v(0.0, 0.0), v(10.0, 0.0));
    let r = add_annot(
        &mut ed,
        Annot::Dimension(Dimension {
            anchors: vec![Some(Anchor::new(l, AnchorTarget::Center))],
            ..dim(DimMode::Radial, vec![v(0.0, 0.0), v(0.0, 5.0)])
        }),
    );
    ed.execute(&Command::DeleteObjects { ids: vec![l] })
        .unwrap();
    let Annot::Dimension(x) = annot_of(&ed, r) else {
        panic!()
    };
    assert!(x.anchors.is_empty() && !x.detached);
    // An anchor on a shape that has no reference point detaches.
    let site = add(
        &mut ed,
        Shape::SiteModel {
            points: vec![],
            lines: vec![],
            settings: Default::default(),
        },
    );
    let _ = site;
}

// ---------------------------------------------------------------- tags follow every command

fn tag_style_res(ed: &mut Editor) -> ResourceId {
    res(ed, "Tag", AnnotResource::TagStyle(TagStyle::area()))
}

fn tag_of(ed: &mut Editor, style: ResourceId, object: ObjectId) -> ObjectId {
    add_annot(
        ed,
        Annot::DataTag(DataTag {
            style,
            object,
            offset: v(10.0, 5.0),
            at: DVec2::ZERO,
            anchor: DVec2::ZERO,
        }),
    )
}

/// The tag sits at the reference point of its object plus its paper offset.
fn assert_tag_follows(ed: &Editor, tag: ObjectId, object: ObjectId) {
    let Annot::DataTag(t) = annot_of(ed, tag) else {
        panic!()
    };
    let o = ed.doc().object(t.object).unwrap();
    let p = ref_point(&o.shape).unwrap();
    let paper = ed
        .doc()
        .layer(ed.doc().object(tag).unwrap().layer)
        .unwrap()
        .scale;
    assert_eq!(t.object, object);
    assert!((t.anchor - p).length() < 1e-6, "{:?} {:?}", t.anchor, p);
    assert!((t.at - (p + t.offset * paper)).length() < 1e-6);
}

fn square(x: f64, y: f64, side: f64) -> Vec<crate::curve::Vertex> {
    vec![
        vtx(x, y),
        vtx(x + side, y),
        vtx(x + side, y + side),
        vtx(x, y + side),
    ]
}

#[test]
fn tags_follow_rotate_scale_mirror_and_area_commands() {
    use crate::edit2d::Transform;
    let mut ed = Editor::default();
    let style = tag_style_res(&mut ed);
    let land = add(
        &mut ed,
        Shape::LandscapeArea {
            boundary: square(0.0, 0.0, 4000.0),
            name: "Bed".into(),
        },
    );
    let paving = add(
        &mut ed,
        Shape::Hardscape {
            boundary: square(10_000.0, 0.0, 2000.0),
            material: "Granite".into(),
            thickness: None,
            elevation: None,
        },
    );
    let rect = add(
        &mut ed,
        Shape::Rect {
            min: v(20_000.0, 0.0),
            max: v(22_000.0, 1000.0),
        },
    );
    let poly = add(
        &mut ed,
        Shape::Polyline {
            points: square(30_000.0, 0.0, 1500.0),
            closed: true,
        },
    );
    let tags = [
        (land, tag_of(&mut ed, style, land)),
        (paving, tag_of(&mut ed, style, paving)),
        (rect, tag_of(&mut ed, style, rect)),
        (poly, tag_of(&mut ed, style, poly)),
    ];
    let check = |ed: &Editor| {
        tags.iter()
            .for_each(|(o, t)| assert_tag_follows(ed, *t, *o))
    };
    check(&ed);
    ed.execute(&Command::RotateObjects {
        ids: vec![rect, poly],
        center: v(500.0, 500.0),
        angle: 0.7,
    })
    .unwrap();
    check(&ed);
    ed.execute(&Command::TransformObjects {
        ids: vec![rect, poly],
        transform: Transform::Scale {
            center: v(0.0, 0.0),
            sx: 2.0,
            sy: None,
        },
        copy: false,
    })
    .unwrap();
    check(&ed);
    ed.execute(&Command::TransformObjects {
        ids: vec![rect, poly],
        transform: Transform::Mirror {
            a: v(0.0, 0.0),
            b: v(1.0, 3.0),
        },
        copy: false,
    })
    .unwrap();
    check(&ed);
    // Landscape areas and hardscapes cannot be scaled or turned, but they move.
    ed.execute(&Command::TransformObjects {
        ids: vec![land, paving],
        transform: Transform::Move {
            delta: v(700.0, -300.0),
        },
        copy: false,
    })
    .unwrap();
    check(&ed);
    ed.execute(&Command::SetLandscapeArea {
        ids: vec![land],
        patch: crate::landscape::LandscapePatch {
            holes: Some(vec![square(1700.0, 200.0, 1000.0)]),
            name: Some("Renamed".into()),
            ..Default::default()
        },
    })
    .unwrap();
    check(&ed);
    ed.execute(&Command::SetHardscape {
        ids: vec![paving],
        patch: crate::hardscape::HardscapePatch {
            thickness: Some(80.0),
            ..Default::default()
        },
    })
    .unwrap();
    check(&ed);
}

#[test]
fn a_tag_follows_its_object_when_a_group_moves() {
    let mut ed = Editor::default();
    let style = tag_style_res(&mut ed);
    let a = add(
        &mut ed,
        Shape::Rect {
            min: v(0.0, 0.0),
            max: v(1000.0, 1000.0),
        },
    );
    let b = add(
        &mut ed,
        Shape::Circle {
            center: v(5000.0, 0.0),
            radius: 500.0,
        },
    );
    let tag = tag_of(&mut ed, style, b);
    let g = ObjectId(
        ed.execute(&Command::Group {
            ids: vec![a, b],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    assert_tag_follows(&ed, tag, b);
    ed.execute(&Command::MoveObjects {
        ids: vec![g],
        delta: v(300.0, -200.0),
    })
    .unwrap();
    assert_tag_follows(&ed, tag, b);
    let Annot::DataTag(t) = annot_of(&ed, tag) else {
        panic!()
    };
    assert_eq!(t.anchor, v(5300.0, -200.0));
}

#[test]
fn copies_of_annotations_link_to_the_copies_of_their_objects() {
    let mut ed = Editor::default();
    let style = tag_style_res(&mut ed);
    let rect = add(
        &mut ed,
        Shape::Rect {
            min: v(0.0, 0.0),
            max: v(1000.0, 2000.0),
        },
    );
    let tag = tag_of(&mut ed, style, rect);
    let dim_id = add_annot(
        &mut ed,
        Annot::Dimension(Dimension {
            anchors: vec![
                Some(Anchor::new(rect, AnchorTarget::Vertex { index: 0 })),
                Some(Anchor::new(rect, AnchorTarget::Vertex { index: 1 })),
            ],
            ..dim(horiz(), vec![v(0.0, 0.0), v(1000.0, 0.0)])
        }),
    );
    let callout = add_annot(
        &mut ed,
        Annot::Callout(Callout {
            at: v(3000.0, 3000.0),
            text: "x".into(),
            note: None,
            notes_db: None,
            style: None,
            leaders: vec![Leader {
                points: vec![v(1000.0, 2000.0)],
                anchor: Some(Anchor::new(rect, AnchorTarget::Vertex { index: 2 })),
            }],
            shoulder_mm: 5.0,
            terminator: Terminator::Arrow,
            detached: false,
        }),
    );
    // The tag alone: it still tags the original, and its label stays where the copy put it.
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![tag],
            delta: v(2000.0, 0.0),
        })
        .unwrap();
    let copy = ObjectId(out.created[0]);
    let Annot::DataTag(t) = annot_of(&ed, copy) else {
        panic!()
    };
    let Annot::DataTag(orig) = annot_of(&ed, tag) else {
        panic!()
    };
    assert_eq!(t.object, rect);
    assert!((t.at - (orig.at + v(2000.0, 0.0))).length() < 1e-6);
    // The dimension and the callout alone: no link to the original, the points stay.
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![dim_id, callout],
            delta: v(0.0, 5000.0),
        })
        .unwrap();
    let Annot::Dimension(d) = annot_of(&ed, ObjectId(out.created[0])) else {
        panic!()
    };
    assert!(d.anchors.is_empty());
    assert_eq!(d.points, vec![v(0.0, 5000.0), v(1000.0, 5000.0)]);
    let Annot::Callout(c) = annot_of(&ed, ObjectId(out.created[1])) else {
        panic!()
    };
    assert_eq!(c.leaders[0].anchor, None);
    assert_eq!(c.leaders[0].points, vec![v(1000.0, 7000.0)]);
    // The shape and the annotations together: the copies are linked to each other.
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![rect, tag, dim_id, callout],
            delta: v(20_000.0, 0.0),
        })
        .unwrap();
    let (r2, t2, d2, c2) = (
        ObjectId(out.created[0]),
        ObjectId(out.created[1]),
        ObjectId(out.created[2]),
        ObjectId(out.created[3]),
    );
    assert_tag_follows(&ed, t2, r2);
    let Annot::Dimension(d) = annot_of(&ed, d2) else {
        panic!()
    };
    assert_eq!(
        d.anchors
            .iter()
            .flatten()
            .map(|a| a.object)
            .collect::<Vec<_>>(),
        vec![r2, r2]
    );
    assert_eq!(d.points, vec![v(20_000.0, 0.0), v(21_000.0, 0.0)]);
    let Annot::Callout(c) = annot_of(&ed, c2) else {
        panic!()
    };
    assert_eq!(c.leaders[0].anchor.map(|a| a.object), Some(r2));
    // The same through a transformed copy and an array.
    let out = ed
        .execute(&Command::TransformObjects {
            ids: vec![rect, tag],
            transform: crate::edit2d::Transform::Move {
                delta: v(0.0, 30_000.0),
            },
            copy: true,
        })
        .unwrap();
    assert_tag_follows(&ed, ObjectId(out.created[1]), ObjectId(out.created[0]));
    let out = ed
        .execute(&Command::ArrayObjects {
            ids: vec![rect, tag],
            array: crate::edit2d::spec::ArrayParams {
                layout: crate::edit2d::spec::ArrayLayout::Linear {
                    count: 2,
                    offset: Some(v(0.0, 9000.0)),
                    distance: None,
                    angle: None,
                },
                resize: None,
                rotate: None,
                retain: true,
            },
        })
        .unwrap();
    assert_tag_follows(&ed, ObjectId(out.created[1]), ObjectId(out.created[0]));
}
