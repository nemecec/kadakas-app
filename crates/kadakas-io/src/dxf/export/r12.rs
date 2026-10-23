//! DXF release 12 (`AC1009`) for 2D plain entities (IOP-15).
//!
//! Old CAD, CAM and laser programs read this release. It has LINE, CIRCLE, ARC, POINT, TEXT and
//! POLYLINE with VERTEX records (the bulge of a vertex is an arc). It has no handles, no
//! subclass markers, no LWPOLYLINE, no MTEXT, no true color, no line weights, no unit, no
//! georeference, no layouts and no Unicode. The file is in the code page `ANSI_1252`.
//!
//! What cannot be written is not dropped silently. The report counts every object that was left
//! out in `skipped`, and the notes say what changed (names, cut text, characters that the code
//! page has no letter for).

use std::collections::BTreeMap;
use std::fmt::Write;

use kadakas_core::DVec2;
use kadakas_core::model::{Document, Object, Shape, arc_sweep};

use super::{bulged_vertices, rotation_of};
use crate::dxf::nearest_aci;
use crate::options::{ExportOrigin, LayerMode};
use crate::{ExportOptions, ExportResult, IoError, Report};

const REASON: &str = "not in DXF release 12, which holds 2D plain entities only";
/// The longest text of a TEXT entity in release 12.
const TEXT_LIMIT: usize = 250;

/// A layer name of release 12: capital letters, digits, `_`, `-` and `$`, at most 31 characters.
fn r12_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            let c = c.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '$') {
                c
            } else {
                '_'
            }
        })
        .take(31)
        .collect();
    if s.is_empty() { "0".into() } else { s }
}

struct Out<'a> {
    doc: &'a Document,
    mode: LayerMode,
    tol: f64,
    unit: f64,
    buf: String,
    /// R12 layer name to (color index, hidden), and the names that were mapped to it.
    layers: BTreeMap<String, (u8, bool)>,
    originals: BTreeMap<String, Vec<String>>,
    report: Report,
    cut_texts: usize,
}

