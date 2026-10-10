use glam::DVec2;

use super::draw::{Ctx, Fill, PlainSource, Prim, ViewMap};
use super::style::{AnnotResource, DimensionStyle, Justify, LengthUnit, Note, NotesDb, TagFrame, TagStyle, TextStyle, Terminator, Tolerance};
use super::*;
use crate::command::{Command, Editor};
use crate::model::{Attributes, ClassId, Document, LayerId, ObjectId, Shape};
use crate::resource::{ResourceBody, ResourceId};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn vtx(x: f64, y: f64) -> crate::curve::Vertex {
    crate::curve::Vertex::new(DVec2::new(x, y), crate::curve::VertexKind::Corner)
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0])
}

fn add_annot(ed: &mut Editor, a: Annot) -> ObjectId {
    add(ed, Shape::Annotation(Box::new(a)))
}

fn res(ed: &mut Editor, name: &str, r: AnnotResource) -> ResourceId {
    ResourceId(ed.execute(&Command::AddResource { name: name.into(), body: ResourceBody::annot(r) }).unwrap().created[0])
}

fn annot_of(ed: &Editor, id: ObjectId) -> Annot {
    match &ed.doc().object(id).unwrap().shape {
        Shape::Annotation(a) => (**a).clone(),
        _ => panic!("not an annotation"),
    }
}

fn dim(mode: DimMode, points: Vec<DVec2>) -> Dimension {
    Dimension { mode, points, anchors: vec![], offset: 8.0, style: None, text: None }
}

fn horiz() -> DimMode {
    DimMode::Linear { axis: Axis::Horizontal }
}

fn line(ed: &mut Editor, a: DVec2, b: DVec2) -> ObjectId {
    add(ed, Shape::Line { a, b })
}

fn vertex(object: ObjectId, index: usize) -> Option<Anchor> {
    Some(Anchor { object, target: AnchorTarget::Vertex { index } })
}

fn all_kinds() -> Vec<Annot> {
    vec![
        Annot::Dimension(dim(horiz(), vec![v(0.0, 0.0), v(10.0, 0.0)])),
        Annot::Text(StyledText { at: v(1.0, 2.0), text: "a\nb".into(), style: None }),
        Annot::Callout(Callout {
            at: v(5.0, 5.0),
            text: "n".into(),
            note: None,
            notes_db: None,
            style: None,
            leaders: vec![Leader { points: vec![v(0.0, 0.0), v(2.0, 2.0)], anchor: None }],
            shoulder_mm: 5.0,
            terminator: Terminator::Arrow,
        }),
        Annot::DataTag(DataTag { style: ResourceId(1), object: ObjectId(2), offset: v(1.0, 1.0), at: v(3.0, 3.0), anchor: v(2.0, 2.0) }),
        Annot::Legend(Legend { at: v(1.0, 1.0), title: String::new(), criteria: String::new(), group_by: "Style".into(), show_count: true, swatch_mm: 6.0, style: None }),
        Annot::NorthArrow(NorthArrow { at: v(1.0, 1.0), size_mm: 15.0, rotation_deg: 0.0, use_georef: false, style: NorthStyle::Arrow }),
        Annot::ScaleBar(ScaleBar { at: v(1.0, 1.0), unit: BarUnit::M, segment: 5.0, segments: 4, height_mm: 2.0, scale: None, style: None }),
        Annot::RevisionCloud(RevisionCloud { points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)], bump_mm: 6.0, revision: "A".into(), date: String::new(), description: String::new(), author: String::new(), mark: true }),
    ]
}

#[test]
fn every_kind_round_trips_through_json_and_has_a_name() {
    for (a, name) in all_kinds().into_iter().zip(TYPE_NAMES) {
        assert_eq!(a.type_name(), name);
        let shape = Shape::Annotation(Box::new(a.clone()));
        assert_eq!(shape.kind(), "annotation");
        let json = serde_json::to_string(&shape).unwrap();
        assert!(json.contains(&format!("\"type\":\"{name}\"")), "{json}");
        let back: Shape = serde_json::from_str(&json).unwrap();
        assert_eq!(back, shape);
    }
}

#[test]
fn numbers_translate_and_bounds_cover_every_kind() {
    for a in all_kinds() {
        assert!(a.is_finite() && a.is_in_range());
        assert!(a.problem().is_none(), "{:?}", a.problem());
        let mut count = 0;
        a.numbers(&mut |_| count += 1);
        assert!(count >= 2);
        let (lo, hi) = a.bounds();
        let mut moved = a.clone();
        moved.translate(v(100.0, 50.0));
        let (lo2, hi2) = moved.bounds();
        assert!((lo2 - lo - v(100.0, 50.0)).length() < 1e-9 && (hi2 - hi - v(100.0, 50.0)).length() < 1e-9);
    }
    let mut bad = all_kinds().remove(0);
    if let Annot::Dimension(d) = &mut bad {
        d.offset = f64::NAN;
    }
    assert!(!bad.is_finite() && bad.problem().unwrap().contains("finite"));
    if let Annot::Dimension(d) = &mut bad {
        d.offset = 1.0e12;
    }
    assert!(!bad.is_in_range());
    // An empty point list has a zero box.
    let empty = Annot::Dimension(dim(horiz(), vec![]));
    assert_eq!(empty.bounds(), (DVec2::ZERO, DVec2::ZERO));
}

#[test]
fn problems_are_reported() {
    let mut kinds = all_kinds();
    let cases: Vec<(Annot, &str)> = vec![
        (Annot::Dimension(dim(horiz(), vec![v(0.0, 0.0)])), "needs 2 points"),
        (Annot::Dimension(dim(DimMode::Chain { axis: Axis::Aligned }, vec![v(0.0, 0.0), v(1.0, 0.0)])), "3 to"),
        (Annot::Dimension(Dimension { anchors: vec![None, None, None], ..dim(horiz(), vec![v(0.0, 0.0), v(1.0, 0.0)]) }), "at most one anchor"),
        (Annot::Dimension(dim(DimMode::Radial, vec![v(1.0, 1.0), v(1.0, 1.0)])), "radius"),
        (Annot::Dimension(Dimension { text: Some("x".repeat(1001)), ..dim(horiz(), vec![v(0.0, 0.0), v(1.0, 0.0)]) }), "too long"),
        (Annot::Text(StyledText { at: DVec2::ZERO, text: "x".repeat(MAX_TEXT_CHARS + 1), style: None }), "too long"),
    ];
    for (a, msg) in cases {
        assert!(a.problem().unwrap().contains(msg), "{msg}: {:?}", a.problem());
    }
    let Annot::Callout(c) = kinds.remove(2) else { panic!() };
    let mut c1 = c.clone();
    c1.leaders.clear();
    assert!(Annot::Callout(c1).problem().unwrap().contains("1 to 20 leaders"));
    let mut c2 = c.clone();
    c2.leaders[0].points.clear();
    assert!(Annot::Callout(c2).problem().unwrap().contains("1 to 20 points"));
    let mut c3 = c.clone();
    c3.shoulder_mm = -1.0;
    assert!(Annot::Callout(c3).problem().unwrap().contains("shoulder"));
    let mut c4 = c;
    c4.text = "x".repeat(MAX_TEXT_CHARS + 1);
    assert!(Annot::Callout(c4).problem().unwrap().contains("too long"));
    let Annot::Legend(l) = all_kinds().remove(4) else { panic!() };
    let mut l1 = l.clone();
    l1.swatch_mm = 0.0;
    assert!(Annot::Legend(l1).problem().unwrap().contains("swatch"));
    let mut l2 = l;
    l2.criteria = "x".repeat(2001);
    assert!(Annot::Legend(l2).problem().unwrap().contains("too long"));
    let Annot::NorthArrow(n) = all_kinds().remove(5) else { panic!() };
    assert!(Annot::NorthArrow(NorthArrow { size_mm: 1.0, ..n }).problem().unwrap().contains("north arrow"));
    let Annot::ScaleBar(b) = all_kinds().remove(6) else { panic!() };
    assert!(Annot::ScaleBar(ScaleBar { segment: 0.0, ..b.clone() }).problem().unwrap().contains("segment"));
    assert!(Annot::ScaleBar(ScaleBar { segments: 21, ..b.clone() }).problem().is_some());
    assert!(Annot::ScaleBar(ScaleBar { height_mm: 0.1, ..b.clone() }).problem().unwrap().contains("height"));
    assert!(Annot::ScaleBar(ScaleBar { scale: Some(0.0), ..b }).problem().unwrap().contains("scale"));
    let Annot::RevisionCloud(r) = all_kinds().remove(7) else { panic!() };
    assert!(Annot::RevisionCloud(RevisionCloud { points: vec![v(0.0, 0.0)], ..r.clone() }).problem().unwrap().contains("3 to"));
    assert!(Annot::RevisionCloud(RevisionCloud { bump_mm: 0.5, ..r.clone() }).problem().unwrap().contains("bump"));
    assert!(Annot::RevisionCloud(RevisionCloud { revision: "x".repeat(501), ..r }).problem().unwrap().contains("too long"));
}

