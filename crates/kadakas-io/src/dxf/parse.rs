//! Reads the group-code structure of an ASCII DXF file.
//!
//! A DXF file is a list of pairs: a numeric group code, then a value. This
//! module turns the pairs into sections, tables, blocks and entities. It does
//! not know what the entities mean (see `import.rs`). It never panics. A file
//! that stops early gives what was read so far, and sets `truncated`.

use std::collections::HashMap;

use crate::IoError;

/// Most pairs (group code and value) of one file, text or binary. A pair in memory is about 70
/// bytes (the group code, the text and its allocation), so a file at the limit needs about 3 GB
/// for its pairs. The file size limit (`max_bytes`) does not bound the memory by itself: a text
/// file makes about 15 bytes of memory for each byte of the file.
pub const MAX_PAIRS: usize = 40_000_000;
/// Longest value of one pair, in bytes. A real value is under 2,100 bytes. Long XML text of a
/// GEODATA object is the largest.
pub const MAX_STRING: usize = 1 << 20;
/// Most records (lines starting with a code 0, so entities, table rows and objects) of one file.
pub const MAX_RECORDS: usize = 20_000_000;

/// One record: `0 TYPE` followed by its pairs.
#[derive(Clone, Debug, Default)]
pub struct Entity {
    pub kind: String,
    pub items: Vec<(i32, String)>,
    /// VERTEX records of a POLYLINE.
    pub children: Vec<Entity>,
}

impl Entity {
    pub fn get(&self, code: i32) -> Option<&str> {
        self.items
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
    }

    pub fn f(&self, code: i32) -> Option<f64> {
        self.get(code).and_then(parse_f64)
    }

    pub fn fd(&self, code: i32, default: f64) -> f64 {
        self.f(code).unwrap_or(default)
    }

    pub fn int(&self, code: i32) -> Option<i64> {
        self.get(code).and_then(parse_int)
    }

    /// All numbers with this code, in order.
    pub fn all_f(&self, code: i32) -> Vec<f64> {
        self.items
            .iter()
            .filter(|(c, _)| *c == code)
            .filter_map(|(_, v)| parse_f64(v))
            .collect()
    }

    /// The handle (group code 5) in its normal form (`hash::normal_handle`). None if the entity
    /// has none, or the handle is not a plain word of at most 32 letters and digits.
    pub fn handle(&self) -> Option<String> {
        self.get(5).and_then(super::hash::normal_handle)
    }

    pub fn layer(&self) -> &str {
        self.get(8).unwrap_or("0")
    }

    /// The extrusion direction (210, 220, 230). Default: +Z.
    pub fn normal(&self) -> [f64; 3] {
        [self.fd(210, 0.0), self.fd(220, 0.0), self.fd(230, 1.0)]
    }
}

/// A finite number. DXF writers sometimes write `1.5D+2` or leave spaces.
pub fn parse_f64(s: &str) -> Option<f64> {
    let t = s.trim();
    let v = t
        .parse::<f64>()
        .ok()
        .or_else(|| t.replace(['D', 'd'], "e").parse::<f64>().ok())?;
    v.is_finite().then_some(v)
}

pub fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    t.parse::<i64>()
        .ok()
        .or_else(|| parse_f64(t).map(|v| v as i64))
}

#[derive(Clone, Debug, Default)]
pub struct LayerDef {
    pub name: String,
    /// The handle of the layer record (group code 5), in normal form. A viewport names the layers
    /// that it freezes by these handles.
    pub handle: Option<String>,
    /// ACI color. Negative means the layer is off.
    pub color: i64,
    pub true_color: Option<u32>,
    /// Frozen (flag bit 1).
    pub frozen: bool,
    /// Line weight in 1/100 mm. Negative means default.
    pub lineweight: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    /// Handle of the BLOCK entity (group code 5), in normal form.
    pub handle: Option<String>,
    /// Handle of the BLOCK_RECORD (the owner pointer, group code 330), in normal form.
    pub record: Option<String>,
    pub base: [f64; 3],
    /// Block type flags (group code 70): 4 and 8 are external references, 16 is dependent on one.
    pub flags: i64,
    /// The path of an external reference (group code 1).
    pub path: String,
    pub entities: Vec<Entity>,
}

