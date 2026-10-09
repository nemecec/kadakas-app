//! Landscape areas: planted regions with a style (a plant mix), excluded holes, quantities per
//! plant, a deterministic 2D scatter of plant symbols, and a plant list that adds the plants of
//! the areas to the single plants of the drawing.
//!
//! The landscape area style is a resource. An area keeps its settings in
//! `Object::data["landscape_area"]`. An area without this key is a plain outline with a name:
//! files from before landscape area styles load as these.

pub mod plant_list;
pub mod style;

use std::fmt::Write;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use plant_list::{PlantList, PlantListRow, plant_list, plant_list_csv};
pub use style::{LandscapeAreaStyle, MixEntry};

use crate::curve::{self, Vertex};
use crate::geom;
use crate::hardscape::surface::{self, DRAPE_STEP, Terrain};
use crate::mesh::SceneSink;
use crate::model::{Color, Document, MAX_COORD, Object, ObjectId, Shape};
use crate::plants::graphic;
use crate::plants::style::{PlantCategory, PlantGraphic, QuantityType};
use crate::resource::ResourceId;

/// The key in `Object::data` that holds the settings.
pub const KEY: &str = "landscape_area";
/// Most holes in one area.
pub const MAX_HOLES: usize = 1000;
/// Most plant symbols that one area draws in 2D.
pub const MAX_SCATTER: usize = 1500;
/// An area with at most this many symbols draws the real plant graphic. Larger areas draw circles.
const DETAILED_SYMBOLS: usize = 300;
const TOLERANCE: f64 = 1.0;
/// Default fill of an area in 3D.
const DEFAULT_COLOR: [u8; 4] = [128, 176, 104, 255];
const EDGE_COLOR: [u8; 4] = [45, 48, 56, 255];
/// How far above the ground a draped area lies, in mm. This avoids z-fighting with the terrain.
const LIFT: f64 = 5.0;

/// The settings of a landscape area.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LandscapeData {
    pub style: Option<ResourceId>,
    /// Draw the plants. `None` follows the style.
    pub show_plants: Option<bool>,
    /// Parts of the area that stay empty, for example a pond or a tree pit.
    pub holes: Vec<Vec<Vertex>>,
    /// Follow the site model on the layer.
    pub drape: bool,
}

/// The settings of a landscape area object. `None` for other objects and for settings that
/// cannot be read.
pub fn data(obj: &Object) -> Option<LandscapeData> {
    if !matches!(obj.shape, Shape::LandscapeArea { .. }) {
        return None;
    }
    serde_json::from_value(obj.data.get(KEY)?.clone()).ok()
}

/// The ID of the landscape area style that an object refers to, without checking that it exists.
pub fn style_id(obj: &Object) -> Option<ResourceId> {
    obj.data.get(KEY)?.get("style")?.as_u64().map(ResourceId)
}

/// The style of a landscape area, if the area has one and the style exists.
pub fn style_of(doc: &Document, obj: &Object) -> Option<(ResourceId, LandscapeAreaStyle)> {
    let id = data(obj)?.style?;
    Some((id, doc.resources.landscape_area_style(id)?.clone()))
}

pub(crate) fn set_data(obj: &mut Object, d: &LandscapeData) {
    obj.data.insert(KEY.into(), serde_json::to_value(d).expect("settings are JSON"));
}

/// Moves the holes with the object.
pub fn translate_data(obj: &mut Object, delta: DVec2) {
    let Some(mut d) = data(obj) else { return };
    if d.holes.is_empty() {
        return;
    }
    for v in d.holes.iter_mut().flatten() {
        v.p += delta;
    }
    set_data(obj, &d);
}

/// A change of landscape area settings. Missing fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LandscapePatch {
    pub style: Option<ResourceId>,
    pub unstyle: bool,
    pub show_plants: Option<bool>,
    pub holes: Option<Vec<Vec<Vertex>>>,
    pub drape: Option<bool>,
    pub name: Option<String>,
}

fn inside_or_on(ring: &[DVec2], p: DVec2) -> bool {
    let n = ring.len();
    geom::point_in_polygon(p, ring) || (0..n).any(|i| geom::dist_to_segment(p, ring[i], ring[(i + 1) % n]) < 1e-6)
}