#[test]
fn dim_mode_names_round_trip() {
    for name in DimMode::NAMES {
        let m = DimMode::from_name(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(m.name(), name);
    }
    assert_eq!(DimMode::from_name("Radius"), Some(DimMode::Radial));
    assert_eq!(DimMode::from_name("arc"), Some(DimMode::ArcLength));
    assert_eq!(DimMode::from_name("linear"), Some(DimMode::Linear { axis: Axis::Aligned }));
    assert_eq!(DimMode::from_name("chain horizontal"), Some(DimMode::Chain { axis: Axis::Horizontal }));
    assert_eq!(DimMode::from_name("nonsense"), None);
    assert_eq!(DimMode::Ordinate { axis: Axis::Aligned }.name(), "ordinate_horizontal");
    assert_eq!(DimMode::Angular.point_range(), (3, 3));
    assert_eq!(DimMode::Ordinate { axis: Axis::Vertical }.point_range().0, 2);
}

#[test]
fn shape_helpers_find_vertices_edges_and_reference_points() {
    let rect = Shape::Rect { min: v(10.0, 10.0), max: v(0.0, 0.0) };
    assert_eq!(shape_vertices(&rect), vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)]);
    assert_eq!(shape_edges(&rect).len(), 4);
    assert_eq!(ref_point(&rect), Some(v(5.0, 5.0)));
    let circle = Shape::Circle { center: v(1.0, 1.0), radius: 2.0 };
    assert_eq!(shape_vertices(&circle)[0], v(3.0, 1.0));
    assert!(shape_edges(&circle).is_empty());
    assert_eq!(ref_point(&circle), Some(v(1.0, 1.0)));
    let arc = Shape::Arc { center: DVec2::ZERO, radius: 2.0, start: 0.0, end: std::f64::consts::PI };
    let av = shape_vertices(&arc);
    assert!((av[1] - v(-2.0, 0.0)).length() < 1e-9 && (av[2] - v(0.0, 2.0)).length() < 1e-9);
    let line = Shape::Line { a: v(0.0, 0.0), b: v(4.0, 0.0) };
    assert_eq!(shape_edges(&line), vec![(v(0.0, 0.0), v(4.0, 0.0))]);
    assert_eq!(ref_point(&line), Some(v(2.0, 0.0)));
    let open = Shape::Polyline { points: vec![vtx(0.0, 0.0), vtx(2.0, 0.0), vtx(2.0, 2.0)], closed: false };
    assert_eq!(shape_edges(&open).len(), 2);
    assert_eq!(ref_point(&open), Some(v(4.0 / 3.0, 2.0 / 3.0)));
    let closed = Shape::Polyline { points: vec![vtx(0.0, 0.0), vtx(4.0, 0.0), vtx(4.0, 4.0), vtx(0.0, 4.0)], closed: true };
    assert_eq!(shape_edges(&closed).len(), 4);
    assert!((ref_point(&closed).unwrap() - v(2.0, 2.0)).length() < 1e-9);
    let p3 = Shape::Polyline3d { points: vec![crate::DVec3::new(0.0, 0.0, 1.0), crate::DVec3::new(2.0, 0.0, 1.0)], closed: false };
    assert_eq!(shape_vertices(&p3).len(), 2);
    assert!(ref_point(&p3).is_some());
    assert_eq!(shape_vertices(&Shape::Locus3d { p: crate::DVec3::new(1.0, 2.0, 3.0) }), vec![v(1.0, 2.0)]);
    assert_eq!(ref_point(&Shape::Locus3d { p: crate::DVec3::new(1.0, 2.0, 3.0) }), Some(v(1.0, 2.0)));
    let plant = Shape::Plant { at: v(3.0, 3.0), species: "x".into(), spread: 1.0, height: 1.0 };
    assert_eq!(shape_vertices(&plant), vec![v(3.0, 3.0)]);
    assert_eq!(ref_point(&plant), Some(v(3.0, 3.0)));
    assert!(shape_vertices(&Shape::SiteModel { points: vec![], lines: vec![], settings: Default::default() }).is_empty());
    assert_eq!(ref_point(&Shape::SiteModel { points: vec![], lines: vec![], settings: Default::default() }), None);
    assert_eq!(ref_point(&Shape::Polyline { points: vec![], closed: false }), None);
    assert!(shape_edges(&plant).is_empty());
    let a = Shape::Annotation(Box::new(all_kinds().remove(1)));
    assert_eq!(ref_point(&a), Some(v(1.0, 2.0)));
    assert_eq!(centroid(&[v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0)]), v(1.0, 0.0));
}

#[test]
fn anchors_resolve_to_points() {
    let mut ed = Editor::default();
    let l = line(&mut ed, v(0.0, 0.0), v(10.0, 0.0));
    let doc = ed.doc();
    let a = |target| Anchor { object: l, target };
    assert_eq!(anchor_point(doc, &a(AnchorTarget::Vertex { index: 1 })), Some(v(10.0, 0.0)));
    assert_eq!(anchor_point(doc, &a(AnchorTarget::Vertex { index: 2 })), None);
    assert_eq!(anchor_point(doc, &a(AnchorTarget::Edge { index: 0, t: 0.25 })), Some(v(2.5, 0.0)));
    assert_eq!(anchor_point(doc, &a(AnchorTarget::Edge { index: 0, t: 7.0 })), Some(v(10.0, 0.0)));
    assert_eq!(anchor_point(doc, &a(AnchorTarget::Edge { index: 3, t: 0.5 })), None);
    assert_eq!(anchor_point(doc, &a(AnchorTarget::Center)), Some(v(5.0, 0.0)));
    assert_eq!(anchor_point(doc, &Anchor { object: ObjectId(99), target: AnchorTarget::Center }), None);
    let json = serde_json::json!({"object": 5, "target": {"at": "edge", "index": 1}});
    let back: Anchor = serde_json::from_value(json).unwrap();
    assert_eq!(back.target, AnchorTarget::Edge { index: 1, t: 0.5 });
}

