//! Path borders on one or both sides, and landscape areas with tree plants: plant graphics other than
//! the circle and the protection zones.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::hardscape::{Border, HardscapePatch, HardscapeStyle, Mode, Sides};
use kadakas_core::landscape::{LandscapeAreaStyle, LandscapePatch, MixEntry};
use kadakas_core::model::*;
use kadakas_core::plants::style::{PlantCategory, PlantGraphic, PlantStyle};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::sheets::Viewport;
use kadakas_print::compose_page;
use kadakas_print::display::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn run(ed: &mut Editor, c: Command) -> Vec<kadakas_core::uid::Uid> {
    ed.execute(&c).unwrap().created
}

fn plan_page(ed: &mut Editor) -> Vec<Prim> {
    let sheet = LayerId(
        run(
            ed,
            Command::AddSheet {
                name: "S".into(),
                paper: None,
                orientation: None,
                number: None,
                title: None,
            },
        )[0],
    );
    let vp = Viewport::new(
        v(10.0, 10.0),
        v(400.0, 280.0),
        v(5000.0, 2000.0),
        50.0,
        vec![FIRST_LAYER],
    );
    run(
        ed,
        Command::AddObject {
            shape: Shape::Viewport(Box::new(vp)),
            layer: Some(sheet),
            class: None,
            attrs: Attributes::default(),
        },
    );
    compose_page(ed.doc(), sheet).unwrap().prims
}

fn border_lines(prims: &[Prim]) -> usize {
    prims
        .iter()
        .filter(|p| matches!(p, Prim::Path(p) if !p.closed && p.stroke.is_some() && p.segs.len() >= 2 && p.width > 0.15))
        .count()
}

#[test]
fn path_borders_follow_one_or_both_sides() {
    let mut counts = Vec::new();
    for sides in [Sides::Both, Sides::Left, Sides::Right] {
        let mut ed = Editor::default();
        let style = ResourceId(
            run(
                &mut ed,
                Command::AddResource {
                    name: "Path".into(),
                    body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                        border: Some(Border {
                            width: 300.0,
                            sides,
                            ..Default::default()
                        }),
                        ..Default::default()
                    })),
                },
            )[0],
        );
        let id = ObjectId(
            run(
                &mut ed,
                Command::AddObject {
                    shape: Shape::Hardscape {
                        boundary: corners(&[
                            v(0.0, 0.0),
                            v(6000.0, 0.0),
                            v(6000.0, 500.0),
                            v(0.0, 500.0),
                        ]),
                        material: "m".into(),
                        thickness: None,
                        elevation: None,
                    },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                },
            )[0],
        );
        run(
            &mut ed,
            Command::SetHardscape {
                ids: vec![id],
                patch: HardscapePatch {
                    style: Some(style),
                    mode: Some(Mode::Path),
                    center: Some(corners(&[v(0.0, 0.0), v(6000.0, 0.0), v(6000.0, 3000.0)])),
                    width: Some(1200.0),
                    ..Default::default()
                },
            },
        );
        counts.push(border_lines(&plan_page(&mut ed)));
    }
    assert!(counts[0] >= 2, "{counts:?}");
    assert!(counts[1] >= 1 && counts[2] >= 1, "{counts:?}");
    assert!(counts[0] > counts[1] && counts[0] > counts[2], "{counts:?}");
}

#[test]
fn trees_in_a_landscape_area_show_their_graphic_and_protection_zone() {
    let mut counts = Vec::new();
    for graphic in [PlantGraphic::Star, PlantGraphic::Circle] {
        let mut ed = Editor::default();
        let plant = ResourceId(
            run(
                &mut ed,
                Command::AddResource {
                    name: "T".into(),
                    body: ResourceBody::plant_style(PlantStyle {
                        spread: 1500.0,
                        graphic,
                        category: PlantCategory::Tree,
                        protection_zone: true,
                        trunk_diameter: 300.0,
                        ..Default::default()
                    }),
                },
            )[0],
        );
        let ls = ResourceId(
            run(
                &mut ed,
                Command::AddResource {
                    name: "Grove".into(),
                    body: ResourceBody::LandscapeAreaStyle(Box::new(LandscapeAreaStyle {
                        mix: vec![MixEntry {
                            plant,
                            density: Some(0.2),
                            ..Default::default()
                        }],
                        show_plants: true,
                        ..Default::default()
                    })),
                },
            )[0],
        );
        let area = ObjectId(
            run(
                &mut ed,
                Command::AddObject {
                    shape: Shape::LandscapeArea {
                        boundary: corners(&[
                            v(0.0, 0.0),
                            v(8000.0, 0.0),
                            v(8000.0, 6000.0),
                            v(0.0, 6000.0),
                        ]),
                        name: "Grove".into(),
                    },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                },
            )[0],
        );
        run(
            &mut ed,
            Command::SetLandscapeArea {
                ids: vec![area],
                patch: LandscapePatch {
                    style: Some(ls),
                    ..Default::default()
                },
            },
        );
        let p = plan_page(&mut ed);
        let dashed = p
            .iter()
            .filter(|x| matches!(x, Prim::Path(p) if p.dash == Some([2.0, 1.0]) && p.closed))
            .count();
        assert!(dashed >= 1, "the protection zones are dashed circles");
        counts.push(p.len());
    }
    assert_eq!(counts.len(), 2);
}
