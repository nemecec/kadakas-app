use cadektor_core::annot::style::{AnnotResource, TagStyle};
use cadektor_core::annot::*;
use cadektor_core::command::{Command, Editor};
use cadektor_core::model::{Attributes, Shape};
use cadektor_core::resource::{ResourceBody, ResourceId};
use glam::DVec2;

use super::*;

fn add(ed: &mut Editor, shape: Shape) -> cadektor_core::model::ObjectId {
    cadektor_core::model::ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn plant(ed: &mut Editor, x: f64, name: &str) -> cadektor_core::model::ObjectId {
    add(
        ed,
        Shape::Plant {
            at: DVec2::new(x, 0.0),
            species: name.into(),
            spread: 2000.0,
            height: 3000.0,
        },
    )
}

fn tag_style(ed: &mut Editor, lines: &[&str]) -> ResourceId {
    let s = TagStyle {
        lines: lines.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    };
    ResourceId(
        ed.execute(&Command::AddResource {
            name: "Tag".into(),
            body: ResourceBody::annot(AnnotResource::TagStyle(s)),
        })
        .unwrap()
        .created[0],
    )
}

#[test]
fn templates_use_worksheet_expressions() {
    let mut ed = Editor::default();
    let r = add(
        &mut ed,
        Shape::Rect {
            min: DVec2::ZERO,
            max: DVec2::new(2000.0, 3000.0),
        },
    );
    let o = ed.doc().object(r).unwrap();
    let doc = ed.doc();
    assert_eq!(
        render_template(doc, o, "{Kind}: {ROUND(Area, 1)} m2"),
        "rect: 6 m2"
    );
    assert_eq!(render_template(doc, o, "{Nope}"), "#NAME?");
    assert_eq!(render_template(doc, o, "{1 +}"), "#PARSE!");
    assert_eq!(render_template(doc, o, "open {Kind"), "open {Kind");
    assert_eq!(render_template(doc, o, "plain"), "plain");
    assert_eq!(eval_for(doc, o, "Area * 2"), Value::Number(12.0));
}

#[test]
fn tags_show_live_values_and_tag_all_is_repeatable() {
    let mut ed = Editor::default();
    let style = tag_style(&mut ed, &["{Kind}", "{ROUND(Area, 2)}"]);
    let a = add(
        &mut ed,
        Shape::Rect {
            min: DVec2::ZERO,
            max: DVec2::new(1000.0, 1000.0),
        },
    );
    plant(&mut ed, 5000.0, "Oak");
    let cmds = tag_all(
        ed.doc(),
        style,
        "Kind = \"rect\"",
        None,
        DVec2::new(5.0, 5.0),
    )
    .unwrap();
    assert_eq!(cmds.len(), 1);
    ed.execute(&Command::Batch { commands: cmds }).unwrap();
    assert!(
        tag_all(ed.doc(), style, "Kind = \"rect\"", None, DVec2::ZERO)
            .unwrap()
            .is_empty()
    );
    let tag = ed
        .doc()
        .objects
        .values()
        .find(|o| matches!(&o.shape, Shape::Annotation(_)))
        .unwrap()
        .clone();
    let Shape::Annotation(an) = &tag.shape else {
        panic!()
    };
    let Annot::DataTag(t) = &**an else { panic!() };
    let t = &t.clone();
    let st = &ed.doc().resources.tag_style(style).unwrap().clone();
    assert_eq!(tag_lines(ed.doc(), t, st), vec!["rect", "1"]);
    // The text follows the object.
    ed.execute(&Command::ModifyObject {
        id: a,
        shape: Some(Shape::Rect {
            min: DVec2::ZERO,
            max: DVec2::new(2000.0, 1000.0),
        }),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    assert_eq!(tag_lines(ed.doc(), t, st)[1], "2");
    assert_eq!(
        tag_lines(
            ed.doc(),
            &DataTag {
                object: cadektor_core::model::ObjectId::from_num(999),
                ..t.clone()
            },
            st
        ),
        vec!["?"]
    );
    assert!(
        prims(ed.doc(), &tag)
            .iter()
            .any(|p| matches!(p, Prim::Text { text, .. } if text == "rect"))
    );
    assert!(tag_all(ed.doc(), ResourceId::from_num(999), "", None, DVec2::ZERO).is_err());
    assert!(tag_all(ed.doc(), style, "((", None, DVec2::ZERO).is_err());
    // Only objects of the layer, and none on a locked layer.
    let none = tag_all(
        ed.doc(),
        style,
        "",
        Some(cadektor_core::model::LayerId::from_num(77)),
        DVec2::ZERO,
    )
    .unwrap();
    assert!(none.is_empty());
}

#[test]
fn legend_groups_and_counts_by_criteria() {
    let mut ed = Editor::default();
    for (x, n) in [(0.0, "Oak"), (1.0, "Oak"), (2.0, "Birch")] {
        plant(&mut ed, x, n);
    }
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
    let mut l = Legend {
        at: DVec2::ZERO,
        title: "Plants".into(),
        criteria: "Kind = \"plant\"".into(),
        group_by: "Species".into(),
        show_count: true,
        swatch_mm: 6.0,
        style: None,
    };
    let rows = legend_rows(ed.doc(), &l);
    assert_eq!(
        rows.iter()
            .map(|r| (r.label.as_str(), r.count))
            .collect::<Vec<_>>(),
        vec![("Birch", 1), ("Oak", 2)]
    );
    // The legend is live.
    plant(&mut ed, 3.0, "Birch");
    assert_eq!(legend_rows(ed.doc(), &l)[0].count, 2);
    l.criteria = String::new();
    l.group_by = "Kind".into();
    assert_eq!(legend_rows(ed.doc(), &l).len(), 2);
    l.group_by = "Nope".into();
    assert!(legend_rows(ed.doc(), &l).is_empty());
    l.group_by = String::new();
    let _ = legend_rows(ed.doc(), &l);
    l.group_by = "((".into();
    assert!(legend_rows(ed.doc(), &l).is_empty());
    l.group_by = "Kind".into();
    l.criteria = "((".into();
    assert!(legend_rows(ed.doc(), &l).is_empty());
    assert!(select(ed.doc(), "((").is_err());
    // Draw through the sheet source.
    l.criteria = "Kind = \"plant\"".into();
    let id = add(&mut ed, Shape::Annotation(Box::new(Annot::Legend(l))));
    let p = prims(ed.doc(), ed.doc().object(id).unwrap());
    assert!(
        p.iter()
            .any(|x| matches!(x, Prim::Text { text, .. } if text == "plant"))
    );
    assert!(prims(ed.doc(), ed.doc().objects.values().next().unwrap()).is_empty());
}

#[test]
fn revisions_group_by_mark() {
    let mut ed = Editor::default();
    let cloud = |rev: &str, desc: &str| {
        Annot::RevisionCloud(RevisionCloud {
            points: vec![DVec2::ZERO, DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0)],
            bump_mm: 6.0,
            revision: rev.into(),
            date: "2026-01-01".into(),
            description: desc.into(),
            author: String::new(),
            mark: true,
        })
    };
    add(&mut ed, Shape::Annotation(Box::new(cloud("B", ""))));
    add(&mut ed, Shape::Annotation(Box::new(cloud("A", "First"))));
    add(&mut ed, Shape::Annotation(Box::new(cloud("b", "Second"))));
    let r = revisions(ed.doc());
    assert_eq!(r.len(), 2);
    assert_eq!((r[0].revision.as_str(), r[0].clouds.len()), ("A", 1));
    assert_eq!(
        (
            r[1].description.as_str(),
            r[1].clouds.len(),
            r[1].date.as_str()
        ),
        ("Second", 2, "2026-01-01")
    );
}

#[test]
fn svg_has_lines_and_text() {
    let mut ed = Editor::default();
    let t = add(
        &mut ed,
        Shape::Annotation(Box::new(Annot::Text(StyledText {
            at: DVec2::new(10.0, 20.0),
            text: "a<b".into(),
            style: None,
        }))),
    );
    let n = add(
        &mut ed,
        Shape::Annotation(Box::new(Annot::NorthArrow(NorthArrow {
            at: DVec2::ZERO,
            size_mm: 15.0,
            rotation_deg: 0.0,
            use_georef: false,
            style: NorthStyle::Arrow,
        }))),
    );
    let mut out = String::new();
    svg::object_svg(
        ed.doc(),
        ed.doc().object(t).unwrap(),
        "stroke=\"red\" stroke-width=\"3\"",
        &mut out,
    );
    assert!(
        out.contains("a&lt;b")
            && out.contains("fill=\"red\"")
            && out.contains("data-type=\"text\""),
        "{out}"
    );
    let mut out = String::new();
    svg::object_svg(ed.doc(), ed.doc().object(n).unwrap(), "", &mut out);
    assert!(
        out.contains("<polygon")
            && out.contains("text-anchor=\"middle\"")
            && out.contains("font-weight=\"bold\"")
    );
    let mut none = String::new();
    let line = add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
    svg::object_svg(ed.doc(), ed.doc().object(line).unwrap(), "", &mut none);
    assert!(none.is_empty());
}

#[test]
fn the_cache_keeps_primitives_per_revision() {
    let mut ed = Editor::default();
    let style = tag_style(&mut ed, &["{Species}"]);
    let p = plant(&mut ed, 0.0, "Oak");
    let tag = add(
        &mut ed,
        Shape::Annotation(Box::new(Annot::DataTag(DataTag {
            style,
            object: p,
            offset: DVec2::new(5.0, 5.0),
            at: DVec2::ZERO,
            anchor: DVec2::ZERO,
        }))),
    );
    let legend = add(
        &mut ed,
        Shape::Annotation(Box::new(Annot::Legend(Legend {
            at: DVec2::ZERO,
            title: String::new(),
            criteria: "Kind = \"plant\"".into(),
            group_by: "Species".into(),
            show_count: true,
            swatch_mm: 6.0,
            style: None,
        }))),
    );
    let other = add(
        &mut ed,
        Shape::Annotation(Box::new(Annot::Legend(Legend {
            at: DVec2::new(5.0, 5.0),
            title: String::new(),
            criteria: "Kind = \"plant\"".into(),
            group_by: "Species".into(),
            show_count: true,
            swatch_mm: 6.0,
            style: None,
        }))),
    );
    let cache = AnnotCache::default();
    let doc = ed.doc();
    let first = cache.prims(doc, doc.object(tag).unwrap(), 7);
    let again = cache.prims(doc, doc.object(tag).unwrap(), 7);
    assert!(
        std::sync::Arc::ptr_eq(&first, &again),
        "same revision: the same primitives"
    );
    assert!(
        cache
            .prims(doc, doc.object(legend).unwrap(), 7)
            .iter()
            .any(|x| matches!(x, Prim::Text { text, .. } if text == "Oak"))
    );
    assert!(
        !cache.prims(doc, doc.object(other).unwrap(), 7).is_empty(),
        "the second legend shares the rows"
    );
    // A new revision starts again; revision 0 keeps nothing.
    let next = cache.prims(doc, doc.object(tag).unwrap(), 8);
    assert!(!std::sync::Arc::ptr_eq(&first, &next));
    let zero = cache.prims(doc, doc.object(tag).unwrap(), 0);
    assert!(!std::sync::Arc::ptr_eq(
        &zero,
        &cache.prims(doc, doc.object(tag).unwrap(), 0)
    ));
    assert!(
        cache.prims(doc, doc.object(p).unwrap(), 8).is_empty(),
        "not an annotation"
    );
    let a = Annot::Text(StyledText {
        at: DVec2::ZERO,
        text: "x".into(),
        style: None,
    });
    assert_eq!(
        prims_in(
            doc,
            &a,
            &cadektor_core::annot::draw::Ctx {
                paper: 1.0,
                real: 1.0,
                view: None
            }
        )
        .len(),
        1
    );
}

#[test]
fn legends_cap_their_rows_and_use_the_style_colors_of_plants() {
    let mut ed = Editor::default();
    let cmds: Vec<Command> = (0..600)
        .map(|i| Command::AddObject {
            shape: Shape::Plant {
                at: DVec2::new(i as f64, 0.0),
                species: format!("S{i:04}"),
                spread: 10.0,
                height: 10.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .collect();
    ed.execute(&Command::Batch { commands: cmds }).unwrap();
    let l = Legend {
        at: DVec2::ZERO,
        title: String::new(),
        criteria: String::new(),
        group_by: "Species".into(),
        show_count: true,
        swatch_mm: 6.0,
        style: None,
    };
    let rows = legend_rows(ed.doc(), &l);
    assert_eq!(rows.len(), MAX_LEGEND_ROWS + 1);
    assert_eq!(rows[MAX_LEGEND_ROWS].label, "... 100 more");
    assert_eq!(rows[MAX_LEGEND_ROWS].count, 100);
    // A plant with a style shows the colors of the style.
    let mut ed = Editor::default();
    let style = cadektor_core::plants::style::PlantStyle {
        fill: Some(cadektor_core::model::Color([10, 200, 10, 255])),
        ..Default::default()
    };
    let sid = ed
        .execute(&Command::AddResource {
            name: "Oak".into(),
            body: ResourceBody::plant_style(style),
        })
        .unwrap()
        .created[0];
    ed.execute(&Command::AddPlants {
        style: Some(ResourceId(sid)),
        overrides: Default::default(),
        positions: vec![DVec2::ZERO],
        layer: None,
        class: None,
        variation: None,
    })
    .unwrap();
    let rows = legend_rows(
        ed.doc(),
        &Legend {
            group_by: "Style".into(),
            ..l.clone()
        },
    );
    assert_eq!(
        rows[0].fill,
        Some(cadektor_core::model::Color([10, 200, 10, 255]))
    );
    // Objects without a value for the group are left out.
    ed.execute(&Command::AddObject {
        shape: Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    assert_eq!(
        legend_rows(
            ed.doc(),
            &Legend {
                group_by: "Style".into(),
                ..l
            }
        )
        .len(),
        1
    );
}
