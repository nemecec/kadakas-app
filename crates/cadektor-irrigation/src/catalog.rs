//! The irrigation catalog: head models, pipe sizes and pipe materials.
//!
//! A document keeps its catalog as resources of type `irrigation_catalog` (JSON). A built-in
//! catalog with generic parts is always there. Entries of the document catalogs replace built-in
//! entries of the same name (heads) or the same size and material (pipes).
//!
//! Units: radius and diameter in mm, flow in l/min, pressure in kPa, price per piece or per m.
//!
//! CSV import reuses the reader of the plant catalog. A file with semicolons as delimiters and
//! decimal commas (`1,5`) is read too. Names that appear twice in one file are reported by
//! `duplicate_heads`, `duplicate_pipes` and `duplicate_materials` (the later row wins). The first row names the columns (any
//! order, upper or lower case). Heads: `name, kind, radius, min_arc, max_arc, flow, pressure,
//! min_pressure, price`. Pipes: `size, material, inner_diameter, price`. Materials: `name, c`.

use serde::{Deserialize, Serialize};

use cadektor_core::model::Document;
use cadektor_core::plants::catalog::{parse_csv, write_csv};
use cadektor_core::resource::ResourceBody;

/// The kind of an outlet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadKind {
    #[default]
    Spray,
    Rotor,
    /// A drip emitter. Its flow does not depend on the arc.
    Drip,
    /// A bubbler. Its flow does not depend on the arc.
    Bubbler,
}

impl HeadKind {
    pub fn id(self) -> &'static str {
        match self {
            HeadKind::Spray => "spray",
            HeadKind::Rotor => "rotor",
            HeadKind::Drip => "drip",
            HeadKind::Bubbler => "bubbler",
        }
    }

    pub fn from_name(s: &str) -> Option<HeadKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "spray" | "spray head" | "sprinkler" => Some(HeadKind::Spray),
            "rotor" | "rotary" => Some(HeadKind::Rotor),
            "drip" | "emitter" | "drip emitter" => Some(HeadKind::Drip),
            "bubbler" => Some(HeadKind::Bubbler),
            _ => None,
        }
    }

    /// True if the flow is in proportion to the arc (sprays and rotors with matched precipitation).
    pub fn flow_scales_with_arc(self) -> bool {
        matches!(self, HeadKind::Spray | HeadKind::Rotor)
    }
}

fn d360() -> f64 {
    360.0
}

/// A head model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeadModel {
    pub name: String,
    #[serde(default)]
    pub kind: HeadKind,
    /// Throw radius in mm.
    pub radius: f64,
    /// Smallest and largest arc in degrees.
    #[serde(default)]
    pub min_arc: f64,
    #[serde(default = "d360")]
    pub max_arc: f64,
    /// Flow of a full circle (360 degrees) at the rated pressure, in l/min.
    pub flow: f64,
    /// Rated operating pressure at the head, in kPa.
    pub pressure: f64,
    /// Lowest pressure at which the head works well, in kPa. Zero means 80 percent of `pressure`.
    #[serde(default)]
    pub min_pressure: f64,
    #[serde(default)]
    pub price: f64,
}

impl HeadModel {
    /// Lowest pressure at the head, in kPa.
    pub fn min_required(&self) -> f64 {
        if self.min_pressure > 0.0 {
            self.min_pressure
        } else {
            0.8 * self.pressure
        }
    }

    /// Flow in l/min for a spray angle in degrees. A spray or rotor uses `flow * arc / 360`.
    pub fn flow_at_arc(&self, arc_deg: f64) -> f64 {
        if self.kind.flow_scales_with_arc() {
            self.flow * arc_deg.clamp(0.0, 360.0) / 360.0
        } else {
            self.flow
        }
    }

