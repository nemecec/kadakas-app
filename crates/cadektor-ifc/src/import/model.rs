//! The structure around the geometry: spatial tree, presentation layers, colors, georeference and
//! property sets. One pass over the relationship entities builds all of it.

use std::collections::{HashMap, HashSet};

use cadektor_core::georef::Georeference;
use glam::DVec2;

use super::ctx::{Ctx, attr, enum_of, int, items, num, text};
use crate::step::Value;

/// Names of the IFC product classes that the importer looks at (mixed case, as in the schema).
const PRODUCT_CLASSES: &str = "IfcActuator IfcAirTerminal IfcAirTerminalBox IfcAirToAirHeatRecovery IfcAlarm IfcAnnotation \
IfcAudioVisualAppliance IfcBeam IfcBoiler IfcBuilding IfcBuildingElementPart IfcBuildingElementProxy IfcBuildingStorey IfcBurner \
IfcCableCarrierFitting IfcCableCarrierSegment IfcCableFitting IfcCableSegment IfcCaissonFoundation IfcChiller IfcChimney \
IfcCivilElement IfcCoil IfcColumn IfcColumnStandardCase IfcCommunicationsAppliance IfcCompressor IfcCondenser IfcController \
IfcConveyorSegment IfcCooledBeam IfcCoolingTower IfcCourse IfcCovering IfcCurtainWall IfcDamper IfcDiscreteAccessory \
IfcDistributionChamberElement IfcDistributionElement IfcDoor IfcDoorStandardCase IfcDuctFitting IfcDuctSegment IfcDuctSilencer \
IfcEarthworksCut IfcEarthworksElement IfcEarthworksFill IfcElectricAppliance IfcElectricDistributionBoard \
IfcElectricFlowStorageDevice IfcElectricGenerator IfcElectricMotor IfcElectricTimeControl IfcElementAssembly IfcEngine \
IfcEvaporativeCooler IfcEvaporator IfcExternalSpatialElement IfcFan IfcFastener IfcFilter IfcFireSuppressionTerminal \
IfcFlowInstrument IfcFlowMeter IfcFooting IfcFurnishingElement IfcFurniture IfcGeographicElement IfcGeotechnicalAssembly \
IfcGeotechnicalStratum IfcGrid IfcHeatExchanger IfcHumidifier IfcImpactProtectionDevice IfcInterceptor IfcJunctionBox IfcKerb IfcLamp \
IfcLightFixture IfcLiquidTerminal IfcMechanicalFastener IfcMedicalDevice IfcMember IfcMemberStandardCase IfcMooringDevice \
IfcMotorConnection IfcNavigationElement IfcOpeningElement IfcOutlet IfcPavement IfcPile IfcPipeFitting IfcPipeSegment IfcPlate \
IfcPlateStandardCase IfcProtectiveDevice IfcProtectiveDeviceTrippingUnit IfcProxy IfcPump IfcRail IfcRailing IfcRamp IfcRampFlight \
IfcReinforcedSoil IfcReinforcingBar IfcReinforcingMesh IfcRoof IfcSanitaryTerminal IfcSensor IfcShadingDevice IfcSign IfcSignal \
IfcSite IfcSlab IfcSlabElementedCase IfcSlabStandardCase IfcSolarDevice IfcSpace IfcSpaceHeater IfcStackTerminal IfcStair \
IfcStairFlight IfcSurfaceFeature IfcSwitchingDevice IfcSystemFurnitureElement IfcTank IfcTendon IfcTendonAnchor IfcTendonConduit \
IfcTrackElement IfcTransformer IfcTransportElement IfcTubeBundle IfcUnitaryControlElement IfcUnitaryEquipment IfcValve IfcVehicle \
IfcVibrationIsolator IfcVirtualElement IfcWall IfcWallElementedCase IfcWallStandardCase IfcWasteTerminal IfcWindow IfcWindowStandardCase";

/// Spatial structure classes (they contain elements and are not elements themselves).
pub fn is_spatial(upper: &str) -> bool {
    matches!(
        upper,
        "IFCSITE"
            | "IFCBUILDING"
            | "IFCBUILDINGSTOREY"
            | "IFCSPACE"
            | "IFCFACILITY"
            | "IFCFACILITYPART"
    )
}