#[test]
fn resources_validate_and_look_up() {
    assert!(TextStyle::default().validate().is_ok());
    assert!(TextStyle { size_mm: 0.0, ..Default::default() }.validate().is_err());
    assert!(TextStyle { font: " ".into(), ..Default::default() }.validate().is_err());
    assert!(TextStyle { line_spacing: 9.0, ..Default::default() }.validate().is_err());
    assert!(DimensionStyle::default().validate().is_ok());
    assert!(DimensionStyle { precision: 9, ..Default::default() }.validate().is_err());
    assert!(DimensionStyle { text_size_mm: f64::NAN, ..Default::default() }.validate().is_err());
    assert!(DimensionStyle { tolerance: Some(Tolerance { plus: -1.0, minus: 0.0 }), ..Default::default() }.validate().is_err());
    assert!(DimensionStyle { font: String::new(), ..Default::default() }.validate().is_err());
    assert!(TagStyle::default().validate().is_ok());
    assert!(TagStyle { lines: vec![], ..Default::default() }.validate().is_err());
    assert!(TagStyle { lines: vec!["{a".into()], ..Default::default() }.validate().is_err());
    assert!(TagStyle { lines: vec!["a}".into()], ..Default::default() }.validate().is_err());
    assert!(TagStyle { lines: vec!["{{a}}".into()], ..Default::default() }.validate().is_err());
    assert!(TagStyle { size_mm: 0.0, ..Default::default() }.validate().is_err());
    let db = NotesDb { notes: vec![Note { key: "N1".into(), text: "a".into() }] };
    assert_eq!(db.get(" n1 ").unwrap().text, "a");
    assert!(db.get("N2").is_none());
    assert!(db.validate().is_ok());
    assert!(NotesDb { notes: vec![Note { key: " ".into(), text: String::new() }] }.validate().is_err());
    assert!(NotesDb { notes: vec![Note { key: "a".into(), text: String::new() }, Note { key: "A".into(), text: String::new() }] }.validate().is_err());

    let mut ed = Editor::default();
    let t = res(&mut ed, "Text", AnnotResource::TextStyle(TextStyle::default()));
    let d = res(&mut ed, "Dim", AnnotResource::DimensionStyle(DimensionStyle::default()));
    let g = res(&mut ed, "Tag", AnnotResource::TagStyle(TagStyle::default()));
    let n = res(&mut ed, "Notes", AnnotResource::NotesDatabase(db));
    let r = &ed.doc().resources;
    assert!(r.text_style(t).is_some() && r.text_style(d).is_none());
    assert!(r.dimension_style(d).is_some() && r.dimension_style(t).is_none());
    assert!(r.tag_style(g).is_some() && r.tag_style(n).is_none());
    assert!(r.notes_db(n).is_some() && r.notes_db(g).is_none());
    assert_eq!(r.find_annot("tag_style", "TAG"), Some(g));
    assert_eq!(r.find_annot("tag_style", "nope"), None);
    assert!(r.text_style(ResourceId(999)).is_none());
    // The body names and JSON.
    let names: Vec<&str> = r.iter().map(|x| x.body.type_name()).collect();
    assert_eq!(names, TYPE_NAMES_OF_STYLES);
    let json = serde_json::to_string(&ed.doc()).unwrap();
    let back = Document::from_json(&json).unwrap();
    assert_eq!(&back, ed.doc());
    // A bad resource body is refused, and other types stay unknown.
    let bad = serde_json::json!({"type": "text_style", "size_mm": "big"});
    assert!(serde_json::from_value::<ResourceBody>(bad).is_err());
    assert!(AnnotResource::from_json(&serde_json::json!({"type": "hatch"})).is_none());
    assert!(AnnotResource::from_json(&serde_json::json!(5)).is_none());
    assert!(ed.execute(&Command::AddResource { name: "Bad".into(), body: ResourceBody::annot(AnnotResource::TextStyle(TextStyle { size_mm: -1.0, ..Default::default() })) }).is_err());
    assert!(ed.execute(&Command::AddResource { name: "Text".into(), body: ResourceBody::annot(AnnotResource::TextStyle(TextStyle::default())) }).is_err());
}

const TYPE_NAMES_OF_STYLES: [&str; 4] = ["text_style", "dimension_style", "tag_style", "notes_database"];

#[test]
fn formats_lengths_and_angles() {
    let mut s = DimensionStyle::default();
    assert_eq!(dim::format_length(1234.6, &s), "1235");
    s.precision = 2;
    s.unit = LengthUnit::M;
    s.show_unit = true;
    assert_eq!(dim::format_length(1234.5, &s), "1.23 m");
    s.unit = LengthUnit::Cm;
    assert_eq!(dim::format_length(1234.5, &s), "123.45 cm");
    s.unit = LengthUnit::Mm;
    assert_eq!(dim::format_length(10.0, &s), "10.00 mm");
    s.unit = LengthUnit::In;
    assert_eq!(dim::format_length(25.4, &s), "1.00\"");
    s.unit = LengthUnit::Ft;
    assert_eq!(dim::format_length(304.8, &s), "1.00'");
    s.show_unit = false;
    assert_eq!(dim::format_length(304.8, &s), "1.00");
    s.unit = LengthUnit::FtIn;
    s.precision = 1;
    assert_eq!(dim::format_length(25.4 * 40.5, &s), "3'-4.5\"");
    assert_eq!(dim::format_length(25.4 * 5.0, &s), "5.0\"");
    // 11.99 inches rounds up to the next foot.
    assert_eq!(dim::format_length(25.4 * 11.99, &s), "1'-0.0\"");
    assert_eq!(dim::format_length(-25.4 * 40.5, &s), "-3'-4.5\"");
    s.unit = LengthUnit::Mm;
    s.precision = 0;
    assert_eq!(dim::format_length(-0.2, &s), "0");
    assert_eq!(dim::format_angle(90.0, &s), "90.0\u{b0}");
    s.angle_precision = 0;
    assert_eq!(dim::format_angle(-0.0, &s), "0\u{b0}");
}

#[test]
fn values_match_the_geometry_of_every_mode() {
    let h = dim(horiz(), vec![v(0.0, 0.0), v(30.0, 40.0)]);
    assert_eq!(dim::values(&h), vec![30.0]);
    let vert = dim(DimMode::Linear { axis: Axis::Vertical }, vec![v(0.0, 0.0), v(30.0, 40.0)]);
    assert_eq!(dim::values(&vert), vec![40.0]);
    let al = dim(DimMode::Linear { axis: Axis::Aligned }, vec![v(0.0, 0.0), v(30.0, 40.0)]);
    assert_eq!(dim::values(&al), vec![50.0]);
    let chain = dim(DimMode::Chain { axis: Axis::Horizontal }, vec![v(0.0, 0.0), v(10.0, 0.0), v(25.0, 5.0)]);
    assert_eq!(dim::values(&chain), vec![10.0, 15.0]);
    let base = dim(DimMode::Baseline { axis: Axis::Horizontal }, vec![v(0.0, 0.0), v(10.0, 0.0), v(25.0, 5.0)]);
    assert_eq!(dim::values(&base), vec![10.0, 25.0]);
    let ord = dim(DimMode::Ordinate { axis: Axis::Vertical }, vec![v(5.0, 5.0), v(7.0, 9.0), v(1.0, 20.0)]);
    assert_eq!(dim::values(&ord), vec![4.0, 15.0]);
    let ang = dim(DimMode::Angular, vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)]);
    assert!((dim::values(&ang)[0] - 90.0).abs() < 1e-9);
    let reflex = dim(DimMode::Angular, vec![v(0.0, 0.0), v(0.0, 1.0), v(1.0, 0.0)]);
    assert!((dim::values(&reflex)[0] - 270.0).abs() < 1e-9);
    let rad = dim(DimMode::Radial, vec![v(1.0, 1.0), v(4.0, 5.0)]);
    assert_eq!(dim::values(&rad), vec![5.0]);
    let dia = dim(DimMode::Diameter, vec![v(1.0, 1.0), v(4.0, 5.0)]);
    assert_eq!(dim::values(&dia), vec![10.0]);
    let arc = dim(DimMode::ArcLength, vec![v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0)]);
    assert!((dim::values(&arc)[0] - 10.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    // Aligned chain with identical points falls back to the X axis.
    let same = dim(DimMode::Chain { axis: Axis::Aligned }, vec![v(1.0, 1.0), v(1.0, 1.0), v(1.0, 1.0)]);
    assert_eq!(dim::values(&same), vec![0.0, 0.0]);
}

#[test]
fn texts_use_the_style_the_scale_and_overrides() {
    let ctx = Ctx { paper: 1.0, real: 1.0, view: None };
    let mut d = dim(horiz(), vec![v(0.0, 0.0), v(5000.0, 0.0)]);
    let style = DimensionStyle { unit: LengthUnit::M, precision: 1, show_unit: true, prefix: "~".into(), suffix: "!".into(), tolerance: Some(Tolerance { plus: 0.5, minus: 0.5 }), ..Default::default() };
    assert_eq!(dim::texts(&d, &style, &ctx), vec!["~5.0 m \u{b1}0.5!"]);
    let style2 = DimensionStyle { precision: 2, tolerance: Some(Tolerance { plus: 1.0, minus: 0.25 }), ..Default::default() };
    assert_eq!(dim::texts(&d, &style2, &ctx), vec!["5000.00 +1.00/-0.25"]);
    // In a viewport the points are paper mm and the real length is the scale times that.
    let vp = Ctx { paper: 1.0, real: 1.0 / 100.0, view: None };
    d.points[1] = v(50.0, 0.0);
    assert_eq!(dim::texts(&d, &DimensionStyle::default(), &vp), vec!["5000"]);
    d.text = Some("custom".into());
    assert_eq!(dim::texts(&d, &DimensionStyle::default(), &ctx), vec!["custom"]);
    let rad = dim(DimMode::Radial, vec![v(0.0, 0.0), v(3.0, 4.0)]);
    assert_eq!(dim::texts(&rad, &DimensionStyle::default(), &ctx), vec!["R5"]);
    let dia = dim(DimMode::Diameter, vec![v(0.0, 0.0), v(3.0, 4.0)]);
    assert_eq!(dim::texts(&dia, &DimensionStyle::default(), &ctx), vec!["\u{d8}10"]);
    let arc = dim(DimMode::ArcLength, vec![v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0)]);
    assert!(dim::texts(&arc, &DimensionStyle::default(), &ctx)[0].starts_with("Arc 16"));
    let ang = dim(DimMode::Angular, vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)]);
    assert_eq!(dim::texts(&ang, &DimensionStyle::default(), &ctx), vec!["90.0\u{b0}"]);
    // A text override only applies to a single dimension.
    let mut chain = dim(DimMode::Chain { axis: Axis::Horizontal }, vec![v(0.0, 0.0), v(1.0, 0.0), v(3.0, 0.0)]);
    chain.text = Some("x".into());
    assert_eq!(dim::texts(&chain, &DimensionStyle::default(), &ctx), vec!["1", "2"]);
}

