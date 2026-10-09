//! Document to ASCII DXF (release 2000 layout).
//!
//! Every object goes to a layer that is named after its class (or its design
//! layer, by option). Curved polylines keep their arcs as bulges. Plants become
//! a circle and an INSERT of a small cross block. The file is pure ASCII:
//! other characters are written as `\U+XXXX`.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::fmt::{Display, Write};

use cadektor_core::curve::{self, Seg};
use cadektor_core::model::{arc_sweep, Attributes, Color, Document, Object, Shape};
use cadektor_core::DVec2;

use super::nearest_aci;
use crate::options::LayerMode;
use crate::{ExportOptions, ExportResult, IoError, Report};

/// Longest text that one TEXT entity holds. Longer text becomes MTEXT.
const TEXT_LIMIT: usize = 250;

struct W {
    buf: String,
    next: u64,
}

impl W {
    fn new(next: u64) -> W {
        W { buf: String::new(), next }
    }

    fn pair(&mut self, code: i32, v: impl Display) {
        let _ = write!(self.buf, "{code}\n{v}\n");
    }

    fn num(&mut self, code: i32, v: f64) {
        let v = if v == 0.0 { 0.0 } else { v };
        self.pair(code, v);
    }

    /// Starts an entity with its handle.
    fn start(&mut self, kind: &str) {
        self.pair(0, kind);
        let h = self.next;
        self.next += 1;
        self.pair(5, format!("{h:X}"));
    }
}

/// A DXF-safe name: forbidden characters become `_`, other characters are escaped.
fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>/\\\":;?*|=`\n\r".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = escape_unicode(&s);
    let s: String = s.chars().take(200).collect();
    if s.trim().is_empty() { "0".into() } else { s }
}

/// Writes every non-ASCII character as `\U+XXXX`.
fn escape_unicode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else if (c as u32) <= 0xFFFF {
            let _ = write!(out, "\\U+{:04X}", c as u32);
        } else {
            out.push('?');
        }
    }
    out
}

fn mtext_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\P"),
            '\r' => {}
            c => out.push(c),
        }
    }
    escape_unicode(&out)
}

fn aci_and_true(c: Color) -> (u8, u32) {
    let [r, g, b, _] = c.0;
    (nearest_aci([r, g, b]), (r as u32) << 16 | (g as u32) << 8 | b as u32)
}

/// Group codes for pen and line weight of an entity or layer.
fn write_look(w: &mut W, attrs: &Attributes, hidden: bool) {
    if let Some(pen) = attrs.pen.filter(|p| p.0[3] > 0) {
        let (aci, tc) = aci_and_true(pen);
        w.pair(62, if hidden { -(aci as i32) } else { aci as i32 });
        w.pair(420, tc);
    } else if hidden {
        w.pair(62, -7);
    }
    if let Some(lw) = attrs.line_weight {
        w.pair(370, (lw * 100.0).round().clamp(0.0, 211.0) as i32);
    }
}

struct Ctx<'a> {
    unit: f64,
    tol: f64,
    layer_mode: LayerMode,
    doc: &'a Document,
    plant_blocks: BTreeSet<String>,
}

impl Ctx<'_> {
    fn layer_name(&self, o: &Object) -> String {
        match self.layer_mode {
            LayerMode::Classes => self.doc.class(o.class).map_or("0".into(), |c| if c.name == "None" { "0".into() } else { safe_name(&c.name) }),
            LayerMode::Layers => self.doc.layer(o.layer).map_or("0".into(), |l| safe_name(&l.name)),
        }
    }

    fn pt(&self, w: &mut W, base: i32, p: DVec2) {
        w.num(base, p.x / self.unit);
        w.num(base + 10, p.y / self.unit);
    }

    fn pt3(&self, w: &mut W, base: i32, x: f64, y: f64, z: f64) {
        w.num(base, x / self.unit);
        w.num(base + 10, y / self.unit);
        w.num(base + 20, z / self.unit);
    }
}