impl Out<'_> {
    fn pair(&mut self, code: i32, v: impl std::fmt::Display) {
        let _ = write!(self.buf, "{code}\n{v}\n");
    }

    fn num(&mut self, code: i32, v: f64) {
        let v = if v == 0.0 { 0.0 } else { v };
        self.pair(code, v);
    }

    fn pt(&mut self, base: i32, x: f64, y: f64, z: f64) {
        self.num(base, x / self.unit);
        self.num(base + 10, y / self.unit);
        self.num(base + 20, z / self.unit);
    }

    /// The R12 layer of an object, registered with the color and visibility of its class or layer.
    fn layer_of(&mut self, o: &Object) -> String {
        let (name, pen, hidden) = match self.mode {
            LayerMode::Classes => match self.doc.class(o.class) {
                Some(c) if c.name != "None" => (c.name.clone(), c.attrs.pen, !c.visible),
                _ => ("0".to_string(), None, false),
            },
            LayerMode::Layers => match self.doc.layer(o.layer) {
                Some(l) => (l.name.clone(), None, !l.visible),
                None => ("0".to_string(), None, false),
            },
        };
        let short = r12_name(&name);
        let aci = pen
            .filter(|p| p.0[3] > 0)
            .map_or(7, |p| nearest_aci([p.0[0], p.0[1], p.0[2]]));
        self.layers.entry(short.clone()).or_insert((aci, hidden));
        let list = self.originals.entry(short.clone()).or_default();
        if !list.contains(&name) {
            list.push(name);
        }
        short
    }

    /// Group codes common to the entities: the layer and the color of the object.
    fn head(&mut self, kind: &str, o: &Object, layer: &str) {
        self.pair(0, kind);
        self.pair(8, layer);
        if let Some(p) = o.attrs.pen.filter(|p| p.0[3] > 0) {
            self.pair(62, nearest_aci([p.0[0], p.0[1], p.0[2]]));
        }
    }

    fn polyline(
        &mut self,
        o: &Object,
        layer: &str,
        verts: &[(f64, f64, f64, f64)],
        closed: bool,
        flags_3d: bool,
    ) {
        self.head("POLYLINE", o, layer);
        self.pair(66, 1);
        self.pt(10, 0.0, 0.0, 0.0);
        self.pair(70, i32::from(closed) | if flags_3d { 8 } else { 0 });
        for (x, y, z, bulge) in verts {
            self.pair(0, "VERTEX");
            self.pair(8, layer);
            self.pt(10, *x, *y, *z);
            if flags_3d {
                self.pair(70, 32);
            }
            if *bulge != 0.0 {
                self.num(42, *bulge);
            }
        }
        self.pair(0, "SEQEND");
        self.pair(8, layer);
    }

    fn circle(&mut self, o: &Object, layer: &str, c: DVec2, r: f64) {
        self.head("CIRCLE", o, layer);
        self.pt(10, c.x, c.y, 0.0);
        self.num(40, r / self.unit);
    }

    fn text(&mut self, o: &Object, layer: &str, at: DVec2, height: f64, line: &str) {
        let mut line: String = line.replace('\r', " ");
        if line.chars().count() > TEXT_LIMIT {
            line = line.chars().take(TEXT_LIMIT).collect();
            self.cut_texts += 1;
        }
        self.head("TEXT", o, layer);
        self.pt(10, at.x, at.y, 0.0);
        self.num(40, height / self.unit);
        self.pair(1, line);
        if let Some(deg) = rotation_of(o) {
            self.num(50, deg);
        }
    }

    /// Writes an object. Returns false when release 12 cannot hold it.
    fn object(&mut self, o: &Object) -> bool {
        let layer = self.layer_of(o);
        match &o.shape {
            Shape::Group { .. } => {}
            Shape::Symbol { .. } => {
                let doc = self.doc;
                for v in kadakas_core::symbols::instance_objects(doc, o) {
                    if !self.object(&v) {
                        self.report.skip(v.shape.kind(), REASON);
                    }
                }
            }
            Shape::Line { a, b } => {
                self.head("LINE", o, &layer);
                self.pt(10, a.x, a.y, 0.0);
                self.pt(11, b.x, b.y, 0.0);
            }
            Shape::Rect { min, max } => {
                let pts = [
                    (min.x, min.y),
                    (max.x, min.y),
                    (max.x, max.y),
                    (min.x, max.y),
                ];
                let v: Vec<_> = pts.iter().map(|p| (p.0, p.1, 0.0, 0.0)).collect();
                self.polyline(o, &layer, &v, true, false);
            }
            Shape::Circle { center, radius } => self.circle(o, &layer, *center, *radius),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = arc_sweep(*start, *end);
                if sweep >= std::f64::consts::TAU - 1e-12 {
                    self.circle(o, &layer, *center, *radius);
                } else {
                    self.head("ARC", o, &layer);
                    self.pt(10, center.x, center.y, 0.0);
                    self.num(40, radius / self.unit);
                    let a0 = start.to_degrees().rem_euclid(360.0);
                    self.num(50, a0);
                    self.num(51, (a0 + sweep.to_degrees()).rem_euclid(360.0));
                }
            }
            Shape::Polyline { points, closed } => {
                let (verts, closed) = bulged_vertices(points, *closed, self.tol);
                let v: Vec<_> = verts.iter().map(|(p, b)| (p.x, p.y, 0.0, *b)).collect();
                self.polyline(o, &layer, &v, closed, false);
            }
            Shape::LandscapeArea { boundary, .. }
            | Shape::Hardscape { boundary, .. }
            | Shape::SiteModifier { boundary, .. } => {
                let (verts, _) = bulged_vertices(boundary, true, self.tol);
                let v: Vec<_> = verts.iter().map(|(p, b)| (p.x, p.y, 0.0, *b)).collect();
                self.polyline(o, &layer, &v, true, false);
            }
            Shape::Polyline3d { points, closed } => {
                let v: Vec<_> = points.iter().map(|p| (p.x, p.y, p.z, 0.0)).collect();
                self.polyline(o, &layer, &v, *closed, true);
            }
            Shape::Locus3d { p } => {
                self.head("POINT", o, &layer);
                self.pt(10, p.x, p.y, p.z);
            }
            Shape::Text { at, text, height } => {
                // Release 12 has no multiline text: each line is a TEXT below the one before.
                let down = DVec2::from_angle(
                    rotation_of(o).unwrap_or(0.0).to_radians() - std::f64::consts::FRAC_PI_2,
                );
                for (i, line) in text.split('\n').enumerate() {
                    if !line.trim().is_empty() {
                        self.text(
                            o,
                            &layer,
                            *at + down * (*height * 1.5 * i as f64),
                            *height,
                            line,
                        );
                    }
                }
            }
            Shape::Plant { at, spread, .. } | Shape::ExistingTree { at, spread, .. } => {
                self.circle(o, &layer, *at, spread / 2.0);
            }
            _ => return false,
        }
        true
    }
}

