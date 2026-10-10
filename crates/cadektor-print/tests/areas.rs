//! Hardscapes and landscape areas in plan viewports: styles, joint patterns, borders, holes and
//! plants are drawn at the scale of the viewport, and the by-class overrides of the viewport win.

use cadektor_core::command::{Command, Editor};
use cadektor_core::curve::corners;
use cadektor_core::hardscape::{Border, HardscapePatch, HardscapeStyle, JointKind, JointPattern};
use cadektor_core::landscape::{LandscapeAreaStyle, LandscapePatch, MixEntry};
use cadektor_core::model::*;
use cadektor_core::plants::style::PlantStyle;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::sheets::{ClassOverride, PaperSize, Viewport};
use cadektor_print::compose_page;
use cadektor_print::display::*;
use glam::DVec2;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

const GREEN: Color = Color([20, 160, 40, 255]);
const BLUE: Color = Color([0, 0, 220, 255]);

fn run(ed: &mut Editor, c: Command) -> Vec<u64> {
    ed.execute(&c).unwrap().created
}

fn project() -> (Editor, LayerId, ObjectId, ObjectId, ObjectId) {
    let mut ed = Editor::default();
    let style = HardscapeStyle {
        fill: Some(GREEN),
        joints: Some(JointPattern {
            kind: JointKind::Stack,
            unit_length: 500.0,
            unit_width: 500.0,
            spacing: 5.0,
            ..Default::default()
        }),
        border: Some(Border {
            width: 200.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let hs = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Setts".into(),
                body: ResourceBody::HardscapeStyle(Box::new(style)),
            },
        )[0],
    );
    let hard = ObjectId(
        run(
            &mut ed,
            Command::AddObject {
                shape: Shape::Hardscape {
                    boundary: corners(&[
                        v(0.0, 0.0),
                        v(5000.0, 0.0),
                        v(5000.0, 3000.0),
                        v(0.0, 3000.0),
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
            ids: vec![hard],
            patch: HardscapePatch {
                style: Some(hs),
                ..Default::default()
            },
        },
    );
    let plant = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Lavender".into(),
                body: ResourceBody::plant_style(PlantStyle {
                    spread: 500.0,
                    ..Default::default()
                }),
            },
        )[0],
    );
    let mix = LandscapeAreaStyle {
        mix: vec![MixEntry {
            plant,
            density: Some(2.0),
            ..Default::default()
        }],
        show_plants: true,
        fill: Some(GREEN),
        ..Default::default()
    };
    let ls = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Meadow".into(),
                body: ResourceBody::LandscapeAreaStyle(Box::new(mix)),
            },
        )[0],
    );
    let area = ObjectId(
        run(
            &mut ed,
            Command::AddObject {
                shape: Shape::LandscapeArea {
                    boundary: corners(&[
                        v(6000.0, 0.0),
                        v(10_000.0, 0.0),
                        v(10_000.0, 4000.0),
                        v(6000.0, 4000.0),
                    ]),
                    name: "Bed".into(),
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    );
    let hole = vec![corners(&[
        v(7000.0, 1000.0),
        v(8000.0, 1000.0),
        v(8000.0, 2000.0),
        v(7000.0, 2000.0),
    ])];
    run(
        &mut ed,
        Command::SetLandscapeArea {
            ids: vec![area],
            patch: LandscapePatch {
                style: Some(ls),
                holes: Some(hole),
                ..Default::default()
            },
        },
    );
    let sheet = LayerId(
        run(
            &mut ed,
            Command::AddSheet {
                name: "Plan".into(),
                paper: Some(PaperSize::A3),
                orientation: None,
                number: None,
                title: None,
            },
        )[0],
    );
    let mut vp = Viewport::new(
        v(20.0, 60.0),
        v(400.0, 280.0),
        v(5000.0, 2000.0),
        50.0,
        vec![FIRST_LAYER],
    );
    vp.fit(ed.doc());
    let vp_id = ObjectId(
        run(
            &mut ed,
            Command::AddObject {
                shape: Shape::Viewport(Box::new(vp)),
                layer: Some(sheet),
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    );
    (ed, sheet, vp_id, hard, area)
}

fn paths(ed: &Editor, sheet: LayerId) -> Vec<PathPrim> {
    compose_page(ed.doc(), sheet)
        .unwrap()
        .prims
        .into_iter()
        .filter_map(|p| if let Prim::Path(p) = p { Some(p) } else { None })
        .collect()
}

#[test]
fn styled_areas_draw_their_extras_in_a_viewport() {
    let (ed, sheet, ..) = project();
    let all = paths(&ed, sheet);
    // The fill of the styles, joint lines (thin, many), the border, the hole and the plants.
    assert!(
        all.iter().any(|p| p.fill == Some(GREEN.0)),
        "the style fill of the hardscape"
    );
    let thin = all
        .iter()
        .filter(|p| {
            !p.closed && (p.width - 0.1).abs() < 1e-9 && p.stroke.is_some() && p.segs.len() == 2
        })
        .count();
    assert!(
        thin > 8,
        "joint lines of 0.5 m units at 1:50 are 10 mm on paper: {thin}"
    );
    assert!(
        all.iter().any(|p| p.dash == Some([2.0, 1.0])
            && p.closed
            && p.fill == Some([255, 255, 255, 230])),
        "the hole"
    );
    let circles = all
        .iter()
        .filter(|p| {
            p.closed && p.segs.len() >= 32 && p.fill.is_some_and(|f| f[3] > 0 && f[3] < 255)
        })
        .count();
    assert!(circles >= 10, "plants: {circles}");
    // The same page twice is the same: the scatter is deterministic.
    assert_eq!(
        compose_page(ed.doc(), sheet).unwrap().prims,
        compose_page(ed.doc(), sheet).unwrap().prims
    );
}

#[test]
fn class_overrides_of_the_viewport_win_over_the_style() {
    let (mut ed, sheet, vp_id, ..) = project();
    let Shape::Viewport(vp) = &ed.doc().object(vp_id).unwrap().shape else {
        panic!()
    };
    let mut vp = vp.clone();
    vp.class_overrides.insert(
        NONE_CLASS,
        ClassOverride {
            fill: Some(BLUE),
            ..Default::default()
        },
    );
    run(
        &mut ed,
        Command::ModifyObject {
            id: vp_id,
            shape: Some(Shape::Viewport(vp)),
            layer: None,
            class: None,
            attrs: None,
        },
    );
    let all = paths(&ed, sheet);
    assert!(
        all.iter().any(|p| p.closed && p.fill == Some(BLUE.0)),
        "the override fills the areas"
    );
    assert!(
        !all.iter().any(|p| p.closed && p.fill == Some(GREEN.0)),
        "not the style"
    );
}

#[test]
fn tiny_joint_units_and_hidden_plants_are_left_out() {
    let (mut ed, sheet, vp_id, _, area) = project();
    // Plants off: no circles.
    run(
        &mut ed,
        Command::SetLandscapeArea {
            ids: vec![area],
            patch: LandscapePatch {
                show_plants: Some(false),
                ..Default::default()
            },
        },
    );
    let before = paths(&ed, sheet).len();
    // A very small scale makes the units too small to draw.
    let Shape::Viewport(vp) = &ed.doc().object(vp_id).unwrap().shape else {
        panic!()
    };
    let mut vp = vp.clone();
    vp.scale = 2000.0;
    run(
        &mut ed,
        Command::ModifyObject {
            id: vp_id,
            shape: Some(Shape::Viewport(vp)),
            layer: None,
            class: None,
            attrs: None,
        },
    );
    let after = paths(&ed, sheet);
    assert!(after.len() < before, "{} < {before}", after.len());
    assert!(
        !after
            .iter()
            .any(|p| p.width == 0.1 && !p.closed && p.segs.len() == 2)
    );
}
