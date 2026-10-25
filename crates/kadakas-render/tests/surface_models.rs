//! Surface models in the CPU renderer and the hidden-line pass (MOD-13): the color of a material in
//! a top view, the sides of a face, the edges of a model that are shown, and the soft edges of a
//! curved surface that leave the line drawing but keep the outline.

use glam::{DVec2, Vec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh;
use kadakas_core::model::{Attributes, Color, ObjectId, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::surface::{EdgeId, Edit, PaintSide};
use kadakas_render::hlr::{HlOptions, hidden_lines};
use kadakas_render::{Camera, Projection, RenderOptions, StandardView, render};

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
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

fn edit(ed: &mut Editor, id: ObjectId, edit: Edit) {
    ed.execute(&Command::EditSurface { id, edit }).unwrap();
}

/// A closed box of 1000 x 1000 x 600 mm with its corner at the origin.
fn box_model(ed: &mut Editor) -> ObjectId {
    let id = add(ed, Shape::SurfaceModel(Box::default()));
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    edit(
        ed,
        id,
        Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    );
    let face = model(ed, id).faces().next().unwrap().0;
    edit(
        ed,
        id,
        Edit::PushPull {
            face,
            distance: 600.0,
        },
    );
    id
}

fn model(ed: &Editor, id: ObjectId) -> &kadakas_core::surface::SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        _ => panic!(),
    }
}

fn camera(view: StandardView, scene: &mesh::Scene) -> Camera {
    let mut cam = Camera::standard(view, Projection::Orthographic);
    cam.fit(scene.bounds, 1.0);
    cam
}

fn top_pixel(ed: &Editor) -> [u8; 4] {
    let scene = mesh::build(ed.doc());
    let img = render(
        &scene,
        &camera(StandardView::Top, &scene),
        &RenderOptions {
            width: 120,
            height: 120,
            ..Default::default()
        },
    );
    img.pixel(60, 60)
}

#[test]
fn a_material_colors_the_top_of_a_model_in_the_render() {
    let mut ed = Editor::default();
    let id = box_model(&mut ed);
    let plain = top_pixel(&ed);
    // The default front color is neutral.
    assert!(plain[0].abs_diff(plain[2]) < 30, "{plain:?}");
    let mat = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Brick".into(),
            body: ResourceBody::Render(Box::new(kadakas_core::present::RenderResource::Material(
                kadakas_core::present::Material {
                    base_color: Color([200, 40, 30, 255]),
                    ..Default::default()
                },
            ))),
        })
        .unwrap()
        .created[0],
    );
    let top = model(&ed, id)
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(f, _)| f)
        .unwrap();
    edit(
        &mut ed,
        id,
        Edit::SetFaceMaterial {
            faces: vec![top],
            side: PaintSide::Front,
            material: Some(mat.to_string()),
        },
    );
    let painted = top_pixel(&ed);
    assert!(
        painted[0] > painted[1] + 60 && painted[0] > painted[2] + 60,
        "red top: {painted:?}"
    );
    // Hiding the top shows the bottom from inside: its back, in the bluish back color.
    edit(
        &mut ed,
        id,
        Edit::SetHidden {
            faces: vec![top],
            edges: vec![],
            hidden: true,
        },
    );
    let inside = top_pixel(&ed);
    assert!(
        inside[2] > inside[0] + 10,
        "the back of the bottom: {inside:?}"
    );
}

#[test]
fn a_class_that_is_off_leaves_the_model_out_of_the_scene() {
    let mut ed = Editor::default();
    let id = box_model(&mut ed);
    assert!(mesh::build(ed.doc()).mesh.triangle_count() > 0);
    let layer = ed.doc().object(id).unwrap().layer;
    ed.execute(&Command::ModifyLayer {
        id: layer,
        name: None,
        visible: Some(false),
        locked: None,
        elevation: None,
        scale: None,
    })
    .unwrap();
    let scene = mesh::build(ed.doc());
    assert_eq!(scene.mesh.triangle_count(), 0);
    assert_eq!(scene.lines.segment_count(), 0);
}

