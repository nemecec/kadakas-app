//! `IfcAdvancedBrep` and `IfcAdvancedBrepWithVoids`: boundary representations whose faces lie on
//! planes, cylinders, spheres, tori and B-spline surfaces.
//!
//! IFC uses the geometry of ISO 10303-42, the same entities as a STEP file (`IfcAdvancedFace` is
//! `ADVANCED_FACE`, `IfcEdgeCurve` is `EDGE_CURVE`, `IfcBSplineSurfaceWithKnots` is
//! `B_SPLINE_SURFACE_WITH_KNOTS`). Only the names differ, and STEP entities have a leading `name`
//! attribute. So this module writes the entities that a solid reaches as a small STEP file and gives
//! it to the STEP reader (`kadakas-step`), which cuts the faces into triangles within a tolerance
//! and shares the points of neighbouring faces. One implementation reads both formats.
//!
//! Not translated (the STEP reader then names them as unsupported and leaves the face out):
//! surfaces of revolution and of extrusion (in IFC their curve is a profile, not a curve), pcurves
//! and seam curves.

use std::collections::HashSet;
use std::io::Write;

use super::ctx::Ctx;
use crate::step::{COMPLEX, Entity, Value};

/// The most entities that one solid may reach.
const MAX_ENTITIES: usize = 2_000_000;

/// The STEP name of an IFC entity and whether the STEP entity has a `name` attribute first.
fn step_name(ifc: &str) -> Option<(&'static str, bool)> {
    Some(match ifc {
        "IFCADVANCEDBREP" => ("MANIFOLD_SOLID_BREP", true),
        "IFCADVANCEDBREPWITHVOIDS" => ("BREP_WITH_VOIDS", true),
        "IFCCLOSEDSHELL" => ("CLOSED_SHELL", true),
        "IFCOPENSHELL" => ("OPEN_SHELL", true),
        "IFCFACE" => ("FACE", true),
        "IFCADVANCEDFACE" => ("ADVANCED_FACE", true),
        "IFCFACEBOUND" => ("FACE_BOUND", true),
        "IFCFACEOUTERBOUND" => ("FACE_OUTER_BOUND", true),
        "IFCEDGELOOP" => ("EDGE_LOOP", true),
        "IFCPOLYLOOP" => ("POLY_LOOP", true),
        "IFCORIENTEDEDGE" => ("ORIENTED_EDGE", true),
        "IFCEDGECURVE" => ("EDGE_CURVE", true),
        "IFCVERTEXPOINT" => ("VERTEX_POINT", true),
        "IFCCARTESIANPOINT" => ("CARTESIAN_POINT", true),
        "IFCDIRECTION" => ("DIRECTION", true),
        "IFCVECTOR" => ("VECTOR", true),
        "IFCAXIS2PLACEMENT3D" => ("AXIS2_PLACEMENT_3D", true),
        "IFCAXIS2PLACEMENT2D" => ("AXIS2_PLACEMENT_2D", true),
        "IFCAXIS1PLACEMENT" => ("AXIS1_PLACEMENT", true),
        "IFCLINE" => ("LINE", true),
        "IFCCIRCLE" => ("CIRCLE", true),
        "IFCELLIPSE" => ("ELLIPSE", true),
        "IFCPOLYLINE" => ("POLYLINE", true),
        "IFCTRIMMEDCURVE" => ("TRIMMED_CURVE", true),
        "IFCCOMPOSITECURVE" => ("COMPOSITE_CURVE", true),
        "IFCCOMPOSITECURVESEGMENT" => ("COMPOSITE_CURVE_SEGMENT", false),
        "IFCPLANE" => ("PLANE", true),
        "IFCCYLINDRICALSURFACE" => ("CYLINDRICAL_SURFACE", true),
        "IFCSPHERICALSURFACE" => ("SPHERICAL_SURFACE", true),
        "IFCTOROIDALSURFACE" => ("TOROIDAL_SURFACE", true),
        "IFCSURFACECURVE" => ("SURFACE_CURVE", true),
        "IFCINTERSECTIONCURVE" => ("INTERSECTION_CURVE", true),
        "IFCSEAMCURVE" => ("SEAM_CURVE", true),
        "IFCBOUNDEDSURFACECURVE" => ("BOUNDED_SURFACE_CURVE", true),
        "IFCPCURVE" => ("PCURVE", true),
        "IFCFACETEDBREP" => ("FACETED_BREP", true),
        "IFCFACETEDBREPWITHVOIDS" => ("BREP_WITH_VOIDS", true),
        "IFCSHELLBASEDSURFACEMODEL" => ("SHELL_BASED_SURFACE_MODEL", true),
        "IFCFACEBASEDSURFACEMODEL" => ("FACE_BASED_SURFACE_MODEL", true),
        "IFCCONNECTEDFACESET" => ("CONNECTED_FACE_SET", true),
        "IFCBSPLINESURFACEWITHKNOTS" => ("B_SPLINE_SURFACE_WITH_KNOTS", true),
        "IFCBSPLINECURVEWITHKNOTS" => ("B_SPLINE_CURVE_WITH_KNOTS", true),
        _ => return None,
    })
}

