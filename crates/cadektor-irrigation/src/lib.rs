//! Irrigation design for Cadektor: a catalog of heads and pipes, hydraulic formulas
//! (Hazen-Williams), a network solver (POC, mainline, valves, laterals, outlets), layout helpers
//! and reports. Pure and wasm-compatible.
//!
//! The drawing objects are `Shape::Irrigation` in `cadektor-core`. This crate reads them from a
//! document ([`extract::analyze`]) and never changes the document by itself. [`zone_commands`]
//! returns the commands that store the calculated zone in the outlets.
//!
//! Units: lengths and coordinates in mm (the model unit), pipe diameters in mm, flow in l/min,
//! pressure in kPa, velocity in m/s, precipitation in mm/h, areas in m2 in reports.

pub mod catalog;
pub mod extract;
pub mod hydraulics;
pub mod layout;
pub mod network;
pub mod report;

pub use catalog::{Catalog, HeadKind, HeadModel, PipeMaterial, PipeSize};
pub use extract::{Analysis, DEFAULT_TOLERANCE, analyze};
pub use network::{Warning, WarningKind};

use cadektor_core::command::Command;
use cadektor_core::irrigation::IrrigationShape;
use cadektor_core::model::{Attributes, Document, ObjectId, Shape};

/// Commands that set the `zone` of every outlet to the zone found by the calculation (and clear it
/// for outlets that have no water path). Outlets that already have the right zone give no command.
pub fn zone_commands(doc: &Document, analysis: &Analysis) -> Vec<Command> {
    let zones = analysis.outlet_zones();
    let mut out = vec![];
    for o in doc.objects.values() {
        let Shape::Irrigation(i) = &o.shape else {
            continue;
        };
        let IrrigationShape::Outlet { zone, .. } = i.as_ref() else {
            continue;
        };
        let want = zones.get(&o.id.0).copied().filter(|z| *z != 0);
        if *zone != want {
            let mut shape = i.clone();
            if let IrrigationShape::Outlet { zone, .. } = shape.as_mut() {
                *zone = want;
            }
            out.push(Command::ModifyObject {
                id: ObjectId(o.id.0),
                shape: Some(Shape::Irrigation(shape)),
                layer: None,
                class: None,
                attrs: None::<Attributes>,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::command::Editor;

    #[test]
    fn zone_commands_set_and_clear_zones() {
        let mut ed = extract::fixtures::small_zone();
        // A lonely outlet with a stale zone.
        extract::fixtures::add(
            &mut ed,
            IrrigationShape::Outlet {
                at: extract::fixtures::v(9e4, 9e4),
                model: "Spray-4.0".into(),
                arc: 360.0,
                direction: 0.0,
                radius: None,
                zone: Some(5),
            },
        );
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        let cmds = zone_commands(ed.doc(), &a);
        // Four outlets get zone 1, the lonely one loses zone 5.
        assert_eq!(cmds.len(), 5);
        let batch = Command::Batch { commands: cmds };
        ed.execute(&batch).unwrap();
        let zones: Vec<Option<u32>> = ed
            .doc()
            .objects
            .values()
            .filter_map(|o| match &o.shape {
                Shape::Irrigation(i) => match i.as_ref() {
                    IrrigationShape::Outlet { zone, .. } => Some(*zone),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(zones, [Some(1), Some(1), Some(1), Some(1), None]);
        // Nothing left to do, and one undo takes it all back.
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        assert!(zone_commands(ed.doc(), &a).is_empty());
        assert!(ed.undo());
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        assert_eq!(zone_commands(ed.doc(), &a).len(), 5);
        let _ = Editor::default();
    }
}
