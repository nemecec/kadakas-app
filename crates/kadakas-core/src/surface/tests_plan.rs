//! Tests of the plan view of surface models and of what Object Info tells (MOD-13).

use glam::{DAffine3, DVec2, DVec3};

use super::info::{self, Check, SolidState};
use super::plan::{self, Detail, Part, PlanItem};
use super::*;
use crate::command::{Command, Editor};
use crate::model::{Attributes, Color, Document, ObjectId, Shape};

fn square(m: &mut SurfaceModel, x: f64, y: f64, s: f64) {
    let p = [
        [x, y, 0.0],
        [x + s, y, 0.0],
        [x + s, y + s, 0.0],
        [x, y + s, 0.0],
    ];
    m.apply(&Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    })
    .unwrap();
}

/// A closed box of 1000 x 1000 x 500 mm.
fn box_model() -> SurfaceModel {
    let mut m = SurfaceModel::new();
    square(&mut m, 0.0, 0.0, 1000.0);
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 500.0,
    })
    .unwrap();
    m
}

fn part(m: &SurfaceModel) -> Part<'_> {
    Part {
        model: m,
        xf: m.transform,
        fill: None,
    }
}

fn faces(d: &plan::PlanDrawing) -> Vec<&plan::PlanFace> {
    d.items
        .iter()
        .filter_map(|i| match i {
            PlanItem::Face(f) => Some(f),
            _ => None,
        })
        .collect()
}

#[test]
fn a_box_shows_its_top_over_its_bottom_with_the_edges_of_each() {
    let m = box_model();
    let d = plan::drawing(&[part(&m)], None);
    assert_eq!(d.detail, Detail::Full);
    // The four walls have no area in the plan. The vertical edges are points.
    assert_eq!((d.faces(), d.edges()), (2, 8));
    assert_eq!((d.faces_total, d.edges_total), (6, 12));
    // The lowest face first: the bottom (its back is seen), then its edges, then the top.
    let PlanItem::Face(bottom) = &d.items[0] else {
        panic!("{:?}", d.items[0])
    };
    assert_eq!(bottom.color, crate::mesh::SURFACE_BACK);
    assert!(
        d.items[1..5]
            .iter()
            .all(|i| matches!(i, PlanItem::Edge(..)))
    );
    let PlanItem::Face(top) = &d.items[5] else {
        panic!("{:?}", d.items[5])
    };
    assert_eq!(top.color, crate::mesh::SURFACE_FRONT);
    // The outer ring runs counterclockwise: the area is 1 m2.
    assert!((crate::geom::signed_area(&top.rings[0]) - 1.0e6).abs() < 1e-6);
    assert_eq!(d.bounds(), Some((DVec2::ZERO, DVec2::splat(1000.0))));
}

#[test]
fn the_fill_of_the_object_colours_the_faces_without_a_material() {
    let m = box_model();
    let fill = [10, 120, 30, 255];
    let d = plan::drawing(
        &[Part {
            fill: Some(fill),
            ..part(&m)
        }],
        None,
    );
    assert!(faces(&d).iter().all(|f| f.color == fill));
}

#[test]
fn a_material_colours_the_faces_it_paints() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let mat = ed
        .execute(&Command::AddResource {
            name: "Brick".into(),
            body: crate::resource::ResourceBody::Render(Box::new(
                crate::present::RenderResource::Material(crate::present::Material {
                    base_color: Color([180, 60, 40, 255]),
                    ..Default::default()
                }),
            )),
        })
        .unwrap()
        .created[0];
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(box_model())),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let top = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            unreachable!()
        };
        m.faces()
            .find(|(_, f)| f.plane.normal.z > 0.9)
            .map(|(f, _)| f)
            .unwrap()
    };
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetFaceMaterial {
            faces: vec![top],
            side: PaintSide::Front,
            material: Some(mat.to_string()),
        },
    })
    .unwrap();
    let doc = ed.doc();
    let Some(plan::Outcome::Drawing(d)) = plan::plan_of_object(doc, doc.object(id).unwrap()) else {
        panic!("no drawing")
    };
    let colors: Vec<[u8; 4]> = faces(&d).iter().map(|f| f.color).collect();
    assert_eq!(colors, [crate::mesh::SURFACE_BACK, [180, 60, 40, 255]]);
    // The materials are in the numbers of Object Info too.
    let i = info::of_object(doc, doc.object(id).unwrap(), Check::Auto).unwrap();
    assert_eq!(
        i.materials,
        [info::MaterialCount {
            name: "Brick".into(),
            faces: 1
        }]
    );
    assert_eq!(i.unpainted_faces, 5);
}