/// Applies a patch to a landscape area object. The style must exist (the caller checks).
/// Nothing changes on error.
pub fn apply_patch(obj: &mut Object, patch: &LandscapePatch) -> Result<(), String> {
    let Shape::LandscapeArea { boundary, .. } = &obj.shape else { return Err(format!("object {} is not a landscape area", obj.id)) };
    if obj.data.get(KEY).is_some_and(|v| !matches!(v, Value::Null)) && data(obj).is_none() {
        return Err(format!("object {} has landscape area data that cannot be read", obj.id));
    }
    if patch.unstyle && patch.style.is_some() {
        return Err("give a style or unstyle, not both".into());
    }
    let mut d = data(obj).unwrap_or_default();
    if patch.unstyle {
        d.style = None;
    }
    if let Some(s) = patch.style {
        d.style = Some(s);
    }
    if let Some(v) = patch.show_plants {
        d.show_plants = Some(v);
    }
    if let Some(v) = patch.drape {
        d.drape = v;
    }
    if let Some(h) = &patch.holes {
        if h.len() > MAX_HOLES {
            return Err(format!("an area has at most {MAX_HOLES} holes"));
        }
        let ring = curve::flatten(boundary, true, TOLERANCE);
        for (i, hole) in h.iter().enumerate() {
            if hole.len() < 3 {
                return Err(format!("hole {} needs 3 or more points", i + 1));
            }
            if hole.iter().any(|v| !v.is_finite() || v.p.abs().max_element() > MAX_COORD) {
                return Err("numbers must be finite".into());
            }
            if !curve::flatten(hole, true, TOLERANCE).iter().all(|p| inside_or_on(&ring, *p)) {
                return Err(format!("hole {} is not inside the area", i + 1));
            }
        }
        d.holes = h.clone();
    }
    if let Some(n) = &patch.name
        && let Shape::LandscapeArea { name, .. } = &mut obj.shape
    {
        *name = n.clone();
    }
    set_data(obj, &d);
    Ok(())
}

/// The quantity of one plant of the mix in an area.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlantQty {
    pub plant: ResourceId,
    pub name: String,
    pub id_code: String,
    pub category: PlantCategory,
    pub percent: f64,
    pub density_per_m2: f64,
    /// The spacing that goes with the density, for a square layout.
    pub spacing_mm: f64,
    pub min_count: u32,
    pub count: usize,
    /// The part of the area that this plant covers, in m2.
    pub covered_area_m2: f64,
    pub quantity: QuantityType,
    pub unit_price: f64,
    /// For a plant counted by area, the price per m2 times the covered area. Else price times count.
    pub total_price: f64,
}

/// The quantities of a landscape area.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaQuantities {
    pub gross_area_m2: f64,
    pub holes_area_m2: f64,
    /// The area that is planted: gross minus holes.
    pub net_area_m2: f64,
    pub rows: Vec<PlantQty>,
}

/// Quantities of the plants of an area. Plants of the mix whose style no longer exists are skipped.
pub fn quantities(doc: &Document, obj: &Object) -> Option<AreaQuantities> {
    let Shape::LandscapeArea { boundary, .. } = &obj.shape else { return None };
    let gross = curve::area(boundary) / 1e6;
    let d = data(obj).unwrap_or_default();
    let holes: f64 = d.holes.iter().map(|h| curve::area(h) / 1e6).sum();
    let net = (gross - holes).max(0.0);
    let mut rows = Vec::new();
    if let Some((_, s)) = style_of(doc, obj) {
        for e in &s.mix {
            let Some(p) = doc.resources.plant_style(e.plant) else { continue };
            let name = doc.resources.get(e.plant).map(|r| r.name.clone()).unwrap_or_default();
            let density = e.density_per_m2(p.spread);
            let covered = net * e.percent / 100.0;
            let count = ((covered * density).round().min(1.0e9) as usize).max(e.min_count as usize);
            let total_price = if p.quantity == QuantityType::Area { p.price * covered } else { p.price * count as f64 };
            rows.push(PlantQty {
                plant: e.plant,
                name,
                id_code: p.id_code.clone(),
                category: p.category,
                percent: e.percent,
                density_per_m2: density,
                spacing_mm: 1000.0 / density.sqrt(),
                min_count: e.min_count,
                count,
                covered_area_m2: covered,
                quantity: p.quantity,
                unit_price: p.price,
                total_price,
            });
        }
    }
    Some(AreaQuantities { gross_area_m2: gross, holes_area_m2: holes.min(gross), net_area_m2: net, rows })
}

/// The area of the draped surface over the whole outline (holes included), in m2.
pub fn draped_area_m2(obj: &Object, terrain: Terrain<'_>) -> Option<f64> {
    let Shape::LandscapeArea { boundary, .. } = &obj.shape else { return None };
    let ring = curve::flatten(boundary, true, TOLERANCE);
    let z = |p: DVec2| terrain(p).unwrap_or(0.0);
    let (points, indices) = surface::top_surface(&ring, &z, Some(DRAPE_STEP));
    let plan = surface::plan_mesh_area(&points, &indices);
    let ratio = if plan > 0.0 { surface::mesh_area(&points, &indices) / plan } else { 1.0 };
    Some(curve::area(boundary) / 1e6 * ratio)
}

/// A small deterministic random generator (splitmix64), so a scatter is the same every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number from -1 to 1.
    fn signed(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A plant symbol at a position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub plant: ResourceId,
    pub at: DVec2,
}