pub fn export(doc: &Document, opts: &ExportOptions) -> Result<ExportResult, IoError> {
    let mut report = Report::new("dxf");
    let mut cx = Ctx { unit: opts.unit.mm_per_unit(), tol: opts.tolerance, layer_mode: opts.layer_mode, doc, plant_blocks: BTreeSet::new() };
    let mut ents = W::new(0x100);
    for o in doc.objects.values() {
        if opts.visible_only && !doc.is_visible(o) {
            continue;
        }
        let layer = cx.layer_name(o);
        let hidden_layer = doc.layer(o.layer).is_some_and(|l| !l.visible);
        write_object(&mut ents, &mut cx, o, &layer, hidden_layer);
        report.count(o.shape.kind());
    }
    let mut tail = W::new(ents.next);

    // Layer table.
    let mut layers: BTreeMap<String, (Attributes, bool)> = BTreeMap::new();
    layers.insert("0".into(), (Attributes { pen: Some(Color::BLACK), ..Attributes::default() }, false));
    match opts.layer_mode {
        LayerMode::Classes => {
            for c in &doc.classes {
                let name = if c.name == "None" { "0".to_string() } else { safe_name(&c.name) };
                layers.insert(name, (c.attrs.clone(), !c.visible));
            }
        }
        LayerMode::Layers => {
            for l in &doc.layers {
                layers.insert(safe_name(&l.name), (Attributes::default(), !l.visible));
            }
        }
    }
    let mut tables = W::new(tail.next);
    let table_handle = |w: &mut W| {
        let h = w.next;
        w.next += 1;
        format!("{h:X}")
    };
    // LTYPE
    tables.pair(0, "TABLE");
    tables.pair(2, "LTYPE");
    let h = table_handle(&mut tables);
    tables.pair(5, h);
    tables.pair(100, "AcDbSymbolTable");
    tables.pair(70, 1);
    tables.start("LTYPE");
    tables.pair(100, "AcDbSymbolTableRecord");
    tables.pair(100, "AcDbLinetypeTableRecord");
    tables.pair(2, "CONTINUOUS");
    tables.pair(70, 0);
    tables.pair(3, "Solid line");
    tables.pair(72, 65);
    tables.pair(73, 0);
    tables.num(40, 0.0);
    tables.pair(0, "ENDTAB");
    // LAYER
    tables.pair(0, "TABLE");
    tables.pair(2, "LAYER");
    let h = table_handle(&mut tables);
    tables.pair(5, h);
    tables.pair(100, "AcDbSymbolTable");
    tables.pair(70, layers.len());
    for (name, (attrs, hidden)) in &layers {
        tables.start("LAYER");
        tables.pair(100, "AcDbSymbolTableRecord");
        tables.pair(100, "AcDbLayerTableRecord");
        tables.pair(2, name);
        tables.pair(70, 0);
        if attrs.pen.is_none() && !hidden {
            tables.pair(62, 7);
        } else {
            write_look(&mut tables, attrs, *hidden);
        }
        tables.pair(6, "CONTINUOUS");
    }
    tables.pair(0, "ENDTAB");
    // STYLE
    tables.pair(0, "TABLE");
    tables.pair(2, "STYLE");
    let h = table_handle(&mut tables);
    tables.pair(5, h);
    tables.pair(100, "AcDbSymbolTable");
    tables.pair(70, 1);
    tables.start("STYLE");
    tables.pair(100, "AcDbSymbolTableRecord");
    tables.pair(100, "AcDbTextStyleTableRecord");
    tables.pair(2, "STANDARD");
    tables.pair(70, 0);
    tables.num(40, 0.0);
    tables.num(41, 1.0);
    tables.num(50, 0.0);
    tables.pair(71, 0);
    tables.num(42, 2.5);
    tables.pair(3, "txt");
    tables.pair(4, "");
    tables.pair(0, "ENDTAB");
    // BLOCK_RECORD
    let block_names: Vec<String> = ["*Model_Space".to_string(), "*Paper_Space".to_string()].into_iter().chain(cx.plant_blocks.iter().cloned()).collect();
    tables.pair(0, "TABLE");
    tables.pair(2, "BLOCK_RECORD");
    let h = table_handle(&mut tables);
    tables.pair(5, h);
    tables.pair(100, "AcDbSymbolTable");
    tables.pair(70, block_names.len());
    for n in &block_names {
        tables.start("BLOCK_RECORD");
        tables.pair(100, "AcDbSymbolTableRecord");
        tables.pair(100, "AcDbBlockTableRecord");
        tables.pair(2, n);
    }
    tables.pair(0, "ENDTAB");

    // Blocks.
    let mut blocks = W::new(tables.next);
    for n in &block_names {
        blocks.start("BLOCK");
        blocks.pair(100, "AcDbEntity");
        blocks.pair(8, "0");
        blocks.pair(100, "AcDbBlockBegin");
        blocks.pair(2, n);
        blocks.pair(70, 0);
        blocks.num(10, 0.0);
        blocks.num(20, 0.0);
        blocks.num(30, 0.0);
        blocks.pair(3, n);
        blocks.pair(1, "");
        if n.starts_with("PLANT") {
            for (a, b) in [((-1.0, 0.0), (1.0, 0.0)), ((0.0, -1.0), (0.0, 1.0))] {
                blocks.start("LINE");
                blocks.pair(100, "AcDbEntity");
                blocks.pair(8, "0");
                blocks.pair(100, "AcDbLine");
                for (base, p) in [(10, a), (11, b)] {
                    blocks.num(base, p.0);
                    blocks.num(base + 10, p.1);
                    blocks.num(base + 20, 0.0);
                }
            }
        }
        blocks.start("ENDBLK");
        blocks.pair(100, "AcDbEntity");
        blocks.pair(8, "0");
        blocks.pair(100, "AcDbBlockEnd");
    }
    tail.next = blocks.next;

    let mut out = String::new();
    let mut hdr = W::new(0);
    hdr.pair(0, "SECTION");
    hdr.pair(2, "HEADER");
    hdr.pair(9, "$ACADVER");
    hdr.pair(1, "AC1015");
    hdr.pair(9, "$HANDSEED");
    hdr.pair(5, format!("{:X}", blocks.next + 1));
    hdr.pair(9, "$INSUNITS");
    hdr.pair(70, opts.unit.insunits());
    hdr.pair(9, "$MEASUREMENT");
    hdr.pair(70, 1);
    hdr.pair(0, "ENDSEC");
    hdr.pair(0, "SECTION");
    hdr.pair(2, "TABLES");
    out.push_str(&hdr.buf);
    out.push_str(&tables.buf);
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n");
    out.push_str(&blocks.buf);
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    out.push_str(&ents.buf);
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Ok(ExportResult { bytes: out.into_bytes(), report })
}

