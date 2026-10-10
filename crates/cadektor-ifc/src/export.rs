//! The IFC4 writer: spatial structure, elements, bodies, styles and data.
//!
//! The file is written in one pass. Every entity goes to the output when it is made, so memory use
//! does not grow with the size of the model. Only the IDs of the elements are kept, to write the
//! relationships at the end.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

mod elements;
mod finish;

use cadektor_core::curve;
use cadektor_core::datamap::{self, DataMap, IfcType, IfcValue, Mapped};
use cadektor_core::hardscape::{self, TerrainSource, surface};
use cadektor_core::landscape;
use cadektor_core::model::{Class, ClassId, Document, LayerId, LayerKind, NONE_CLASS, Object, Shape, arc_sweep};
use cadektor_core::plants::style::PlantCategory;
use cadektor_core::plants::{link, tree};
use cadektor_core::props::{self, Prop};
use cadektor_core::record;
use cadektor_core::resource::ResourceBody;
use cadektor_io::Report;
use cadektor_site::{SiteCache, SiteResult, SiteTerrain, Surface};
use glam::{DVec2, DVec3};
use serde_json::Value as Json;

use crate::geo::{self, GeoInfo};
use crate::geom::{self, r4};
use crate::guid;
use crate::options::IfcOptions;
use crate::step::{StepWriter, Value};
use crate::IfcError;

type R<T> = Result<T, IfcError>;

/// Why an object was not written, or `None` when it was.
type Outcome = R<Option<String>>;

fn skip(reason: &str) -> Outcome {
    Ok(Some(reason.to_string()))
}

const DEFAULT_GREEN: [u8; 4] = [70, 130, 60, 255];
const TRUNK_BROWN: [u8; 4] = [105, 80, 55, 255];
const TERRAIN_BROWN: [u8; 4] = [150, 130, 100, 255];
const MIN_SIZE: f64 = 10.0;

/// The IFC classes that the writer can make, with their entity names and predefined types.
const CLASSES: [(&str, &str, &[&str]); 4] = [
    ("IfcSlab", "IFCSLAB", &["FLOOR", "ROOF", "LANDING", "BASESLAB", "USERDEFINED", "NOTDEFINED"]),
    (
        "IfcCovering",
        "IFCCOVERING",
        &["CEILING", "FLOORING", "CLADDING", "ROOFING", "MOLDING", "SKIRTINGBOARD", "INSULATION", "MEMBRANE", "SLEEVING", "WRAPPING", "USERDEFINED", "NOTDEFINED"],
    ),
    ("IfcGeographicElement", "IFCGEOGRAPHICELEMENT", &["TERRAIN", "USERDEFINED", "NOTDEFINED"]),
    ("IfcBuildingElementProxy", "IFCBUILDINGELEMENTPROXY", &["COMPLEX", "ELEMENT", "PARTIAL", "PROVISIONFORVOID", "PROVISIONFORSPACE", "USERDEFINED", "NOTDEFINED"]),
];

/// The class and predefined type of an object kind when the data map says nothing.
/// IFC4 has no VEGETATION type (IFC4x3 has): vegetation is a user-defined geographic element with the object type `VEGETATION`.
fn default_class(kind: &str) -> (&'static str, Option<&'static str>) {
    match kind {
        "hardscape" => ("IfcSlab", Some("BASESLAB")),
        "site_model" => ("IfcGeographicElement", Some("TERRAIN")),
        _ => ("IfcGeographicElement", Some("USERDEFINED")),
    }
}

/// The built-in mapping: the property sets of the landscape template, and the default classes.
pub fn builtin_map() -> DataMap {
    DataMap { entities: Vec::new(), rules: DataMap::landscape_template().rules }
}

/// A body: its shape representations, and the main one for layer assignment.
#[derive(Default)]
struct Body {
    reps: Vec<u64>,
    main: Option<u64>,
}

/// What an element needs to be written.
struct ElementIn<'o> {
    obj: &'o Object,
    kind: &'static str,
    name: String,
    placement: u64,
    body: Body,
}