/// Positions of the plants of an area for the 2D drawing: at most [`MAX_SCATTER`] symbols,
/// evenly spread, inside the area and outside the holes. The same document gives the same
/// positions. When the area has more plants, every plant style gets its share of the symbols.
pub fn scatter(doc: &Document, obj: &Object) -> Vec<Placed> {
    let Some(q) = quantities(doc, obj) else { return Vec::new() };
    let Shape::LandscapeArea { boundary, .. } = &obj.shape else { return Vec::new() };
    let total: usize = q.rows.iter().map(|r| r.count).sum();
    if total == 0 || q.net_area_m2 <= 0.0 {
        return Vec::new();
    }
    let ring = curve::flatten(boundary, true, TOLERANCE);
    let holes: Vec<Vec<DVec2>> = data(obj).unwrap_or_default().holes.iter().map(|h| curve::flatten(h, true, TOLERANCE)).collect();
    let want = total.min(MAX_SCATTER);
    let cell = (q.net_area_m2 * 1e6 / want as f64).sqrt();
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in &ring {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let mut rng = Rng(obj.id.0.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0xA5A5);
    let (nx, ny) = (((hi.x - lo.x) / cell).ceil() as usize + 1, ((hi.y - lo.y) / cell).ceil() as usize + 1);
    if nx.saturating_mul(ny) > 4 * MAX_SCATTER * 40 {
        return Vec::new();
    }
    let mut candidates: Vec<DVec2> = Vec::new();
    for i in 0..nx {
        for j in 0..ny {
            let jitter = DVec2::new(rng.signed(), rng.signed()) * 0.35 * cell;
            let p = lo + DVec2::new(i as f64 + 0.5, j as f64 + 0.5) * cell + jitter;
            if geom::point_in_polygon(p, &ring) && !holes.iter().any(|h| geom::point_in_polygon(p, h)) {
                candidates.push(p);
            }
        }
    }
    for i in (1..candidates.len()).rev() {
        candidates.swap(i, rng.below(i + 1));
    }
    let scale = (want.min(candidates.len()) as f64) / total as f64;
    let mut out = Vec::new();
    let mut next = 0;
    for r in &q.rows {
        let take = ((r.count as f64 * scale).round() as usize).min(candidates.len() - next);
        out.extend(candidates[next..next + take].iter().map(|p| Placed { plant: r.plant, at: *p }));
        next += take;
    }
    out
}

fn css(c: Option<Color>, fallback: &str) -> String {
    c.map_or_else(|| fallback.to_string(), Color::to_css)
}

fn path_d(points: &[DVec2], closed: bool) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        let _ = write!(d, "{} {} {} ", if i == 0 { "M" } else { "L" }, p.x, -p.y);
    }
    if closed {
        d.push('Z');
    }
    d.trim_end().to_string()
}

/// True if the area draws its plants: the setting of the area, else the setting of the style.
pub fn shows_plants(doc: &Document, obj: &Object) -> bool {
    let Some(d) = data(obj) else { return false };
    d.show_plants.unwrap_or_else(|| style_of(doc, obj).is_some_and(|(_, s)| s.show_plants))
}