    fn validate(&self) -> Result<(), String> {
        let n = &self.name;
        if n.trim().is_empty() {
            return Err("a head model needs a name".into());
        }
        let nums = [
            self.radius,
            self.min_arc,
            self.max_arc,
            self.flow,
            self.pressure,
            self.min_pressure,
            self.price,
        ];
        if nums.iter().any(|x| !x.is_finite()) {
            return Err(format!("head '{n}' has a number that is not finite"));
        }
        if self.radius <= 0.0 || self.flow <= 0.0 || self.pressure <= 0.0 {
            return Err(format!(
                "head '{n}': radius, flow and pressure must be positive"
            ));
        }
        if self.min_arc < 0.0 || self.max_arc > 360.0 || self.min_arc > self.max_arc {
            return Err(format!(
                "head '{n}': arcs must satisfy 0 <= min_arc <= max_arc <= 360"
            ));
        }
        if self.min_pressure < 0.0 || self.price < 0.0 {
            return Err(format!(
                "head '{n}': pressure and price must not be negative"
            ));
        }
        Ok(())
    }
}

/// A pipe size of a material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipeSize {
    /// Nominal size label, for example `32`.
    pub size: String,
    pub material: String,
    /// Inner diameter in mm.
    pub inner_diameter: f64,
    /// Price per m.
    #[serde(default)]
    pub price: f64,
}

impl PipeSize {
    fn validate(&self) -> Result<(), String> {
        if self.size.trim().is_empty() || self.material.trim().is_empty() {
            return Err("a pipe size needs a size and a material".into());
        }
        if !self.inner_diameter.is_finite() || self.inner_diameter <= 0.0 {
            return Err(format!(
                "pipe {} {}: the inner diameter must be positive",
                self.material, self.size
            ));
        }
        if !self.price.is_finite() || self.price < 0.0 {
            return Err(format!(
                "pipe {} {}: the price must not be negative",
                self.material, self.size
            ));
        }
        Ok(())
    }
}

/// A pipe material with its Hazen-Williams roughness coefficient `c` (higher is smoother).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipeMaterial {
    pub name: String,
    pub c: f64,
}

/// Heads, pipes and materials.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub heads: Vec<HeadModel>,
    #[serde(default)]
    pub pipes: Vec<PipeSize>,
    #[serde(default)]
    pub materials: Vec<PipeMaterial>,
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Largest number of entries per list in one catalog.
pub const MAX_ENTRIES: usize = 5000;

impl Catalog {
    /// The built-in catalog: generic heads, PE and PVC pipes and their materials.
    pub fn builtin() -> Catalog {
        let head = |name: &str, kind, radius, flow, pressure, min_pressure, price| HeadModel {
            name: name.into(),
            kind,
            radius,
            min_arc: 40.0,
            max_arc: 360.0,
            flow,
            pressure,
            min_pressure,
            price,
        };
        let mut heads = vec![
            head("Spray-2.5", HeadKind::Spray, 2500.0, 4.0, 200.0, 140.0, 3.5),
            head("Spray-4.0", HeadKind::Spray, 4000.0, 8.0, 200.0, 140.0, 4.2),
            head("Rotor-8", HeadKind::Rotor, 8000.0, 12.0, 300.0, 250.0, 14.0),
            head(
                "Rotor-12",
                HeadKind::Rotor,
                12000.0,
                24.0,
                350.0,
                280.0,
                22.0,
            ),
        ];
        heads.push(HeadModel {
            name: "Drip-2lph".into(),
            kind: HeadKind::Drip,
            radius: 300.0,
            min_arc: 360.0,
            max_arc: 360.0,
            flow: 2.0 / 60.0,
            pressure: 100.0,
            min_pressure: 70.0,
            price: 0.4,
        });
        heads.push(HeadModel {
            name: "Bubbler-3".into(),
            kind: HeadKind::Bubbler,
            radius: 600.0,
            min_arc: 360.0,
            max_arc: 360.0,
            flow: 3.0,
            pressure: 150.0,
            min_pressure: 100.0,
            price: 2.5,
        });
        let pipe = |size: &str, material: &str, inner_diameter, price| PipeSize {
            size: size.into(),
            material: material.into(),
            inner_diameter,
            price,
        };
        let pipes = vec![
            pipe("16", "PE", 12.4, 0.4),
            pipe("20", "PE", 16.0, 0.6),
            pipe("25", "PE", 20.4, 0.9),
            pipe("32", "PE", 26.2, 1.4),
            pipe("40", "PE", 32.6, 2.1),
            pipe("50", "PE", 40.8, 3.2),
            pipe("63", "PE", 51.4, 5.0),
            pipe("25", "PVC", 21.6, 0.8),
            pipe("32", "PVC", 28.4, 1.2),
            pipe("40", "PVC", 36.4, 1.8),
            pipe("50", "PVC", 45.2, 2.6),
            pipe("63", "PVC", 57.0, 4.0),
            pipe("75", "PVC", 67.8, 5.6),
        ];
        let materials = [
            ("PVC", 150.0),
            ("PE", 140.0),
            ("Copper", 140.0),
            ("Galvanized steel", 120.0),
            ("Cast iron", 130.0),
        ]
        .into_iter()
        .map(|(name, c)| PipeMaterial {
            name: name.into(),
            c,
        })
        .collect();
        Catalog {
            heads,
            pipes,
            materials,
        }
    }