#[test]
fn hidden_faces_hidden_edges_and_soft_edges_are_not_drawn() {
    let mut m = box_model();
    let top = m
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(f, _)| f)
        .unwrap();
    let edges: Vec<EdgeId> = m
        .edges()
        .filter(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            a.z == 500.0 && b.z == 500.0
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(edges.len(), 4);
    m.apply(&Edit::SetEdgeStyle {
        edges: vec![edges[0]],
        soft: Some(true),
        smooth: None,
    })
    .unwrap();
    m.apply(&Edit::SetHidden {
        faces: vec![],
        edges: vec![edges[1]],
        hidden: true,
    })
    .unwrap();
    let d = plan::drawing(&[part(&m)], None);
    assert_eq!(d.edges(), 6);
    assert_eq!(d.edges_total, 10);
    m.apply(&Edit::SetHidden {
        faces: vec![top],
        edges: vec![],
        hidden: true,
    })
    .unwrap();
    let d = plan::drawing(&[part(&m)], None);
    assert_eq!(d.faces(), 1);
    assert_eq!(d.faces_total, 5);
}

#[test]
fn a_face_with_a_hole_keeps_the_hole() {
    let mut m = SurfaceModel::new();
    square(&mut m, 0.0, 0.0, 1000.0);
    square(&mut m, 300.0, 300.0, 400.0);
    let d = plan::drawing(&[part(&m)], None);
    let with_hole = faces(&d)
        .into_iter()
        .find(|f| f.rings.len() == 2)
        .expect("a face with a hole");
    assert!(crate::geom::signed_area(&with_hole.rings[0]) > 0.0);
    assert!(crate::geom::signed_area(&with_hole.rings[1]) < 0.0);
    assert_eq!(d.faces(), 2);
}