fn common(w: &mut W, kind: &str, layer: &str, o: &Object, hidden_layer: bool) {
    w.start(kind);
    w.pair(100, "AcDbEntity");
    w.pair(8, layer);
    write_look(w, &Attributes { pen: o.attrs.pen, fill: None, line_weight: o.attrs.line_weight }, false);
    if hidden_layer {
        w.pair(60, 1);
    }
}

fn write_object(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool) {
    match &o.shape {
        Shape::Line { a, b } => {
            common(w, "LINE", layer, o, hidden);
            w.pair(100, "AcDbLine");
            cx.pt3(w, 10, a.x, a.y, 0.0);
            cx.pt3(w, 11, b.x, b.y, 0.0);
        }
        Shape::Rect { min, max } => {
            let pts = [*min, DVec2::new(max.x, min.y), *max, DVec2::new(min.x, max.y)];
            lwpolyline(w, cx, o, layer, hidden, &pts.iter().map(|p| (*p, 0.0)).collect::<Vec<_>>(), true);
        }
        Shape::Circle { center, radius } => circle(w, cx, o, layer, hidden, *center, *radius),
        Shape::Arc { center, radius, start, end } => {
            let sweep = arc_sweep(*start, *end);
            if sweep >= TAU - 1e-12 {
                circle(w, cx, o, layer, hidden, *center, *radius);
            } else {
                common(w, "ARC", layer, o, hidden);
                w.pair(100, "AcDbCircle");
                cx.pt3(w, 10, center.x, center.y, 0.0);
                w.num(40, radius / cx.unit);
                w.pair(100, "AcDbArc");
                let a0 = start.to_degrees().rem_euclid(360.0);
                w.num(50, a0);
                w.num(51, (a0 + sweep.to_degrees()).rem_euclid(360.0));
            }
        }
        Shape::Polyline { points, closed } => {
            let (verts, closed) = bulged_vertices(points, *closed, cx.tol);
            lwpolyline(w, cx, o, layer, hidden, &verts, closed);
        }
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            let (verts, _) = bulged_vertices(boundary, true, cx.tol);
            lwpolyline(w, cx, o, layer, hidden, &verts, true);
        }
        Shape::Text { at, text, height } => {
            if text.chars().count() > TEXT_LIMIT || text.contains('\n') {
                common(w, "MTEXT", layer, o, hidden);
                w.pair(100, "AcDbMText");
                cx.pt3(w, 10, at.x, at.y, 0.0);
                w.num(40, height / cx.unit);
                w.pair(71, 7);
                let esc = mtext_escape(text);
                let chars: Vec<char> = esc.chars().collect();
                let chunks: Vec<String> = chars.chunks(TEXT_LIMIT).map(|c| c.iter().collect()).collect();
                for (i, chunk) in chunks.iter().enumerate() {
                    w.pair(if i + 1 == chunks.len() { 1 } else { 3 }, chunk);
                }
            } else {
                common(w, "TEXT", layer, o, hidden);
                w.pair(100, "AcDbText");
                cx.pt3(w, 10, at.x, at.y, 0.0);
                w.num(40, height / cx.unit);
                w.pair(1, escape_unicode(text).replace('\r', " "));
                w.pair(100, "AcDbText");
            }
        }
        Shape::Plant { at, species, spread, .. } => {
            circle(w, cx, o, layer, hidden, *at, spread / 2.0);
            let name = format!("PLANT_{}", safe_name(species).replace(' ', "_"));
            cx.plant_blocks.insert(name.clone());
            common(w, "INSERT", layer, o, hidden);
            w.pair(100, "AcDbBlockReference");
            w.pair(2, &name);
            cx.pt3(w, 10, at.x, at.y, 0.0);
            let s = spread / 2.0 / cx.unit;
            w.num(41, s);
            w.num(42, s);
            w.num(43, s);
            w.num(50, 0.0);
        }
        Shape::Locus3d { p } => {
            common(w, "POINT", layer, o, hidden);
            w.pair(100, "AcDbPoint");
            cx.pt3(w, 10, p.x, p.y, p.z);
        }
        Shape::Polyline3d { points, closed } => {
            common(w, "POLYLINE", layer, o, hidden);
            w.pair(100, "AcDb3dPolyline");
            w.pair(66, 1);
            cx.pt3(w, 10, 0.0, 0.0, 0.0);
            w.pair(70, 8 | i32::from(*closed));
            for p in points {
                w.start("VERTEX");
                w.pair(100, "AcDbEntity");
                w.pair(8, layer);
                w.pair(100, "AcDbVertex");
                w.pair(100, "AcDb3dPolylineVertex");
                cx.pt3(w, 10, p.x, p.y, p.z);
                w.pair(70, 32);
            }
            w.start("SEQEND");
            w.pair(100, "AcDbEntity");
            w.pair(8, layer);
        }
    }
}

