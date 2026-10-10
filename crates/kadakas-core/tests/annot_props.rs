//! Property tests: dimension values match the geometry after any moves, and scale bars are right at any scale.

use glam::DVec2;
use kadakas_core::annot::draw::{Ctx, scale_bar_length};
use kadakas_core::annot::*;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, VertexKind};
use kadakas_core::model::{Attributes, ObjectId, Shape};
use proptest::prelude::*;

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

proptest! {
    #[test]
    fn dimension_values_match_geometry_after_moves(
        x in -1.0e5..1.0e5f64, y in -1.0e5..1.0e5f64, len in 1.0..1.0e5f64,
        moves in proptest::collection::vec((-1.0e4..1.0e4f64, -1.0e4..1.0e4f64), 1..6),
    ) {
        let mut ed = Editor::default();
        let a = DVec2::new(x, y);
        let b = a + DVec2::new(len, 0.0);
        let l = add(&mut ed, Shape::Line { a, b });
        let anchor = |i| Some(Anchor { object: l, target: AnchorTarget::Vertex { index: i }, count: None });
        let d = add(&mut ed, Shape::Annotation(Box::new(Annot::Dimension(Dimension {
            mode: DimMode::Linear { axis: Axis::Aligned }, points: vec![a, b], anchors: vec![anchor(0), anchor(1)], offset: 5.0, style: None, text: None, detached: false,
        }))));
        for (dx, dy) in moves {
            ed.execute(&Command::MoveObjects { ids: vec![l], delta: DVec2::new(dx, dy) }).unwrap();
            let Shape::Line { a, b } = ed.doc().object(l).unwrap().shape.clone() else { panic!() };
            let Shape::Annotation(an) = &ed.doc().object(d).unwrap().shape else { panic!() };
            let Annot::Dimension(dm) = &**an else { panic!() };
            prop_assert_eq!(&dm.points, &vec![a, b]);
            prop_assert!((dim::values(dm)[0] - a.distance(b)).abs() < 1e-6);
        }
    }

    #[test]
    fn scale_bar_length_is_right_at_any_scale(scale in 1.0..5000.0f64, seg in 0.5..50.0f64, n in 1u32..=20) {
        let bar = ScaleBar { at: DVec2::ZERO, unit: BarUnit::M, segment: seg, segments: n, height_mm: 2.0, scale: None, style: None };
        let real = seg * n as f64 * 1000.0;
        // A viewport at 1:scale: the bar is the real length over the scale, in paper mm.
        let vp = Ctx { paper: 1.0, real: 1.0 / scale, view: None };
        prop_assert!((scale_bar_length(&bar, &vp) - real / scale).abs() < 1e-6);
        // A layer at 1:scale: the bar is the real length in model units, which is real / scale paper mm.
        let layer = Ctx { paper: scale, real: 1.0, view: None };
        prop_assert!((scale_bar_length(&bar, &layer) / layer.paper - real / scale).abs() < 1e-6);
    }

    #[test]
    fn anchored_dimensions_agree_with_a_model_after_random_edits(
        edits in proptest::collection::vec((0u8..3, 0usize..8, 0.0..3.0f64, -500.0..500.0f64), 1..25),
        ia in 0usize..4, ib in 0usize..4,
    ) {
        // Vertices have identities. A model tracks where each one is; the dimension must agree with it.
        let mut next_id = 0u32;
        let mut verts: Vec<(u32, DVec2)> = Vec::new();
        let mut fresh = |y: f64, verts: &mut Vec<(u32, DVec2)>, at: usize| {
            let id = next_id;
            next_id += 1;
            verts.insert(at.min(verts.len()), (id, DVec2::new(id as f64 * 10.0, y)));
            id
        };
        for i in 0..4 { fresh(i as f64, &mut verts, i); }
        let shape = |verts: &[(u32, DVec2)]| Shape::Polyline { points: verts.iter().map(|(_, p)| Vertex::new(*p, VertexKind::Corner)).collect(), closed: false };
        let mut ed = Editor::default();
        let pl = add(&mut ed, shape(&verts));
        let (ia, ib) = (ia, if ib == ia { (ib + 1) % 4 } else { ib });
        let (ida, idb) = (verts[ia].0, verts[ib].0);
        let (pa, pb) = (verts[ia].1, verts[ib].1);
        let anchor = |i| Some(Anchor::new(pl, AnchorTarget::Vertex { index: i }));
        let d = add(&mut ed, Shape::Annotation(Box::new(Annot::Dimension(Dimension {
            mode: DimMode::Linear { axis: Axis::Aligned }, points: vec![pa, pb], anchors: vec![anchor(ia), anchor(ib)], offset: 5.0, style: None, text: None, detached: false,
        }))));
        let mut last = [pa, pb];
        let mut attached = [true, true];
        for (kind, at, jitter, y) in edits {
            match kind {
                0 => { fresh(y, &mut verts, at); }
                1 if verts.len() > 3 => { verts.remove(at % verts.len()); }
                _ => { let i = at % verts.len(); let id = verts[i].0; verts[i].1 = DVec2::new(id as f64 * 10.0 + jitter, y); }
            }
            ed.execute(&Command::ModifyObject { id: pl, shape: Some(shape(&verts)), layer: None, class: None, attrs: None }).unwrap();
            for (k, id) in [ida, idb].into_iter().enumerate() {
                match verts.iter().find(|(v, _)| *v == id) {
                    Some((_, p)) if attached[k] => last[k] = *p,
                    Some(_) => {}
                    None => attached[k] = false,
                }
            }
            let Shape::Annotation(an) = &ed.doc().object(d).unwrap().shape else { panic!() };
            let Annot::Dimension(dm) = &**an else { panic!() };
            prop_assert_eq!(dm.points[0], last[0]);
            prop_assert_eq!(dm.points[1], last[1]);
            prop_assert_eq!(dm.detached, !(attached[0] && attached[1]));
        }
        // Undo all edits: the dimension is as it was.
        while ed.undo() {}
        prop_assert!(ed.doc().object(d).is_none());
    }
}
