use glam::{DVec2, DVec3};

use super::super::build_at;
use crate::command::{Command, Editor};
use crate::model::{Attributes, ObjectId, Shape};

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    let out = ed
        .execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    ObjectId(out.created[0])
}

fn square(x: f64) -> Shape {
    Shape::Rect {
        min: DVec2::new(x, 0.0),
        max: DVec2::new(x + 100.0, 100.0),
    }
}

fn scene_of(ed: &Editor) -> super::Scene {
    build_at(ed.doc(), DVec3::ZERO)
}

#[test]
fn recolor_changes_triangles_and_segments_of_the_chosen_objects_once() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, square(0.0));
    let b = add(&mut ed, square(500.0));
    let mut scene = scene_of(&ed);
    assert!(scene.mesh.triangle_count() > 0 && scene.lines.segment_count() > 0);
    let calls = std::cell::Cell::new(0);
    scene.recolor(&|id, c| {
        calls.set(calls.get() + 1);
        (id == a).then_some([1, 2, 3, c[3]])
    });
    // One call for each vertex and each segment, not for each triangle corner.
    assert_eq!(
        calls.get(),
        scene.mesh.positions.len() + scene.lines.segment_count()
    );
    for t in 0..scene.mesh.triangle_count() {
        let c = scene.mesh.colors[scene.mesh.indices[t * 3] as usize];
        assert_eq!(c[..3] == [1, 2, 3], scene.mesh.tri_object[t] == a);
    }
    for s in 0..scene.lines.segment_count() {
        assert_eq!(
            scene.lines.colors[s][..3] == [1, 2, 3],
            scene.lines.objects[s] == a
        );
    }
    assert!(scene.objects().any(|o| o == b));
}

#[test]
fn retain_keeps_the_chosen_objects_with_valid_indices() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, square(0.0));
    let b = add(&mut ed, square(500.0));
    let scene = scene_of(&ed);
    let only_b = scene.retain(&|id| id == b);
    assert_eq!(only_b.objects().collect::<Vec<_>>(), vec![b]);
    assert!(only_b.mesh.tri_object.iter().all(|o| *o == b));
    assert!(only_b.lines.objects.iter().all(|o| *o == b));
    assert!(
        only_b
            .mesh
            .indices
            .iter()
            .all(|i| (*i as usize) < only_b.mesh.positions.len())
    );
    let (lo, hi) = only_b.object_bounds(b).unwrap();
    assert_eq!((lo.x, hi.x), (500.0, 600.0));
    let b_in_whole = scene.object_bounds(b).unwrap();
    assert_eq!(b_in_whole, (lo, hi));
    assert_eq!(only_b.bounds, Some((lo, hi)));
    // Keeping nothing gives an empty scene without a box.
    let none = scene.retain(&|_| false);
    assert!(none.is_empty() && none.bounds.is_none());
    // Keeping both gives the same geometry.
    let both = scene.retain(&|id| id == a || id == b);
    assert_eq!(both.mesh.triangle_count(), scene.mesh.triangle_count());
    assert_eq!(both.mesh.positions.len(), scene.mesh.positions.len());
    assert_eq!(both.lines.positions, scene.lines.positions);
}

#[test]
fn append_adds_geometry_and_moves_it_to_the_origin_of_the_scene() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, square(0.0));
    let mut scene = scene_of(&ed);
    let mut other = Editor::with_sequential_ids();
    // The first object takes the id of `a`; the second one has an id of its own.
    let skipped = add(&mut other, square(900.0));
    assert_eq!(skipped, a);
    let g = add(&mut other, square(300.0));
    // The other scene has another origin: its positions are moved.
    let ghost = build_at(other.doc(), DVec3::new(100.0, 0.0, 0.0));
    let (tris, segs) = (scene.mesh.triangle_count(), scene.lines.segment_count());
    scene.append(&ghost);
    assert_eq!(
        scene.mesh.triangle_count(),
        tris + ghost.mesh.triangle_count()
    );
    assert_eq!(
        scene.lines.segment_count(),
        segs + ghost.lines.segment_count()
    );
    assert!(
        scene
            .mesh
            .indices
            .iter()
            .all(|i| (*i as usize) < scene.mesh.positions.len())
    );
    let (lo, hi) = scene.object_bounds(g).unwrap();
    // Model x 300 to 400, with the origin of the scene at 0: positions are x 300 to 400.
    assert_eq!((lo.x, hi.x), (300.0, 400.0));
    assert!(scene.object_bounds(a).is_some());
    assert_eq!(scene.objects().count(), 2);
    assert_eq!(scene.object_bounds(a).unwrap().0.x, 0.0);
    // An object that the scene has already keeps its own range.
    let before = scene.object_bounds(a).unwrap();
    let again = build_at(other.doc(), DVec3::ZERO);
    scene.append(&again);
    assert_eq!(scene.object_bounds(a).unwrap().0, before.0);
}