/// The mixed-case name of an upper-case IFC class (`IFCSLAB` gives `IfcSlab`).
pub fn class_name(upper: &str) -> String {
    thread_local! {
        static NAMES: HashMap<String, &'static str> = PRODUCT_CLASSES
            .split_whitespace()
            .map(|n| (n.to_ascii_uppercase(), n))
            .collect();
    }
    if let Some(n) = NAMES.with(|m| m.get(upper).copied()) {
        return n.to_string();
    }
    let rest = upper
        .strip_prefix("IFC")
        .unwrap_or(upper)
        .to_ascii_lowercase();
    let mut c = rest.chars();
    match c.next() {
        Some(f) => format!("Ifc{}{}", f.to_ascii_uppercase(), c.as_str()),
        None => "Ifc".to_string(),
    }
}

pub fn product_names() -> impl Iterator<Item = String> {
    PRODUCT_CLASSES
        .split_whitespace()
        .map(|n| n.to_ascii_uppercase())
}

/// A property value for a record field.
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: String,
    pub value: serde_json::Value,
    pub ty: cadektor_core::record::FieldType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropSet {
    pub name: String,
    pub props: Vec<Prop>,
}

#[derive(Default)]
pub struct Model {
    /// Element to the spatial element or assembly that contains it.
    pub parent: HashMap<u64, u64>,
    /// Elements that other relationships name (candidates when the class is not in the list).
    pub related: HashSet<u64>,
    /// Property set and quantity set IDs per element.
    pub defs: HashMap<u64, Vec<u64>>,
    /// Presentation layer name per assigned item or representation ID, and whether it is on.
    pub layers: HashMap<u64, (String, bool)>,
    /// Surface color per styled item ID.
    pub colors: HashMap<u64, [u8; 4]>,
    /// Number of mapped items per representation map.
    pub map_uses: HashMap<u64, usize>,
    /// Number of `IfcRelVoidsElement`.
    pub voids: usize,
}

