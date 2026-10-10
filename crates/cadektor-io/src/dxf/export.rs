//! Document to ASCII DXF (release 2004, `AC1018`; the layout is the same as release 2000 plus true colors).
//!
//! The file has all sections and tables that AutoCAD expects: HEADER, CLASSES, TABLES (VPORT, LTYPE, LAYER,
//! STYLE, VIEW, UCS, APPID, DIMSTYLE, BLOCK_RECORD), BLOCKS, ENTITIES and OBJECTS (root dictionary, layouts,
//! plot style and MLINE style dictionaries). Every record has a handle and an owner handle, and `$HANDSEED`
//! is above all handles. Line weights are snapped to the values that AutoCAD allows.
//!
//! Every object goes to a layer that is named after its class (or its design
//! layer, by option). Curved polylines keep their arcs as bulges. Plants become
//! a circle and an INSERT of a small cross block. The file is pure ASCII:
//! other characters are written as `\U+XXXX`.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::fmt::{Display, Write};

use cadektor_core::DVec2;
use cadektor_core::curve::{self, Seg};
use cadektor_core::model::{Attributes, Color, Document, Object, Shape, arc_sweep};

use super::nearest_aci;
use crate::options::{ExportOrigin, LayerMode};
use crate::{ExportOptions, ExportResult, IoError, Report};

/// Longest text that one TEXT entity holds. Longer text becomes MTEXT.
const TEXT_LIMIT: usize = 250;

struct W {
    buf: String,
    next: u64,
    /// Handle of the owner (block record) that new entities get as group 330. Empty: none is written.
    owner: String,
}

impl W {
    fn new(next: u64) -> W {
        W {
            buf: String::new(),
            next,
            owner: String::new(),
        }
    }

    /// A new handle, as hex text.
    fn handle(&mut self) -> String {
        let h = self.next;
        self.next += 1;
        format!("{h:X}")
    }

    fn pair(&mut self, code: i32, v: impl Display) {
        let _ = write!(self.buf, "{code}\n{v}\n");
    }

    fn num(&mut self, code: i32, v: f64) {
        let v = if v == 0.0 { 0.0 } else { v };
        self.pair(code, v);
    }

    /// Starts an entity or object with its handle and, if there is one, its owner.
    fn start(&mut self, kind: &str) {
        self.pair(0, kind);
        let h = self.handle();
        self.pair(5, h);
        if !self.owner.is_empty() {
            let o = self.owner.clone();
            self.pair(330, o);
        }
    }

    /// Starts a table record with a given handle and its table as owner.
    fn record(&mut self, kind: &str, handle: &str, table: &str) {
        self.pair(0, kind);
        self.pair(5, handle);
        self.pair(330, table);
        self.pair(100, "AcDbSymbolTableRecord");
    }

    fn table(&mut self, name: &str, handle: &str, count: usize) {
        self.pair(0, "TABLE");
        self.pair(2, name);
        self.pair(5, handle);
        self.pair(330, 0);
        self.pair(100, "AcDbSymbolTable");
        self.pair(70, count);
    }
}

/// A DXF-safe name: forbidden characters become `_`, other characters are escaped.
fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if "<>/\\\":;?*|=`\n\r".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
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
    (
        nearest_aci([r, g, b]),
        (r as u32) << 16 | (g as u32) << 8 | b as u32,
    )
}

/// Line weights (1/100 mm) that DXF allows.
const LINE_WEIGHTS: [i32; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

/// The allowed line weight (1/100 mm) that is nearest to a weight in mm.
fn snap_line_weight(mm: f64) -> i32 {
    let v = if mm.is_finite() {
        (mm * 100.0).clamp(0.0, 211.0)
    } else {
        25.0
    };
    *LINE_WEIGHTS
        .iter()
        .min_by(|a, b| {
            (f64::from(**a) - v)
                .abs()
                .total_cmp(&(f64::from(**b) - v).abs())
        })
        .expect("not empty")
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
        w.pair(370, snap_line_weight(lw));
    }
}

struct Ctx<'a> {
    unit: f64,
    tol: f64,
    layer_mode: LayerMode,
    doc: &'a Document,
    plant_blocks: BTreeSet<String>,
    /// Shift (mm) that was added to the shapes for real-world coordinates. Derived data (contours) needs it too.
    shift: DVec2,
}