pub struct Exporter<'a, W: Write> {
    doc: &'a Document,
    opts: &'a IfcOptions,
    map: DataMap,
    ground: SiteTerrain<'a>,
    w: StepWriter<W>,
    pub report: Report,
    project: String,
    // Spatial structure.
    context: u64,
    sub_contexts: BTreeMap<&'static str, u64>,
    site: u64,
    site_placement: u64,
    building: Option<(u64, u64)>,
    storeys: BTreeMap<LayerId, (u64, u64)>,
    /// Elements by container: 0 is the site, else a layer ID.
    contained: BTreeMap<u64, Vec<u64>>,
    // Shared entities.
    styles: BTreeMap<[u8; 4], u64>,
    materials: BTreeMap<String, u64>,
    layer_sets: BTreeMap<String, (u64, Vec<u64>)>,
    class_layers: BTreeMap<ClassId, Vec<u64>>,
    notes: BTreeSet<String>,
    /// The element entity of each exported object.
    elements: BTreeMap<u64, u64>,
    up: Option<u64>,
    /// Properties of the element being written, by property set name. Sets that come from several
    /// sources (standard, Data Manager, records) merge here, so one set is written once.
    pending: BTreeMap<String, Vec<(String, Value)>>,
}

impl<'a, W: Write> Exporter<'a, W> {
    pub fn new(doc: &'a Document, opts: &'a IfcOptions, project: String, cache: &'a SiteCache, out: W) -> R<Self> {
        let map = pick_map(doc, opts)?;
        let p = &doc.drawing.project;
        let time = opts.time_stamp.clone().unwrap_or_else(|| default_time(&p.date));
        let name = opts.file_name.clone().unwrap_or_else(|| "model.ifc".into());
        let w = StepWriter::begin(out, &name, &time, &p.drawn_by, &p.client, "ViewDefinition [ReferenceView_V1.2]")?;
        Ok(Exporter {
            doc,
            opts,
            map,
            ground: SiteTerrain::new(cache, 0, doc),
            w,
            report: Report::new("ifc"),
            project,
            context: 0,
            sub_contexts: BTreeMap::new(),
            site: 0,
            site_placement: 0,
            building: None,
            storeys: BTreeMap::new(),
            contained: BTreeMap::new(),
            styles: BTreeMap::new(),
            materials: BTreeMap::new(),
            layer_sets: BTreeMap::new(),
            class_layers: BTreeMap::new(),
            notes: BTreeSet::new(),
            elements: BTreeMap::new(),
            up: None,
            pending: BTreeMap::new(),
        })
    }

    /// Writes the whole file and returns the output and the report.
    pub fn run(mut self) -> R<(W, Report)> {
        self.header_structure()?;
        let doc = self.doc;
        for obj in doc.objects.values() {
            let reason = if doc.layer(obj.layer).is_none_or(|l| l.kind != LayerKind::Design) {
                Some("on a sheet layer (paper space)".to_string())
            } else if self.opts.visible_only && !doc.is_visible(obj) {
                Some("hidden".to_string())
            } else {
                self.object(obj)?
            };
            match reason {
                None => self.report.count(obj.shape.kind()),
                Some(r) => self.report.skip(obj.shape.kind(), &r),
            }
        }
        self.flush()?;
        let out = self.w.finish()?;
        Ok((out, self.report))
    }

    // ------------------------------------------------------------ low level

    fn ent(&mut self, name: &str, attrs: Vec<Value>) -> R<u64> {
        Ok(self.w.entity(name, &attrs)?)
    }

    fn note(&mut self, text: String) {
        if self.notes.insert(text.clone()) {
            self.report.notes.push(text);
        }
    }

    fn pt3(&mut self, p: [f64; 3]) -> R<u64> {
        self.ent("IFCCARTESIANPOINT", vec![Value::reals(&p.map(r4))])
    }

    fn pt2(&mut self, p: DVec2) -> R<u64> {
        self.ent("IFCCARTESIANPOINT", vec![Value::reals(&[r4(p.x), r4(p.y)])])
    }

    fn axis3(&mut self, loc: [f64; 3]) -> R<u64> {
        let p = self.pt3(loc)?;
        self.ent("IFCAXIS2PLACEMENT3D", vec![Value::Ref(p), Value::Null, Value::Null])
    }

    fn placement(&mut self, rel: Option<u64>, loc: [f64; 3]) -> R<u64> {
        let a = self.axis3(loc)?;
        self.ent("IFCLOCALPLACEMENT", vec![rel.map_or(Value::Null, Value::Ref), Value::Ref(a)])
    }

    fn guid(&self, tag: &str, id: u64) -> Value {
        Value::Str(guid::derive(&self.project, tag, id))
    }

    fn name_value(s: &str) -> Value {
        if s.trim().is_empty() { Value::Null } else { Value::s(s.trim()) }
    }

    // ------------------------------------------------------------ project, site, georeference