/// The georeference of a DXF file (GEODATA): the point of the drawing that has a known map position,
/// the map position, and grid north in drawing coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoData {
    pub design: [f64; 2],
    pub reference: [f64; 2],
    pub north: [f64; 2],
    /// The coordinate system definition (text; for an EPSG code it holds `EPSG:<n>`).
    pub definition: String,
}

impl GeoData {
    /// The `EPSG:<n>` name found in the definition.
    pub fn epsg(&self) -> Option<String> {
        let at = self.definition.find("EPSG:")?;
        let digits: String = self.definition[at + 5..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        (!digits.is_empty()).then(|| format!("EPSG:{digits}"))
    }
}

#[derive(Debug, Default)]
pub struct Dxf {
    pub header: HashMap<String, Vec<(i32, String)>>,
    pub layers: Vec<LayerDef>,
    pub blocks: HashMap<String, Block>,
    pub entities: Vec<Entity>,
    /// The GEODATA object of the file (the georeference), if it has one.
    pub geodata: Option<GeoData>,
    /// Objects of the OBJECTS section that the importer reads: LAYOUT (paper space layouts),
    /// IMAGEDEF (the file of a raster image) and the dictionaries of clipped block references.
    pub objects: Vec<Entity>,
    /// The file ended before `EOF`.
    pub truncated: bool,
}

impl Dxf {
    /// The release of the file (`$ACADVER`), if the header names one.
    pub fn release(&self) -> Option<super::version::Release> {
        super::version::Release::parse(self.header_value("$ACADVER")?)
    }

    /// The first value of a header variable, for example `$INSUNITS`.
    pub fn header_value(&self, var: &str) -> Option<&str> {
        self.header.get(var)?.first().map(|(_, v)| v.as_str())
    }
}

/// The code page that `$DWGCODEPAGE` names, found in the raw bytes of the header.
fn header_code_page(bytes: &[u8]) -> Option<crate::encoding::Codec> {
    const KEY: &[u8] = b"$DWGCODEPAGE";
    let at = bytes.windows(KEY.len()).position(|w| w == KEY)?;
    let rest = &bytes[at + KEY.len()..bytes.len().min(at + KEY.len() + 64)];
    let text: String = rest.iter().map(|&b| b as char).collect();
    // After the name come the group code (3) and the value, one per line.
    let value = text.lines().filter(|l| !l.trim().is_empty()).nth(1)?;
    crate::encoding::codec_from_dwg_code_page(value)
}

/// Text of the file. UTF-8 when valid. Older files use the code page of `$DWGCODEPAGE`, else Latin-1.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => match header_code_page(bytes) {
            Some(enc) => enc.decode(bytes),
            None => bytes.iter().map(|&b| b as char).collect(),
        },
    }
}

/// Splits text into (code, value) pairs. Stops at a line that is not a code. A line longer than
/// [`MAX_STRING`] or more than [`MAX_PAIRS`] pairs make the file refused, with the line number.
fn pairs(text: &str) -> Result<(Vec<(i32, String)>, bool), IoError> {
    let mut out = Vec::new();
    let mut lines = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .enumerate();
    loop {
        let Some((_, code_line)) = lines.next() else {
            return Ok((out, false));
        };
        if code_line.trim().is_empty() && out.is_empty() {
            continue;
        }
        let Ok(code) = code_line.trim().parse::<i32>() else {
            // A blank line at the very end is fine. Anything else is not a pair.
            return Ok((out, !code_line.trim().is_empty()));
        };
        let Some((n, value)) = lines.next() else {
            return Ok((out, true));
        };
        if value.len() > MAX_STRING {
            return Err(IoError::Format(format!(
                "the DXF file is damaged at line {}: a value is longer than {MAX_STRING} bytes",
                n + 1
            )));
        }
        if out.len() >= MAX_PAIRS {
            return Err(IoError::Format(format!(
                "the DXF file has more than {MAX_PAIRS} pairs (line {}): a file of this size is refused",
                n + 1
            )));
        }
        out.push((code, value.to_string()));
    }
}