    /// The built-in catalog with the catalogs of the document laid over it.
    pub fn of_document(doc: &Document) -> Catalog {
        let mut c = Catalog::builtin();
        for r in doc.resources.iter() {
            if let ResourceBody::IrrigationCatalog(v) = &r.body
                && let Ok(extra) = Catalog::from_value(v)
            {
                c.merge(&extra);
            }
        }
        c
    }

    /// Adds the entries of `other`. An entry with the same key replaces the old one.
    pub fn merge(&mut self, other: &Catalog) {
        for h in &other.heads {
            match self.heads.iter_mut().find(|x| same(&x.name, &h.name)) {
                Some(x) => *x = h.clone(),
                None => self.heads.push(h.clone()),
            }
        }
        for p in &other.pipes {
            match self
                .pipes
                .iter_mut()
                .find(|x| same(&x.size, &p.size) && same(&x.material, &p.material))
            {
                Some(x) => *x = p.clone(),
                None => self.pipes.push(p.clone()),
            }
        }
        for m in &other.materials {
            match self.materials.iter_mut().find(|x| same(&x.name, &m.name)) {
                Some(x) => *x = m.clone(),
                None => self.materials.push(m.clone()),
            }
        }
    }

    pub fn head(&self, name: &str) -> Option<&HeadModel> {
        self.heads.iter().find(|h| same(&h.name, name))
    }

    pub fn pipe(&self, size: &str, material: &str) -> Option<&PipeSize> {
        self.pipes
            .iter()
            .find(|p| same(&p.size, size) && same(&p.material, material))
    }

    /// Hazen-Williams coefficient of a material.
    pub fn material_c(&self, material: &str) -> Option<f64> {
        self.materials
            .iter()
            .find(|m| same(&m.name, material))
            .map(|m| m.c)
    }

    /// Checks all entries: names, positive numbers, unique keys, a limit on the size.
    pub fn validate(&self) -> Result<(), String> {
        if self.heads.len() > MAX_ENTRIES
            || self.pipes.len() > MAX_ENTRIES
            || self.materials.len() > MAX_ENTRIES
        {
            return Err(format!(
                "a catalog has at most {MAX_ENTRIES} entries per list"
            ));
        }
        for (i, h) in self.heads.iter().enumerate() {
            h.validate()?;
            if self.heads[..i].iter().any(|x| same(&x.name, &h.name)) {
                return Err(format!("head '{}' appears twice", h.name));
            }
        }
        for (i, p) in self.pipes.iter().enumerate() {
            p.validate()?;
            if self.pipes[..i]
                .iter()
                .any(|x| same(&x.size, &p.size) && same(&x.material, &p.material))
            {
                return Err(format!("pipe {} {} appears twice", p.material, p.size));
            }
        }
        for (i, m) in self.materials.iter().enumerate() {
            if m.name.trim().is_empty() || !m.c.is_finite() || m.c <= 0.0 {
                return Err(format!("material '{}': c must be positive", m.name));
            }
            if self.materials[..i].iter().any(|x| same(&x.name, &m.name)) {
                return Err(format!("material '{}' appears twice", m.name));
            }
        }
        Ok(())
    }

