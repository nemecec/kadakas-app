//! The twist and the perspective view of a viewport (IOP-27): a twisted plan view turns the model
//! about the center of the view, and a 3D view can have a perspective camera.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::*;
use kadakas_core::sheets::Viewport;
use kadakas_core::underlay::{PngData, Rgba, Underlay};
use kadakas_print::compose_page;
use kadakas_print::display::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn run(ed: &mut Editor, c: Command) -> Vec<kadakas_core::uid::Uid> {
    ed.execute(&c).unwrap().created
}

fn add(ed: &mut Editor, shape: Shape) {
    run(
        ed,
        Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        },
    );
}

/// A sheet with one viewport (frame 10..410 x 10..290, center 5000, 5000 at 1:100).
fn page_with(ed: &mut Editor, tweak: impl FnOnce(&mut Viewport)) -> Page {
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
    let mut vp = Viewport::new(
        v(10.0, 10.0),
        v(410.0, 290.0),
        v(5000.0, 5000.0),
        100.0,
        vec![FIRST_LAYER],
    );
    tweak(&mut vp);
    run(
        ed,
        Command::AddObject {
            shape: Shape::Viewport(Box::new(vp)),
            layer: Some(sheet),
            class: None,
            attrs: Attributes::default(),
        },
    );
    compose_page(ed.doc(), sheet).unwrap()
}