/// Groups pairs into records, each starting at a code 0. Pairs before the first
/// code 0 are dropped.
fn records(items: &[(i32, String)]) -> Vec<Entity> {
    let mut out: Vec<Entity> = Vec::new();
    for (code, value) in items {
        if *code == 0 {
            out.push(Entity {
                kind: value.trim().to_string(),
                ..Entity::default()
            });
        } else if let Some(last) = out.last_mut() {
            last.items.push((*code, value.clone()));
        }
    }
    out
}

/// Joins VERTEX records to their POLYLINE and drops SEQEND.
fn nest_polylines(records: Vec<Entity>) -> Vec<Entity> {
    let mut out: Vec<Entity> = Vec::new();
    let mut open = false;
    for r in records {
        match r.kind.as_str() {
            "POLYLINE" => {
                out.push(r);
                open = true;
            }
            "VERTEX" if open => out.last_mut().expect("open polyline").children.push(r),
            "SEQEND" => open = false,
            _ => {
                open = false;
                out.push(r);
            }
        }
    }
    out
}

/// Parses a DXF file, text or binary.
pub fn parse(bytes: &[u8]) -> Result<Dxf, IoError> {
    let (items, bad_line) = if super::binary::is_binary(bytes) {
        super::binary::read_pairs(bytes)?
    } else {
        pairs(&decode(bytes))?
    };
    if !items.iter().any(|(c, v)| *c == 0 && v.trim() == "SECTION") {
        return Err(IoError::Format("not a DXF file: no SECTION found".into()));
    }
    let record_count = items.iter().filter(|(c, _)| *c == 0).count();
    if record_count > MAX_RECORDS {
        return Err(IoError::Format(format!(
            "the DXF file has {record_count} records, more than the limit of {MAX_RECORDS}"
        )));
    }
    let mut dxf = Dxf {
        truncated: bad_line,
        ..Dxf::default()
    };
    let mut i = 0;
    let mut saw_eof = false;
    while i < items.len() {
        let (code, value) = &items[i];
        i += 1;
        if *code == 0 && value.trim() == "EOF" {
            saw_eof = true;
            break;
        }
        if !(*code == 0 && value.trim() == "SECTION") {
            continue;
        }
        let name = match items.get(i) {
            Some((2, n)) => n.trim().to_string(),
            _ => String::new(),
        };
        let start = i + usize::from(!name.is_empty());
        let end = items[start..]
            .iter()
            .position(|(c, v)| *c == 0 && v.trim() == "ENDSEC")
            .map_or(items.len(), |p| start + p);
        let body = &items[start..end];
        match name.as_str() {
            "HEADER" => read_header(&mut dxf, body),
            "TABLES" => read_tables(&mut dxf, body),
            "BLOCKS" => read_blocks(&mut dxf, body),
            // A file that was joined from several files can have more than one section.
            "ENTITIES" => dxf.entities.extend(nest_polylines(records(body))),
            "OBJECTS" => {
                let recs = records(body);
                dxf.geodata = recs.iter().find(|e| e.kind == "GEODATA").map(|e| GeoData {
                    design: [e.fd(10, 0.0), e.fd(20, 0.0)],
                    reference: [e.fd(11, 0.0), e.fd(21, 0.0)],
                    north: [e.fd(12, 0.0), e.fd(22, 1.0)],
                    definition: e
                        .items
                        .iter()
                        .filter(|(c, _)| *c == 301 || *c == 303)
                        .map(|(_, v)| v.as_str())
                        .collect(),
                });
                dxf.objects = recs
                    .into_iter()
                    .filter(|e| match e.kind.as_str() {
                        "LAYOUT" | "IMAGEDEF" => true,
                        // The dictionary of a clipped block reference (XCLIP) has the entry ACAD_FILTER.
                        "DICTIONARY" => e.items.iter().any(|(c, v)| *c == 3 && v == "ACAD_FILTER"),
                        _ => false,
                    })
                    .collect();
            }
            _ => {}
        }
        i = (end + 1).min(items.len());
        if end >= items.len() {
            dxf.truncated = true;
        }
    }
    if !saw_eof {
        dxf.truncated = true;
    }
    Ok(dxf)
}

fn read_header(dxf: &mut Dxf, body: &[(i32, String)]) {
    let mut current: Option<String> = None;
    for (code, value) in body {
        if *code == 9 {
            current = Some(value.trim().to_string());
            dxf.header.entry(value.trim().to_string()).or_default();
        } else if let Some(var) = &current {
            dxf.header
                .get_mut(var)
                .expect("inserted")
                .push((*code, value.clone()));
        }
    }
}