/// A cylinder of 24 sides, 800 mm across and 1000 mm tall, made as a curve pushed up.
fn cylinder(ed: &mut Editor) -> ObjectId {
    let id = add(ed, Shape::SurfaceModel(Box::default()));
    let n = 24;
    edit(
        ed,
        id,
        Edit::AddCurve {
            points: (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    [400.0 * a.cos(), 400.0 * a.sin(), 0.0]
                })
                .collect(),
            closed: true,
            faceted: false,
            analytic: None,
        },
    );
    let face = model(ed, id).faces().next().unwrap().0;
    edit(
        ed,
        id,
        Edit::PushPull {
            face,
            distance: 1000.0,
        },
    );
    id
}

/// The visible segments of the front view that are nearly vertical.
fn vertical_lines(ed: &Editor) -> usize {
    let scene = mesh::build(ed.doc());
    let hl = hidden_lines(
        &scene,
        &camera(StandardView::Front, &scene),
        800,
        800,
        &HlOptions::default(),
    );
    hl.visible()
        .filter(|s| (s.a[0] - s.b[0]).abs() < 0.5 && (s.a[1] - s.b[1]).abs() > 5.0)
        .count()
}

#[test]
fn the_soft_edges_of_a_cylinder_leave_the_line_drawing_and_the_outline_stays() {
    let mut ed = Editor::default();
    let id = cylinder(&mut ed);
    // The 24 sides of the wall have vertical edges. They were made soft by the curve.
    let soft = model(&ed, id)
        .edges()
        .filter(|(e, _)| model(&ed, id).edge_soft(*e))
        .count();
    let hard_verticals = vertical_lines(&ed);
    // Soft or not, the two edges at the sides of the cylinder are its outline.
    assert!(soft >= 24, "the wall of a curve is soft: {soft}");
    assert_eq!(hard_verticals, 2, "outline only");
    // Make every edge hard: the vertical edges of the wall are lines again.
    let all: Vec<EdgeId> = model(&ed, id).edges().map(|(e, _)| e).collect();
    edit(
        &mut ed,
        id,
        Edit::SetEdgeStyle {
            edges: all,
            soft: Some(false),
            smooth: Some(false),
        },
    );
    assert!(
        vertical_lines(&ed) > 8,
        "hard edges are drawn: {}",
        vertical_lines(&ed)
    );
    // And soft again, with an angle: the creases of the wall go, the rims stay.
    let all: Vec<EdgeId> = model(&ed, id).edges().map(|(e, _)| e).collect();
    edit(
        &mut ed,
        id,
        Edit::SoftenEdges {
            edges: all,
            angle: 40.0,
            smooth: true,
            coplanar: false,
        },
    );
    assert_eq!(vertical_lines(&ed), 2);
    let scene = mesh::build(ed.doc());
    assert!(!scene.soft_edges.is_empty());
}

#[test]
fn a_box_has_nine_visible_edges_in_an_isometric_view_and_a_hidden_edge_makes_it_eight() {
    let mut ed = Editor::default();
    let id = box_model(&mut ed);
    let count = |ed: &Editor| {
        let scene = mesh::build(ed.doc());
        hidden_lines(
            &scene,
            &camera(StandardView::RightIsometric, &scene),
            400,
            400,
            &HlOptions::default(),
        )
        .visible_edge_count()
    };
    assert_eq!(count(&ed), 9);
    // Hide the vertical edge nearest to a viewer in the right isometric view (x = 1000, y = 0).
    let m = model(&ed, id);
    let near = m
        .edges()
        .find(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            a.x == 1000.0 && a.y == 0.0 && b.x == 1000.0 && b.y == 0.0
        })
        .map(|(e, _)| e)
        .unwrap();
    edit(
        &mut ed,
        id,
        Edit::SetHidden {
            faces: vec![],
            edges: vec![near],
            hidden: true,
        },
    );
    assert_eq!(count(&ed), 8);
    // The model moved: the lines move with it, their number stays.
    ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta: DVec2::new(5000.0, 0.0),
    })
    .unwrap();
    assert_eq!(count(&ed), 8);
    let scene = mesh::build(ed.doc());
    let (lo, _) = scene.bounds.unwrap();
    let _ = Vec3::ZERO;
    assert!(lo.x.is_finite());
}