impl Ctx<'_> {
    fn layer_name(&self, o: &Object) -> String {
        match self.layer_mode {
            LayerMode::Classes => self.doc.class(o.class).map_or("0".into(), |c| {
                if c.name == "None" {
                    "0".into()
                } else {
                    safe_name(&c.name)
                }
            }),
            LayerMode::Layers => self
                .doc
                .layer(o.layer)
                .map_or("0".into(), |l| safe_name(&l.name)),
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

// Handles of the fixed records. Everything else gets a handle from 0x100 on.
const H_LAYER_TABLE: &str = "1";
const H_LTYPE_TABLE: &str = "2";
const H_APPID_TABLE: &str = "3";
const H_DIMSTYLE_TABLE: &str = "4";
const H_STYLE_TABLE: &str = "5";
const H_UCS_TABLE: &str = "6";
const H_VIEW_TABLE: &str = "7";
const H_VPORT_TABLE: &str = "8";
const H_BLOCK_RECORD_TABLE: &str = "9";
const H_ROOT_DICT: &str = "A";
const H_GROUP_DICT: &str = "C";
const H_LAYOUT_DICT: &str = "D";
const H_MLINE_DICT: &str = "10";
const H_PLOTSTYLE_DICT: &str = "12";
const H_PLOTSTYLE_NORMAL: &str = "13";
const H_MODEL_RECORD: &str = "17";
const H_MODEL_BLOCK: &str = "18";
const H_MODEL_ENDBLK: &str = "19";
const H_MODEL_LAYOUT: &str = "1A";
const H_PAPER_RECORD: &str = "1B";
const H_PAPER_BLOCK: &str = "1C";
const H_PAPER_ENDBLK: &str = "1D";
const H_PAPER_LAYOUT: &str = "1E";
const H_MLINE_STANDARD: &str = "22";
const H_VPORT_ACTIVE: &str = "23";
const H_LTYPE_BYBLOCK: &str = "24";
const H_LTYPE_BYLAYER: &str = "25";
const H_LTYPE_CONTINUOUS: &str = "26";
const H_LAYER_ZERO: &str = "27";
const H_STYLE_STANDARD: &str = "29";
const H_APPID_ACAD: &str = "2A";
const H_DIMSTYLE_STANDARD: &str = "2B";
/// First handle of entities and of records that depend on the drawing.
const FIRST_HANDLE: u64 = 0x100;

pub fn export(doc: &Document, opts: &ExportOptions) -> Result<ExportResult, IoError> {
    let doc = &*crate::model_only(doc);
    let mut report = Report::new("dxf");
    let mut cx = Ctx {
        unit: opts.unit.mm_per_unit(),
        tol: opts.tolerance,
        layer_mode: opts.layer_mode,
        doc,
        plant_blocks: BTreeSet::new(),
        shift: DVec2::ZERO,
    };
    // Real-world coordinates: the origin of the georeference goes back into the numbers (a rotation is not applied).
    let mut shift = DVec2::ZERO;
    if let (Some(g), ExportOrigin::File) = (&doc.georef, opts.origin) {
        shift = g.origin * 1000.0;
        if g.rotation != 0.0 {
            report.note(
                "the georeference is rotated: the rotation is not applied to the coordinates",
            );
        }
    }
    cx.shift = shift;
    let mut ents = W::new(FIRST_HANDLE);
    ents.owner = H_MODEL_RECORD.into();
    for o in doc.objects.values() {
        if opts.visible_only && !doc.is_visible(o) {
            continue;
        }
        let layer = cx.layer_name(o);
        let hidden_layer = doc.layer(o.layer).is_some_and(|l| !l.visible);
        let shifted;
        let o = if shift != DVec2::ZERO {
            let mut c = o.clone();
            c.shape.translate(shift);
            shifted = c;
            &shifted
        } else {
            o
        };
        if write_object(&mut ents, &mut cx, o, &layer, hidden_layer) {
            report.count(o.shape.kind());
        } else {
            report.skip(o.shape.kind(), "unsupported kind");
        }
    }

    // Layers. Names are unique without regard to case.
    let mut layers: BTreeMap<String, (String, Attributes, bool)> = BTreeMap::new();
    let mut add_layer = |name: String, attrs: Attributes, hidden: bool| {
        layers
            .entry(name.to_uppercase())
            .or_insert((name, attrs, hidden));
    };
    add_layer(
        "0".into(),
        Attributes {
            pen: Some(Color::BLACK),
            ..Attributes::default()
        },
        false,
    );
    match opts.layer_mode {
        LayerMode::Classes => {
            for c in &doc.classes {
                let name = if c.name == "None" {
                    "0".to_string()
                } else {
                    safe_name(&c.name)
                };
                add_layer(name, c.attrs.clone(), !c.visible);
            }
        }
        LayerMode::Layers => {
            for l in &doc.layers {
                add_layer(safe_name(&l.name), Attributes::default(), !l.visible);
            }
        }
    }
    // Entities use layers by name. A layer that no table entry names would make the file invalid.
    for l in ents
        .buf
        .lines()
        .collect::<Vec<_>>()
        .chunks(2)
        .filter(|c| c[0].trim() == "8")
        .map(|c| c.get(1).copied().unwrap_or("0"))
    {
        layers
            .entry(l.to_uppercase())
            .or_insert((l.to_string(), Attributes::default(), false));
    }

    let mut tables = W::new(ents.next);
    // VPORT
    tables.table("VPORT", H_VPORT_TABLE, 1);
    tables.record("VPORT", H_VPORT_ACTIVE, H_VPORT_TABLE);
    tables.pair(100, "AcDbViewportTableRecord");
    tables.pair(2, "*Active");
    tables.pair(70, 0);
    for (c, v) in [
        (10, 0.0),
        (20, 0.0),
        (11, 1.0),
        (21, 1.0),
        (12, 0.0),
        (22, 0.0),
        (13, 0.0),
        (23, 0.0),
        (14, 0.5),
        (24, 0.5),
        (15, 0.5),
        (25, 0.5),
        (16, 0.0),
        (26, 0.0),
        (36, 1.0),
        (17, 0.0),
        (27, 0.0),
        (37, 0.0),
        (40, 1000.0),
        (41, 1.34),
        (42, 50.0),
        (43, 0.0),
        (44, 0.0),
        (50, 0.0),
        (51, 0.0),
    ] {
        tables.num(c, v);
    }
    for (c, v) in [
        (71, 0),
        (72, 1000),
        (73, 1),
        (74, 3),
        (75, 0),
        (76, 0),
        (77, 0),
        (78, 0),
    ] {
        tables.pair(c, v);
    }
    tables.pair(0, "ENDTAB");
    // LTYPE: BYBLOCK, BYLAYER and CONTINUOUS exist in every file.
    tables.table("LTYPE", H_LTYPE_TABLE, 3);
    for (h, name) in [
        (H_LTYPE_BYBLOCK, "ByBlock"),
        (H_LTYPE_BYLAYER, "ByLayer"),
        (H_LTYPE_CONTINUOUS, "Continuous"),
    ] {
        tables.record("LTYPE", h, H_LTYPE_TABLE);
        tables.pair(100, "AcDbLinetypeTableRecord");
        tables.pair(2, name);
        tables.pair(70, 0);
        tables.pair(
            3,
            if name == "Continuous" {
                "Solid line"
            } else {
                ""
            },
        );
        tables.pair(72, 65);
        tables.pair(73, 0);
        tables.num(40, 0.0);
    }
    tables.pair(0, "ENDTAB");
    // LAYER
    tables.table("LAYER", H_LAYER_TABLE, layers.len());
    for (name, attrs, hidden) in layers.values() {
        let handle = if name == "0" {
            H_LAYER_ZERO.to_string()
        } else {
            tables.handle()
        };
        tables.record("LAYER", &handle, H_LAYER_TABLE);
        tables.pair(100, "AcDbLayerTableRecord");
        tables.pair(2, name);
        tables.pair(70, 0);
        if attrs.pen.is_none() && !hidden {
            tables.pair(62, 7);
        } else {
            write_look(&mut tables, attrs, *hidden);
        }
        tables.pair(6, "Continuous");
        if attrs.line_weight.is_none() {
            tables.pair(370, -3);
        }
        tables.pair(390, H_PLOTSTYLE_NORMAL);
    }
    tables.pair(0, "ENDTAB");
    // STYLE
    tables.table("STYLE", H_STYLE_TABLE, 1);
    tables.record("STYLE", H_STYLE_STANDARD, H_STYLE_TABLE);
    tables.pair(100, "AcDbTextStyleTableRecord");
    tables.pair(2, "Standard");
    tables.pair(70, 0);
    tables.num(40, 0.0);
    tables.num(41, 1.0);
    tables.num(50, 0.0);
    tables.pair(71, 0);
    tables.num(42, 2.5);
    tables.pair(3, "txt");
    tables.pair(4, "");
    tables.pair(0, "ENDTAB");
    // VIEW and UCS are empty.
    tables.table("VIEW", H_VIEW_TABLE, 0);
    tables.pair(0, "ENDTAB");
    tables.table("UCS", H_UCS_TABLE, 0);
    tables.pair(0, "ENDTAB");
    // APPID
    tables.table("APPID", H_APPID_TABLE, 1);
    tables.record("APPID", H_APPID_ACAD, H_APPID_TABLE);
    tables.pair(100, "AcDbRegAppTableRecord");
    tables.pair(2, "ACAD");
    tables.pair(70, 0);
    tables.pair(0, "ENDTAB");
    // DIMSTYLE (its handle is group 105, and the table has an extra subclass marker)
    tables.table("DIMSTYLE", H_DIMSTYLE_TABLE, 1);
    tables.pair(100, "AcDbDimStyleTable");
    tables.pair(0, "DIMSTYLE");
    tables.pair(105, H_DIMSTYLE_STANDARD);
    tables.pair(330, H_DIMSTYLE_TABLE);
    tables.pair(100, "AcDbSymbolTableRecord");
    tables.pair(100, "AcDbDimStyleTableRecord");
    tables.pair(2, "Standard");
    tables.pair(70, 0);
    tables.pair(3, "");
    tables.pair(4, "");
    for (c, v) in [
        (40, 1.0),
        (41, 2.5),
        (42, 0.625),
        (43, 3.75),
        (44, 1.25),
        (45, 0.0),
        (46, 0.0),
        (47, 0.0),
        (48, 0.0),
        (140, 2.5),
        (141, 2.5),
        (142, 0.0),
        (143, 0.03937007874),
        (144, 1.0),
        (145, 0.0),
        (146, 1.0),
        (147, 0.625),
        (148, 0.0),
    ] {
        tables.num(c, v);
    }
    for (c, v) in [
        (71, 0),
        (72, 0),
        (73, 0),
        (74, 0),
        (75, 0),
        (76, 0),
        (77, 1),
        (78, 8),
        (79, 3),
        (170, 0),
        (171, 3),
        (172, 1),
        (173, 0),
        (174, 0),
        (175, 0),
        (176, 0),
        (177, 0),
        (178, 0),
        (179, 2),
        (271, 2),
        (272, 2),
        (273, 2),
        (274, 3),
        (275, 0),
        (276, 0),
        (277, 2),
        (278, 44),
        (279, 0),
        (280, 0),
        (281, 0),
        (282, 0),
        (283, 0),
        (284, 8),
        (285, 0),
        (286, 0),
        (288, 0),
        (289, 3),
        (371, -2),
        (372, -2),
    ] {
        tables.pair(c, v);
    }
    tables.pair(0, "ENDTAB");
    // BLOCK_RECORD: model space, paper space and one per plant symbol.
    let plant_names: Vec<String> = cx.plant_blocks.iter().cloned().collect();
    tables.table("BLOCK_RECORD", H_BLOCK_RECORD_TABLE, 2 + plant_names.len());
    tables.record("BLOCK_RECORD", H_MODEL_RECORD, H_BLOCK_RECORD_TABLE);
    tables.pair(100, "AcDbBlockTableRecord");
    tables.pair(2, "*Model_Space");
    tables.pair(340, H_MODEL_LAYOUT);
    tables.record("BLOCK_RECORD", H_PAPER_RECORD, H_BLOCK_RECORD_TABLE);
    tables.pair(100, "AcDbBlockTableRecord");
    tables.pair(2, "*Paper_Space");
    tables.pair(340, H_PAPER_LAYOUT);
    let mut plant_blocks: Vec<(String, String)> = Vec::new();
    for n in &plant_names {
        let h = tables.handle();
        tables.record("BLOCK_RECORD", &h, H_BLOCK_RECORD_TABLE);
        tables.pair(100, "AcDbBlockTableRecord");
        tables.pair(2, n);
        plant_blocks.push((n.clone(), h));
    }
    tables.pair(0, "ENDTAB");

    // Blocks. Each block and its end marker belong to the block record.
    let mut blocks = W::new(tables.next);
    // (name, block record handle, fixed handles of the block and its end marker)
    type BlockDef = (String, String, Option<(String, String)>);
    let mut block_list: Vec<BlockDef> = vec![
        (
            "*Model_Space".into(),
            H_MODEL_RECORD.into(),
            Some((H_MODEL_BLOCK.into(), H_MODEL_ENDBLK.into())),
        ),
        (
            "*Paper_Space".into(),
            H_PAPER_RECORD.into(),
            Some((H_PAPER_BLOCK.into(), H_PAPER_ENDBLK.into())),
        ),
    ];
    block_list.extend(
        plant_blocks
            .iter()
            .map(|(n, h)| (n.clone(), h.clone(), None)),
    );
    for (n, record, fixed) in &block_list {
        blocks.owner = record.clone();
        let (block_h, end_h) = fixed
            .clone()
            .unwrap_or_else(|| (blocks.handle(), blocks.handle()));
        blocks.pair(0, "BLOCK");
        blocks.pair(5, &block_h);
        blocks.pair(330, record);
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
        blocks.pair(0, "ENDBLK");
        blocks.pair(5, &end_h);
        blocks.pair(330, record);
        blocks.pair(100, "AcDbEntity");
        blocks.pair(8, "0");
        blocks.pair(100, "AcDbBlockEnd");
    }

    // Objects: the dictionaries and layouts that tie model space and paper space together.
    let mut objects = W::new(blocks.next);
    objects.pair(0, "DICTIONARY");
    objects.pair(5, H_ROOT_DICT);
    objects.pair(330, 0);
    objects.pair(100, "AcDbDictionary");
    objects.pair(281, 1);
    for (name, h) in [
        ("ACAD_GROUP", H_GROUP_DICT),
        ("ACAD_LAYOUT", H_LAYOUT_DICT),
        ("ACAD_MLINESTYLE", H_MLINE_DICT),
        ("ACAD_PLOTSTYLENAME", H_PLOTSTYLE_DICT),
    ] {
        objects.pair(3, name);
        objects.pair(350, h);
    }
    let dict = |o: &mut W, handle: &str, entries: &[(&str, &str)]| {
        o.pair(0, "DICTIONARY");
        o.pair(5, handle);
        o.pair(330, H_ROOT_DICT);
        o.pair(100, "AcDbDictionary");
        o.pair(281, 1);
        for (n, h) in entries {
            o.pair(3, n);
            o.pair(350, h);
        }
    };
    dict(&mut objects, H_GROUP_DICT, &[]);
    dict(
        &mut objects,
        H_LAYOUT_DICT,
        &[("Model", H_MODEL_LAYOUT), ("Layout1", H_PAPER_LAYOUT)],
    );
    dict(
        &mut objects,
        H_MLINE_DICT,
        &[("Standard", H_MLINE_STANDARD)],
    );
    objects.pair(0, "ACDBDICTIONARYWDFLT");
    objects.pair(5, H_PLOTSTYLE_DICT);
    objects.pair(330, H_ROOT_DICT);
    objects.pair(100, "AcDbDictionary");
    objects.pair(281, 1);
    objects.pair(3, "Normal");
    objects.pair(350, H_PLOTSTYLE_NORMAL);
    objects.pair(100, "AcDbDictionaryWithDefault");
    objects.pair(340, H_PLOTSTYLE_NORMAL);
    objects.pair(0, "ACDBPLACEHOLDER");
    objects.pair(5, H_PLOTSTYLE_NORMAL);
    objects.pair(330, H_PLOTSTYLE_DICT);
    for (handle, name, tab, owner, flags) in [
        (H_MODEL_LAYOUT, "Model", 1, H_MODEL_RECORD, 1024),
        (H_PAPER_LAYOUT, "Layout1", 1, H_PAPER_RECORD, 0),
    ] {
        objects.pair(0, "LAYOUT");
        objects.pair(5, handle);
        objects.pair(330, H_LAYOUT_DICT);
        objects.pair(100, "AcDbPlotSettings");
        objects.pair(1, "");
        objects.pair(4, "A3");
        objects.pair(6, "");
        for (c, v) in [
            (40, 7.5),
            (41, 20.0),
            (42, 7.5),
            (43, 20.0),
            (44, 420.0),
            (45, 297.0),
            (46, 0.0),
            (47, 0.0),
            (48, 0.0),
            (49, 0.0),
            (140, 0.0),
            (141, 0.0),
            (142, 1.0),
            (143, 1.0),
        ] {
            objects.num(c, v);
        }
        for (c, v) in [(70, flags), (72, 1), (73, 0), (74, 5)] {
            objects.pair(c, v);
        }
        objects.pair(7, "");
        for (c, v) in [(75, 16), (76, 0), (77, 2), (78, 300)] {
            objects.pair(c, v);
        }
        for (c, v) in [(147, 1.0), (148, 0.0), (149, 0.0)] {
            objects.num(c, v);
        }
        objects.pair(100, "AcDbLayout");
        objects.pair(1, name);
        objects.pair(70, tab);
        objects.pair(71, i32::from(name != "Model"));
        for (c, v) in [
            (10, 0.0),
            (20, 0.0),
            (11, 420.0),
            (21, 297.0),
            (12, 0.0),
            (22, 0.0),
            (32, 0.0),
            (14, 1e20),
            (24, 1e20),
            (34, 1e20),
            (15, -1e20),
            (25, -1e20),
            (35, -1e20),
            (146, 0.0),
            (13, 0.0),
            (23, 0.0),
            (33, 0.0),
            (16, 1.0),
            (26, 0.0),
            (36, 0.0),
            (17, 0.0),
            (27, 1.0),
            (37, 0.0),
        ] {
            objects.num(c, v);
        }
        objects.pair(76, 1);
        objects.pair(330, owner);
    }
    objects.pair(0, "MLINESTYLE");
    objects.pair(5, H_MLINE_STANDARD);
    objects.pair(102, "{ACAD_REACTORS");
    objects.pair(330, H_MLINE_DICT);
    objects.pair(102, "}");
    objects.pair(330, H_MLINE_DICT);
    objects.pair(100, "AcDbMlineStyle");
    objects.pair(2, "Standard");
    objects.pair(70, 0);
    objects.pair(3, "");
    objects.pair(62, 256);
    objects.num(51, 90.0);
    objects.num(52, 90.0);
    objects.pair(71, 2);
    for off in [0.5, -0.5] {
        objects.num(49, off);
        objects.pair(62, 256);
        objects.pair(6, "BYLAYER");
    }

    let mut hdr = W::new(0);
    hdr.pair(0, "SECTION");
    hdr.pair(2, "HEADER");
    hdr.pair(9, "$ACADVER");
    hdr.pair(1, "AC1018");
    hdr.pair(9, "$DWGCODEPAGE");
    hdr.pair(3, "ANSI_1252");
    hdr.pair(9, "$HANDSEED");
    hdr.pair(5, format!("{:X}", objects.next + 1));
    hdr.pair(9, "$INSUNITS");
    hdr.pair(70, opts.unit.insunits());
    hdr.pair(9, "$MEASUREMENT");
    hdr.pair(70, 1);
    hdr.pair(9, "$LWDISPLAY");
    hdr.pair(290, 1);
    hdr.pair(0, "ENDSEC");
    // CLASSES: the object classes that OBJECTS uses.
    hdr.pair(0, "SECTION");
    hdr.pair(2, "CLASSES");
    for (name, cpp) in [
        ("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault"),
        ("ACDBPLACEHOLDER", "AcDbPlaceHolder"),
        ("LAYOUT", "AcDbLayout"),
    ] {
        hdr.pair(0, "CLASS");
        hdr.pair(1, name);
        hdr.pair(2, cpp);
        hdr.pair(3, "ObjectDBX Classes");
        hdr.pair(90, 0);
        hdr.pair(280, 0);
        hdr.pair(281, 0);
    }
    hdr.pair(0, "ENDSEC");
    hdr.pair(0, "SECTION");
    hdr.pair(2, "TABLES");

    let mut out = String::new();
    out.push_str(&hdr.buf);
    out.push_str(&tables.buf);
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n");
    out.push_str(&blocks.buf);
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    out.push_str(&ents.buf);
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n");
    out.push_str(&objects.buf);
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Ok(ExportResult {
        bytes: out.into_bytes(),
        report,
    })
}

fn common(w: &mut W, kind: &str, layer: &str, o: &Object, hidden_layer: bool) {
    w.start(kind);
    w.pair(100, "AcDbEntity");
    w.pair(8, layer);
    write_look(
        w,
        &Attributes {
            pen: o.attrs.pen,
            fill: None,
            line_weight: o.attrs.line_weight,
            ..Default::default()
        },
        false,
    );
    if hidden_layer {
        w.pair(60, 1);
    }
}

/// Writes the entities of an object, and the lines of its hatch, tile or line type as plain LINE
/// entities (exploded: every CAD program shows them the same). False for a kind of shape this writer does not know.
fn write_object(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool) -> bool {
    let doc = cx.doc;
    let attrs = cadektor_core::hardscape::styled_attrs(doc, o, doc.resolved_attrs(o));
    let scale = doc.layer(o.layer).map_or(100.0, |l| l.scale);
    let decor = cadektor_core::pattern::decorate(doc, o, &attrs, scale);
    let replaces = decor.as_ref().is_some_and(|d| d.replaces_outline);
    let ok = if replaces {
        true
    } else {
        write_plain(w, cx, o, layer, hidden)
    };
    let Some(d) = decor else { return ok };
    let line = |w: &mut W, cx: &mut Ctx, a: DVec2, b: DVec2| {
        common(w, "LINE", layer, o, hidden);
        w.pair(100, "AcDbLine");
        cx.pt3(w, 10, a.x, a.y, 0.0);
        cx.pt3(w, 11, b.x, b.y, 0.0);
    };
    for seg in &d.fill {
        line(w, cx, seg.a, seg.b);
    }
    for stroke in &d.strokes {
        for pair in stroke.windows(2) {
            line(w, cx, pair[0], pair[1]);
        }
    }
    for m in &d.marks {
        let inst = cadektor_core::symbols::mark_object(
            o,
            o.attrs.pen,
            o.attrs.line_weight,
            m.symbol,
            m.at,
            m.angle,
            m.scale,
        );
        write_plain(w, cx, &inst, layer, hidden);
    }
    ok || replaces
}

fn write_plain(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool) -> bool {
    match &o.shape {
        // A group is its members, which are written on their own.
        Shape::Group { .. } => {}
        // A symbol is written exploded, as the plain entities of its definition.
        Shape::Symbol { .. } => {
            let doc = cx.doc;
            for v in cadektor_core::symbols::instance_objects(doc, o) {
                let lay = cx.layer_name(&v);
                write_object(w, cx, &v, &lay, hidden);
            }
        }
        Shape::Line { a, b } => {
            common(w, "LINE", layer, o, hidden);
            w.pair(100, "AcDbLine");
            cx.pt3(w, 10, a.x, a.y, 0.0);
            cx.pt3(w, 11, b.x, b.y, 0.0);
        }
        Shape::Rect { min, max } => {
            let pts = [
                *min,
                DVec2::new(max.x, min.y),
                *max,
                DVec2::new(min.x, max.y),
            ];
            lwpolyline(
                w,
                cx,
                o,
                layer,
                hidden,
                &pts.iter().map(|p| (*p, 0.0)).collect::<Vec<_>>(),
                true,
            );
        }
        Shape::Circle { center, radius } => circle(w, cx, o, layer, hidden, *center, *radius),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
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
                if let Some(deg) = rotation_of(o) {
                    cx.pt3(w, 11, deg.to_radians().cos(), deg.to_radians().sin(), 0.0);
                }
                let esc = mtext_escape(text);
                let chars: Vec<char> = esc.chars().collect();
                let chunks: Vec<String> = chars
                    .chunks(TEXT_LIMIT)
                    .map(|c| c.iter().collect())
                    .collect();
                for (i, chunk) in chunks.iter().enumerate() {
                    w.pair(if i + 1 == chunks.len() { 1 } else { 3 }, chunk);
                }
            } else {
                common(w, "TEXT", layer, o, hidden);
                w.pair(100, "AcDbText");
                cx.pt3(w, 10, at.x, at.y, 0.0);
                w.num(40, height / cx.unit);
                w.pair(1, escape_unicode(text).replace('\r', " "));
                if let Some(deg) = rotation_of(o) {
                    w.num(50, deg);
                }
                w.pair(100, "AcDbText");
            }
        }
        Shape::Plant {
            at,
            species,
            spread,
            ..
        }
        | Shape::ExistingTree {
            at,
            species,
            spread,
            ..
        } => {
            plant_symbol(w, cx, o, layer, hidden, *at, species, *spread);
        }
        Shape::Locus3d { p } => {
            common(w, "POINT", layer, o, hidden);
            w.pair(100, "AcDbPoint");
            cx.pt3(w, 10, p.x, p.y, p.z);
        }
        Shape::Polyline3d { points, closed } => polyline3d(
            w,
            cx,
            o,
            layer,
            hidden,
            points.iter().map(|p| (p.x, p.y, p.z)),
            *closed,
        ),
        Shape::SiteModifier { boundary, .. } => {
            let (verts, _) = bulged_vertices(boundary, true, cx.tol);
            lwpolyline(w, cx, o, layer, hidden, &verts, true);
        }
        Shape::SiteModel { points, .. } => {
            // The contours of the surface (the proposed one if there is one) as 3D polylines at their height,
            // and the source points as POINT entities. The TIN itself is not exported.
            let Ok(site) = cadektor_site::SiteResult::compute(cx.doc, o.id) else {
                return false;
            };
            let Ok(contours) = site.contours(cadektor_site::Surface::Proposed) else {
                return false;
            };
            let shift = cx.shift;
            for c in contours {
                polyline3d(
                    w,
                    cx,
                    o,
                    layer,
                    hidden,
                    c.points
                        .iter()
                        .map(|p| (p.x + shift.x, p.y + shift.y, c.level)),
                    c.closed,
                );
            }
            for p in points {
                common(w, "POINT", layer, o, hidden);
                w.pair(100, "AcDbPoint");
                // The object was shifted already, so its points are in the final coordinates.
                cx.pt3(w, 10, p.x, p.y, p.z);
            }
        }
        // A plant tag is a label that the drawing makes from a style. There is nothing to write.
        Shape::PlantTag { .. } => return false,
        // Other crates add shape kinds. They are reported as skipped, not guessed.
        #[allow(unreachable_patterns)]
        _ => return false,
    }
    true
}

