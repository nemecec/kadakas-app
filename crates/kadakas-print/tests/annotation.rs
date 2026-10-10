//! Annotation on sheets: paper sizes stay paper sizes when the viewport scale differs from the layer scale.

use glam::DVec2;
use kadakas_core::annot::style::{AnnotResource, TagStyle};
use kadakas_core::annot::{Annot, DataTag};
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::*;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::sheets::Viewport;
use kadakas_print::compose_page;
use kadakas_print::display::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

#[test]
fn tags_keep_their_paper_offset_and_size_in_a_viewport_with_another_scale() {
    let mut ed = Editor::default();
    // The layer is 1:100; the viewport below shows it at 1:500.
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "T".into(),
            body: ResourceBody::annot(AnnotResource::TagStyle(TagStyle {
                lines: vec!["{Species}".into()],
                leader: false,
                ..Default::default()
            })),
        })
        .unwrap()
        .created[0],
    );
    let mut ids = Vec::new();
    for (x, name) in [(0.0, "Alpha"), (3000.0, "Beta")] {
        let p = ed
            .execute(&Command::AddObject {
                shape: Shape::Plant {
                    at: v(x, 0.0),
                    species: name.into(),
                    spread: 1000.0,
                    height: 1000.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0];
        ids.push(ObjectId(p));
    }
    for id in &ids {
        let tag = Annot::DataTag(DataTag {
            style,
            object: *id,
            offset: v(5.0, 5.0),
            at: DVec2::ZERO,
            anchor: DVec2::ZERO,
        });
        ed.execute(&Command::AddObject {
            shape: Shape::Annotation(Box::new(tag)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "S".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap()
        .created[0],
    );
    let vp = Viewport::new(
        v(10.0, 10.0),
        v(280.0, 200.0),
        v(1500.0, 0.0),
        500.0,
        vec![LayerId::from_num(1)],
    );
    ed.execute(&Command::AddObject {
        shape: Shape::Viewport(Box::new(vp.clone())),
        layer: Some(sheet),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let page = compose_page(ed.doc(), sheet).unwrap();
    let texts: Vec<&TextPrim> = page
        .prims
        .iter()
        .filter_map(|p| if let Prim::Text(t) = p { Some(t) } else { None })
        .collect();
    for (name, x) in [("Alpha", 0.0), ("Beta", 3000.0)] {
        let t = texts
            .iter()
            .find(|t| t.text == name)
            .unwrap_or_else(|| panic!("{name} missing"));
        assert!(
            (t.height - 2.5).abs() < 1e-9,
            "text is 2.5 paper mm, not {}",
            t.height
        );
        let plant_on_paper = vp.to_paper(v(x, 0.0));
        // The label is centered 5 paper mm right and up from the plant (the baseline sits a bit lower).
        assert!(
            (t.at.x - (plant_on_paper.x + 5.0)).abs() < 1e-6,
            "{name}: {:?} vs {:?}",
            t.at,
            plant_on_paper
        );
        assert!((t.at.y - plant_on_paper.y - 5.0).abs() < 3.0);
    }
    // The two labels, 6 paper mm apart at 1:500, do not overlap each other more than they would anywhere:
    // their centers keep the distance of the plants on paper.
    let a = texts.iter().find(|t| t.text == "Alpha").unwrap();
    let b = texts.iter().find(|t| t.text == "Beta").unwrap();
    assert!((b.at.x - a.at.x - 6.0).abs() < 1e-6);
}