fn polys(p: &[Prim]) -> usize {
    p.iter().filter(|x| matches!(x, Prim::Poly { .. })).count()
}

fn texts_of(p: &[Prim]) -> Vec<String> {
    p.iter().filter_map(|x| if let Prim::Text { text, .. } = x { Some(text.clone()) } else { None }).collect()
}

#[test]
fn dimensions_draw_lines_ends_and_text_for_every_mode() {
    let ctx = Ctx { paper: 100.0, real: 1.0, view: None };
    let style = DimensionStyle::default();
    for (mode, pts, want_text) in [
        (horiz(), vec![v(0.0, 0.0), v(5000.0, 0.0)], vec!["5000"]),
        (DimMode::Linear { axis: Axis::Vertical }, vec![v(0.0, 0.0), v(0.0, 3000.0)], vec!["3000"]),
        (DimMode::Linear { axis: Axis::Aligned }, vec![v(0.0, 0.0), v(3000.0, 4000.0)], vec!["5000"]),
        (DimMode::Chain { axis: Axis::Horizontal }, vec![v(0.0, 0.0), v(2000.0, 0.0), v(5000.0, 0.0)], vec!["2000", "3000"]),
        (DimMode::Baseline { axis: Axis::Horizontal }, vec![v(0.0, 0.0), v(2000.0, 0.0), v(5000.0, 0.0)], vec!["2000", "5000"]),
        (DimMode::Ordinate { axis: Axis::Horizontal }, vec![v(0.0, 0.0), v(2000.0, 500.0), v(5000.0, 900.0)], vec!["2000", "5000"]),
        (DimMode::Ordinate { axis: Axis::Vertical }, vec![v(0.0, 0.0), v(2000.0, 500.0)], vec!["500"]),
        (DimMode::Angular, vec![v(0.0, 0.0), v(3000.0, 0.0), v(0.0, 3000.0)], vec!["90.0\u{b0}"]),
        (DimMode::Radial, vec![v(0.0, 0.0), v(1500.0, 0.0)], vec!["R1500"]),
        (DimMode::Diameter, vec![v(0.0, 0.0), v(0.0, 1500.0)], vec!["\u{d8}3000"]),
        (DimMode::ArcLength, vec![v(0.0, 0.0), v(2000.0, 0.0), v(0.0, 2000.0)], vec!["Arc 3142"]),
    ] {
        let d = dim(mode, pts);
        let prims = dim::prims(&d, &style, &ctx);
        assert!(polys(&prims) >= 2, "{}: {prims:?}", mode.name());
        assert_eq!(texts_of(&prims), want_text, "{}", mode.name());
        for p in &prims {
            let (lo, hi) = p.bounds();
            assert!(lo.is_finite() && hi.is_finite() && lo.cmple(hi).all());
        }
    }
    // A negative offset draws the line on the other side; baseline steps the other way too.
    let mut d = dim(DimMode::Baseline { axis: Axis::Horizontal }, vec![v(0.0, 0.0), v(2000.0, 0.0), v(5000.0, 0.0)]);
    d.offset = -8.0;
    let prims = dim::prims(&d, &style, &ctx);
    let ys: Vec<f64> = prims.iter().filter_map(|p| if let Prim::Text { at, .. } = p { Some(at.y) } else { None }).collect();
    assert!(ys[0] < 0.0 && ys[1] < ys[0]);
    d.mode = DimMode::Ordinate { axis: Axis::Horizontal };
    assert!(dim::prims(&d, &style, &ctx).iter().any(|p| matches!(p, Prim::Text { at, .. } if at.y < 0.0)));
    // A short dimension puts its ends outside. A zero length draws no dimension line.
    let short = dim(horiz(), vec![v(0.0, 0.0), v(100.0, 0.0)]);
    assert!(polys(&dim::prims(&short, &style, &ctx)) >= 3);
    let zero = dim(horiz(), vec![v(0.0, 0.0), v(0.0, 0.0)]);
    assert!(!dim::prims(&zero, &style, &ctx).is_empty());
    // Angular with an explicit radius, and the other ends.
    let mut a = dim(DimMode::Angular, vec![v(0.0, 0.0), v(3000.0, 0.0), v(0.0, 3000.0)]);
    a.offset = 20.0;
    assert!(polys(&dim::prims(&a, &style, &ctx)) >= 3);
    let flat = dim(DimMode::Angular, vec![v(0.0, 0.0), v(3000.0, 0.0), v(6000.0, 0.0)]);
    let _ = dim::prims(&flat, &style, &ctx);
    for t in [Terminator::OpenArrow, Terminator::Tick, Terminator::Dot, Terminator::None] {
        let s = DimensionStyle { terminator: t, ..Default::default() };
        let prims = dim::prims(&dim(horiz(), vec![v(0.0, 0.0), v(5000.0, 0.0)]), &s, &ctx);
        assert!(!prims.is_empty());
    }
    // Arc length with the dimension line inside.
    let mut al = dim(DimMode::ArcLength, vec![v(0.0, 0.0), v(2000.0, 0.0), v(0.0, 2000.0)]);
    al.offset = -5.0;
    assert!(polys(&dim::prims(&al, &style, &ctx)) >= 2);
    al.offset = 0.0;
    assert!(polys(&dim::prims(&al, &style, &ctx)) >= 2);
}

