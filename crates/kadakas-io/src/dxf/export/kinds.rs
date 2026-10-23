//! Irrigation objects, massing models and plant tags in DXF.
//!
//! - Irrigation: plain geometry on one layer per kind (`IRR-OUTLET`, `IRR-MAINLINE`, `IRR-LATERAL`,
//!   `IRR-DRIP`, `IRR-VALVE`, `IRR-POC`, `IRR-CONTROLLER`). The main entity carries XDATA of the
//!   application `KADAKAS` with the data of the object.
//! - Massing: the outline as a closed polyline with XDATA (heights in mm, roof), and the solid as a
//!   mesh in the entity type of the option `mesh_entity`.
//! - Plant tag: the leader as a LINE, the ID bubble as a CIRCLE and the text lines as TEXT.

use kadakas_core::annot::draw::text_width;
use kadakas_core::irrigation::{IrrigationShape as I, LineType, sector_points};
use kadakas_core::massing::Massing;
use kadakas_core::model::{Object, Shape};
use kadakas_core::{DVec2, DVec3};

use super::{Ctx, W, bulged_vertices, circle, common, escape_unicode, lwpolyline, native};

/// The application name of the XDATA.
pub const XDATA_APP: &str = "KADAKAS";
/// The longest XDATA string that the format allows.
const XDATA_LIMIT: usize = 250;

/// Appends XDATA to the entity that was written last, as `key=value` strings.
fn xdata(w: &mut W, cx: &mut Ctx, items: &[(&str, String)]) {
    cx.xdata = true;
    w.pair(1001, XDATA_APP);
    for (k, v) in items {
        // Cut the text, not the escaped text: a cut must not fall inside a `\U+XXXX` escape.
        let mut s = format!("{k}=");
        for c in v.chars().filter(|c| !c.is_control()) {
            let e = escape_unicode(&c.to_string());
            if s.len() + e.len() > XDATA_LIMIT {
                break;
            }
            s.push_str(&e);
        }
        w.pair(1000, s);
    }
}

fn opt<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map(T::to_string).unwrap_or_default()
}

/// The entities of an irrigation object. `hidden` hides them (hidden design layer).
pub(super) fn irrigation(w: &mut W, cx: &mut Ctx, o: &Object, base: &str, hidden: bool, i: &I) {
    // One layer for each kind, after the layer (or class) of the object when it has a name.
    let named = |kind: &str| {
        if base == "0" {
            kind.to_string()
        } else {
            format!("{base}-{kind}")
        }
    };
    let scale = cx.doc.layer(o.layer).map_or(100.0, |l| l.scale);
    let kind = i.kind().to_string();
    let square = |at: DVec2, r: f64| {
        [
            at + DVec2::new(-r, -r),
            at + DVec2::new(r, -r),
            at + DVec2::new(r, r),
            at + DVec2::new(-r, r),
        ]
        .map(|p| (p, 0.0))
    };
    match i {
        I::Outlet {
            at,
            model,
            arc,
            direction,
            radius,
            zone,
        } => {
            let layer = named("IRR-OUTLET");
            let layer = layer.as_str();
            circle(w, cx, o, layer, hidden, *at, 1.2 * scale);
            xdata(
                w,
                cx,
                &[
                    ("kind", kind),
                    ("model", model.clone()),
                    ("zone", opt(zone)),
                    ("arc", arc.to_string()),
                    ("direction", direction.to_string()),
                    ("radius", opt(radius)),
                ],
            );
            // The reach of the head, as the outline of the fan.
            if let Some(r) = radius {
                let fan: Vec<(DVec2, f64)> = sector_points(*at, *r, *arc, *direction, 48)
                    .into_iter()
                    .map(|p| (p, 0.0))
                    .collect();
                lwpolyline(w, cx, o, layer, hidden, &fan, true);
            }
        }
        I::Line {
            points,
            line_type,
            size,
            material,
            emitter_spacing,
            emitter_flow,
        } => {
            let layer = named(match line_type {
                LineType::Mainline => "IRR-MAINLINE",
                LineType::Lateral => "IRR-LATERAL",
                LineType::Drip => "IRR-DRIP",
            });
            let layer = layer.as_str();
            let verts: Vec<(DVec2, f64)> = points.iter().map(|p| (*p, 0.0)).collect();
            lwpolyline(w, cx, o, layer, hidden, &verts, false);
            xdata(
                w,
                cx,
                &[
                    ("kind", kind),
                    ("line_type", line_type.id().to_string()),
                    ("size", size.clone()),
                    ("material", material.clone()),
                    ("emitter_spacing", opt(emitter_spacing)),
                    ("emitter_flow", opt(emitter_flow)),
                ],
            );
        }
        I::Poc {
            at,
            name,
            static_pressure,
            available_flow,
            supply_loss,
            meter_loss,
        } => {
            let layer = named("IRR-POC");
            let layer = layer.as_str();
            let r = 2.0 * scale;
            circle(w, cx, o, layer, hidden, *at, r);
            xdata(
                w,
                cx,
                &[
                    ("kind", kind),
                    ("name", name.clone()),
                    ("static_pressure_kpa", static_pressure.to_string()),
                    ("available_flow_lpm", available_flow.to_string()),
                    ("supply_loss_kpa", opt(supply_loss)),
                    ("meter_loss_kpa", opt(meter_loss)),
                ],
            );
            for (a, b) in [
                (DVec2::new(-r, 0.0), DVec2::new(r, 0.0)),
                (DVec2::new(0.0, -r), DVec2::new(0.0, r)),
            ] {
                common(w, "LINE", layer, o, hidden);
                w.pair(100, "AcDbLine");
                cx.pt3(w, 10, at.x + a.x, at.y + a.y, 0.0);
                cx.pt3(w, 11, at.x + b.x, at.y + b.y, 0.0);
            }
        }
        I::Valve {
            at,
            zone,
            name,
            kv,
            controller,
            station,
        } => {
            // A bow tie.
            let r = 1.6 * scale;
            let bow = [
                (*at + DVec2::new(-r, -r), 0.0),
                (*at + DVec2::new(r, r), 0.0),
                (*at + DVec2::new(r, -r), 0.0),
                (*at + DVec2::new(-r, r), 0.0),
            ];
            lwpolyline(w, cx, o, &named("IRR-VALVE"), hidden, &bow, true);
            xdata(
                w,
                cx,
                &[
                    ("kind", kind),
                    ("zone", zone.to_string()),
                    ("name", name.clone()),
                    ("kv", kv.to_string()),
                    ("controller", controller.clone().unwrap_or_default()),
                    ("station", opt(station)),
                ],
            );
        }
        I::Controller { at, name, stations } => {
            lwpolyline(
                w,
                cx,
                o,
                &named("IRR-CONTROLLER"),
                hidden,
                &square(*at, 2.0 * scale),
                true,
            );
            xdata(
                w,
                cx,
                &[
                    ("kind", kind),
                    ("name", name.clone()),
                    ("stations", stations.to_string()),
                ],
            );
        }
    }
}