    /// The JSON of a catalog resource (with `"type": "irrigation_catalog"`).
    pub fn to_value(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        v["type"] = "irrigation_catalog".into();
        v
    }

    pub fn from_value(v: &serde_json::Value) -> Result<Catalog, String> {
        serde_json::from_value(v.clone()).map_err(|e| e.to_string())
    }

    /// Heads as CSV text.
    pub fn heads_csv(&self) -> String {
        let mut rows = vec![
            "name,kind,radius,min_arc,max_arc,flow,pressure,min_pressure,price"
                .split(',')
                .map(String::from)
                .collect::<Vec<_>>(),
        ];
        for h in &self.heads {
            rows.push(vec![
                h.name.clone(),
                h.kind.id().into(),
                h.radius.to_string(),
                h.min_arc.to_string(),
                h.max_arc.to_string(),
                h.flow.to_string(),
                h.pressure.to_string(),
                h.min_pressure.to_string(),
                h.price.to_string(),
            ]);
        }
        write_csv(&rows)
    }

    /// Pipe materials as CSV text.
    pub fn materials_csv(&self) -> String {
        let mut rows = vec![vec!["name".to_string(), "c".into()]];
        for m in &self.materials {
            rows.push(vec![m.name.clone(), m.c.to_string()]);
        }
        write_csv(&rows)
    }

    /// Pipes as CSV text.
    pub fn pipes_csv(&self) -> String {
        let mut rows = vec![vec![
            "size".to_string(),
            "material".into(),
            "inner_diameter".into(),
            "price".into(),
        ]];
        for p in &self.pipes {
            rows.push(vec![
                p.size.clone(),
                p.material.clone(),
                p.inner_diameter.to_string(),
                p.price.to_string(),
            ]);
        }
        write_csv(&rows)
    }
}

/// True if the header line has semicolons and no commas (a European CSV file).
fn is_semicolon_file(text: &str) -> bool {
    let first = text
        .trim_start_matches('\u{feff}')
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    first.contains(';') && !first.contains(',')
}

/// Reads a file with semicolons as delimiters. A field may be in double quotes.
fn parse_semicolon(text: &str) -> Vec<Vec<String>> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.split(';')
                .map(|f| f.trim().trim_matches('"').replace("\"\"", "\""))
                .collect()
        })
        .collect()
}

/// The columns of a CSV table: header names to indexes.
struct Table {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn parse(text: &str, required: &[&str]) -> Result<Table, String> {
        let mut rows = if is_semicolon_file(text) {
            parse_semicolon(text)
        } else {
            parse_csv(text)?
        };
        if rows.is_empty() {
            return Err("the CSV text is empty".into());
        }
        let header: Vec<String> = rows
            .remove(0)
            .iter()
            .map(|h| h.trim().to_ascii_lowercase().replace([' ', '-'], "_"))
            .collect();
        for r in required {
            if !header.iter().any(|h| h == r) {
                return Err(format!("the CSV header needs a column '{r}'"));
            }
        }
        if rows.len() > MAX_ENTRIES {
            return Err(format!("at most {MAX_ENTRIES} rows"));
        }
        Ok(Table { header, rows })
    }

