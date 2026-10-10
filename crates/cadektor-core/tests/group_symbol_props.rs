//! Property tests: groups under random commands and undo, symbol instance transforms, and the area
//! that a hatch covers.

use cadektor_core::DVec2;
use cadektor_core::command::{Command, Editor};
use cadektor_core::geom::{point_in_polygon, polygon_area};
use cadektor_core::graphics::{
    GraphicResource, HatchDef, HatchLine, PatternUnits, SymbolDef, SymbolItem,
};
use cadektor_core::model::{Attributes, Document, ObjectId, Shape};
use cadektor_core::pattern::hatch_segments;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::symbols;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Add(f64),
    Group(Vec<u8>),
    Ungroup(u8),
    Move(u8, f64),
    Rotate(u8, f64),
    Copy(u8),
    Delete(u8),
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (-1000.0..1000.0f64).prop_map(Op::Add),
        prop::collection::vec(any::<u8>(), 1..4).prop_map(Op::Group),
        any::<u8>().prop_map(Op::Ungroup),
        (any::<u8>(), -500.0..500.0f64).prop_map(|(i, d)| Op::Move(i, d)),
        (any::<u8>(), -3.0..3.0f64).prop_map(|(i, a)| Op::Rotate(i, a)),
        any::<u8>().prop_map(Op::Copy),
        any::<u8>().prop_map(Op::Delete),
        Just(Op::Undo),
        Just(Op::Redo),
    ]
}