/// The base of a massing model in mm: the ground under it plus the elevation of its layer. `o` is in
/// final (shifted) coordinates, but the site models are not.
fn base_z(cx: &Ctx, o: &Object, m: &Massing) -> f64 {
    let mut local = o.clone();
    local.shape.translate(-cx.shift);
    let base = match &local.shape {
        Shape::Massing(lm) => cx.terrain.massing_base(&local, lm),
        _ => m.base.unwrap_or(0.0),
    };
    base + cx.doc.layer(o.layer).map_or(0.0, |l| l.elevation)
}

/// A massing model: the outline with its data, and the solid as a mesh.
pub(super) fn massing(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool, m: &Massing) {
    let (verts, _) = bulged_vertices(&m.footprint, true, cx.tol);
    lwpolyline(w, cx, o, layer, hidden, &verts, true);
    xdata(
        w,
        cx,
        &[
            ("kind", "massing".to_string()),
            ("name", m.name.clone()),
            ("height_mm", m.eave_height().to_string()),
            ("floors", m.floor_count().to_string()),
            ("floor_height_mm", m.floor_height.to_string()),
            ("roof", m.roof.name().to_string()),
            ("pitch_deg", m.pitch_deg.to_string()),
            ("ridge_angle", opt(&m.ridge_angle)),
            ("base_mm", opt(&m.base)),
        ],
    );
    if m.problem().is_some() {
        return;
    }
    let solid = m.solid(base_z(cx, o, m), true);
    if solid.tris.is_empty() {
        return;
    }
    let vertices: Vec<DVec3> = solid.positions;
    native::mesh(w, cx, o, layer, hidden, &vertices, &solid.tris, None);
}

/// A plant tag: the leader, the ID bubble and the text lines.
pub(super) fn plant_tag(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool) {
    let Shape::PlantTag {
        at, anchor, height, ..
    } = &o.shape
    else {
        return;
    };
    common(w, "LINE", layer, o, hidden);
    w.pair(100, "AcDbLine");
    cx.pt3(w, 10, anchor.x, anchor.y, 0.0);
    cx.pt3(w, 11, at.x, at.y, 0.0);
    let text = |w: &mut W, cx: &Ctx, p: DVec2, s: &str| {
        common(w, "TEXT", layer, o, hidden);
        w.pair(100, "AcDbText");
        cx.pt3(w, 10, p.x, p.y, 0.0);
        w.num(40, height / cx.unit);
        w.pair(1, escape_unicode(s).replace('\r', " "));
        w.pair(100, "AcDbText");
    };
    let info = kadakas_core::plants::tag::tag_info_with(cx.doc, o.id, &cx.tag_counts);
    let Some(layout) = info.map(|t| t.layout(*height)) else {
        // The style is gone: the same mark as in the drawing.
        text(w, cx, *at, "?");
        return;
    };
    if let Some((c, r)) = layout.bubble {
        circle(w, cx, o, layer, hidden, *at + c, r);
    }
    for t in &layout.texts {
        let p = *at + t.at;
        let x = if t.centered {
            p.x - text_width(&t.text, *height) / 2.0
        } else {
            p.x
        };
        // Centered text is centered on its middle line.
        let y = if t.centered { p.y - height / 2.0 } else { p.y };
        text(w, cx, DVec2::new(x, y), &t.text);
    }
}