impl Model {
    pub fn load(cx: &Ctx) -> Model {
        let mut m = Model::default();
        let x = cx.x;
        let ids: Vec<(u64, String)> = x
            .iter()
            .filter(|(_, n)| {
                n.starts_with("IFCREL")
                    || matches!(
                        *n,
                        "IFCSTYLEDITEM"
                            | "IFCPRESENTATIONLAYERASSIGNMENT"
                            | "IFCPRESENTATIONLAYERWITHSTYLE"
                            | "IFCMAPPEDITEM"
                    )
            })
            .map(|(i, n)| (i, n.to_string()))
            .collect();
        for (id, name) in ids {
            let Some(e) = cx.ent(id) else { continue };
            match name.as_str() {
                "IFCRELAGGREGATES" => {
                    if let Some(parent) = attr(&e, 4).as_ref_id() {
                        for c in items(attr(&e, 5)) {
                            if let Some(c) = c.as_ref_id() {
                                m.parent.entry(c).or_insert(parent);
                                m.related.insert(c);
                            }
                        }
                    }
                }
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                    if let Some(parent) = attr(&e, 5).as_ref_id() {
                        for c in items(attr(&e, 4)) {
                            if let Some(c) = c.as_ref_id() {
                                m.parent.insert(c, parent);
                                m.related.insert(c);
                            }
                        }
                    }
                }
                "IFCRELDEFINESBYPROPERTIES" => {
                    // IFC4 allows a set of definitions in attribute 5, IFC2x3 one definition.
                    let defs: Vec<u64> = match attr(&e, 5) {
                        Value::Ref(r) => vec![*r],
                        Value::List(l) => l.iter().filter_map(Value::as_ref_id).collect(),
                        _ => vec![],
                    };
                    for o in items(attr(&e, 4)) {
                        if let Some(o) = o.as_ref_id() {
                            m.defs.entry(o).or_default().extend(defs.iter().copied());
                            m.related.insert(o);
                        }
                    }
                }
                "IFCRELDEFINESBYTYPE" => {
                    // Property sets of the type apply to its occurrences.
                    if let Some(t) = cx.get(attr(&e, 5)) {
                        let sets: Vec<u64> = items(attr(&t, 5))
                            .iter()
                            .filter_map(Value::as_ref_id)
                            .collect();
                        for o in items(attr(&e, 4)) {
                            if let Some(o) = o.as_ref_id() {
                                m.defs.entry(o).or_default().extend(sets.iter().copied());
                                m.related.insert(o);
                            }
                        }
                    }
                }
                "IFCRELVOIDSELEMENT" => m.voids += 1,
                "IFCPRESENTATIONLAYERASSIGNMENT" | "IFCPRESENTATIONLAYERWITHSTYLE" => {
                    let Some(n) = text(attr(&e, 0)).map(str::trim).filter(|n| !n.is_empty()) else {
                        continue;
                    };
                    let on = name != "IFCPRESENTATIONLAYERWITHSTYLE"
                        || !matches!(attr(&e, 4), Value::Bool(false));
                    for a in items(attr(&e, 2)) {
                        if let Some(a) = a.as_ref_id() {
                            m.layers.entry(a).or_insert((n.to_string(), on));
                        }
                    }
                }
                "IFCSTYLEDITEM" => {
                    if let Some(item) = attr(&e, 0).as_ref_id()
                        && let Some(c) = find_color(cx, attr(&e, 1), 0)
                    {
                        m.colors.entry(item).or_insert(c);
                    }
                }
                "IFCMAPPEDITEM" => {
                    if let Some(s) = attr(&e, 0).as_ref_id() {
                        *m.map_uses.entry(s).or_default() += 1;
                    }
                }
                _ => {}
            }
        }
        m
    }

    /// The nearest `IfcBuildingStorey` above an element.
    pub fn storey_of(&self, cx: &Ctx, id: u64) -> Option<u64> {
        let mut cur = id;
        for _ in 0..32 {
            let p = *self.parent.get(&cur)?;
            if cx.x.name_of(p) == Some("IFCBUILDINGSTOREY") {
                return Some(p);
            }
            cur = p;
        }
        None
    }

    /// The property sets and quantity sets of an element.
    pub fn property_sets(&self, cx: &Ctx, id: u64) -> Vec<PropSet> {
        let mut out: Vec<PropSet> = Vec::new();
        for d in self.defs.get(&id).into_iter().flatten().take(200) {
            let Some(e) = cx.ent(*d) else { continue };
            let Some(name) = text(attr(&e, 2)).map(str::trim).filter(|n| !n.is_empty()) else {
                continue;
            };
            let name = name.strip_prefix("ePset_").unwrap_or(name).to_string();
            let mut props = Vec::new();
            match e.name.as_str() {
                "IFCPROPERTYSET" => {
                    for p in cx.get_all(attr(&e, 4)) {
                        read_property(cx, &p, "", &mut props, 0);
                    }
                }
                "IFCELEMENTQUANTITY" => {
                    for q in cx.get_all(attr(&e, 5)) {
                        if let Some(p) = read_quantity(cx, &q) {
                            props.push(p);
                        }
                    }
                }
                _ => continue,
            }
            if props.is_empty() {
                continue;
            }
            match out.iter_mut().find(|s| s.name == name) {
                Some(s) => s.props.extend(props),
                None => out.push(PropSet { name, props }),
            }
        }
        out
    }
}

/// The first `IfcColourRgb` below a style value, as RGBA. Transparency lowers the alpha.
fn find_color(cx: &Ctx, v: &Value, depth: usize) -> Option<[u8; 4]> {
    if depth > 6 {
        return None;
    }
    match v {
        Value::List(l) => l.iter().find_map(|x| find_color(cx, x, depth + 1)),
        Value::Ref(_) => {
            let e = cx.get(v)?;
            match e.name.as_str() {
                "IFCCOLOURRGB" => {
                    let c = |i: usize| {
                        num(attr(&e, i)).map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8)
                    };
                    Some([c(1)?, c(2)?, c(3)?, 255])
                }
                "IFCSURFACESTYLE" => find_color(cx, attr(&e, 2), depth + 1),
                "IFCSURFACESTYLERENDERING" | "IFCSURFACESTYLESHADING" => {
                    let mut col = find_color(cx, attr(&e, 0), depth + 1)?;
                    if e.name == "IFCSURFACESTYLERENDERING"
                        && let Some(t) = num(attr(&e, 1))
                    {
                        col[3] = ((1.0 - t.clamp(0.0, 1.0)) * 255.0).round() as u8;
                    }
                    Some(col)
                }
                "IFCPRESENTATIONSTYLEASSIGNMENT" => find_color(cx, attr(&e, 0), depth + 1),
                "IFCSTYLEDITEM" => find_color(cx, attr(&e, 1), depth + 1),
                _ => None,
            }
        }
        _ => None,
    }
}

use cadektor_core::record::FieldType;

fn json_int(i: i64) -> Option<serde_json::Value> {
    (i.unsigned_abs() as f64 <= cadektor_core::record::MAX_FIELD).then(|| serde_json::json!(i))
}