#[test]
fn a_model_turned_over_by_a_mirror_still_shows_the_right_side() {
    let mut m = box_model();
    // The mirror turns the bottom into... nothing: x is mirrored, z stays. Up stays up.
    m.place(DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
    let d = plan::drawing(&[part(&m)], None);
    let colors: Vec<[u8; 4]> = faces(&d).iter().map(|f| f.color).collect();
    assert_eq!(
        colors,
        [crate::mesh::SURFACE_BACK, crate::mesh::SURFACE_FRONT]
    );
    assert_eq!(
        d.bounds(),
        Some((DVec2::new(-1000.0, 0.0), DVec2::new(0.0, 1000.0)))
    );
}

/// A flat grid of `n` x `n` squares, each cut into two triangles: `2 n n` faces.
fn grid(n: usize) -> SurfaceModel {
    let mut vertices = std::collections::BTreeMap::new();
    let id = |x: usize, y: usize| VertexId((y * (n + 1) + x) as u32 + 1);
    for y in 0..=n {
        for x in 0..=n {
            vertices.insert(id(x, y), DVec3::new(x as f64 * 10.0, y as f64 * 10.0, 0.0));
        }
    }
    let mut edges = std::collections::BTreeMap::new();
    let mut faces = std::collections::BTreeMap::new();
    let mut next = 1u32;
    let mut edge = |a: VertexId,
                    b: VertexId,
                    edges: &mut std::collections::BTreeMap<EdgeId, Edge>,
                    seen: &mut std::collections::HashSet<(VertexId, VertexId)>| {
        if seen.insert(key(a, b)) {
            edges.insert(EdgeId(next), Edge { v: [a, b] });
            next += 1;
        }
    };
    let mut seen = std::collections::HashSet::new();
    let up = Plane::new(DVec3::ZERO, DVec3::Z).unwrap();
    let mut fid = 1u32;
    for y in 0..n {
        for x in 0..n {
            let (a, b, c, d) = (id(x, y), id(x + 1, y), id(x + 1, y + 1), id(x, y + 1));
            for tri in [[a, b, c], [a, c, d]] {
                for k in 0..3 {
                    edge(tri[k], tri[(k + 1) % 3], &mut edges, &mut seen);
                }
                faces.insert(FaceId(fid), Face::new(up, tri.to_vec(), vec![]));
                fid += 1;
            }
        }
    }
    SurfaceModel::from_parts(vertices, edges, faces, 1_000_000)
}

#[test]
fn a_big_model_is_drawn_as_its_outline() {
    // 98 x 98 squares (19,208 faces, 29,008 edges) are still drawn one by one.
    let m = grid(98);
    let d = plan::drawing(&[part(&m)], None);
    assert_eq!(d.detail, Detail::Full);
    assert_eq!((d.faces(), d.edges()), (19_208, 29_008));
    // 101 x 101 is over the limit: the border only, and nothing is filled.
    let m = grid(101);
    let d = plan::drawing(&[part(&m)], None);
    assert_eq!(d.detail, Detail::Outline);
    assert_eq!((d.faces(), d.edges()), (0, 4 * 101));
    assert_eq!(d.faces_total, 2 * 101 * 101);
    assert_eq!(d.bounds(), Some((DVec2::ZERO, DVec2::splat(1010.0))));
}

#[test]
fn an_outline_that_is_still_too_long_is_the_box() {
    // Separate triangles: every edge is a border.
    let mut vertices = std::collections::BTreeMap::new();
    let mut edges = std::collections::BTreeMap::new();
    let mut faces = std::collections::BTreeMap::new();
    let up = Plane::new(DVec3::ZERO, DVec3::Z).unwrap();
    let n = 10_100u32;
    for i in 0..n {
        let x = f64::from(i % 100) * 10.0;
        let y = f64::from(i / 100) * 10.0;
        let v = [
            VertexId(3 * i + 1),
            VertexId(3 * i + 2),
            VertexId(3 * i + 3),
        ];
        vertices.insert(v[0], DVec3::new(x, y, 0.0));
        vertices.insert(v[1], DVec3::new(x + 8.0, y, 0.0));
        vertices.insert(v[2], DVec3::new(x, y + 8.0, 0.0));
        for k in 0..3 {
            edges.insert(
                EdgeId(3 * i + 1 + k as u32),
                Edge {
                    v: [v[k], v[(k + 1) % 3]],
                },
            );
        }
        faces.insert(FaceId(i + 1), Face::new(up, v.to_vec(), vec![]));
    }
    let m = SurfaceModel::from_parts(vertices, edges, faces, 100_000);
    // 30,300 edges are over the limit of the edges, and so are the 30,300 border edges of the outline.
    let d = plan::drawing(&[part(&m)], None);
    assert_eq!(d.detail, Detail::Box);
    assert_eq!((d.faces(), d.edges()), (1, 4));
    assert_eq!(d.bounds(), Some((DVec2::ZERO, DVec2::new(998.0, 1008.0))));
}

#[test]
fn picking_follows_the_faces_and_the_edges_that_are_shown() {
    let mut m = SurfaceModel::new();
    square(&mut m, 0.0, 0.0, 1000.0);
    square(&mut m, 300.0, 300.0, 400.0);
    let p = |x: f64, y: f64| DVec2::new(x, y);
    assert!(plan::hit(&[part(&m)], p(100.0, 100.0), 5.0));
    assert!(plan::hit(&[part(&m)], p(500.0, 500.0), 5.0));
    assert!(plan::hit(&[part(&m)], p(-3.0, 500.0), 5.0));
    assert!(!plan::hit(&[part(&m)], p(-30.0, 500.0), 5.0));
    assert!(!plan::hit(&[part(&m)], p(1500.0, 500.0), 5.0));
}

#[test]
fn a_hole_is_not_picked_but_its_edge_is() {
    let mut m = SurfaceModel::new();
    square(&mut m, 0.0, 0.0, 1000.0);
    square(&mut m, 300.0, 300.0, 400.0);
    let inner = m
        .faces()
        .filter(|(_, f)| f.holes.is_empty())
        .min_by(|a, b| {
            m.face_area(a.0)
                .unwrap()
                .total_cmp(&m.face_area(b.0).unwrap())
        })
        .map(|(f, _)| f)
        .unwrap();
    m.apply(&Edit::SetHidden {
        faces: vec![inner],
        edges: vec![],
        hidden: true,
    })
    .unwrap();
    let p = |x: f64, y: f64| DVec2::new(x, y);
    // The middle of the hidden face: not picked. Its border edge is.
    assert!(!plan::hit(&[part(&m)], p(500.0, 500.0), 5.0));
    assert!(plan::hit(&[part(&m)], p(300.0, 500.0), 5.0));
    assert!(plan::hit(&[part(&m)], p(100.0, 500.0), 5.0));
}

#[test]
fn snap_points_are_vertices_middles_and_face_centres() {
    let m = box_model();
    let g = plan::snap_geometry(&[part(&m)], None, 1000);
    assert_eq!(g.vertices.len(), 8);
    // The four vertical edges are points in the plan.
    assert_eq!(g.edges.len(), 8);
    assert_eq!(g.midpoints.len(), 8);
    // The top and the bottom are seen from above: two centres, at the same place.
    assert_eq!(g.centers, [DVec2::splat(500.0); 2]);
    assert!(g.midpoints.contains(&DVec2::new(500.0, 0.0)));
    // Only what is near the pointer.
    let near = plan::snap_geometry(&[part(&m)], Some((DVec2::ZERO, 50.0)), 1000);
    assert!(near.vertices.contains(&DVec2::ZERO));
    assert!(near.midpoints.is_empty());
    assert!(near.centers.is_empty());
    assert_eq!(near.edges.len(), 4);
    // The cap on the edges.
    assert!(plan::snap_geometry(&[part(&m)], None, 3).edges.len() <= 3);
}

#[test]
fn the_numbers_of_a_closed_box() {
    let doc = Document::with_sequential_ids();
    let m = box_model();
    let i = info::of_parts(&doc, &[part(&m)], Check::Auto);
    assert_eq!((i.vertices, i.edges, i.faces), (8, 12, 6));
    assert_eq!(
        i.solid,
        SolidState::Closed {
            solids: 1,
            volume: 5.0e8
        }
    );
    assert!((i.area - 4.0e6).abs() < 1e-3);
    assert_eq!(i.bounds, Some(([0.0, 0.0, 0.0], [1000.0, 1000.0, 500.0])));
    assert_eq!((i.hidden_faces, i.hidden_edges, i.soft_edges), (0, 0, 0));
    assert_eq!(i.unpainted_faces, 6);
}

#[test]
fn the_transform_scales_the_volume_and_the_area() {
    let doc = Document::with_sequential_ids();
    let mut m = box_model();
    m.place(DAffine3::from_scale(DVec3::new(2.0, 1.0, 3.0)));
    let i = info::of_parts(&doc, &[part(&m)], Check::Auto);
    assert_eq!(
        i.solid,
        SolidState::Closed {
            solids: 1,
            volume: 5.0e8 * 6.0
        }
    );
    // Top and bottom 2 x 1 m, the long walls 2 x 1.5 m, the short walls 1 x 1.5 m.
    let want = 2.0 * 2.0e6 + 2.0 * 3.0e6 + 2.0 * 1.5e6;
    assert!((i.area - want).abs() < 1e-3, "{} {want}", i.area);
    assert_eq!(i.bounds, Some(([0.0, 0.0, 0.0], [2000.0, 1000.0, 1500.0])));
}

#[test]
fn a_surface_that_is_not_closed_is_open() {
    let doc = Document::with_sequential_ids();
    let mut m = SurfaceModel::new();
    square(&mut m, 0.0, 0.0, 1000.0);
    let i = info::of_parts(&doc, &[part(&m)], Check::Auto);
    assert_eq!(i.solid, SolidState::Open { open_edges: 4 });
    assert_eq!(
        info::of_parts(&doc, &[], Check::Auto).solid,
        SolidState::Empty
    );
}

#[test]
fn a_big_model_is_not_checked_unless_asked() {
    let doc = Document::with_sequential_ids();
    let m = grid(101);
    let i = info::of_parts(&doc, &[part(&m)], Check::Auto);
    assert_eq!(i.solid, SolidState::Unchecked);
    assert_eq!(i.faces, 20_402);
    let i = info::of_parts(&doc, &[part(&m)], Check::Always);
    assert_eq!(i.solid, SolidState::Open { open_edges: 404 });
}

#[test]
fn a_check_that_is_never_made_leaves_the_state_open() {
    let doc = Document::with_sequential_ids();
    let m = box_model();
    assert_eq!(
        info::of_parts(&doc, &[part(&m)], Check::Never).solid,
        SolidState::Unchecked
    );
}

#[test]
fn a_transform_that_flattens_the_model_has_no_area_and_no_volume() {
    let doc = Document::with_sequential_ids();
    let mut m = box_model();
    m.place(DAffine3::from_scale(DVec3::new(0.0, 1.0, 1.0)));
    let i = info::of_parts(&doc, &[part(&m)], Check::Auto);
    assert_eq!(i.area, 0.0);
    assert_eq!(
        i.solid,
        SolidState::Closed {
            solids: 1,
            volume: 0.0
        }
    );
}

#[test]
fn a_loose_edge_has_vertices_a_middle_and_no_face_centre() {
    let mut m = SurfaceModel::new();
    let p = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]];
    m.apply(&Edit::AddEdges {
        edges: vec![[p[0], p[1]]],
    })
    .unwrap();
    let g = plan::snap_geometry(&[part(&m)], None, 100);
    assert!(g.centers.is_empty());
    assert_eq!(g.vertices.len(), 2);
    assert_eq!(g.midpoints, [DVec2::new(500.0, 0.0)]);
    // A loose edge is drawn, last.
    let d = plan::drawing(&[part(&m)], None);
    assert_eq!((d.faces(), d.edges()), (0, 1));
}