fn circle(w: &mut W, cx: &Ctx, o: &Object, layer: &str, hidden: bool, c: DVec2, r: f64) {
    common(w, "CIRCLE", layer, o, hidden);
    w.pair(100, "AcDbCircle");
    cx.pt3(w, 10, c.x, c.y, 0.0);
    w.num(40, r / cx.unit);
}

fn lwpolyline(w: &mut W, cx: &Ctx, o: &Object, layer: &str, hidden: bool, verts: &[(DVec2, f64)], closed: bool) {
    common(w, "LWPOLYLINE", layer, o, hidden);
    w.pair(100, "AcDbPolyline");
    w.pair(90, verts.len());
    w.pair(70, i32::from(closed));
    for (p, bulge) in verts {
        cx.pt(w, 10, *p);
        if *bulge != 0.0 {
            w.num(42, *bulge);
        }
    }
}

/// Vertices with bulges for the exact path of a curved polyline. Bezier pieces
/// are cut into lines. Returns the vertices and whether the polyline is closed.
fn bulged_vertices(points: &[cadektor_core::curve::Vertex], closed: bool, tol: f64) -> (Vec<(DVec2, f64)>, bool) {
    let path = curve::path(points, closed);
    let mut out: Vec<(DVec2, f64)> = Vec::new();
    for seg in &path.segs {
        match *seg {
            Seg::Line { from, .. } => out.push((from, 0.0)),
            Seg::Arc { from, sweep, .. } => out.push((from, (sweep / 4.0).tan())),
            Seg::Quad { from, c, to } => {
                let n = pieces((c - from).length() + (to - c).length(), tol);
                out.extend((0..n).map(|i| {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    (from * (u * u) + c * (2.0 * u * t) + to * (t * t), 0.0)
                }));
            }
            Seg::Cubic { from, c1, c2, to } => {
                let n = pieces((c1 - from).length() + (c2 - c1).length() + (to - c2).length(), tol);
                out.extend((0..n).map(|i| {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    (from * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + to * (t * t * t), 0.0)
                }));
            }
        }
    }
    if let Some(last) = path.segs.last() {
        let end = last.to();
        if !(path.closed && end.distance(path.start) < 1e-9) {
            out.push((end, 0.0));
        }
    } else {
        out.push((path.start, 0.0));
    }
    (out, path.closed)
}