/// Windows-1252 bytes of the text. A character that the page has no letter for becomes `?`.
fn to_ansi_1252(text: &str) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(text.len());
    let mut lost = 0;
    let mut buf = [0u8; 4];
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c as u8);
            continue;
        }
        let (bytes, _, bad) = encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf));
        if bad || bytes.len() != 1 {
            out.push(b'?');
            lost += 1;
        } else {
            out.push(bytes[0]);
        }
    }
    (out, lost)
}

pub fn export(doc: &Document, opts: &ExportOptions) -> Result<ExportResult, IoError> {
    let sheets = doc.sheet_layers().count();
    let doc = &*crate::model_only(doc);
    let mut report = Report::new("dxf");
    report.release = Some("AC1009 (release 12)".into());
    if sheets > 0 {
        report.note(&format!(
            "release 12 has no layouts: the {sheets} sheet layers and their objects are not written"
        ));
    }
    let mut shift = DVec2::ZERO;
    if let (Some(g), ExportOrigin::File) = (&doc.georef, opts.origin) {
        shift = g.origin * 1000.0;
        if g.rotation != 0.0 {
            report.note(
                "the georeference is rotated: the rotation is not applied to the coordinates",
            );
        }
    }
    if doc.georef.is_some() {
        report.note("release 12 has no georeference (GEODATA object): the coordinates are real-world numbers without the CRS");
    }
    report.note(&format!(
        "release 12 stores no unit: the coordinates are in {} (option unit)",
        match opts.unit {
            crate::options::Unit::Mm => "millimeters",
            crate::options::Unit::Cm => "centimeters",
            crate::options::Unit::M => "meters",
            crate::options::Unit::Km => "kilometers",
            crate::options::Unit::Inch => "inches",
            crate::options::Unit::Foot => "feet",
            crate::options::Unit::Yard => "yards",
            crate::options::Unit::Mile => "miles",
        }
    ));
    let mut out = Out {
        doc,
        mode: opts.layer_mode,
        tol: opts.tolerance,
        unit: opts.unit.mm_per_unit(),
        buf: String::new(),
        layers: BTreeMap::new(),
        originals: BTreeMap::new(),
        report,
        cut_texts: 0,
    };
    out.layers.insert("0".into(), (7, false));
    for o in doc.objects.values() {
        if opts.visible_only && !doc.is_visible(o) {
            continue;
        }
        let shifted;
        let o = if shift != DVec2::ZERO {
            let mut c = o.clone();
            c.shape.translate(shift);
            shifted = c;
            &shifted
        } else {
            o
        };
        if matches!(o.shape, Shape::Group { .. }) {
            continue;
        }
        let mark = out.buf.len();
        if out.object(o) {
            out.report.count(o.shape.kind());
        } else {
            out.buf.truncate(mark);
            out.report.skip(o.shape.kind(), REASON);
        }
    }
    if out.cut_texts > 0 {
        let n = out.cut_texts;
        out.report.note(&format!(
            "{n} texts were cut to {TEXT_LIMIT} characters, the longest TEXT of release 12"
        ));
    }
    let merged: Vec<String> = out
        .originals
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, v)| format!("{k} ({})", v.join(", ")))
        .collect();
    if !merged.is_empty() {
        out.report.note(&format!(
            "release 12 names have capital letters, digits, _, - and $ only: these layers share a name: {}",
            merged.join("; ")
        ));
    }
    let renamed = out
        .originals
        .iter()
        .filter(|(k, v)| v.len() == 1 && v[0] != **k)
        .count();
    if renamed > 0 {
        out.report.note(&format!(
            "{renamed} layer names were changed to the form of release 12"
        ));
    }

    let mut f = String::new();
    let mut pair = |code: i32, v: &dyn std::fmt::Display| {
        let _ = write!(f, "{code}\n{v}\n");
    };
    pair(0, &"SECTION");
    pair(2, &"HEADER");
    pair(9, &"$ACADVER");
    pair(1, &"AC1009");
    pair(9, &"$DWGCODEPAGE");
    pair(3, &"ANSI_1252");
    pair(0, &"ENDSEC");
    pair(0, &"SECTION");
    pair(2, &"TABLES");
    // LTYPE
    pair(0, &"TABLE");
    pair(2, &"LTYPE");
    pair(70, &1);
    pair(0, &"LTYPE");
    pair(2, &"CONTINUOUS");
    pair(70, &0);
    pair(3, &"Solid line");
    pair(72, &65);
    pair(73, &0);
    pair(40, &0.0);
    pair(0, &"ENDTAB");
    // LAYER
    pair(0, &"TABLE");
    pair(2, &"LAYER");
    pair(70, &out.layers.len());
    for (name, (aci, hidden)) in &out.layers {
        pair(0, &"LAYER");
        pair(2, name);
        pair(70, &0);
        pair(
            62,
            &(if *hidden {
                -i32::from(*aci)
            } else {
                i32::from(*aci)
            }),
        );
        pair(6, &"CONTINUOUS");
    }
    pair(0, &"ENDTAB");
    // STYLE
    pair(0, &"TABLE");
    pair(2, &"STYLE");
    pair(70, &1);
    pair(0, &"STYLE");
    pair(2, &"STANDARD");
    pair(70, &0);
    pair(40, &0.0);
    pair(41, &1.0);
    pair(50, &0.0);
    pair(71, &0);
    pair(42, &2.5);
    pair(3, &"txt");
    pair(4, &"");
    pair(0, &"ENDTAB");
    for table in ["VIEW", "UCS"] {
        pair(0, &"TABLE");
        pair(2, &table);
        pair(70, &0);
        pair(0, &"ENDTAB");
    }
    pair(0, &"TABLE");
    pair(2, &"APPID");
    pair(70, &1);
    pair(0, &"APPID");
    pair(2, &"ACAD");
    pair(70, &0);
    pair(0, &"ENDTAB");
    pair(0, &"ENDSEC");
    pair(0, &"SECTION");
    pair(2, &"BLOCKS");
    pair(0, &"ENDSEC");
    pair(0, &"SECTION");
    pair(2, &"ENTITIES");
    f.push_str(&out.buf);
    f.push_str("0\nENDSEC\n0\nEOF\n");
    let (bytes, lost) = to_ansi_1252(&f);
    if lost > 0 {
        out.report.note(&format!(
            "{lost} characters have no letter in the code page ANSI_1252 of release 12 and became ?"
        ));
    }
    Ok(ExportResult {
        bytes,
        report: out.report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_release_12() {
        assert_eq!(r12_name("Walls & Paths"), "WALLS___PATHS");
        assert_eq!(r12_name("a-b_c$"), "A-B_C$");
        assert_eq!(r12_name(""), "0");
        assert_eq!(r12_name(&"x".repeat(40)).len(), 31);
        assert_eq!(r12_name("K\u{e4}ed"), "K_ED");
    }

    #[test]
    fn text_goes_to_the_code_page() {
        let (b, lost) = to_ansi_1252("K\u{e4}ed \u{20ac} \u{4e2d}");
        assert_eq!(b, b"K\xe4ed \x80 ?");
        assert_eq!(lost, 1);
    }
}