#[test]
fn offset_for_puts_the_line_through_the_point() {
    let pts = [v(0.0, 100.0), v(5000.0, 100.0)];
    assert_eq!(dim::offset_for(horiz(), &pts, v(1000.0, 1100.0), 100.0), 10.0);
    // Vertical dimensions measure along Y; the normal of Y is -X.
    let vp = [v(0.0, 0.0), v(0.0, 5000.0)];
    assert_eq!(dim::offset_for(DimMode::Linear { axis: Axis::Vertical }, &vp, v(-300.0, 0.0), 100.0), 3.0);
    assert_eq!(dim::offset_for(DimMode::Angular, &pts, v(0.0, 0.0), 100.0), 0.0);
    assert_eq!(dim::offset_for(horiz(), &[], v(0.0, 0.0), 100.0), 0.0);
    assert_eq!(dim::offset_for(horiz(), &pts, v(0.0, 0.0), 0.0), 0.0);
    // The line really sits where it was asked.
    let d = Dimension { offset: dim::offset_for(horiz(), &pts, v(2000.0, 900.0), 100.0), ..dim(horiz(), pts.to_vec()) };
    let ctx = Ctx { paper: 100.0, real: 1.0, view: None };
    let prims = dim::prims(&d, &DimensionStyle::default(), &ctx);
    assert!(prims.iter().any(|p| matches!(p, Prim::Poly { pts, .. } if pts.len() == 2 && (pts[0].y - 900.0).abs() < 1e-6 && (pts[1].y - 900.0).abs() < 1e-6)));
}

#[test]
fn associative_dimensions_follow_their_objects() {
    let mut ed = Editor::default();
    let l = line(&mut ed, v(0.0, 0.0), v(5000.0, 0.0));
    let d = add_annot(
        &mut ed,
        Annot::Dimension(Dimension { anchors: vec![vertex(l, 0), vertex(l, 1)], ..dim(horiz(), vec![v(0.0, 0.0), v(5000.0, 0.0)]) }),
    );
    // Move the line: the dimension follows.
    ed.execute(&Command::MoveObjects { ids: vec![l], delta: v(100.0, 50.0) }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, d) else { panic!() };
    assert_eq!(x.points, vec![v(100.0, 50.0), v(5100.0, 50.0)]);
    // Change the end of the line: the value follows.
    ed.execute(&Command::ModifyObject { id: l, shape: Some(Shape::Line { a: v(100.0, 50.0), b: v(8100.0, 50.0) }), layer: None, class: None, attrs: None }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, d) else { panic!() };
    assert_eq!(dim::values(&x), vec![8000.0]);
    // Delete the line: the dimension stays at its last position and loses its anchors.
    ed.execute(&Command::DeleteObjects { ids: vec![l] }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, d) else { panic!() };
    assert_eq!(x.points, vec![v(100.0, 50.0), v(8100.0, 50.0)]);
    assert!(x.anchors.is_empty());
    // Undo brings the line and the anchors back, as one step.
    assert!(ed.undo());
    let Annot::Dimension(x) = annot_of(&ed, d) else { panic!() };
    assert_eq!(x.anchors.len(), 2);
    assert!(ed.redo());
    // A dimension that is moved on its own snaps back to its anchors; without anchors it moves.
    let free = add_annot(&mut ed, Annot::Dimension(dim(horiz(), vec![v(0.0, 0.0), v(10.0, 0.0)])));
    ed.execute(&Command::MoveObjects { ids: vec![free], delta: v(5.0, 5.0) }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, free) else { panic!() };
    assert_eq!(x.points[0], v(5.0, 5.0));
}

#[test]
fn a_moved_dimension_with_anchors_stays_with_its_objects() {
    let mut ed = Editor::default();
    let l = line(&mut ed, v(0.0, 0.0), v(100.0, 0.0));
    let d = add_annot(&mut ed, Annot::Dimension(Dimension { anchors: vec![vertex(l, 0), vertex(l, 1)], ..dim(horiz(), vec![v(0.0, 0.0), v(100.0, 0.0)]) }));
    ed.execute(&Command::MoveObjects { ids: vec![d], delta: v(7.0, 7.0) }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, d) else { panic!() };
    assert_eq!(x.points, vec![v(0.0, 0.0), v(100.0, 0.0)]);
    // Moving both together keeps them together.
    ed.execute(&Command::MoveObjects { ids: vec![d, l], delta: v(7.0, 7.0) }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, d) else { panic!() };
    assert_eq!(x.points, vec![v(7.0, 7.0), v(107.0, 7.0)]);
}

#[test]
fn radial_and_arc_length_follow_circles_and_arcs() {
    let mut ed = Editor::default();
    let c = add(&mut ed, Shape::Circle { center: v(0.0, 0.0), radius: 1000.0 });
    let r = add_annot(
        &mut ed,
        Annot::Dimension(Dimension { anchors: vec![Some(Anchor { object: c, target: AnchorTarget::Center })], ..dim(DimMode::Radial, vec![v(0.0, 0.0), v(0.0, 1000.0)]) }),
    );
    ed.execute(&Command::ModifyObject { id: c, shape: Some(Shape::Circle { center: v(500.0, 0.0), radius: 2000.0 }), layer: None, class: None, attrs: None }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, r) else { panic!() };
    // The direction (up) stays; the center and radius follow.
    assert_eq!(x.points, vec![v(500.0, 0.0), v(500.0, 2000.0)]);
    assert_eq!(dim::values(&x), vec![2000.0]);
    let arc = add(&mut ed, Shape::Arc { center: v(0.0, 0.0), radius: 1000.0, start: 0.0, end: std::f64::consts::FRAC_PI_2 });
    let al = add_annot(
        &mut ed,
        Annot::Dimension(Dimension { anchors: vec![Some(Anchor { object: arc, target: AnchorTarget::Center })], ..dim(DimMode::ArcLength, vec![v(0.0, 0.0), v(1000.0, 0.0), v(0.0, 1000.0)]) }),
    );
    ed.execute(&Command::ModifyObject { id: arc, shape: Some(Shape::Arc { center: v(0.0, 0.0), radius: 2000.0, start: 0.0, end: std::f64::consts::PI }), layer: None, class: None, attrs: None }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, al) else { panic!() };
    assert!((dim::values(&x)[0] - 2000.0 * std::f64::consts::PI).abs() < 1e-6);
    // A center anchor on another kind of object changes nothing.
    let l = line(&mut ed, v(0.0, 0.0), v(1.0, 0.0));
    let odd = add_annot(&mut ed, Annot::Dimension(Dimension { anchors: vec![Some(Anchor { object: l, target: AnchorTarget::Center })], ..dim(DimMode::Radial, vec![v(0.0, 0.0), v(0.0, 5.0)]) }));
    ed.execute(&Command::MoveObjects { ids: vec![l], delta: v(1.0, 1.0) }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, odd) else { panic!() };
    assert_eq!(x.points[1], v(0.0, 5.0));
}