/// The rotation of a text in degrees (`rotation_deg` in the data of the object), if it has one.
fn rotation_of(o: &Object) -> Option<f64> {
    o.data
        .get("rotation_deg")
        .and_then(serde_json::Value::as_f64)
        .filter(|d| d.is_finite() && *d != 0.0)
}

/// A plant as a circle of its canopy and an INSERT of a small cross block, turned by its rotation.
#[allow(clippy::too_many_arguments)]
fn plant_symbol(
    w: &mut W,
    cx: &mut Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    at: DVec2,
    species: &str,
    spread: f64,
) {
    circle(w, cx, o, layer, hidden, at, spread / 2.0);
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
    w.num(50, cadektor_core::plants::link::rotation(o).to_degrees());
}

/// A 3D polyline with its vertices. Coordinates are in mm.
fn polyline3d(
    w: &mut W,
    cx: &Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    points: impl Iterator<Item = (f64, f64, f64)>,
    closed: bool,
) {
    common(w, "POLYLINE", layer, o, hidden);
    w.pair(100, "AcDb3dPolyline");
    w.pair(66, 1);
    cx.pt3(w, 10, 0.0, 0.0, 0.0);
    w.pair(70, 8 | i32::from(closed));
    for (x, y, z) in points {
        w.start("VERTEX");
        w.pair(100, "AcDbEntity");
        w.pair(8, layer);
        w.pair(100, "AcDbVertex");
        w.pair(100, "AcDb3dPolylineVertex");
        cx.pt3(w, 10, x, y, z);
        w.pair(70, 32);
    }
    w.start("SEQEND");
    w.pair(100, "AcDbEntity");
    w.pair(8, layer);
}