    fn get<'a>(&self, row: &'a [String], col: &str) -> Option<&'a str> {
        let i = self.header.iter().position(|h| h == col)?;
        row.get(i).map(|s| s.trim()).filter(|s| !s.is_empty())
    }

    fn num(&self, row: &[String], col: &str, line: usize) -> Result<Option<f64>, String> {
        match self.get(row, col) {
            None => Ok(None),
            Some(s) => s
                .parse::<f64>()
                // A decimal comma, as in "1,5".
                .or_else(|_| s.replace(',', ".").parse::<f64>())
                .ok()
                .filter(|x| x.is_finite())
                .map(Some)
                .ok_or_else(|| format!("row {line}: '{s}' is not a number (column {col})")),
        }
    }

    fn need(&self, row: &[String], col: &str, line: usize) -> Result<f64, String> {
        self.num(row, col, line)?
            .ok_or_else(|| format!("row {line}: column '{col}' is empty"))
    }
}

fn duplicates(keys: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen: Vec<String> = vec![];
    let mut out: Vec<String> = vec![];
    for k in keys {
        let low = k.to_ascii_lowercase();
        if seen.contains(&low) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&k)) {
                out.push(k);
            }
        } else {
            seen.push(low);
        }
    }
    out
}

/// Names that appear more than once in a list of heads.
pub fn duplicate_heads(heads: &[HeadModel]) -> Vec<String> {
    duplicates(heads.iter().map(|h| h.name.trim().to_string()))
}

/// Pipes (as "material size") that appear more than once.
pub fn duplicate_pipes(pipes: &[PipeSize]) -> Vec<String> {
    duplicates(
        pipes
            .iter()
            .map(|p| format!("{} {}", p.material.trim(), p.size.trim())),
    )
}

/// Names that appear more than once in a list of materials.
pub fn duplicate_materials(materials: &[PipeMaterial]) -> Vec<String> {
    duplicates(materials.iter().map(|m| m.name.trim().to_string()))
}

/// Reads head models from CSV text.
pub fn import_heads_csv(text: &str) -> Result<Vec<HeadModel>, String> {
    let t = Table::parse(text, &["name", "radius", "flow", "pressure"])?;
    let mut out = Vec::new();
    for (i, row) in t.rows.iter().enumerate() {
        let line = i + 2;
        let name = t
            .get(row, "name")
            .ok_or_else(|| format!("row {line}: the name is empty"))?;
        let kind = match t.get(row, "kind") {
            None => HeadKind::Spray,
            Some(k) => {
                HeadKind::from_name(k).ok_or_else(|| format!("row {line}: unknown kind '{k}'"))?
            }
        };
        let h = HeadModel {
            name: name.to_string(),
            kind,
            radius: t.need(row, "radius", line)?,
            min_arc: t.num(row, "min_arc", line)?.unwrap_or(0.0),
            max_arc: t.num(row, "max_arc", line)?.unwrap_or(360.0),
            flow: t.need(row, "flow", line)?,
            pressure: t.need(row, "pressure", line)?,
            min_pressure: t.num(row, "min_pressure", line)?.unwrap_or(0.0),
            price: t.num(row, "price", line)?.unwrap_or(0.0),
        };
        h.validate().map_err(|e| format!("row {line}: {e}"))?;
        out.push(h);
    }
    Ok(out)
}

/// Reads pipe sizes from CSV text.
pub fn import_pipes_csv(text: &str) -> Result<Vec<PipeSize>, String> {
    let t = Table::parse(text, &["size", "material", "inner_diameter"])?;
    let mut out = Vec::new();
    for (i, row) in t.rows.iter().enumerate() {
        let line = i + 2;
        let p = PipeSize {
            size: t
                .get(row, "size")
                .ok_or_else(|| format!("row {line}: the size is empty"))?
                .to_string(),
            material: t
                .get(row, "material")
                .ok_or_else(|| format!("row {line}: the material is empty"))?
                .to_string(),
            inner_diameter: t.need(row, "inner_diameter", line)?,
            price: t.num(row, "price", line)?.unwrap_or(0.0),
        };
        p.validate().map_err(|e| format!("row {line}: {e}"))?;
        out.push(p);
    }
    Ok(out)
}