#[test]
fn callouts_follow_their_anchors_and_show_notes() {
    let mut ed = Editor::default();
    let l = line(&mut ed, v(0.0, 0.0), v(100.0, 0.0));
    let db = res(&mut ed, "Notes", AnnotResource::NotesDatabase(NotesDb { notes: vec![Note { key: "N1".into(), text: "Check on site".into() }] }));
    let c = add_annot(
        &mut ed,
        Annot::Callout(Callout {
            at: v(200.0, 200.0),
            text: "plain".into(),
            note: Some("N1".into()),
            notes_db: Some(db),
            style: None,
            leaders: vec![Leader { points: vec![v(100.0, 0.0)], anchor: Some(Anchor { object: l, target: AnchorTarget::Vertex { index: 1 } }) }],
            shoulder_mm: 5.0,
            terminator: Terminator::Arrow,
        }),
    );
    ed.execute(&Command::MoveObjects { ids: vec![l], delta: v(10.0, 10.0) }).unwrap();
    let Annot::Callout(x) = annot_of(&ed, c) else { panic!() };
    assert_eq!(x.leaders[0].points[0], v(110.0, 10.0));
    assert_eq!(draw::callout_text(ed.doc(), &x), "Check on site");
    // A change of the note reaches the callout.
    let id = db;
    ed.execute(&Command::ModifyResource { id, name: None, body: Some(ResourceBody::annot(AnnotResource::NotesDatabase(NotesDb { notes: vec![Note { key: "N1".into(), text: "Changed".into() }] }))) }).unwrap();
    assert_eq!(draw::callout_text(ed.doc(), &x), "Changed");
    let mut missing = x.clone();
    missing.note = Some("N9".into());
    assert_eq!(draw::callout_text(ed.doc(), &missing), "plain");
    ed.execute(&Command::DeleteObjects { ids: vec![l] }).unwrap();
    let Annot::Callout(x) = annot_of(&ed, c) else { panic!() };
    assert!(x.leaders[0].anchor.is_none());
    assert_eq!(x.leaders[0].points[0], v(110.0, 10.0));
    // The resource is in use, so it cannot go; a reassign works.
    assert!(ed.execute(&Command::DeleteResource { id: db, reassign: None }).is_err());
    let other = res(&mut ed, "Other", AnnotResource::NotesDatabase(NotesDb::default()));
    ed.execute(&Command::DeleteResource { id: db, reassign: Some(other) }).unwrap();
    let Annot::Callout(x) = annot_of(&ed, c) else { panic!() };
    assert_eq!(x.notes_db, Some(other));
}

#[test]
fn data_tags_follow_and_go_with_their_object() {
    let mut ed = Editor::default();
    let style = res(&mut ed, "T", AnnotResource::TagStyle(TagStyle::default()));
    let p = add(&mut ed, Shape::Plant { at: v(1000.0, 1000.0), species: "Oak".into(), spread: 2000.0, height: 5000.0 });
    let t = add_annot(&mut ed, Annot::DataTag(DataTag { style, object: p, offset: v(10.0, 5.0), at: DVec2::ZERO, anchor: DVec2::ZERO }));
    let Annot::DataTag(x) = annot_of(&ed, t) else { panic!() };
    // The layer scale is 100, so 10 paper mm are 1000 units.
    assert_eq!((x.anchor, x.at), (v(1000.0, 1000.0), v(2000.0, 1500.0)));
    ed.execute(&Command::MoveObjects { ids: vec![p], delta: v(500.0, 0.0) }).unwrap();
    let Annot::DataTag(x) = annot_of(&ed, t) else { panic!() };
    assert_eq!((x.anchor, x.at), (v(1500.0, 1000.0), v(2500.0, 1500.0)));
    // Dragging the tag changes its offset.
    ed.execute(&Command::MoveObjects { ids: vec![t], delta: v(100.0, 0.0) }).unwrap();
    let Annot::DataTag(x) = annot_of(&ed, t) else { panic!() };
    assert!((x.offset - v(11.0, 5.0)).length() < 1e-9);
    // Deleting the object deletes its tag; undo brings both back.
    ed.execute(&Command::DeleteObjects { ids: vec![p] }).unwrap();
    assert!(ed.doc().object(t).is_none());
    assert!(ed.undo());
    assert!(ed.doc().object(t).is_some());
    // The tag style cannot be deleted while a tag uses it.
    assert!(ed.execute(&Command::DeleteResource { id: style, reassign: None }).is_err());
    // A tag of a missing object or style is refused.
    let bad = Annot::DataTag(DataTag { style, object: ObjectId(999), offset: DVec2::ZERO, at: DVec2::ZERO, anchor: DVec2::ZERO });
    assert!(ed.execute(&Command::AddObject { shape: Shape::Annotation(Box::new(bad)), layer: None, class: None, attrs: Attributes::default() }).is_err());
    let bad = Annot::DataTag(DataTag { style: ResourceId(999), object: p, offset: DVec2::ZERO, at: DVec2::ZERO, anchor: DVec2::ZERO });
    assert!(ed.execute(&Command::AddObject { shape: Shape::Annotation(Box::new(bad)), layer: None, class: None, attrs: Attributes::default() }).is_err());
    // A tag cannot tag an annotation.
    let bad = Annot::DataTag(DataTag { style, object: t, offset: DVec2::ZERO, at: DVec2::ZERO, anchor: DVec2::ZERO });
    assert!(ed.execute(&Command::AddObject { shape: Shape::Annotation(Box::new(bad)), layer: None, class: None, attrs: Attributes::default() }).is_err());
}

#[test]
fn a_tag_on_a_locked_layer_stays_when_its_object_goes() {
    let mut ed = Editor::default();
    let style = res(&mut ed, "T", AnnotResource::TagStyle(TagStyle::default()));
    let p = add(&mut ed, Shape::Plant { at: v(0.0, 0.0), species: "Oak".into(), spread: 2000.0, height: 5000.0 });
    let t = add_annot(&mut ed, Annot::DataTag(DataTag { style, object: p, offset: DVec2::ZERO, at: DVec2::ZERO, anchor: DVec2::ZERO }));
    ed.execute(&Command::ModifyLayer { id: LayerId(1), name: None, visible: None, locked: Some(true), elevation: None, scale: None }).unwrap();
    // The plant is on the locked layer too, so the delete itself is refused.
    assert!(ed.execute(&Command::DeleteObjects { ids: vec![p] }).is_err());
    assert!(ed.doc().object(t).is_some());
}

#[test]
fn annotations_check_their_resources() {
    let mut ed = Editor::default();
    let text = res(&mut ed, "Text", AnnotResource::TextStyle(TextStyle::default()));
    let dimstyle = res(&mut ed, "Dim", AnnotResource::DimensionStyle(DimensionStyle::default()));
    let try_add = |ed: &mut Editor, a: Annot| ed.execute(&Command::AddObject { shape: Shape::Annotation(Box::new(a)), layer: None, class: None, attrs: Attributes::default() });
    let mut d = dim(horiz(), vec![v(0.0, 0.0), v(1.0, 0.0)]);
    d.style = Some(text);
    assert!(try_add(&mut ed, Annot::Dimension(d.clone())).unwrap_err().to_string().contains("not a dimension style"));
    d.style = Some(ResourceId(999));
    assert!(try_add(&mut ed, Annot::Dimension(d.clone())).unwrap_err().to_string().contains("does not exist"));
    d.style = Some(dimstyle);
    let id = ObjectId(try_add(&mut ed, Annot::Dimension(d)).unwrap().created[0]);
    assert!(ed.execute(&Command::DeleteResource { id: dimstyle, reassign: None }).is_err());
    let other = res(&mut ed, "Dim2", AnnotResource::DimensionStyle(DimensionStyle { text_size_mm: 5.0, ..Default::default() }));
    ed.execute(&Command::DeleteResource { id: dimstyle, reassign: Some(other) }).unwrap();
    let Annot::Dimension(x) = annot_of(&ed, id) else { panic!() };
    assert_eq!(x.style, Some(other));
    let all = [
        Annot::Text(StyledText { at: DVec2::ZERO, text: "a".into(), style: Some(dimstyle) }),
        Annot::Legend(Legend { style: Some(dimstyle), ..legend_default() }),
        Annot::ScaleBar(ScaleBar { style: Some(dimstyle), ..bar_default() }),
        Annot::Callout(Callout { style: Some(dimstyle), ..callout_default() }),
        Annot::Callout(Callout { notes_db: Some(text), ..callout_default() }),
        Annot::Callout(Callout { note: Some("N".into()), ..callout_default() }),
    ];
    for a in all {
        assert!(try_add(&mut ed, a).is_err());
    }
    // A style of the right type works, and `uses_resource` and `replace_resource` see it.
    let mut ok = vec![
        Annot::Text(StyledText { at: DVec2::ZERO, text: "a".into(), style: Some(text) }),
        Annot::Legend(Legend { style: Some(text), ..legend_default() }),
        Annot::ScaleBar(ScaleBar { style: Some(text), ..bar_default() }),
        Annot::Callout(Callout { style: Some(text), ..callout_default() }),
    ];
    for a in &mut ok {
        assert!(a.uses_resource(text) && !a.uses_resource(other));
        a.replace_resource(text, other);
        assert!(a.uses_resource(other));
    }
    let mut north = Annot::NorthArrow(NorthArrow { at: DVec2::ZERO, size_mm: 15.0, rotation_deg: 0.0, use_georef: false, style: NorthStyle::Arrow });
    north.replace_resource(text, other);
    assert!(!north.uses_resource(text));
    let mut tag = Annot::DataTag(DataTag { style: text, object: ObjectId(1), offset: DVec2::ZERO, at: DVec2::ZERO, anchor: DVec2::ZERO });
    tag.replace_resource(text, other);
    assert!(tag.uses_resource(other));
    tag.replace_resource(text, other);
    // Anchors to missing objects are refused.
    let d = Dimension { anchors: vec![vertex(ObjectId(999), 0)], ..dim(horiz(), vec![v(0.0, 0.0), v(1.0, 0.0)]) };
    assert!(try_add(&mut ed, Annot::Dimension(d)).unwrap_err().to_string().contains("does not exist"));
}