fn circle(w: &mut W, cx: &Ctx, o: &Object, layer: &str, hidden: bool, c: DVec2, r: f64) {
    common(w, "CIRCLE", layer, o, hidden);
    w.pair(100, "AcDbCircle");
    cx.pt3(w, 10, c.x, c.y, 0.0);
    w.num(40, r / cx.unit);
}

fn lwpolyline(
    w: &mut W,
    cx: &Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    verts: &[(DVec2, f64)],
    closed: bool,
) {
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
fn bulged_vertices(
    points: &[cadektor_core::curve::Vertex],
    closed: bool,
    tol: f64,
) -> (Vec<(DVec2, f64)>, bool) {
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
                let n = pieces(
                    (c1 - from).length() + (c2 - c1).length() + (to - c2).length(),
                    tol,
                );
                out.extend((0..n).map(|i| {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    (
                        from * (u * u * u)
                            + c1 * (3.0 * u * u * t)
                            + c2 * (3.0 * u * t * t)
                            + to * (t * t * t),
                        0.0,
                    )
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
    use cadektor_core::curve::{Vertex, VertexKind, corners};

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
        assert!(
            (v[0].1 - q).abs() < 1e-9 && (v[1].1 - q).abs() < 1e-9,
            "{v:?}"
        );
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
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(10.0, 0.0),
            },
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(10.0, 5.0),
            },
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
            Shape::Arc {
                center: DVec2::ZERO,
                radius: 5.0,
                start: 0.5,
                end: 2.0,
            },
            Shape::Arc {
                center: DVec2::ZERO,
                radius: 5.0,
                start: 0.0,
                end: 7.0,
            },
            Shape::Polyline {
                points: corners(&[DVec2::ZERO, DVec2::X]),
                closed: false,
            },
            Shape::Text {
                at: DVec2::ZERO,
                text: "short".into(),
                height: 3.0,
            },
            Shape::Text {
                at: DVec2::ZERO,
                text: "two\nlines".into(),
                height: 3.0,
            },
            Shape::Text {
                at: DVec2::ZERO,
                text: "x".repeat(600),
                height: 3.0,
            },
            Shape::Plant {
                at: DVec2::ZERO,
                species: "Betula pendula".into(),
                spread: 4000.0,
                height: 5000.0,
            },
            Shape::LandscapeArea {
                boundary: corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]),
                name: "n".into(),
            },
            Shape::Hardscape {
                boundary: corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]),
                material: "m".into(),
                thickness: None,
                elevation: None,
            },
            Shape::Locus3d {
                p: cadektor_core::DVec3::new(1.0, 2.0, 3.0),
            },
            Shape::Polyline3d {
                points: vec![cadektor_core::DVec3::ZERO, cadektor_core::DVec3::ONE],
                closed: true,
            },
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
        let r = export(
            &doc,
            &ExportOptions {
                unit: Unit::M,
                ..Default::default()
            },
        )
        .unwrap();
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
        let none = export(
            &doc,
            &ExportOptions {
                visible_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(none.report.total(), 0);
        let by_layer = export(
            &doc,
            &ExportOptions {
                layer_mode: LayerMode::Layers,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            String::from_utf8(by_layer.bytes)
                .unwrap()
                .contains("Layer 1")
        );
    }

    #[test]
    fn file_structure_has_handles_owners_and_valid_line_weights() {
        let mut doc = Document::default();
        let c = doc.classes[0].id;
        let ly = doc.layers[0].id;
        for (i, (s, lw)) in [
            (
                Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::X,
                },
                0.37,
            ),
            (
                Shape::Plant {
                    at: DVec2::ZERO,
                    species: "Acer".into(),
                    spread: 1000.0,
                    height: 1000.0,
                },
                0.01,
            ),
            (
                Shape::Polyline3d {
                    points: vec![cadektor_core::DVec3::ZERO, cadektor_core::DVec3::ONE],
                    closed: false,
                },
                5.0,
            ),
            (
                Shape::Text {
                    at: DVec2::ZERO,
                    text: "hi".into(),
                    height: 3.0,
                },
                0.25,
            ),
            (
                Shape::Text {
                    at: DVec2::ZERO,
                    text: "a\nb".into(),
                    height: 3.0,
                },
                0.25,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let id = doc.alloc_id().unwrap();
            let mut o = Object::new(cadektor_core::model::ObjectId(id), ly, c, s);
            o.attrs.line_weight = Some(lw);
            if i >= 3 {
                o.data
                    .insert("rotation_deg".into(), serde_json::Value::from(30.0));
            }
            doc.objects.insert(o.id, o);
        }
        let text =
            String::from_utf8(export(&doc, &ExportOptions::default()).unwrap().bytes).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let pairs: Vec<(i32, &str)> = lines
            .chunks(2)
            .map(|c| (c[0].trim().parse().unwrap(), c[1]))
            .collect();
        // Sections in the order of a file from AutoCAD.
        let sections: Vec<&str> = pairs
            .windows(2)
            .filter(|w| w[0] == (0, "SECTION"))
            .map(|w| w[1].1)
            .collect();
        assert_eq!(
            sections,
            [
                "HEADER", "CLASSES", "TABLES", "BLOCKS", "ENTITIES", "OBJECTS"
            ]
        );
        // The tables of a release 2000 file.
        let tables: Vec<&str> = pairs
            .windows(2)
            .filter(|w| w[0] == (0, "TABLE"))
            .map(|w| w[1].1)
            .collect();
        assert_eq!(
            tables,
            [
                "VPORT",
                "LTYPE",
                "LAYER",
                "STYLE",
                "VIEW",
                "UCS",
                "APPID",
                "DIMSTYLE",
                "BLOCK_RECORD"
            ]
        );
        for lt in ["ByBlock", "ByLayer", "Continuous"] {
            assert!(pairs.contains(&(2, lt)), "{lt}");
        }
        // Handles are unique, owners exist, and $HANDSEED is above all of them.
        let mut handles = std::collections::BTreeSet::new();
        let mut max = 0;
        let body = pairs.iter().position(|p| *p == (2, "TABLES")).unwrap();
        for (code, v) in &pairs[body..] {
            if matches!(code, 5 | 105) && !handles.contains(*v) {
                handles.insert(v.to_string());
                max = max.max(u64::from_str_radix(v, 16).unwrap());
            } else if matches!(code, 5 | 105) {
                panic!("duplicate handle {v}");
            }
        }
        for (code, v) in &pairs {
            if matches!(code, 330 | 340 | 350 | 390) && *v != "0" {
                assert!(
                    handles.contains(*v),
                    "group {code} points to {v}, which does not exist"
                );
            }
        }
        let seed = pairs
            .iter()
            .position(|p| *p == (9, "$HANDSEED"))
            .map(|i| pairs[i + 1].1)
            .unwrap();
        assert!(u64::from_str_radix(seed, 16).unwrap() > max);
        // Every entity has an owner.
        let ents_start = pairs.iter().position(|p| *p == (2, "ENTITIES")).unwrap();
        let ents_end = pairs.iter().position(|p| *p == (2, "OBJECTS")).unwrap();
        let mut owned = 0;
        for w in pairs[ents_start..ents_end].windows(3) {
            if w[0].0 == 0 && w[0].1 != "ENDSEC" && w[0].1 != "SECTION" {
                assert_eq!((w[1].0, w[2].0), (5, 330), "{}", w[0].1);
                owned += 1;
            }
        }
        assert!(owned >= 7, "{owned}");
        // Line weights are values that DXF allows: 0.37 mm is 35, 0.01 mm is 5 (not 1), 5 mm is 211.
        let weights: Vec<i32> = pairs
            .iter()
            .filter(|p| p.0 == 370)
            .map(|p| p.1.trim().parse().unwrap())
            .collect();
        assert!(
            weights.iter().all(|w| *w < 0 || LINE_WEIGHTS.contains(w)),
            "{weights:?}"
        );
        assert!(
            weights.contains(&35) && weights.contains(&0) && weights.contains(&211),
            "{weights:?}"
        );
        // Text rotation: degrees in TEXT, a direction vector in MTEXT.
        assert!(pairs.contains(&(50, "30")));
        assert!(pairs.iter().any(|p| p.0 == 11 && p.1.starts_with("0.866")));
        assert_eq!(snap_line_weight(0.37), 35);
        assert_eq!(snap_line_weight(f64::NAN), 25);
        assert_eq!(snap_line_weight(-1.0), 0);
        // Round trip: the rotation comes back.
        let src = crate::Source {
            name: "x".into(),
            data: text.into_bytes(),
            extra: Default::default(),
        };
        let r = super::super::import::import(
            &src,
            &crate::Context::of(&Document::default()),
            &crate::ImportOptions::default(),
        )
        .unwrap();
        let mut ed = cadektor_core::command::Editor::new(Document::default());
        ed.execute(&r.command).unwrap();
        let rotated = ed
            .doc()
            .objects
            .values()
            .filter(|o| {
                o.data
                    .get("rotation_deg")
                    .is_some_and(|v| (v.as_f64().unwrap() - 30.0).abs() < 1e-6)
            })
            .count();
        assert_eq!(rotated, 2);
    }
}