    fn header_structure(&mut self) -> R<()> {
        let doc = self.doc;
        let geo = doc.georef.as_ref().and_then(GeoInfo::from_georef);
        let si = |w: &mut Self, t: &str, prefix: Value, name: &str| w.ent("IFCSIUNIT", vec![Value::Derived, Value::e(t), prefix, Value::e(name)]);
        let units = [
            si(self, "LENGTHUNIT", Value::e("MILLI"), "METRE")?,
            si(self, "AREAUNIT", Value::Null, "SQUARE_METRE")?,
            si(self, "VOLUMEUNIT", Value::Null, "CUBIC_METRE")?,
            si(self, "PLANEANGLEUNIT", Value::Null, "RADIAN")?,
        ];
        let unit_set = self.ent("IFCUNITASSIGNMENT", vec![Value::refs(units)])?;

        let wcs = self.axis3([0.0; 3])?;
        let north = match &geo {
            Some(g) => Value::Ref(self.ent("IFCDIRECTION", vec![Value::reals(&g.true_north)])?),
            None => Value::Null,
        };
        self.context = self.ent("IFCGEOMETRICREPRESENTATIONCONTEXT", vec![Value::Null, Value::s("Model"), Value::Int(3), Value::Real(1.0e-3), Value::Ref(wcs), north])?;

        if let Some(g) = &geo {
            let metre = self.ent("IFCSIUNIT", vec![Value::Derived, Value::e("LENGTHUNIT"), Value::Null, Value::e("METRE")])?;
            let crs = self.ent(
                "IFCPROJECTEDCRS",
                vec![
                    Value::s(&g.crs_name),
                    g.description.as_ref().map_or(Value::Null, Value::s),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Ref(metre),
                ],
            )?;
            let real = |x: f64| Value::Real(if x.is_finite() { x } else { 0.0 });
            self.ent(
                "IFCMAPCONVERSION",
                vec![Value::Ref(self.context), Value::Ref(crs), real(g.eastings), real(g.northings), Value::Real(0.0), real(g.x_abscissa), real(g.x_ordinate), real(g.scale)],
            )?;
            self.report.georeferenced = true;
            if g.lat_lon.is_none() {
                self.note(format!("latitude and longitude of the site could not be derived from the CRS '{}'", g.crs_name));
            }
        } else if let Some(g) = &doc.georef {
            self.note(format!("the georeference has no CRS (origin shift {}, {}), so no map conversion is written", g.origin.x, g.origin.y));
        }

        let p = &doc.drawing.project;
        let project_name = if p.name.trim().is_empty() { "Cadektor project" } else { p.name.trim() };
        let project = self.ent(
            "IFCPROJECT",
            vec![
                self.guid("project", 0),
                Value::Null,
                Value::s(project_name),
                Self::name_value(&p.address),
                Value::Null,
                Self::name_value(&p.client),
                Self::name_value(&p.revision),
                Value::refs([self.context]),
                Value::Ref(unit_set),
            ],
        )?;

        self.site_placement = self.placement(None, [0.0; 3])?;
        let (lat, lon) = match geo.as_ref().and_then(|g| g.lat_lon) {
            Some((lat, lon)) => (Value::List(geo::compound_angle(lat).map(Value::Int).to_vec()), Value::List(geo::compound_angle(lon).map(Value::Int).to_vec())),
            None => (Value::Null, Value::Null),
        };
        let site_name = self.site_name();
        self.site = self.ent(
            "IFCSITE",
            vec![
                self.guid("site", 0),
                Value::Null,
                Value::s(site_name),
                Value::Null,
                Value::Null,
                Value::Ref(self.site_placement),
                Value::Null,
                Value::Null,
                Value::e("ELEMENT"),
                lat,
                lon,
                Value::Real(0.0),
                Value::Null,
                Value::Null,
            ],
        )?;
        self.ent("IFCRELAGGREGATES", vec![self.guid("rel_project", 0), Value::Null, Value::Null, Value::Null, Value::Ref(project), Value::refs([self.site])])?;
        Ok(())
    }

    fn site_name(&self) -> String {
        let p = self.doc.drawing.project.name.trim();
        if p.is_empty() { "Site".into() } else { format!("{p} site") }
    }

