//! The holes, the origin and the angle of a hardscape travel in the collaboration property model
//! as a part of `data.hardscape` (HRD-12, HRD-13).

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::hardscape::{self, HardscapePatch};
use kadakas_core::model::{Attributes, Document, ObjectId};
use kadakas_props::docsync::{ops_from_diff, replica_of};
use kadakas_props::materialize::materialize;
use kadakas_props::op::{Op, Writer};

fn rect(w: f64, h: f64) -> Vec<DVec2> {
    vec![
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, h),
        DVec2::new(0.0, h),
    ]
}

#[test]
fn holes_origin_and_angle_round_trip_and_sync_as_one_property() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let id = ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: Some(corners(&rect(5000.0, 3000.0))),
            patch: Box::new(HardscapePatch {
                holes: Some(vec![corners(&[
                    DVec2::new(1000.0, 1000.0),
                    DVec2::new(2000.0, 1000.0),
                    DVec2::new(2000.0, 2000.0),
                ])]),
                origin: Some(DVec2::new(10.0, 20.0)),
                angle: Some(33.0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let view = replica_of(ed.doc());
    let back = materialize(&view, &Document::with_sequential_ids()).doc;
    let d = hardscape::data(back.object(id).unwrap()).unwrap();
    let want = hardscape::data(ed.doc().object(id).unwrap()).unwrap();
    assert_eq!(d, want);
    assert_eq!(d.holes.len(), 1);
    assert_eq!(
        (d.origin, d.angle),
        (Some(DVec2::new(10.0, 20.0)), Some(33.0))
    );

    // A new angle is one property change, and another client gets it.
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            angle: Some(60.0),
            ..Default::default()
        },
    })
    .unwrap();
    let ops = ops_from_diff(&view, ed.doc());
    let sets: Vec<&Op> = ops
        .iter()
        .filter(|o| matches!(o, Op::Set { prop, .. } if prop == "data.hardscape"))
        .collect();
    assert_eq!(sets.len(), 1, "{ops:?}");
    let mut other = view.clone();
    other.apply_all(Writer::BASELINE, &ops);
    let back = materialize(&other, &Document::with_sequential_ids()).doc;
    assert_eq!(
        hardscape::data(back.object(id).unwrap()).unwrap().angle,
        Some(60.0)
    );
}