fn legend_default() -> Legend {
    Legend { at: DVec2::ZERO, title: String::new(), criteria: String::new(), group_by: "Style".into(), show_count: true, swatch_mm: 6.0, style: None }
}

fn bar_default() -> ScaleBar {
    ScaleBar { at: DVec2::ZERO, unit: BarUnit::M, segment: 5.0, segments: 4, height_mm: 2.0, scale: None, style: None }
}

fn callout_default() -> Callout {
    Callout { at: v(5.0, 5.0), text: "n".into(), note: None, notes_db: None, style: None, leaders: vec![Leader { points: vec![v(0.0, 0.0)], anchor: None }], shoulder_mm: 5.0, terminator: Terminator::Arrow }
}

#[test]
fn annotations_are_allowed_on_sheets_and_in_viewports() {
    let mut ed = Editor::default();
    let sheet = LayerId(ed.execute(&Command::AddSheet { name: "S".into(), paper: None, orientation: None, number: None, title: None }).unwrap().created[0]);
    let a = Annot::NorthArrow(NorthArrow { at: v(20.0, 20.0), size_mm: 15.0, rotation_deg: 0.0, use_georef: false, style: NorthStyle::Arrow });
    ed.execute(&Command::AddObject { shape: Shape::Annotation(Box::new(a.clone())), layer: Some(sheet), class: None, attrs: Attributes::default() }).unwrap();
    // In a viewport the annotation is a shape item.
    let mut vp = crate::sheets::Viewport::new(v(10.0, 10.0), v(210.0, 150.0), v(0.0, 0.0), 100.0, vec![LayerId(1)]);
    vp.annotations.push(crate::sheets::Annotation { id: 1, class: ClassId(0), attrs: Attributes::default(), item: crate::sheets::AnnotationItem::Shape { shape: Box::new(Shape::Annotation(Box::new(a))) } });
    assert!(vp.problem().is_none());
    // A bad annotation inside a viewport is found.
    let bad = Annot::ScaleBar(ScaleBar { segment: -1.0, ..bar_default() });
    vp.annotations[0].item = crate::sheets::AnnotationItem::Shape { shape: Box::new(Shape::Annotation(Box::new(bad))) };
    assert!(vp.problem().is_some());
    assert!(crate::sheets::shape_problem(&Shape::Annotation(Box::new(Annot::ScaleBar(ScaleBar { segments: 0, ..bar_default() })))).is_some());
}

fn ctx100() -> Ctx {
    Ctx { paper: 100.0, real: 1.0, view: None }
}

#[test]
fn ctx_maps_layers_and_viewports() {
    let mut ed = Editor::default();
    ed.execute(&Command::ModifyLayer { id: LayerId(1), name: None, visible: None, locked: None, elevation: None, scale: Some(50.0) }).unwrap();
    let c = Ctx::layer(ed.doc(), LayerId(1));
    assert_eq!((c.paper, c.real), (50.0, 1.0));
    assert_eq!(c.local(v(3.0, 4.0)), v(3.0, 4.0));
    assert_eq!(Ctx::layer(ed.doc(), LayerId(99)).paper, 1.0);
    let vp = crate::sheets::Viewport::new(v(10.0, 10.0), v(110.0, 60.0), v(1000.0, 1000.0), 100.0, vec![]);
    let c = Ctx::viewport(&vp);
    assert_eq!((c.paper, c.real), (1.0, 0.01));
    // The model center shows in the middle of the frame, relative to its lower left corner.
    assert_eq!(c.local(v(1000.0, 1000.0)), v(50.0, 25.0));
    assert_eq!(c.local(v(1100.0, 1000.0)), v(51.0, 25.0));
    assert_eq!(c.view, Some(ViewMap { center: v(1000.0, 1000.0), frame_half: v(50.0, 25.0), scale: 100.0 }));
}

#[test]
fn the_scale_bar_length_is_right_at_every_scale() {
    let bar = bar_default();
    assert_eq!(bar.real_length_mm(), 20000.0);
    // On a layer at 1:100 the bar is 20 m of model, which is 200 mm on paper.
    let c = ctx100();
    assert_eq!(draw::scale_bar_length(&bar, &c), 20000.0);
    assert_eq!(draw::scale_bar_length(&bar, &c) / c.paper, 200.0);
    // In a viewport at 1:250 the bar is 80 mm of paper.
    let vp = Ctx { paper: 1.0, real: 1.0 / 250.0, view: None };
    assert_eq!(draw::scale_bar_length(&bar, &vp), 80.0);
    // A scale of its own wins.
    let own = ScaleBar { scale: Some(500.0), ..bar.clone() };
    assert_eq!(draw::scale_bar_length(&own, &vp), 40.0);
    assert_eq!(draw::scale_bar_length(&own, &c), 4000.0);
    let ft = ScaleBar { unit: BarUnit::Ft, segment: 10.0, segments: 2, ..bar.clone() };
    assert!((ft.real_length_mm() - 6096.0).abs() < 1e-9);
    let mm = ScaleBar { unit: BarUnit::Mm, ..bar };
    assert_eq!(mm.real_length_mm(), 20.0);
    assert_eq!((BarUnit::M.label(), BarUnit::Mm.label(), BarUnit::Ft.label()), ("m", "mm", "ft"));
}