/// Extra SVG elements of a landscape area, drawn after its outline: the holes and the plants.
pub fn svg_extras(doc: &Document, obj: &Object, scale: f64, stroke: &str) -> String {
    let mut out = String::new();
    let Some(d) = data(obj) else { return out };
    for hole in &d.holes {
        let ring = curve::flatten(hole, true, TOLERANCE);
        let _ = writeln!(
            out,
            r#"<path d="{}" fill="white" fill-opacity="0.9" stroke="{stroke}" stroke-width="{}" stroke-dasharray="{} {}" data-part="hole" data-of="{}"/>"#,
            path_d(&ring, true),
            0.18 * scale,
            2.0 * scale,
            scale,
            obj.id
        );
    }
    if !shows_plants(doc, obj) {
        return out;
    }
    let placed = scatter(doc, obj);
    if placed.is_empty() {
        return out;
    }
    let _ = writeln!(out, r#"<g data-part="plants" data-of="{}" data-count="{}">"#, obj.id, placed.len());
    let detailed = placed.len() <= DETAILED_SYMBOLS;
    for p in &placed {
        let Some(s) = doc.resources.plant_style(p.plant) else { continue };
        let fill = css(s.fill, "rgba(90,150,80,1)");
        let pen = css(s.pen, stroke);
        let sw = 0.1 * scale;
        let r = s.spread / 2.0;
        if detailed && s.graphic != PlantGraphic::Circle {
            let d = graphic::paths(s.graphic, p.at, r).iter().map(|g| path_d(&g.points, g.closed)).collect::<Vec<_>>().join(" ");
            let _ = writeln!(out, r#"<path d="{d}" fill="{fill}" fill-opacity="0.35" stroke="{pen}" stroke-width="{sw}" data-plant="{}"/>"#, p.plant);
        } else {
            let _ = writeln!(out, r#"<circle cx="{}" cy="{}" r="{r}" fill="{fill}" fill-opacity="0.35" stroke="{pen}" stroke-width="{sw}" data-plant="{}"/>"#, p.at.x, -p.at.y, p.plant);
        }
        if let Some(zone) = s.protection_radius() {
            let _ = writeln!(
                out,
                r#"<circle cx="{}" cy="{}" r="{zone}" fill="none" stroke="{pen}" stroke-width="{sw}" stroke-dasharray="{} {}" data-part="rpz"/>"#,
                p.at.x,
                -p.at.y,
                4.0 * scale,
                2.0 * scale
            );
        }
    }
    out.push_str("</g>\n");
    out
}

/// Adds the draped face of a landscape area to a sink. `terrain` is the ground.
pub fn emit_object(doc: &Document, obj: &Object, terrain: Option<Terrain<'_>>, sink: &mut SceneSink<'_>) {
    let Shape::LandscapeArea { boundary, .. } = &obj.shape else { return };
    let attrs = crate::hardscape::styled_attrs(doc, obj, doc.resolved_attrs(obj));
    let color = attrs.fill.filter(|c| c.0[3] > 0).map_or(DEFAULT_COLOR, |c| c.0);
    let edge = attrs.pen.filter(|c| c.0[3] > 0).map_or(EDGE_COLOR, |c| c.0);
    let ring = curve::flatten(boundary, true, 2.0);
    let z = |p: DVec2| terrain.and_then(|t| t(p)).map_or(0.0, |g| g + LIFT);
    let (points, indices) = surface::top_surface(&ring, &z, terrain.map(|_| DRAPE_STEP));
    sink.mesh(&points, &indices, color);
    let n = ring.len();
    let mut outline: Vec<DVec3> = Vec::new();
    for i in 0..n {
        let (p, q) = (ring[i], ring[(i + 1) % n]);
        let parts = terrain.map_or(1, |_| ((p.distance(q) / DRAPE_STEP).ceil() as usize).clamp(1, 400));
        for k in 0..parts {
            let a = p.lerp(q, k as f64 / parts as f64);
            outline.push(a.extend(z(a)));
        }
    }
    sink.polyline(&outline, true, edge);
}

/// Everything about a landscape area as the report lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaRow {
    pub id: ObjectId,
    pub name: String,
    pub style: Option<ResourceId>,
    pub style_name: Option<String>,
    pub fields: std::collections::BTreeMap<String, String>,
    pub holes: usize,
    /// Area of the draped surface over the whole outline, when the area drapes and there is ground.
    pub surface_area_m2: Option<f64>,
    #[serde(flatten)]
    pub quantities: AreaQuantities,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaReport {
    pub rows: Vec<AreaRow>,
    pub total_gross_area_m2: f64,
    pub total_net_area_m2: f64,
    pub total_count: usize,
    pub total_price: f64,
}

/// The report of all landscape areas, in document order.
pub fn report(doc: &Document, ground: &dyn crate::hardscape::TerrainSource) -> AreaReport {
    let mut rows = Vec::new();
    for obj in doc.objects.values() {
        let Shape::LandscapeArea { name, .. } = &obj.shape else { continue };
        let Some(q) = quantities(doc, obj) else { continue };
        let style = style_of(doc, obj);
        let at = |p: DVec2| ground.elevation(obj, p);
        let first = obj.shape.vertices().and_then(|v| v.first()).map(|v| v.p);
        let has_ground = first.is_some_and(|p| ground.elevation(obj, p).is_some());
        let surface_area_m2 = if data(obj).is_some_and(|d| d.drape) && has_ground { draped_area_m2(obj, &at) } else { None };
        rows.push(AreaRow {
            id: obj.id,
            name: name.clone(),
            style: style.as_ref().map(|(id, _)| *id),
            style_name: style.as_ref().and_then(|(id, _)| doc.resources.get(*id)).map(|r| r.name.clone()),
            fields: style.map(|(_, s)| s.fields).unwrap_or_default(),
            holes: data(obj).map_or(0, |d| d.holes.len()),
            surface_area_m2,
            quantities: q,
        });
    }
    AreaReport {
        total_gross_area_m2: rows.iter().map(|r| r.quantities.gross_area_m2).sum(),
        total_net_area_m2: rows.iter().map(|r| r.quantities.net_area_m2).sum(),
        total_count: rows.iter().flat_map(|r| &r.quantities.rows).map(|p| p.count).sum(),
        total_price: rows.iter().flat_map(|r| &r.quantities.rows).map(|p| p.total_price).sum(),
        rows,
    }
}

#[cfg(test)]
mod tests;