/// The names of the typed values that STEP writes differently.
fn typed_name(ifc: &str) -> String {
    match ifc {
        "IFCPARAMETERVALUE" => "PARAMETER_VALUE".into(),
        other => other.into(),
    }
}

fn convert(v: &Value) -> Value {
    match v {
        Value::Typed(n, inner) => Value::Typed(typed_name(n), Box::new(convert(inner))),
        Value::List(l) => Value::List(l.iter().map(convert).collect()),
        other => other.clone(),
    }
}

/// Multiplies the `PARAMETER_VALUE`s of a trim by `f`.
fn scale_parameters(v: &mut Value, f: f64) {
    match v {
        Value::Typed(n, inner) if n == "PARAMETER_VALUE" => {
            if let Value::Real(x) = inner.as_mut() {
                *x *= f;
            } else if let Value::Int(i) = inner.as_ref() {
                **inner = Value::Real(*i as f64 * f);
            }
        }
        Value::List(l) => l.iter_mut().for_each(|x| scale_parameters(x, f)),
        _ => {}
    }
}

fn line(out: &mut Vec<u8>, id: u64, name: &str, attrs: &[Value]) {
    let _ = write!(out, "#{id}={name}(");
    for (i, a) in attrs.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        let _ = a.write(out);
    }
    out.extend_from_slice(b");\n");
}

fn part(out: &mut Vec<u8>, name: &str, attrs: &[Value]) {
    let _ = write!(out, "{name}(");
    for (i, a) in attrs.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        let _ = a.write(out);
    }
    out.extend_from_slice(b") ");
}

/// Writes one entity in STEP form. `false` when the entity has no counterpart.
fn emit(out: &mut Vec<u8>, e: &Entity, angle: Option<f64>) -> bool {
    let mut a: Vec<Value> = e.attrs.iter().map(convert).collect();
    // The trims of a circle or an ellipse are angles in the angle unit of the file; STEP has radians.
    if let ("IFCTRIMMEDCURVE", Some(f)) = (e.name.as_str(), angle) {
        for i in [1, 2] {
            if let Some(v) = a.get_mut(i) {
                scale_parameters(v, f);
            }
        }
    }
    match e.name.as_str() {
        // A rational spline is a complex instance in STEP: the weights are a part of their own.
        "IFCRATIONALBSPLINESURFACEWITHKNOTS" if a.len() >= 13 => {
            let _ = write!(out, "#{}=(", e.id);
            part(out, "B_SPLINE_SURFACE", &a[0..7]);
            part(out, "B_SPLINE_SURFACE_WITH_KNOTS", &a[7..12]);
            part(out, "RATIONAL_B_SPLINE_SURFACE", &a[12..13]);
            out.extend_from_slice(b");\n");
            true
        }
        "IFCRATIONALBSPLINECURVEWITHKNOTS" if a.len() >= 9 => {
            let _ = write!(out, "#{}=(", e.id);
            part(out, "B_SPLINE_CURVE", &a[0..5]);
            part(out, "B_SPLINE_CURVE_WITH_KNOTS", &a[5..8]);
            part(out, "RATIONAL_B_SPLINE_CURVE", &a[8..9]);
            out.extend_from_slice(b");\n");
            true
        }
        n => match step_name(n) {
            Some((name, named)) => {
                let mut attrs = Vec::with_capacity(a.len() + 1);
                if named {
                    attrs.push(Value::Str(String::new()));
                }
                attrs.extend(a);
                line(out, e.id, name, &attrs);
                true
            }
            None => false,
        },
    }
}

/// A STEP file with the solid `root` and everything it reaches. The numbers of the entities stay.
/// `Err` when the solid reaches more than [`MAX_ENTITIES`] entities. Entities with no counterpart
/// are written under their IFC name: the STEP reader names them in its report.
pub fn to_step(cx: &Ctx, root: &Entity) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    out.extend_from_slice(
        b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('IfcAdvancedBrep'),'2;1');\nFILE_NAME('brep','',(''),(''),'Kadakas','Kadakas','');\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n",
    );
    let mut seen: HashSet<u64> = HashSet::new();
    let mut stack = vec![root.id];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > MAX_ENTITIES {
            return Err(format!(
                "the solid reaches more than {MAX_ENTITIES} entities"
            ));
        }
        let Some(e) = cx.ent(id) else { continue };
        // Only a circle or an ellipse has angles for parameters.
        let angle = (e.name == "IFCTRIMMEDCURVE"
            && (cx.units.angle_rad - 1.0).abs() > 1e-12
            && cx
                .get(&e.attrs[0])
                .is_some_and(|b| matches!(b.name.as_str(), "IFCCIRCLE" | "IFCELLIPSE")))
        .then_some(cx.units.angle_rad);
        if !emit(&mut out, &e, angle) && e.name != COMPLEX {
            // No counterpart: written as it is, so that the reader can report its name.
            line(&mut out, e.id, &e.name, &e.attrs);
        }
        for v in &e.attrs {
            v.each_ref(&mut |r| {
                if !seen.contains(&r) {
                    stack.push(r);
                }
            });
        }
    }
    out.extend_from_slice(b"ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(out)
}