/// The points of the paths that the model drew (not the frame): the first path with the pen color.
fn points_of(page: &Page) -> Vec<Vec<DVec2>> {
    page.prims
        .iter()
        .filter_map(|p| match p {
            Prim::Path(pp) if pp.stroke == Some([200, 0, 0, 255]) => Some(
                pp.segs
                    .iter()
                    .map(|s| match s {
                        PSeg::Move(a) | PSeg::Line(a) => *a,
                        PSeg::Cubic(_, _, e) => *e,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

fn red() -> Attributes {
    Attributes {
        pen: Some(Color([200, 0, 0, 255])),
        ..Attributes::default()
    }
}

fn add_red(ed: &mut Editor, shape: Shape) {
    run(
        ed,
        Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: red(),
        },
    );
}

#[test]
fn a_twisted_plan_view_turns_the_drawing_about_the_center() {
    let mut ed = Editor::default();
    // A horizontal line, 4 m long, through the center of the view.
    add_red(
        &mut ed,
        Shape::Line {
            a: v(3000.0, 5000.0),
            b: v(7000.0, 5000.0),
        },
    );
    let flat = points_of(&page_with(&mut Editor::new(ed.doc().clone()), |_| {}));
    let turned = points_of(&page_with(&mut Editor::new(ed.doc().clone()), |vp| {
        vp.twist = 90.0
    }));
    let (c, k) = (v(210.0, 150.0), 0.01);
    // Flat: from 20 mm left of the center to 20 mm right.
    assert!(
        (flat[0][0] - (c - v(20.0, 0.0))).length() < 1e-9
            && (flat[0][1] - (c + v(20.0, 0.0))).length() < 1e-9
    );
    // Twisted by 90 degrees counterclockwise: it runs from below the center to above it.
    assert!(
        (turned[0][0] - (c - v(0.0, 4000.0 * k / 2.0))).length() < 1e-9,
        "{:?}",
        turned[0]
    );
    assert!((turned[0][1] - (c + v(0.0, 4000.0 * k / 2.0))).length() < 1e-9);
}

#[test]
fn arcs_circles_and_rectangles_follow_the_twist() {
    let mut ed = Editor::default();
    // A quarter arc from east to north about (5000, 5000), radius 1 m.
    add_red(
        &mut ed,
        Shape::Arc {
            center: v(5000.0, 5000.0),
            radius: 1000.0,
            start: 0.0,
            end: std::f64::consts::FRAC_PI_2,
        },
    );
    add_red(
        &mut ed,
        Shape::Rect {
            min: v(5000.0, 5000.0),
            max: v(6000.0, 5500.0),
        },
    );
    let page = page_with(&mut ed, |vp| vp.twist = 90.0);
    let paths = points_of(&page);
    let c = v(210.0, 150.0);
    // The arc starts at east: after the twist it starts at north of the center (10 mm up).
    assert!(
        (paths[0][0] - (c + v(0.0, 10.0))).length() < 1e-9,
        "{:?}",
        paths[0]
    );
    // And ends at west.
    assert!(
        (*paths[0].last().unwrap() - (c - v(10.0, 0.0))).length() < 1e-9,
        "{:?}",
        paths[0]
    );
    // The rectangle's corner (6000, 5500) goes to (-5, 10) from the center.
    assert!(
        paths[1]
            .iter()
            .any(|p| (*p - (c + v(-5.0, 10.0))).length() < 1e-9),
        "{:?}",
        paths[1]
    );
}

#[test]
fn an_underlay_is_not_drawn_in_a_twisted_view_and_the_page_says_so() {
    let mut ed = Editor::default();
    let img = Rgba {
        width: 2,
        height: 1,
        data: vec![255, 0, 0, 255, 0, 0, 255, 255],
    };
    add(
        &mut ed,
        Shape::Underlay(Box::new(Underlay {
            name: String::new(),
            min: v(4000.0, 4000.0),
            max: v(6000.0, 5000.0),
            width: 2,
            height: 1,
            png: PngData::new(img.to_png()),
            opacity: 1.0,
            source: String::new(),
        })),
    );
    let flat = page_with(&mut Editor::new(ed.doc().clone()), |_| {});
    assert!(flat.prims.iter().any(|p| matches!(p, Prim::Image(_))));
    assert!(flat.notes.is_empty());
    let turned = page_with(&mut Editor::new(ed.doc().clone()), |vp| vp.twist = 30.0);
    assert!(!turned.prims.iter().any(|p| matches!(p, Prim::Image(_))));
    assert!(
        turned
            .notes
            .iter()
            .any(|n| n.contains("not drawn in a viewport with a twist")),
        "{:?}",
        turned.notes
    );
}

#[test]
fn a_3d_view_and_a_section_are_not_turned() {
    let mut ed = Editor::default();
    add_red(
        &mut ed,
        Shape::Line {
            a: v(3000.0, 5000.0),
            b: v(7000.0, 5000.0),
        },
    );
    let a = page_with(&mut Editor::new(ed.doc().clone()), |vp| {
        vp.view = "front".into()
    });
    let b = page_with(&mut Editor::new(ed.doc().clone()), |vp| {
        vp.view = "front".into();
        vp.twist = 45.0;
    });
    assert_eq!(a.prims.len(), b.prims.len());
    let img = |p: &Page| {
        p.prims.iter().find_map(|x| match x {
            Prim::Image(i) => Some(i.rgb.clone()),
            _ => None,
        })
    };
    assert_eq!(img(&a), img(&b));
}

#[test]
fn a_perspective_view_draws_another_picture_than_the_parallel_view() {
    let mut ed = Editor::default();
    add(
        &mut ed,
        Shape::Massing(Box::new(kadakas_core::massing::Massing::new(
            kadakas_core::curve::corners(&[
                v(4000.0, 4000.0),
                v(6000.0, 4000.0),
                v(6000.0, 6000.0),
                v(4000.0, 6000.0),
            ]),
            6000.0,
        ))),
    );
    let image = |p: &Page| {
        p.prims
            .iter()
            .find_map(|x| match x {
                Prim::Image(i) => Some(i.rgb.clone()),
                _ => None,
            })
            .expect("a 3D view is an image")
    };
    let tweak = |persp: bool| {
        move |vp: &mut Viewport| {
            vp.view = "right_isometric".into();
            vp.scale = 50.0;
            vp.dpi = 40.0;
            vp.perspective = persp;
        }
    };
    let parallel = page_with(&mut Editor::new(ed.doc().clone()), tweak(false));
    let persp = page_with(&mut Editor::new(ed.doc().clone()), tweak(true));
    assert_ne!(image(&parallel), image(&persp));
    // The same flag in the parallel viewport is the default: the picture is the same as without it.
    let again = page_with(&mut Editor::new(ed.doc().clone()), tweak(false));
    assert_eq!(image(&parallel), image(&again));
    // The hidden-line mode draws vector lines through the same camera.
    let hidden = |persp: bool| {
        page_with(&mut Editor::new(ed.doc().clone()), move |vp| {
            vp.view = "right_isometric".into();
            vp.scale = 50.0;
            vp.mode = "hidden_line".into();
            vp.perspective = persp;
        })
    };
    let (h1, h2) = (hidden(false), hidden(true));
    assert_ne!(h1.prims, h2.prims);
}