    /// The container of the elements of a layer and its placement. Layers become storeys when asked.
    fn container(&mut self, layer: LayerId) -> R<(u64, u64, u64)> {
        if !self.opts.layers_as_storeys {
            return Ok((0, self.site, self.site_placement));
        }
        if let Some((id, pl)) = self.storeys.get(&layer) {
            return Ok((layer.0, *id, *pl));
        }
        let (_, building_placement) = match self.building {
            Some(b) => b,
            None => {
                let pl = self.placement(Some(self.site_placement), [0.0; 3])?;
                let b = self.ent(
                    "IFCBUILDING",
                    vec![
                        self.guid("building", 0),
                        Value::Null,
                        Value::s("Building"),
                        Value::Null,
                        Value::Null,
                        Value::Ref(pl),
                        Value::Null,
                        Value::Null,
                        Value::e("ELEMENT"),
                        Value::Null,
                        Value::Null,
                        Value::Null,
                    ],
                )?;
                self.building = Some((b, pl));
                (b, pl)
            }
        };
        let l = self.doc.layer(layer).expect("layer of an exported object");
        let pl = self.placement(Some(building_placement), [0.0, 0.0, l.elevation])?;
        let storey = self.ent(
            "IFCBUILDINGSTOREY",
            vec![
                self.guid("storey", layer.0),
                Value::Null,
                Value::s(l.name.trim()),
                Value::Null,
                Value::Null,
                Value::Ref(pl),
                Value::Null,
                Value::Null,
                Value::e("ELEMENT"),
                Value::Real(r4(l.elevation)),
            ],
        )?;
        self.storeys.insert(layer, (storey, pl));
        Ok((layer.0, storey, pl))
    }

    /// The Z of the placement of an element on a layer: the layer elevation, unless a storey carries it.
    fn base_z(&self, layer: LayerId) -> f64 {
        if self.opts.layers_as_storeys { 0.0 } else { self.doc.layer(layer).map_or(0.0, |l| l.elevation) }
    }

    fn sub_context(&mut self, id: &'static str, kind: &str, view: &str) -> R<u64> {
        if let Some(c) = self.sub_contexts.get(id) {
            return Ok(*c);
        }
        let c = self.ent(
            "IFCGEOMETRICREPRESENTATIONSUBCONTEXT",
            vec![Value::s(id), Value::s(kind), Value::Derived, Value::Derived, Value::Derived, Value::Derived, Value::Ref(self.context), Value::Null, Value::e(view), Value::Null],
        )?;
        self.sub_contexts.insert(id, c);
        Ok(c)
    }

    fn shape_rep(&mut self, ctx: &'static str, kind: &str, view: &str, ident: &str, rep_type: &str, items: &[u64]) -> R<u64> {
        let sub = self.sub_context(ctx, kind, view)?;
        self.ent("IFCSHAPEREPRESENTATION", vec![Value::Ref(sub), Value::s(ident), Value::s(rep_type), Value::refs(items.iter().copied())])
    }

    fn body_rep(&mut self, rep_type: &str, items: &[u64]) -> R<u64> {
        self.shape_rep("Body", "Model", "MODEL_VIEW", "Body", rep_type, items)
    }

    // ------------------------------------------------------------ styles and materials