fn json_num(x: f64) -> Option<serde_json::Value> {
    (x.is_finite() && x.abs() <= cadektor_core::record::MAX_FIELD).then(|| serde_json::json!(x))
}

/// A record value from an IFC value. Lengths become millimeters, areas square meters.
fn map_value(cx: &Ctx, v: &Value) -> Option<(serde_json::Value, FieldType)> {
    use serde_json::json;
    let u = &cx.units;
    match v {
        Value::Typed(t, inner) => {
            let t = t.as_str();
            let n = || num(inner);
            match t {
                "IFCLENGTHMEASURE" | "IFCPOSITIVELENGTHMEASURE" | "IFCNONNEGATIVELENGTHMEASURE" => {
                    Some((json_num(n()? * u.length_mm)?, FieldType::Dimension))
                }
                "IFCAREAMEASURE" => Some((json_num(n()? * u.area_m2)?, FieldType::Area)),
                "IFCVOLUMEMEASURE" => Some((json_num(n()? * u.volume_m3)?, FieldType::Volume)),
                "IFCPLANEANGLEMEASURE" | "IFCPOSITIVEPLANEANGLEMEASURE" => Some((
                    json_num((n()? * u.angle_rad).to_degrees())?,
                    FieldType::Angle,
                )),
                "IFCBOOLEAN" | "IFCLOGICAL" => match &**inner {
                    Value::Bool(b) => Some((json!(*b), FieldType::Boolean)),
                    _ => None,
                },
                "IFCINTEGER" => Some((json_int(int(inner)?)?, FieldType::Integer)),
                "IFCLABEL" | "IFCTEXT" | "IFCIDENTIFIER" | "IFCDESCRIPTIVEMEASURE" => {
                    Some((json!(text(inner)?), FieldType::Text))
                }
                _ => match &**inner {
                    Value::Str(s) => Some((json!(s), FieldType::Text)),
                    Value::Enum(s) => Some((json!(s), FieldType::Text)),
                    Value::Bool(b) => Some((json!(*b), FieldType::Boolean)),
                    other => Some((json_num(num(other)?)?, FieldType::Number)),
                },
            }
        }
        Value::Int(i) => Some((json_int(*i)?, FieldType::Integer)),
        Value::Real(x) => Some((json_num(*x)?, FieldType::Number)),
        Value::Str(s) => Some((json!(s), FieldType::Text)),
        Value::Bool(b) => Some((json!(*b), FieldType::Boolean)),
        Value::Enum(s) => Some((json!(s), FieldType::Text)),
        _ => None,
    }
}

fn field_name(prefix: &str, name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    let full = if prefix.is_empty() {
        n.to_string()
    } else {
        format!("{prefix}.{n}")
    };
    let full: String = full.chars().filter(|c| !c.is_control()).take(64).collect();
    (!full.eq_ignore_ascii_case(cadektor_core::record::MARKER)).then_some(full)
}