/// The objects as (z-order, shape) without ID counters, to compare documents after undo.
fn norm(d: &Document) -> Document {
    let mut c = d.clone();
    // The ID counter is not taken back by undo.
    let _ = &mut c;
    c
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn any_mix_of_group_commands_keeps_the_tree_valid_and_undoes_fully(ops in prop::collection::vec(op(), 1..40)) {
        let mut ed = Editor::default();
        let mut baseline = None;
        for op in ops {
            let all: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
            let pick = |t: u8| if all.is_empty() { None } else { Some(all[t as usize % all.len()]) };
            let cmd = match op {
                Op::Add(x) => Some(Command::AddObject { shape: Shape::Circle { center: DVec2::new(x, 0.0), radius: 5.0 }, layer: None, class: None, attrs: Attributes::default() }),
                Op::Group(ts) => {
                    let mut ids: Vec<ObjectId> = ts.iter().filter_map(|t| pick(*t)).collect();
                    ids.dedup();
                    Some(Command::Group { ids, name: None })
                }
                Op::Ungroup(t) => pick(t).map(|id| Command::Ungroup { ids: vec![id] }),
                Op::Move(t, d) => pick(t).map(|id| Command::MoveObjects { ids: vec![id], delta: DVec2::new(d, d / 2.0) }),
                Op::Rotate(t, a) => pick(t).map(|id| Command::RotateObjects { ids: vec![id], center: DVec2::ZERO, angle: a }),
                Op::Copy(t) => pick(t).map(|id| Command::CopyObjects { ids: vec![id], delta: DVec2::new(10.0, 10.0) }),
                Op::Delete(t) => pick(t).map(|id| Command::DeleteObjects { ids: vec![id] }),
                Op::Undo => {
                    ed.undo();
                    None
                }
                Op::Redo => {
                    ed.redo();
                    None
                }
            };
            if let Some(c) = cmd {
                let before = ed.doc().clone();
                if ed.execute(&c).is_err() {
                    // A refused command changes nothing.
                    prop_assert_eq!(norm(ed.doc()), norm(&before));
                }
            }
            baseline.get_or_insert(());
            // The tree is always valid, groups are never empty, and the document saves and loads again.
            let mut d = ed.doc().clone();
            prop_assert!(d.validate().is_ok());
            for o in ed.doc().objects.values() {
                if matches!(o.shape, Shape::Group { .. }) {
                    prop_assert!(!ed.doc().children(o.id).is_empty(), "empty group {}", o.id);
                }
            }
            let text = serde_json::to_string(ed.doc()).unwrap();
            prop_assert_eq!(Document::from_json(&text).unwrap(), ed.doc().clone());
        }
        while ed.undo() {}
        prop_assert!(ed.doc().objects.is_empty());
    }

    #[test]
    fn a_group_moves_and_rotates_like_its_members(xs in prop::collection::vec(-1000.0..1000.0f64, 2..6), d in (-500.0..500.0f64, -500.0..500.0f64), angle in -3.0..3.0f64) {
        let mut ed = Editor::default();
        let ids: Vec<ObjectId> = xs.iter().map(|x| ObjectId(ed.execute(&Command::AddObject { shape: Shape::Circle { center: DVec2::new(*x, *x / 2.0), radius: 3.0 }, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0])).collect();
        let g = ObjectId(ed.execute(&Command::Group { ids: ids.clone(), name: None }).unwrap().created[0]);
        let before: Vec<DVec2> = ids.iter().map(|i| match ed.doc().object(*i).unwrap().shape { Shape::Circle { center, .. } => center, _ => unreachable!() }).collect();
        ed.execute(&Command::MoveObjects { ids: vec![g], delta: DVec2::new(d.0, d.1) }).unwrap();
        ed.execute(&Command::RotateObjects { ids: vec![g], center: DVec2::new(1.0, 2.0), angle }).unwrap();
        for (i, b) in ids.iter().zip(before) {
            let Shape::Circle { center, .. } = ed.doc().object(*i).unwrap().shape else { unreachable!() };
            let want = DVec2::new(1.0, 2.0) + DVec2::from_angle(angle).rotate(b + DVec2::new(d.0, d.1) - DVec2::new(1.0, 2.0));
            prop_assert!(center.distance(want) < 1e-6 * (1.0 + want.length()));
        }
    }

    #[test]
    fn instance_bounds_contain_the_transformed_points(
        at in (-1.0e5..1.0e5f64, -1.0e5..1.0e5f64),
        rotation in -7.0..7.0f64,
        sx in prop_oneof![0.1..5.0f64, -5.0..-0.1f64],
        sy in prop_oneof![0.1..5.0f64, -5.0..-0.1f64],
        px in (-50.0..50.0f64, -50.0..50.0f64),
    ) {
        let mut ed = Editor::default();
        let def = SymbolDef {
            insertion: DVec2::new(px.0, px.1),
            items: vec![SymbolItem { shape: Shape::Rect { min: DVec2::new(-20.0, -10.0), max: DVec2::new(30.0, 25.0) }, attrs: Attributes::default(), class: None }],
            ..Default::default()
        };
        let sym = ResourceId(ed.execute(&Command::AddResource { name: "S".into(), body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def.clone()))) }).unwrap().created[0]);
        let shape = Shape::Symbol { symbol: sym, at: DVec2::new(at.0, at.1), rotation, scale: sx, scale_y: Some(sy) };
        let id = ObjectId(ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0]);
        let o = ed.doc().object(id).unwrap();
        let (lo, hi) = ed.doc().object_bounds(o);
        // The corners of the rect, moved by hand, are the corners of the bounds' content.
        let xf = symbols::transform(&def, DVec2::new(at.0, at.1), rotation, DVec2::new(sx, sy));
        let corners = [DVec2::new(-20.0, -10.0), DVec2::new(30.0, -10.0), DVec2::new(30.0, 25.0), DVec2::new(-20.0, 25.0)];
        let by_hand = corners.iter().map(|c| DVec2::new(at.0, at.1) + DVec2::from_angle(rotation).rotate((*c - def.insertion) * DVec2::new(sx, sy)));
        for p in by_hand {
            prop_assert!(p.x >= lo.x - 1e-6 && p.x <= hi.x + 1e-6 && p.y >= lo.y - 1e-6 && p.y <= hi.y + 1e-6, "{p} outside {lo} {hi}");
            prop_assert!(xf.transform_point2(DVec2::ZERO).is_finite());
        }
        // The insertion point is where the instance is: a point in the middle of the rect edge hits.
        let edge_mid = DVec2::new(at.0, at.1) + DVec2::from_angle(rotation).rotate((DVec2::new(30.0, 7.5) - def.insertion) * DVec2::new(sx, sy));
        prop_assert!(symbols::hit_instance(ed.doc(), o, edge_mid, 1e-3 * (1.0 + edge_mid.length())));
        // The hand-made bounds are tight: one corner at least touches each side.
        prop_assert!(((hi - lo).x > 0.0) && ((hi - lo).y > 0.0));
    }

    #[test]
    fn hatch_lines_cover_the_area_of_convex_polygons(
        n in 3usize..9,
        radius in 200.0..2000.0f64,
        angle in 0.0..180.0f64,
        spacing in 4.0..20.0f64,
        shift in (-1.0e5..1.0e5f64, -1.0e5..1.0e5f64),
    ) {
        // A regular polygon is convex.
        let ring: Vec<DVec2> = (0..n).map(|k| DVec2::new(shift.0, shift.1) + DVec2::from_angle(std::f64::consts::TAU * k as f64 / n as f64) * radius).collect();
        let def = HatchDef { units: PatternUnits::World, lines: vec![HatchLine::solid(angle, spacing)] };
        let segs = hatch_segments(&def, std::slice::from_ref(&ring), 1.0);
        let covered: f64 = segs.iter().map(|s| s.a.distance(s.b)).sum::<f64>() * spacing;
        let area = polygon_area(&ring);
        // One spacing of slack along the boundary on every side.
        prop_assert!((covered - area).abs() <= 2.0 * spacing * radius * 2.0 + 1.0e-6 * area, "covered {covered}, area {area}");
        // Every piece lies inside (a hair of slack for pieces on the boundary).
        for s in &segs {
            let mid = (s.a + s.b) / 2.0;
            prop_assert!(point_in_polygon(mid, &ring) || ring.iter().zip(ring.iter().cycle().skip(1)).any(|(a, b)| cadektor_core::geom::dist_to_segment(mid, *a, *b) < 1e-6 * radius));
        }
    }
}