fn pieces(control_len: f64, tol: f64) -> usize {
    ((control_len / tol).sqrt().ceil() as usize).clamp(4, 512)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Unit;
    use cadektor_core::curve::{corners, Vertex, VertexKind};

    #[test]
    fn names_and_text_are_ascii_safe() {
        assert_eq!(safe_name("a/b:c"), "a_b_c");
        assert_eq!(safe_name("  "), "0");
        assert_eq!(safe_name("P\u{e4}\u{1F600}"), "P\\U+00E4?");
        assert_eq!(mtext_escape("a\\{b}\n\r\u{e4}"), "a\\\\\\{b\\}\\P\\U+00E4");
        assert_eq!(aci_and_true(Color([255, 0, 0, 255])), (1, 0xFF0000));
    }

    #[test]
    fn bulged_vertices_keep_arcs() {
        let pts = vec![
            Vertex::corner(DVec2::new(0.0, 0.0)),
            Vertex::new(DVec2::new(1000.0, -1000.0), VertexKind::PointOnArc),
            Vertex::corner(DVec2::new(2000.0, 0.0)),
        ];
        let (v, closed) = bulged_vertices(&pts, false, 1.0);
        assert!(!closed);
        // Two quarter circles, each with a bulge of tan(sweep / 4).
        assert_eq!(v.len(), 3);
        let q = (std::f64::consts::FRAC_PI_2 / 4.0).tan();
        assert!((v[0].1 - q).abs() < 1e-9 && (v[1].1 - q).abs() < 1e-9, "{v:?}");
        let (v, closed) = bulged_vertices(&pts, true, 1.0);
        assert!(closed);
        assert_eq!(v.len(), 3);
        // Bezier pieces are cut into lines.
        let bez = vec![
            Vertex::corner(DVec2::new(0.0, 0.0)),
            Vertex::new(DVec2::new(500.0, 800.0), VertexKind::Bezier),
            Vertex::corner(DVec2::new(1000.0, 0.0)),
            Vertex::new(DVec2::new(1500.0, 900.0), VertexKind::Cubic),
            Vertex::corner(DVec2::new(2000.0, 0.0)),
        ];
        let (v, _) = bulged_vertices(&bez, false, 1.0);
        assert!(v.len() > 8);
        assert!(v.iter().all(|p| p.1 == 0.0));
        assert_eq!(bulged_vertices(&[], false, 1.0).0.len(), 1);
        let _ = corners(&[]);
    }

    #[test]
    fn exports_every_shape_as_valid_dxf() {
        let shapes = vec![
            Shape::Line { a: DVec2::ZERO, b: DVec2::new(10.0, 0.0) },
            Shape::Rect { min: DVec2::ZERO, max: DVec2::new(10.0, 5.0) },
            Shape::Circle { center: DVec2::ZERO, radius: 5.0 },
            Shape::Arc { center: DVec2::ZERO, radius: 5.0, start: 0.5, end: 2.0 },
            Shape::Arc { center: DVec2::ZERO, radius: 5.0, start: 0.0, end: 7.0 },
            Shape::Polyline { points: corners(&[DVec2::ZERO, DVec2::X]), closed: false },
            Shape::Text { at: DVec2::ZERO, text: "short".into(), height: 3.0 },
            Shape::Text { at: DVec2::ZERO, text: "two\nlines".into(), height: 3.0 },
            Shape::Text { at: DVec2::ZERO, text: "x".repeat(600), height: 3.0 },
            Shape::Plant { at: DVec2::ZERO, species: "Betula pendula".into(), spread: 4000.0, height: 5000.0 },
            Shape::LandscapeArea { boundary: corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]), name: "n".into() },
            Shape::Hardscape { boundary: corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]), material: "m".into(), thickness: None, elevation: None },
            Shape::Locus3d { p: cadektor_core::DVec3::new(1.0, 2.0, 3.0) },
            Shape::Polyline3d { points: vec![cadektor_core::DVec3::ZERO, cadektor_core::DVec3::ONE], closed: true },
        ];
        let mut doc = Document::default();
        let c = doc.classes[0].id;
        let ly = doc.layers[0].id;
        for s in shapes {
            let id = doc.alloc_id().unwrap();
            let mut o = Object::new(cadektor_core::model::ObjectId(id), ly, c, s);
            o.attrs.pen = Some(Color([10, 20, 30, 255]));
            o.attrs.line_weight = Some(0.5);
            doc.objects.insert(o.id, o);
        }
        doc.layers[0].visible = false;
        let r = export(&doc, &ExportOptions { unit: Unit::M, ..Default::default() }).unwrap();
        let text = String::from_utf8(r.bytes).unwrap();
        assert!(text.is_ascii());
        let parsed = super::super::parse::parse(text.as_bytes()).unwrap();
        assert!(!parsed.truncated);
        // Plants: circle + insert. Others: one entity each. Polyline3d has vertices as children.
        assert_eq!(parsed.entities.len(), 15);
        assert_eq!(parsed.blocks.len(), 3);
        assert!(parsed.blocks.contains_key("PLANT_Betula_pendula"));
        assert_eq!(parsed.header_value("$INSUNITS"), Some("6"));
        assert_eq!(r.report.total(), 14);
        // Hidden design layer: entities are invisible.
        assert!(parsed.entities.iter().all(|e| e.int(60) == Some(1)));
        let none = export(&doc, &ExportOptions { visible_only: true, ..Default::default() }).unwrap();
        assert_eq!(none.report.total(), 0);
        let by_layer = export(&doc, &ExportOptions { layer_mode: LayerMode::Layers, ..Default::default() }).unwrap();
        assert!(String::from_utf8(by_layer.bytes).unwrap().contains("Layer 1"));
    }
}