/// Reads pipe materials from CSV text.
pub fn import_materials_csv(text: &str) -> Result<Vec<PipeMaterial>, String> {
    let t = Table::parse(text, &["name", "c"])?;
    let mut out = Vec::new();
    for (i, row) in t.rows.iter().enumerate() {
        let line = i + 2;
        let c = t.need(row, "c", line)?;
        if c <= 0.0 {
            return Err(format!("row {line}: c must be positive"));
        }
        out.push(PipeMaterial {
            name: t
                .get(row, "name")
                .ok_or_else(|| format!("row {line}: the name is empty"))?
                .to_string(),
            c,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_is_valid_and_complete() {
        let c = Catalog::builtin();
        c.validate().unwrap();
        assert!(c.head("spray-4.0").is_some());
        assert!(c.head("nope").is_none());
        assert_eq!(c.pipe(" 32 ", "pe").unwrap().inner_diameter, 26.2);
        assert!(c.pipe("32", "steel").is_none());
        assert_eq!(c.material_c("pvc"), Some(150.0));
        assert_eq!(c.material_c("x"), None);
        // Every pipe has a material with a coefficient.
        assert!(c.pipes.iter().all(|p| c.material_c(&p.material).is_some()));
    }

    #[test]
    fn head_flow_scales_with_arc() {
        let c = Catalog::builtin();
        let spray = c.head("Spray-4.0").unwrap();
        // Full circle 8 l/min: a quarter circle uses 2 l/min.
        assert_eq!(spray.flow_at_arc(360.0), 8.0);
        assert_eq!(spray.flow_at_arc(90.0), 2.0);
        assert_eq!(spray.flow_at_arc(1000.0), 8.0);
        let bubbler = c.head("Bubbler-3").unwrap();
        assert_eq!(bubbler.flow_at_arc(90.0), 3.0);
        assert_eq!(spray.min_required(), 140.0);
        let mut h = spray.clone();
        h.min_pressure = 0.0;
        assert_eq!(h.min_required(), 160.0);
    }

    #[test]
    fn merge_replaces_by_key() {
        let mut c = Catalog::builtin();
        let n = c.heads.len();
        let mut extra = Catalog::default();
        let mut h = c.head("Spray-4.0").unwrap().clone();
        h.name = "SPRAY-4.0".into();
        h.flow = 9.0;
        extra.heads.push(h);
        extra.heads.push(HeadModel {
            name: "New".into(),
            ..c.heads[0].clone()
        });
        extra.pipes.push(PipeSize {
            size: "32".into(),
            material: "pe".into(),
            inner_diameter: 25.0,
            price: 1.0,
        });
        extra.pipes.push(PipeSize {
            size: "90".into(),
            material: "PE".into(),
            inner_diameter: 70.0,
            price: 1.0,
        });
        extra.materials.push(PipeMaterial {
            name: "PE".into(),
            c: 145.0,
        });
        extra.materials.push(PipeMaterial {
            name: "Lead".into(),
            c: 130.0,
        });
        c.merge(&extra);
        assert_eq!(c.heads.len(), n + 1);
        assert_eq!(c.head("Spray-4.0").unwrap().flow, 9.0);
        assert_eq!(c.pipe("32", "PE").unwrap().inner_diameter, 25.0);
        assert!(c.pipe("90", "PE").is_some());
        assert_eq!(c.material_c("PE"), Some(145.0));
        assert_eq!(c.material_c("Lead"), Some(130.0));
    }

    #[test]
    fn json_round_trip_and_document_overlay() {
        let c = Catalog::builtin();
        let v = c.to_value();
        assert_eq!(v["type"], "irrigation_catalog");
        assert_eq!(Catalog::from_value(&v).unwrap(), c);
        assert!(Catalog::from_value(&serde_json::json!({"heads": 5})).is_err());

        let mut doc = Document::default();
        let extra = Catalog {
            heads: vec![HeadModel {
                name: "Mine".into(),
                kind: HeadKind::Rotor,
                radius: 9000.0,
                min_arc: 0.0,
                max_arc: 360.0,
                flow: 10.0,
                pressure: 300.0,
                min_pressure: 0.0,
                price: 0.0,
            }],
            ..Catalog::default()
        };
        doc.resources.push(cadektor_core::resource::Resource {
            id: cadektor_core::resource::ResourceId(900),
            name: "My heads".into(),
            body: ResourceBody::IrrigationCatalog(extra.to_value()),
        });
        // A broken catalog resource is ignored.
        doc.resources.push(cadektor_core::resource::Resource {
            id: cadektor_core::resource::ResourceId(901),
            name: "Broken".into(),
            body: ResourceBody::IrrigationCatalog(serde_json::json!({"heads": 3})),
        });
        let all = Catalog::of_document(&doc);
        assert!(all.head("Mine").is_some() && all.head("Spray-2.5").is_some());
    }

    #[test]
    fn validate_catches_errors() {
        let base = Catalog::builtin();
        let bad = |f: &dyn Fn(&mut Catalog), needle: &str| {
            let mut c = base.clone();
            f(&mut c);
            let e = c.validate().unwrap_err();
            assert!(e.contains(needle), "{e} should contain {needle}");
        };
        bad(&|c| c.heads[0].name = " ".into(), "name");
        bad(&|c| c.heads[0].radius = f64::NAN, "finite");
        bad(&|c| c.heads[0].flow = 0.0, "positive");
        bad(&|c| c.heads[0].min_arc = 400.0, "arcs");
        bad(&|c| c.heads[0].price = -1.0, "negative");
        bad(&|c| c.heads[1].name = "spray-2.5".into(), "twice");
        bad(&|c| c.pipes[0].size = "".into(), "size");
        bad(&|c| c.pipes[0].inner_diameter = 0.0, "inner diameter");
        bad(&|c| c.pipes[0].price = -2.0, "price");
        bad(&|c| c.pipes[1] = c.pipes[0].clone(), "twice");
        bad(&|c| c.materials[0].c = 0.0, "positive");
        bad(&|c| c.materials[1].name = "pvc".into(), "twice");
        bad(
            &|c| {
                c.heads = vec![c.heads[0].clone(); MAX_ENTRIES + 1];
            },
            "at most",
        );
    }

    #[test]
    fn heads_csv_round_trip() {
        let c = Catalog::builtin();
        let back = import_heads_csv(&c.heads_csv()).unwrap();
        assert_eq!(back, c.heads);
        let back = import_pipes_csv(&c.pipes_csv()).unwrap();
        assert_eq!(back, c.pipes);
    }

    #[test]
    fn csv_columns_in_any_order_and_defaults() {
        let h = import_heads_csv("Pressure,FLOW,Radius,Name,Kind\n250,6,3000,\"A, b\",Rotary\n")
            .unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].name, "A, b");
        assert_eq!(h[0].kind, HeadKind::Rotor);
        assert_eq!((h[0].min_arc, h[0].max_arc, h[0].price), (0.0, 360.0, 0.0));
        let m = import_materials_csv("name,c\nPVC,150\nPE,140\n").unwrap();
        assert_eq!(m.len(), 2);
        let p = import_pipes_csv("size,material,inner diameter\n32,PE,26.2\n").unwrap();
        assert_eq!(p[0].inner_diameter, 26.2);
    }

    #[test]
    fn csv_errors_name_the_row() {
        let e = import_heads_csv("name,radius,flow\nA,1,2\n").unwrap_err();
        assert!(e.contains("pressure"), "{e}");
        let e = import_heads_csv("name,radius,flow,pressure\nA,x,2,3\n").unwrap_err();
        assert!(e.contains("row 2") && e.contains("not a number"), "{e}");
        let e = import_heads_csv("name,radius,flow,pressure\nA,,2,3\n").unwrap_err();
        assert!(e.contains("empty"), "{e}");
        let e = import_heads_csv("name,radius,flow,pressure,kind\nA,1,2,3,laser\n").unwrap_err();
        assert!(e.contains("unknown kind"), "{e}");
        let e = import_heads_csv("name,radius,flow,pressure\n,1,2,3\n").unwrap_err();
        assert!(e.contains("name is empty"), "{e}");
        let e = import_heads_csv("name,radius,flow,pressure\nA,-1,2,3\n").unwrap_err();
        assert!(e.contains("row 2") && e.contains("positive"), "{e}");
        assert!(import_heads_csv("").is_err());
        let e = import_pipes_csv("size,material,inner_diameter\n,PE,3\n").unwrap_err();
        assert!(e.contains("size is empty"), "{e}");
        let e = import_pipes_csv("size,material,inner_diameter\n1,,3\n").unwrap_err();
        assert!(e.contains("material is empty"), "{e}");
        let e = import_pipes_csv("size,material,inner_diameter\n1,PE,0\n").unwrap_err();
        assert!(e.contains("positive"), "{e}");
        let e = import_materials_csv("name,c\nX,0\n").unwrap_err();
        assert!(e.contains("positive"), "{e}");
        let e = import_materials_csv("name,c\n,5\n").unwrap_err();
        assert!(e.contains("name is empty"), "{e}");
        let many = format!("name,c\n{}", "X,5\n".repeat(MAX_ENTRIES + 1));
        assert!(import_materials_csv(&many).unwrap_err().contains("at most"));
    }

    #[test]
    fn head_kinds() {
        for k in [
            HeadKind::Spray,
            HeadKind::Rotor,
            HeadKind::Drip,
            HeadKind::Bubbler,
        ] {
            assert_eq!(HeadKind::from_name(k.id()), Some(k));
        }
        assert_eq!(HeadKind::from_name("Emitter"), Some(HeadKind::Drip));
        assert_eq!(HeadKind::from_name("sprinkler"), Some(HeadKind::Spray));
        assert!(HeadKind::Rotor.flow_scales_with_arc());
        assert!(!HeadKind::Drip.flow_scales_with_arc());
    }

    #[test]
    fn semicolon_files_with_decimal_commas() {
        let h = import_heads_csv(
            "\u{feff}name;radius;flow;pressure;price\n\"Euro, spray\";3000;5,5;200;2,25\n",
        )
        .unwrap();
        assert_eq!(h[0].name, "Euro, spray");
        assert_eq!((h[0].flow, h[0].price), (5.5, 2.25));
        // In a comma file a quoted decimal comma works too.
        let h = import_heads_csv("name,radius,flow,pressure\nA,3000,\"5,5\",200\n").unwrap();
        assert_eq!(h[0].flow, 5.5);
        let p = import_pipes_csv("size;material;inner_diameter\n32;PE;26,2\n").unwrap();
        assert_eq!(p[0].inner_diameter, 26.2);
        let m = import_materials_csv("name;c\nPE;140,5\n").unwrap();
        assert_eq!(m[0].c, 140.5);
        assert!(import_heads_csv("name;radius;flow;pressure\nA;x;1;1\n").is_err());
    }

    #[test]
    fn duplicates_are_found_and_materials_export() {
        let h = import_heads_csv(
            "name,radius,flow,pressure\nA,1000,1,100\na,2000,2,100\nB,1000,1,100\nA,3000,3,100\n",
        )
        .unwrap();
        assert_eq!(duplicate_heads(&h), ["a"]);
        assert!(duplicate_heads(&h[2..3]).is_empty());
        let p = import_pipes_csv("size,material,inner_diameter\n32,PE,26\n32,pe,27\n25,PE,20\n")
            .unwrap();
        assert_eq!(duplicate_pipes(&p), ["pe 32"]);
        let m = import_materials_csv("name,c\nPE,140\npe,141\n").unwrap();
        assert_eq!(duplicate_materials(&m), ["pe"]);
        let c = Catalog::builtin();
        let csv = c.materials_csv();
        assert!(csv.starts_with("name,c\n"));
        assert_eq!(import_materials_csv(&csv).unwrap(), c.materials);
    }
}