fn read_tables(dxf: &mut Dxf, body: &[(i32, String)]) {
    for r in records(body).into_iter().filter(|r| r.kind == "LAYER") {
        let Some(name) = r.get(2) else { continue };
        dxf.layers.push(LayerDef {
            name: name.to_string(),
            handle: r.handle(),
            color: r.int(62).unwrap_or(7),
            true_color: r.int(420).and_then(|v| u32::try_from(v).ok()),
            frozen: r.int(70).unwrap_or(0) & 1 != 0,
            lineweight: r.int(370).unwrap_or(-3),
        });
    }
}

fn read_blocks(dxf: &mut Dxf, body: &[(i32, String)]) {
    let mut current: Option<(String, Block)> = None;
    for r in nest_polylines(records(body)) {
        match r.kind.as_str() {
            "BLOCK" => {
                let name = r.get(2).unwrap_or("").to_string();
                let base = [r.fd(10, 0.0), r.fd(20, 0.0), r.fd(30, 0.0)];
                current = Some((
                    name,
                    Block {
                        handle: r.handle(),
                        record: r.get(330).and_then(super::hash::normal_handle),
                        base,
                        flags: r.int(70).unwrap_or(0),
                        path: r.get(1).unwrap_or("").to_string(),
                        entities: vec![],
                    },
                ));
            }
            "ENDBLK" => {
                if let Some((name, block)) = current.take() {
                    dxf.blocks.insert(name, block);
                }
            }
            _ => {
                if let Some((_, block)) = current.as_mut() {
                    block.entities.push(r);
                }
            }
        }
    }
    // A block without ENDBLK at the end of a cut-off file.
    if let Some((name, block)) = current {
        dxf.blocks.insert(name, block);
        dxf.truncated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "0\r\nSECTION\r\n2\r\nHEADER\r\n9\r\n$INSUNITS\r\n70\r\n6\r\n0\r\nENDSEC\r\n\
0\r\nSECTION\r\n2\r\nTABLES\r\n0\r\nTABLE\r\n2\r\nLAYER\r\n0\r\nLAYER\r\n2\r\nWalls\r\n70\r\n1\r\n62\r\n-5\r\n370\r\n50\r\n0\r\nLAYER\r\n70\r\n0\r\n0\r\nENDTAB\r\n0\r\nENDSEC\r\n\
0\r\nSECTION\r\n2\r\nBLOCKS\r\n0\r\nBLOCK\r\n2\r\nB1\r\n10\r\n1.0\r\n20\r\n2\r\n0\r\nLINE\r\n8\r\n0\r\n0\r\nENDBLK\r\n0\r\nENDSEC\r\n\
0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nLINE\r\n8\r\nWalls\r\n10\r\n1.5D+2\r\n11\r\n5\r\n0\r\nPOLYLINE\r\n70\r\n8\r\n0\r\nVERTEX\r\n10\r\n1\r\n0\r\nVERTEX\r\n10\r\n2\r\n0\r\nSEQEND\r\n0\r\nCIRCLE\r\n0\r\nENDSEC\r\n0\r\nEOF\r\n";

    #[test]
    fn parses_sections() {
        let d = parse(SAMPLE.as_bytes()).unwrap();
        assert!(!d.truncated);
        assert_eq!(d.header_value("$INSUNITS"), Some("6"));
        assert_eq!(d.header_value("$NOPE"), None);
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.layers[0].name, "Walls");
        assert!(d.layers[0].frozen);
        assert_eq!(d.layers[0].color, -5);
        assert_eq!(d.layers[0].lineweight, 50);
        assert_eq!(d.blocks["B1"].base, [1.0, 2.0, 0.0]);
        assert_eq!(d.blocks["B1"].entities.len(), 1);
        assert_eq!(d.entities.len(), 3);
        assert_eq!(d.entities[0].f(10), Some(150.0));
        assert_eq!(d.entities[0].layer(), "Walls");
        assert_eq!(d.entities[1].children.len(), 2);
        assert_eq!(d.entities[2].layer(), "0");
        assert_eq!(d.entities[2].normal(), [0.0, 0.0, 1.0]);
        assert_eq!(d.entities[0].all_f(10), vec![150.0]);
        assert_eq!(d.entities[0].fd(99, 7.0), 7.0);
        assert_eq!(d.entities[0].int(11), Some(5));
    }

    #[test]
    fn truncation_is_reported_not_fatal() {
        for cut in [20, 100, 200, SAMPLE.len() - 12] {
            let d = parse(&SAMPLE.as_bytes()[..cut]);
            if let Ok(d) = d {
                assert!(d.truncated, "cut at {cut}");
            }
        }
        // No trailing EOF but otherwise complete.
        let d = parse(SAMPLE.trim_end_matches("0\r\nEOF\r\n").as_bytes()).unwrap();
        assert!(d.truncated);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"hello").is_err());
        assert!(parse(b"").is_err());
        assert!(parse(b"AutoCAD Binary DXF\r\n\x1a\0").is_err());
        assert!(parse(b"AutoCAD Binary DXF").is_err());
        assert!(parse(&[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn decodes_latin1_and_bom() {
        let mut bytes = b"\xEF\xBB\xBF0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\nK".to_vec();
        bytes.push(0xE4);
        bytes.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
        let d = parse(&bytes).unwrap();
        assert_eq!(d.entities[0].get(1), Some("K\u{e4}"));
        let utf = parse("0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\nKä\n0\nENDSEC\n0\nEOF\n".as_bytes())
            .unwrap();
        assert_eq!(utf.entities[0].get(1), Some("Kä"));
    }

    #[test]
    fn decodes_the_header_code_page() {
        // CP1257 "\u{160}\u{f5}\u{f5}": bytes D0 F5 F5. Latin-1 would give "D\u{f5}\u{f5}" with a different first letter.
        let mut bytes = b"0\nSECTION\n2\nHEADER\n9\n$DWGCODEPAGE\n3\nANSI_1257\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\n".to_vec();
        bytes.extend_from_slice(&[0xD0, 0xF5, 0xF5]);
        bytes.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
        let d = parse(&bytes).unwrap();
        assert_eq!(d.entities[0].get(1), Some("\u{160}\u{f5}\u{f5}"));
        // An unknown code page falls back to Latin-1.
        let mut b2 = Vec::new();
        b2.extend_from_slice(b"0\nSECTION\n2\nHEADER\n9\n$DWGCODEPAGE\n3\nUNDEFINED\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\n");
        b2.push(0xE4);
        b2.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
        assert_eq!(parse(&b2).unwrap().entities[0].get(1), Some("\u{e4}"));
        // A DOS code page reads the byte as the character of that page.
        let mut b3 = Vec::new();
        b3.extend_from_slice(b"0\nSECTION\n2\nHEADER\n9\n$DWGCODEPAGE\n3\nDOS437\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\n");
        b3.push(0x84);
        b3.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
        assert_eq!(parse(&b3).unwrap().entities[0].get(1), Some("\u{e4}"));
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_f64(" 1.5 "), Some(1.5));
        assert_eq!(parse_f64("2D1"), Some(20.0));
        assert_eq!(parse_f64("nan"), None);
        assert_eq!(parse_f64("inf"), None);
        assert_eq!(parse_int("12"), Some(12));
        assert_eq!(parse_int("12.7"), Some(12));
        assert_eq!(parse_int("x"), None);
    }

    #[test]
    fn unterminated_section_and_stray_pairs() {
        let d = parse(b"5\nstray\n0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n1\n").unwrap();
        assert_eq!(d.entities.len(), 1);
        assert!(d.truncated);
        let d = parse(b"0\nSECTION\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nLINE\n0\nENDSEC\n0\nEOF\n").unwrap();
        assert!(d.truncated);
        assert_eq!(d.blocks["A"].entities.len(), 1);
    }

    #[test]
    fn geodata_names_its_epsg_code() {
        let mut g = GeoData {
            design: [0.0; 2],
            reference: [0.0; 2],
            north: [0.0, 1.0],
            definition: "<Alias id=\"EPSG:3067\">".into(),
        };
        assert_eq!(g.epsg().as_deref(), Some("EPSG:3067"));
        g.definition = "no code EPSG:x".into();
        assert_eq!(g.epsg(), None);
        g.definition.clear();
        assert_eq!(g.epsg(), None);
    }
}
