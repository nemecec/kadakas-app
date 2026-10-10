//! Property tests: dimension values match the geometry after any moves, and scale bars are right at any scale.

use cadektor_core::annot::draw::{Ctx, scale_bar_length};
use cadektor_core::annot::*;
use cadektor_core::command::{Command, Editor};
use cadektor_core::model::{Attributes, ObjectId, Shape};
use glam::DVec2;
use proptest::prelude::*;

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0])
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
        let anchor = |i| Some(Anchor { object: l, target: AnchorTarget::Vertex { index: i } });
        let d = add(&mut ed, Shape::Annotation(Box::new(Annot::Dimension(Dimension {
            mode: DimMode::Linear { axis: Axis::Aligned }, points: vec![a, b], anchors: vec![anchor(0), anchor(1)], offset: 5.0, style: None, text: None,
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
}