#[test]
fn annotations_draw_primitives() {
    let mut ed = Editor::default();
    let text = res(&mut ed, "T", AnnotResource::TextStyle(TextStyle { size_mm: 5.0, justify: Justify::Center, bold: true, ..Default::default() }));
    let tag_style = res(&mut ed, "Tag", AnnotResource::TagStyle(TagStyle { lines: vec!["a".into(), "bb".into()], frame: TagFrame::Box, ..Default::default() }));
    let p = add(&mut ed, Shape::Plant { at: v(5000.0, 5000.0), species: "Oak".into(), spread: 2000.0, height: 5000.0 });
    let doc = ed.doc();
    let c = ctx100();
    let src = PlainSource;
    let styled = Annot::Text(StyledText { at: v(0.0, 0.0), text: "ab\ncd".into(), style: Some(text) });
    let prims = draw::prims(doc, &styled, &c, &src);
    assert_eq!(texts_of(&prims), vec!["ab", "cd"]);
    // The size is paper mm times the scale.
    assert!(matches!(&prims[0], Prim::Text { height, center: true, bold: true, .. } if *height == 500.0));
    for justify in [Justify::Left, Justify::Right] {
        let mut d = doc.clone();
        d.resources.iter().for_each(|_| ());
        let _ = &mut d;
        let ts = TextStyle { justify, ..TextStyle::default() };
        let mut ed2 = Editor::default();
        let id = res(&mut ed2, "x", AnnotResource::TextStyle(ts));
        let a = Annot::Text(StyledText { at: v(100.0, 0.0), text: "abcd".into(), style: Some(id) });
        let pr = draw::prims(ed2.doc(), &a, &c, &src);
        let Prim::Text { at, .. } = &pr[0] else { panic!() };
        assert_eq!(at.x < 100.0, justify == Justify::Right);
    }
    // Callout: leaders, shoulder and text on the right; tip right of the text puts the text on the left.
    let callout = Annot::Callout(callout_default());
    let pr = draw::prims(doc, &callout, &c, &src);
    assert_eq!(texts_of(&pr), vec!["n"]);
    assert!(polys(&pr) >= 3);
    let mut left = callout_default();
    left.leaders[0].points[0] = v(50.0, 5.0);
    left.shoulder_mm = 0.0;
    let pr = draw::prims(doc, &Annot::Callout(left), &c, &src);
    assert_eq!(polys(&pr), 2);
    // A multi-line callout.
    let mut multi = callout_default();
    multi.text = "a\nb\nc".into();
    assert_eq!(texts_of(&draw::prims(doc, &Annot::Callout(multi), &c, &src)).len(), 3);
    // Tags: with the plain source they show the template, an orphan shows a question mark.
    let tag = Annot::DataTag(DataTag { style: tag_style, object: p, offset: v(10.0, 10.0), at: DVec2::ZERO, anchor: DVec2::ZERO });
    let pr = draw::prims(doc, &tag, &c, &src);
    assert_eq!(texts_of(&pr), vec!["a", "bb"]);
    assert!(polys(&pr) >= 3, "frame, leader and end");
    let orphan = Annot::DataTag(DataTag { style: tag_style, object: ObjectId(999), offset: v(10.0, 10.0), at: v(1.0, 1.0), anchor: v(0.0, 0.0) });
    assert_eq!(texts_of(&draw::prims(doc, &orphan, &c, &src)), vec!["?"]);
    let missing = Annot::DataTag(DataTag { style: ResourceId(999), object: p, offset: DVec2::ZERO, at: DVec2::ZERO, anchor: DVec2::ZERO });
    assert!(draw::prims(doc, &missing, &c, &src).is_empty());
    for frame in [TagFrame::Circle, TagFrame::Underline, TagFrame::None] {
        for leader in [true, false] {
            let mut e = Editor::default();
            let sid = res(&mut e, "Tag", AnnotResource::TagStyle(TagStyle { frame, leader, ..Default::default() }));
            let o = add(&mut e, Shape::Plant { at: v(5000.0, 5000.0), species: "Oak".into(), spread: 2000.0, height: 5000.0 });
            let t = Annot::DataTag(DataTag { style: sid, object: o, offset: v(10.0, 10.0), at: DVec2::ZERO, anchor: DVec2::ZERO });
            let pr = draw::prims(e.doc(), &t, &c, &src);
            assert!(!pr.is_empty());
            assert!(pr.len() > 1 || frame == TagFrame::None);
        }
    }
    // A tag whose label sits on the object has no leader.
    let on = Annot::DataTag(DataTag { style: tag_style, object: p, offset: DVec2::ZERO, at: DVec2::ZERO, anchor: DVec2::ZERO });
    assert_eq!(polys(&draw::prims(doc, &on, &c, &src)), 1);
    // Legend with the plain source: title only. Rows come from `cadektor-annot`.
    let legend = Annot::Legend(Legend { title: "Plants".into(), ..legend_default() });
    assert_eq!(texts_of(&draw::prims(doc, &legend, &c, &src)), vec!["Plants"]);
    // North arrows in all styles; the rotation follows the georeference.
    for style in [NorthStyle::Arrow, NorthStyle::Simple, NorthStyle::Compass] {
        let n = Annot::NorthArrow(NorthArrow { at: v(100.0, 100.0), size_mm: 20.0, rotation_deg: 30.0, use_georef: true, style });
        let pr = draw::prims(doc, &n, &c, &src);
        assert_eq!(texts_of(&pr), vec!["N"]);
        assert!(polys(&pr) >= 2);
        let (lo, hi) = draw::prims_bounds(&pr).unwrap();
        assert!(hi.x - lo.x > 1000.0, "20 paper mm at 1:100 is 2000 units");
    }
    // Scale bar labels.
    let pr = draw::prims(doc, &Annot::ScaleBar(bar_default()), &c, &src);
    assert_eq!(texts_of(&pr), vec!["0", "5", "10", "15", "20 m"]);
    assert_eq!(polys(&pr), 4);
    assert!(matches!(&pr[0], Prim::Poly { fill: Fill::Ink, .. }) && matches!(&pr[1], Prim::Poly { fill: Fill::None, .. }));
    // Revision cloud: scallops and the mark.
    let cloud = Annot::RevisionCloud(RevisionCloud { points: vec![v(0.0, 0.0), v(0.0, 3000.0), v(3000.0, 3000.0), v(3000.0, 0.0)], bump_mm: 6.0, revision: "B".into(), date: String::new(), description: String::new(), author: String::new(), mark: true });
    let pr = draw::prims(doc, &cloud, &c, &src);
    assert_eq!(texts_of(&pr), vec!["B"]);
    let Prim::Poly { pts, .. } = &pr[0] else { panic!() };
    assert!(pts.len() > 100);
    // The mark is optional.
    let Annot::RevisionCloud(r) = cloud else { panic!() };
    assert!(texts_of(&draw::prims(doc, &Annot::RevisionCloud(RevisionCloud { mark: false, ..r }), &c, &src)).is_empty());
    assert!(draw::prims_bounds(&[]).is_none());
}

#[test]
fn georef_rotates_the_north_arrow() {
    let mut ed = Editor::default();
    ed.execute(&Command::SetGeoreference { georef: Some(crate::georef::Georeference { crs: String::new(), origin: DVec2::ZERO, rotation: std::f64::consts::FRAC_PI_2, user_origin: DVec2::ZERO }) }).unwrap();
    let n = Annot::NorthArrow(NorthArrow { at: v(0.0, 0.0), size_mm: 10.0, rotation_deg: 0.0, use_georef: true, style: NorthStyle::Simple });
    let plain = Annot::NorthArrow(NorthArrow { at: v(0.0, 0.0), size_mm: 10.0, rotation_deg: 90.0, use_georef: false, style: NorthStyle::Simple });
    let a = draw::prims(ed.doc(), &n, &ctx100(), &PlainSource);
    let b = draw::prims(ed.doc(), &plain, &ctx100(), &PlainSource);
    assert_eq!(a, b);
}

#[test]
fn cloud_outline_bulges_outward_whatever_the_winding() {
    let ccw = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)];
    let cw: Vec<DVec2> = ccw.iter().rev().copied().collect();
    for pts in [ccw.to_vec(), cw] {
        let out = draw::cloud_outline(&pts, 20.0);
        assert!(out.iter().any(|p| p.y < -5.0) && out.iter().any(|p| p.x > 105.0), "bumps stick out");
        assert!(out.iter().all(|p| p.x > -30.0 && p.x < 130.0));
    }
    // A repeated point is skipped.
    let out = draw::cloud_outline(&[v(0.0, 0.0), v(0.0, 0.0), v(50.0, 0.0), v(50.0, 50.0)], 25.0);
    assert!(!out.is_empty());
}

#[test]
fn model_hooks_cover_the_annotation_shape() {
    let shape = Shape::Annotation(Box::new(all_kinds().remove(0)));
    assert!(shape.is_finite() && shape.is_in_range());
    assert!(shape.area().is_none());
    let mut moved = shape.clone();
    moved.translate(v(1.0, 1.0));
    assert_ne!(moved, shape);
    assert!(crate::sheets::allowed_on_sheet(&shape));
}