    fn style_items(&mut self, items: &[u64], color: [u8; 4]) -> R<()> {
        let style = match self.styles.get(&color) {
            Some(s) => *s,
            None => {
                let c = |v: u8| Value::Real(r4(v as f64 / 255.0));
                let rgb = self.ent("IFCCOLOURRGB", vec![Value::Null, c(color[0]), c(color[1]), c(color[2])])?;
                let shading = self.ent("IFCSURFACESTYLESHADING", vec![Value::Ref(rgb), Value::Real(r4(1.0 - color[3] as f64 / 255.0))])?;
                let s = self.ent("IFCSURFACESTYLE", vec![Value::s(format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])), Value::e("BOTH"), Value::refs([shading])])?;
                self.styles.insert(color, s);
                s
            }
        };
        for item in items {
            self.ent("IFCSTYLEDITEM", vec![Value::Ref(*item), Value::refs([style]), Value::Null])?;
        }
        Ok(())
    }

    fn material(&mut self, name: &str) -> R<u64> {
        let name = if name.trim().is_empty() { "Unspecified" } else { name.trim() };
        if let Some(m) = self.materials.get(name) {
            return Ok(*m);
        }
        let m = self.ent("IFCMATERIAL", vec![Value::s(name), Value::Null, Value::Null])?;
        self.materials.insert(name.to_string(), m);
        Ok(m)
    }

    /// Associates a material layer set with an element. `usage` makes a layer set usage (top down), which
    /// needs a body that is an extrusion from the top surface.
    fn associate_layers(&mut self, element: u64, label: &str, layers: &[(String, f64)], usage: bool) -> R<()> {
        let key = format!("{usage}|{label}|{}", layers.iter().map(|(m, t)| format!("{m}={t}")).collect::<Vec<_>>().join(";"));
        if let Some((_, elements)) = self.layer_sets.get_mut(&key) {
            elements.push(element);
            return Ok(());
        }
        let mut ids = Vec::new();
        for (material, thickness) in layers {
            let m = self.material(material)?;
            ids.push(self.ent("IFCMATERIALLAYER", vec![Value::Ref(m), Value::Real(r4(*thickness)), Value::Bool(false), Value::s(material.trim()), Value::Null, Value::Null, Value::Null])?);
        }
        let set = self.ent("IFCMATERIALLAYERSET", vec![Value::refs(ids), Self::name_value(label), Value::Null])?;
        let target = if usage {
            self.ent("IFCMATERIALLAYERSETUSAGE", vec![Value::Ref(set), Value::e("AXIS3"), Value::e("NEGATIVE"), Value::Real(0.0), Value::Null])?
        } else {
            set
        };
        self.layer_sets.insert(key, (target, vec![element]));
        Ok(())
    }

    // ------------------------------------------------------------ properties and quantities

    fn nominal(v: &IfcValue) -> Value {
        let real = |name: &str| Value::typed(name, Value::Real(v.value.as_f64().unwrap_or(0.0)));
        match v.ifc_type {
            IfcType::Label => Value::typed("IFCLABEL", Value::s(v.value.as_str().unwrap_or_default())),
            IfcType::Text => Value::typed("IFCTEXT", Value::s(v.value.as_str().unwrap_or_default())),
            IfcType::Real => real("IFCREAL"),
            IfcType::Integer => Value::typed("IFCINTEGER", Value::Int(v.value.as_i64().unwrap_or(0))),
            IfcType::Boolean => Value::typed("IFCBOOLEAN", Value::Bool(v.value.as_bool().unwrap_or(false))),
            IfcType::Length => real("IFCLENGTHMEASURE"),
            IfcType::Area => real("IFCAREAMEASURE"),
            IfcType::Volume => real("IFCVOLUMEMEASURE"),
        }
    }

    /// Adds properties to a property set of the current element. A property that is there is replaced.
    fn pset(&mut self, name: &str, props: Vec<(String, Value)>) {
        if props.is_empty() {
            return;
        }
        let set = self.pending.entry(name.to_string()).or_default();
        for (n, v) in props {
            match set.iter_mut().find(|(k, _)| *k == n) {
                Some(slot) => slot.1 = v,
                None => set.push((n, v)),
            }
        }
    }

    /// Writes the collected property sets of an element, one `IfcPropertySet` per name.
    fn write_psets(&mut self, owner: &Object) -> R<()> {
        for (name, props) in std::mem::take(&mut self.pending) {
            let mut ids = Vec::new();
            for (n, v) in props {
                ids.push(self.ent("IFCPROPERTYSINGLEVALUE", vec![Value::s(n), Value::Null, v, Value::Null])?);
            }
            let set = self.ent("IFCPROPERTYSET", vec![self.guid(&format!("pset:{name}"), owner.id.0), Value::Null, Value::s(&name), Value::Null, Value::refs(ids)])?;
            self.define_by(owner, &name, set)?;
        }
        Ok(())
    }

    fn define_by(&mut self, owner: &Object, name: &str, definition: u64) -> R<()> {
        let element = self.elements.get(&owner.id.0).copied().expect("element is written before its data");
        self.ent(
            "IFCRELDEFINESBYPROPERTIES",
            vec![self.guid(&format!("rel_pset:{name}"), owner.id.0), Value::Null, Value::Null, Value::Null, Value::refs([element]), Value::Ref(definition)],
        )?;
        Ok(())
    }
}

fn default_time(date: &str) -> String {
    let d = date.trim();
    let b = d.as_bytes();
    if b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() }) {
        format!("{d}T00:00:00")
    } else {
        "2000-01-01T00:00:00".into()
    }
}

fn pick_map(doc: &Document, opts: &IfcOptions) -> R<DataMap> {
    let maps = || doc.resources.iter().filter_map(|r| if let ResourceBody::DataMap(m) = &r.body { Some((r.name.as_str(), m.as_ref())) } else { None });
    match &opts.data_map {
        Some(name) => maps()
            .find(|(n, _)| n.trim().eq_ignore_ascii_case(name.trim()))
            .map(|(_, m)| m.clone())
            .ok_or_else(|| IfcError::Option(format!("the document has no data map named '{name}'"))),
        None => Ok(maps().next().map_or_else(builtin_map, |(_, m)| m.clone())),
    }
}