fn read_property(
    cx: &Ctx,
    p: &crate::step::Entity,
    prefix: &str,
    out: &mut Vec<Prop>,
    depth: usize,
) {
    let Some(name) = field_name(prefix, text(attr(p, 0)).unwrap_or("")) else {
        return;
    };
    let list_text = |v: &Value| -> String {
        items(v)
            .iter()
            .filter_map(|x| match map_value(cx, x)?.0 {
                serde_json::Value::String(s) => Some(s),
                other => Some(other.to_string()),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    match p.name.as_str() {
        "IFCPROPERTYSINGLEVALUE" => {
            if let Some((value, ty)) = map_value(cx, attr(p, 2)) {
                out.push(Prop { name, value, ty });
            }
        }
        "IFCPROPERTYENUMERATEDVALUE" | "IFCPROPERTYLISTVALUE" => {
            let t = list_text(attr(p, 2));
            if !t.is_empty() {
                out.push(Prop {
                    name,
                    value: serde_json::json!(t),
                    ty: FieldType::Text,
                });
            }
        }
        "IFCPROPERTYBOUNDEDVALUE" => {
            let lo = map_value(cx, attr(p, 3));
            let hi = map_value(cx, attr(p, 2));
            let show = |v: Option<(serde_json::Value, FieldType)>| {
                v.map_or(String::new(), |(v, _)| match v {
                    serde_json::Value::String(s) => s,
                    o => o.to_string(),
                })
            };
            out.push(Prop {
                name,
                value: serde_json::json!(format!("{} .. {}", show(lo), show(hi))),
                ty: FieldType::Text,
            });
        }
        "IFCCOMPLEXPROPERTY" if depth < 3 => {
            for sub in cx.get_all(attr(p, 3)) {
                read_property(cx, &sub, &name, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn read_quantity(cx: &Ctx, q: &crate::step::Entity) -> Option<Prop> {
    let name = field_name("", text(attr(q, 0))?)?;
    let v = num(attr(q, 3))?;
    let u = &cx.units;
    let (value, ty) = match q.name.as_str() {
        "IFCQUANTITYLENGTH" => (v * u.length_mm, FieldType::Dimension),
        "IFCQUANTITYAREA" => (v * u.area_m2, FieldType::Area),
        "IFCQUANTITYVOLUME" => (v * u.volume_m3, FieldType::Volume),
        "IFCQUANTITYCOUNT" => (v, FieldType::Number),
        "IFCQUANTITYWEIGHT" | "IFCQUANTITYTIME" => (v, FieldType::Number),
        _ => return None,
    };
    Some(Prop {
        name,
        value: json_num(value)?,
        ty,
    })
}

/// What the file says about its place on the earth.
#[derive(Clone, Debug, PartialEq)]
pub struct FileGeo {
    pub crs: String,
    /// Eastings and northings in meters.
    pub origin: DVec2,
    /// Angle of the model X axis against the map X axis.
    pub rotation: f64,
    /// Meters per millimeter of the model, as the file says.
    pub meters_per_mm: f64,
    pub height: f64,
}

/// Reads `IfcMapConversion` and `IfcProjectedCRS`.
pub fn file_georef(cx: &Ctx) -> Option<FileGeo> {
    let id =
        cx.x.ids_of("IFCMAPCONVERSION")
            .into_iter()
            .chain(cx.x.ids_of("IFCMAPCONVERSIONSCALED"))
            .next()?;
    let e = cx.ent(id)?;
    let crs_e = cx.get(attr(&e, 1));
    let (name, to_m) = match &crs_e {
        Some(c) => {
            let label = |i: usize| text(attr(c, i)).map(str::trim).filter(|t| !t.is_empty());
            let name = label(0).or(label(1)).unwrap_or("").to_string();
            // The map unit: a length unit of the CRS (attribute 6). Meters when missing.
            let to_m = cx
                .get(attr(c, 6))
                .and_then(|u| {
                    let prefix = enum_of(attr(&u, 2)).map_or(1.0, super::ctx::prefix_factor);
                    match u.name.as_str() {
                        "IFCSIUNIT" => Some(prefix),
                        "IFCCONVERSIONBASEDUNIT" => {
                            let m = cx.get(attr(&u, 3))?;
                            let base = cx.get(attr(&m, 1))?;
                            let bp = enum_of(attr(&base, 2)).map_or(1.0, super::ctx::prefix_factor);
                            Some(num(attr(&m, 0))? * bp)
                        }
                        _ => None,
                    }
                })
                .unwrap_or(1.0);
            (name, to_m)
        }
        None => (String::new(), 1.0),
    };
    let east = num(attr(&e, 2))?;
    let north = num(attr(&e, 3))?;
    let height = num(attr(&e, 4)).unwrap_or(0.0);
    let xa = num(attr(&e, 5)).unwrap_or(1.0);
    let xo = num(attr(&e, 6)).unwrap_or(0.0);
    let scale = num(attr(&e, 7)).filter(|s| *s > 0.0).unwrap_or(1.0);
    let origin = DVec2::new(east, north) * to_m;
    if !(origin.is_finite() && origin.abs().max_element() < 1e8 && height.is_finite()) {
        return None;
    }
    let rotation = if xa == 0.0 && xo == 0.0 {
        0.0
    } else {
        xo.atan2(xa)
    };
    // The scale is map units per project unit; one project unit is `length_mm` millimeters.
    let meters_per_mm = scale * to_m / cx.units.length_mm;
    Some(FileGeo {
        crs: name,
        origin,
        rotation,
        meters_per_mm,
        height,
    })
}

/// The georeference that a file asks for, when the CRS is known.
pub fn to_georef(g: &FileGeo) -> Georeference {
    Georeference {
        crs: g.crs.clone(),
        origin: g.origin,
        rotation: g.rotation,
        user_origin: DVec2::ZERO,
    }
}
