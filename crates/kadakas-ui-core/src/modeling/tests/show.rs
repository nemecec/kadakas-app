//! What is shown (MOD-05): the guides and the hidden geometry in the scenes, the pick of a guide,
//! the window selection of guides and the rules for hidden faces and edges.

use glam::{DVec2, DVec3};
use kadakas_core::command::Command;
use kadakas_core::guide::Guide;
use kadakas_core::model::{Attributes, ClassId, ObjectId, Shape};
use kadakas_core::surface::{Edit, FaceId};
use kadakas_core::visibility::set_hidden_command;

use super::*;
use crate::infer3d::{Camera, Inferencer, Projection};
use crate::modeling::select;

/// A view from above: one pixel is 10 mm and the point (0, 0) is in the middle of 800 x 600.
fn top() -> Camera {
    Camera::look_at(
        DVec3::new(0.0, 0.0, 10_000.0),
        DVec3::ZERO,
        DVec3::Y,
        Projection::Orthographic { height: 6000.0 },
        DVec2::new(800.0, 600.0),
    )
    .unwrap()
}

fn add_guide(r: &mut Rig, g: Guide, class: Option<ClassId>) -> ObjectId {
    ObjectId(
        r.ed.execute(&Command::AddObject {
            shape: Shape::Guide(Box::new(g)),
            layer: None,
            class,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

/// A square model of 1000 x 1000 with a face.
fn square(r: &mut Rig) -> (ObjectId, FaceId) {
    let id = ObjectId(
        r.ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let p = [
        [-500.0, -500.0, 0.0],
        [500.0, -500.0, 0.0],
        [500.0, 500.0, 0.0],
        [-500.0, 500.0, 0.0],
    ];
    r.ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let f = model_of(r.ed.doc(), id).unwrap().faces().next().unwrap().0;
    (id, f)
}

fn hide_face(r: &mut Rig, id: ObjectId, face: FaceId, hidden: bool) {
    r.ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![face],
            edges: vec![],
            hidden,
        },
    })
    .unwrap();
}

fn px(c: &Camera, p: DVec3) -> DVec2 {
    c.project(p).unwrap()
}

#[test]
fn the_scenes_hold_the_shown_guides_and_follow_the_options() {
    let mut r = rig(ModelerTool::Select);
    let a = add_guide(&mut r, Guide::point(DVec3::new(100.0, 0.0, 0.0)), None);
    let b = add_guide(&mut r, Guide::line(DVec3::ZERO, DVec3::X), None);
    let rev = r.ed.revision();
    let s = r.cache.get(r.ed.doc(), rev);
    assert_eq!(s.guide_ids(), &[a, b]);
    assert_eq!(s.guide(Tag::guide(1)), Some(b));
    assert_eq!(s.guide(Tag::guide(5)), None);
    assert_eq!(s.guide(Tag::NONE), None);
    assert_eq!(s.object(Tag::guide(0)), None, "a guide is no model");
    assert_eq!(s.refs().len(), 1, "the guides are one scene");
    // View > Guides off: no guide in the scenes.
    r.cache.show.guides = false;
    let s = r.cache.get(r.ed.doc(), rev);
    assert!(s.guide_ids().is_empty());
    assert!(s.refs().is_empty());
    assert!(s.guide_scene().is_none());
    r.cache.show.guides = true;
    assert_eq!(r.cache.get(r.ed.doc(), rev).guide_ids().len(), 2);
}

#[test]
fn a_guide_on_a_hidden_tag_or_hidden_with_hide_is_not_in_the_scenes() {
    let mut r = rig(ModelerTool::Select);
    let tag = ClassId(
        r.ed.execute(&Command::AddClass {
            name: "Setout".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let on_tag = add_guide(&mut r, Guide::point(DVec3::X), Some(tag));
    let plain = add_guide(&mut r, Guide::point(DVec3::Y), None);
    let rev = r.ed.revision();
    assert_eq!(r.cache.get(r.ed.doc(), rev).guide_ids().len(), 2);
    r.ed.execute(&Command::ModifyClass {
        id: tag,
        name: None,
        visible: Some(false),
        attrs: None,
    })
    .unwrap();
    let rev = r.ed.revision();
    assert_eq!(r.cache.get(r.ed.doc(), rev).guide_ids(), &[plain]);
    // Hide hides the other one too; Hidden Objects shows it again.
    let cmd = set_hidden_command(r.ed.doc(), &[plain], true).unwrap();
    r.ed.execute(&cmd).unwrap();
    let rev = r.ed.revision();
    assert!(r.cache.get(r.ed.doc(), rev).guide_ids().is_empty());
    r.cache.show.hidden = true;
    let ids = r.cache.get(r.ed.doc(), rev).guide_ids().to_vec();
    assert_eq!(ids, vec![plain], "the hidden tag still hides its guide");
    let _ = on_tag;
}

#[test]
fn a_hidden_face_is_not_in_the_scene_nor_picked_until_hidden_geometry_shows_it() {
    let mut r = rig(ModelerTool::Select);
    let (id, face) = square(&mut r);
    let cam = top();
    let mut engine = Inferencer::default();
    let pick = |r: &mut Rig, engine: &mut Inferencer| {
        let rev = r.ed.revision();
        let scenes = r.cache.get(r.ed.doc(), rev);
        let w = World {
            doc: r.ed.doc(),
            scenes,
        };
        select::pick(&w, engine, &cam, px(&cam, DVec3::new(100.0, 100.0, 0.0)))
    };
    assert_eq!(pick(&mut r, &mut engine), Some(Item::Face(id, face)));
    hide_face(&mut r, id, face, true);
    assert_eq!(pick(&mut r, &mut engine), None, "a hidden face is not picked");
    let rev = r.ed.revision();
    assert_eq!(r.cache.get(r.ed.doc(), rev).face_count(), 0);
    r.cache.show.hidden = true;
    assert_eq!(pick(&mut r, &mut engine), Some(Item::Face(id, face)));
    assert_eq!(r.cache.get(r.ed.doc(), rev).face_count(), 1);
    r.cache.show.hidden = false;
    assert_eq!(pick(&mut r, &mut engine), None);
}

#[test]
fn the_window_leaves_out_hidden_faces_and_edges_unless_they_are_shown() {
    let mut r = rig(ModelerTool::Select);
    let (id, face) = square(&mut r);
    let edge = model_of(r.ed.doc(), id).unwrap().edges().next().unwrap().0;
    r.ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![face],
            edges: vec![edge],
            hidden: true,
        },
    })
    .unwrap();
    let cam = top();
    let all = |r: &mut Rig| {
        let rev = r.ed.revision();
        let scenes = r.cache.get(r.ed.doc(), rev);
        let w = World {
            doc: r.ed.doc(),
            scenes,
        };
        select::rect_select(&w, &cam, DVec2::ZERO, DVec2::new(800.0, 600.0), false)
    };
    let items = all(&mut r);
    assert!(!items.contains(&Item::Face(id, face)));
    assert!(!items.contains(&Item::Edge(id, edge)));
    assert_eq!(items.len(), 3, "the other three edges");
    r.cache.show.hidden = true;
    let items = all(&mut r);
    assert!(items.contains(&Item::Face(id, face)));
    assert!(items.contains(&Item::Edge(id, edge)));
    assert_eq!(items.len(), 5);
    // The neighbors of a click do not bring hidden parts in.
    let doc = r.ed.doc();
    let with = select::without_hidden(doc, select::with_neighbors(doc, Item::Face(id, face)));
    assert_eq!(with.len(), 3);
    assert!(!with.contains(&Item::Face(id, face)));
}

#[test]
fn a_guide_is_picked_by_its_point_or_its_line() {
    let mut r = rig(ModelerTool::Select);
    let line = add_guide(&mut r, Guide::line(DVec3::new(0.0, 1000.0, 0.0), DVec3::X), None);
    let point = add_guide(&mut r, Guide::point(DVec3::new(-1000.0, -1000.0, 0.0)), None);
    let cam = top();
    let mut engine = Inferencer::default();
    let mut pick = |r: &mut Rig, p: DVec3| {
        let rev = r.ed.revision();
        let scenes = r.cache.get(r.ed.doc(), rev);
        let w = World {
            doc: r.ed.doc(),
            scenes,
        };
        select::pick_guide(&w, &mut engine, &cam, px(&cam, p) + DVec2::new(2.0, 2.0))
    };
    assert_eq!(pick(&mut r, DVec3::new(1500.0, 1000.0, 0.0)), Some(line));
    assert_eq!(pick(&mut r, DVec3::new(-1000.0, -1000.0, 0.0)), Some(point));
    assert_eq!(pick(&mut r, DVec3::new(0.0, 0.0, 0.0)), None);
    // Not shown, not picked.
    r.cache.show.guides = false;
    assert_eq!(pick(&mut r, DVec3::new(1500.0, 1000.0, 0.0)), None);
}

#[test]
fn a_window_takes_guide_points_and_finite_lines_and_a_crossing_takes_every_line() {
    let mut r = rig(ModelerTool::Select);
    let point = add_guide(&mut r, Guide::point(DVec3::new(-1000.0, 0.0, 0.0)), None);
    let short = add_guide(
        &mut r,
        Guide::segment(DVec3::new(0.0, -1000.0, 0.0), DVec3::X, 800.0),
        None,
    );
    let long = add_guide(&mut r, Guide::line(DVec3::new(0.0, 1000.0, 0.0), DVec3::X), None);
    let cam = top();
    let sel = |r: &mut Rig, a: DVec2, b: DVec2, crossing: bool| {
        let rev = r.ed.revision();
        let scenes = r.cache.get(r.ed.doc(), rev);
        let w = World {
            doc: r.ed.doc(),
            scenes,
        };
        let mut v = select::rect_guides(&w, &cam, a, b, crossing);
        v.sort();
        v
    };
    let whole = (DVec2::ZERO, DVec2::new(800.0, 600.0));
    let mut want = vec![point, short];
    want.sort();
    assert_eq!(sel(&mut r, whole.0, whole.1, false), want, "a line without ends is not inside");
    let mut want = vec![point, short, long];
    want.sort();
    assert_eq!(sel(&mut r, whole.0, whole.1, true), want);
    // A small window around the point takes only the point.
    let p = px(&cam, DVec3::new(-1000.0, 0.0, 0.0));
    assert_eq!(
        sel(&mut r, p - DVec2::splat(10.0), p + DVec2::splat(10.0), false),
        vec![point]
    );
    // Guides that are not shown are never taken.
    r.cache.show.guides = false;
    assert!(sel(&mut r, whole.0, whole.1, true).is_empty());
}

#[test]
fn a_hidden_model_object_is_offered_only_when_hidden_objects_are_shown() {
    let mut r = rig(ModelerTool::Select);
    let (id, _) = square(&mut r);
    let cmd = set_hidden_command(r.ed.doc(), &[id], true).unwrap();
    r.ed.execute(&cmd).unwrap();
    let rev = r.ed.revision();
    assert!(r.cache.get(r.ed.doc(), rev).objects().is_empty());
    r.cache.show.hidden = true;
    assert_eq!(r.cache.get(r.ed.doc(), rev).objects(), &[id]);
}
