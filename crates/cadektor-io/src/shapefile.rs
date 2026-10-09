//! ESRI Shapefile import (`.shp` with `.dbf` and `.prj`).
//!
//! Supported shape types: point, multipoint, polyline and polygon, with or
//! without Z or M values. Multipatch shapes are skipped. A polygon ring that runs
//! clockwise is an outer ring, and a counterclockwise ring is a hole.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::gis::{self, Feature, GisData, Part, Pt};
use crate::{Context, Format, ImportOptions, ImportResult, Importer, IoError, Source};

pub struct ShapefileFormat;

impl Importer for ShapefileFormat {
    fn format(&self) -> Format {
        Format::Shapefile
    }

    fn import(&self, src: &Source, ctx: &Context, opts: &ImportOptions) -> Result<ImportResult, IoError> {
        let mut data = read(&src.data, src.extra.get("dbf").map(Vec::as_slice))?;
        data.file_crs = src.extra.get("prj").map(|b| String::from_utf8_lossy(b).trim().to_string()).filter(|s| !s.is_empty());
        let name = if src.name.is_empty() { "Shapefile" } else { &src.name };
        gis::build("shapefile", name, data, ctx, opts, None)
    }
}

/// Reads little-endian and big-endian numbers with bounds checks.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn arr<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        self.0.get(at..at.checked_add(N)?)?.try_into().ok()
    }
    fn i32le(&self, at: usize) -> Option<i32> {
        self.arr(at).map(i32::from_le_bytes)
    }
    fn i32be(&self, at: usize) -> Option<i32> {
        self.arr(at).map(i32::from_be_bytes)
    }
    fn f64le(&self, at: usize) -> Option<f64> {
        self.arr(at).map(f64::from_le_bytes)
    }
}

/// Reads the geometry of one record. `Ok(None)` is a null shape.
fn read_shape(rec: &[u8]) -> Result<Vec<Part>, &'static str> {
    let b = Bytes(rec);
    let t = b.i32le(0).ok_or("record is too short")?;
    let has_z = matches!(t, 11 | 13 | 15 | 18);
    match t {
        0 => Ok(vec![]),
        1 | 11 | 21 => {
            let (x, y) = (b.f64le(4).ok_or("record is too short")?, b.f64le(12).ok_or("record is too short")?);
            let z = if has_z { Some(b.f64le(20).ok_or("record is too short")?).filter(|z| *z > -1.0e38) } else { None };
            Ok(vec![Part::Point(Pt { x, y, z })])
        }
        8 | 18 | 28 => {
            let n = usize::try_from(b.i32le(36).ok_or("record is too short")?).map_err(|_| "negative point count")?;
            if n > rec.len() / 16 {
                return Err("point count is larger than the record");
            }
            let pts = read_points(&b, 40, n, has_z, 40 + n * 16)?;
            Ok(pts.into_iter().map(Part::Point).collect())
        }
        3 | 13 | 23 | 5 | 15 | 25 => {
            let polygon = matches!(t, 5 | 15 | 25);
            let parts_n = usize::try_from(b.i32le(36).ok_or("record is too short")?).map_err(|_| "negative part count")?;
            let pts_n = usize::try_from(b.i32le(40).ok_or("record is too short")?).map_err(|_| "negative point count")?;
            if parts_n > rec.len() / 4 || pts_n > rec.len() / 16 {
                return Err("counts are larger than the record");
            }
            let mut starts = Vec::with_capacity(parts_n);
            for i in 0..parts_n {
                let s = usize::try_from(b.i32le(44 + i * 4).ok_or("record is too short")?).map_err(|_| "negative part start")?;
                if s > pts_n || starts.last().is_some_and(|l| s < *l) {
                    return Err("part starts are not in order");
                }
                starts.push(s);
            }
            let pts_at = 44 + parts_n * 4;
            let pts = read_points(&b, pts_at, pts_n, has_z, pts_at + pts_n * 16)?;
            let mut out = Vec::new();
            for (i, s) in starts.iter().enumerate() {
                let end = starts.get(i + 1).copied().unwrap_or(pts_n);
                let ring = pts[*s..end].to_vec();
                if ring.len() < 2 {
                    continue;
                }
                if polygon {
                    // Shoelace sum: positive for clockwise rings.
                    let area: f64 = ring.windows(2).map(|w| (w[1].x - w[0].x) * (w[1].y + w[0].y)).sum();
                    out.push(Part::Ring(ring, area < 0.0));
                } else {
                    out.push(Part::Line(ring));
                }
            }
            Ok(out)
        }
        31 => Err("multipatch shapes are not supported"),
        _ => Err("unknown shape type"),
    }
}

/// `n` points at `at`, with Z values after the bounds range if `has_z`.
fn read_points(b: &Bytes, at: usize, n: usize, has_z: bool, z_block: usize) -> Result<Vec<Pt>, &'static str> {
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let x = b.f64le(at + i * 16).ok_or("record is too short")?;
        let y = b.f64le(at + i * 16 + 8).ok_or("record is too short")?;
        pts.push(Pt { x, y, z: None });
    }
    if has_z {
        // Zmin, Zmax, then one Z per point.
        for (i, p) in pts.iter_mut().enumerate() {
            let z = b.f64le(z_block + 16 + i * 8).ok_or("record is too short")?;
            p.z = Some(z).filter(|z| *z > -1.0e38);
        }
    }
    Ok(pts)
}

/// Reads the shapes of a `.shp` file and the records of its `.dbf`.
pub fn read(shp: &[u8], dbf: Option<&[u8]>) -> Result<GisData, IoError> {
    let b = Bytes(shp);
    if shp.len() < 100 || b.i32be(0) != Some(9994) {
        return Err(IoError::Format("not a shapefile: wrong file code".into()));
    }
    if b.i32le(28) != Some(1000) {
        return Err(IoError::Format("not a shapefile: wrong version".into()));
    }
    let mut data = GisData::default();
    let attrs = dbf.map(read_dbf).transpose()?;
    let mut pos = 100usize;
    let mut index = 0usize;
    while pos + 8 <= shp.len() {
        let len = match b.i32be(pos + 4) {
            Some(l) if l >= 2 => l as usize * 2,
            _ => {
                data.notes.push("the shapefile has a bad record: the rest is ignored".into());
                break;
            }
        };
        let Some(rec) = shp.get(pos + 8..pos + 8 + len) else {
            data.notes.push("the shapefile ends early: imported what could be read".into());
            break;
        };
        pos += 8 + len;
        let record = attrs.as_ref().and_then(|a| a.get(index));
        index += 1;
        if let Some(None) = record {
            data.skipped.push(("record".into(), "deleted in the dbf file".into()));
            continue;
        }
        match read_shape(rec) {
            Ok(parts) if parts.is_empty() => data.skipped.push(("record".into(), "null shape".into())),
            Ok(parts) => {
                let props = record.cloned().flatten().unwrap_or_default();
                data.features.push(Feature { parts, props });
            }
            Err(why) => data.skipped.push(("record".into(), why.into())),
        }
    }
    if let Some(a) = &attrs
        && a.len() != index
    {
        data.notes.push(format!("the dbf file has {} records, the shapefile has {index}", a.len()));
    }
    // Index of a feature is its record number.
    Ok(data)
}

/// Text from a dbf field: UTF-8 when valid, else Latin-1.
fn text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// The records of a dbf file. A deleted record is `None`.
fn read_dbf(d: &[u8]) -> Result<Vec<Option<BTreeMap<String, Value>>>, IoError> {
    let b = Bytes(d);
    let bad = || IoError::Format("the dbf file is damaged".into());
    let count = b.i32le(4).filter(|c| *c >= 0).ok_or_else(bad)? as usize;
    let header_len = b.arr::<2>(8).map(u16::from_le_bytes).ok_or_else(bad)? as usize;
    let rec_len = b.arr::<2>(10).map(u16::from_le_bytes).ok_or_else(bad)? as usize;
    if header_len < 33 || rec_len == 0 || header_len > d.len() {
        return Err(bad());
    }
    struct Field {
        name: String,
        kind: u8,
        len: usize,
    }
    let mut fields = Vec::new();
    let mut at = 32;
    while at + 32 <= header_len && d[at] != 0x0D {
        let raw = &d[at..at + 11];
        let name = text(&raw[..raw.iter().position(|c| *c == 0).unwrap_or(11)]);
        fields.push(Field { name, kind: d[at + 11], len: d[at + 16] as usize });
        at += 32;
    }
    let mut out = Vec::new();
    for i in 0..count {
        let start = header_len + i * rec_len;
        let Some(rec) = d.get(start..start + rec_len) else { break };
        if rec[0] == b'*' {
            out.push(None);
            continue;
        }
        let mut props = BTreeMap::new();
        let mut off = 1;
        for f in &fields {
            let Some(raw) = rec.get(off..off + f.len) else { break };
            off += f.len;
            let s = text(raw);
            let s = s.trim_matches(|c: char| c == ' ' || c == '\0');
            let v = match f.kind {
                b'N' | b'F' | b'O' | b'I' => match s.parse::<i64>() {
                    Ok(n) => Value::from(n),
                    Err(_) => s.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number),
                },
                b'L' => match s.to_ascii_lowercase().as_str() {
                    "t" | "y" => Value::Bool(true),
                    "f" | "n" => Value::Bool(false),
                    _ => Value::Null,
                },
                b'D' if s.len() == 8 && s.bytes().all(|c| c.is_ascii_digit()) => Value::from(format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..])),
                _ if s.is_empty() => Value::Null,
                _ => Value::from(s),
            };
            props.insert(f.name.clone(), v);
        }
        out.push(Some(props));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn rec(shape: Vec<u8>) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1i32.to_be_bytes());
        out.extend_from_slice(&((shape.len() / 2) as i32).to_be_bytes());
        out.extend_from_slice(&shape);
        out
    }

    fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    pub(crate) fn header(shape_type: i32) -> Vec<u8> {
        let mut h = vec![0u8; 100];
        h[..4].copy_from_slice(&9994i32.to_be_bytes());
        h[28..32].copy_from_slice(&1000i32.to_le_bytes());
        h[32..36].copy_from_slice(&shape_type.to_le_bytes());
        h
    }

    pub(crate) fn point_shp(points: &[(f64, f64)]) -> Vec<u8> {
        let mut out = header(1);
        for (x, y) in points {
            let mut s = 1i32.to_le_bytes().to_vec();
            s.extend(f64s(&[*x, *y]));
            out.extend(rec(s));
        }
        out
    }

    /// Polyline or polygon record. `z` adds Z values (type + 10).
    pub(crate) fn poly_record(t: i32, parts: &[&[(f64, f64)]], z: Option<f64>) -> Vec<u8> {
        let n: usize = parts.iter().map(|p| p.len()).sum();
        let mut s = t.to_le_bytes().to_vec();
        s.extend(f64s(&[0.0; 4]));
        s.extend((parts.len() as i32).to_le_bytes());
        s.extend((n as i32).to_le_bytes());
        let mut start = 0;
        for p in parts {
            s.extend((start as i32).to_le_bytes());
            start += p.len();
        }
        for p in parts {
            for (x, y) in *p {
                s.extend(f64s(&[*x, *y]));
            }
        }
        if let Some(z) = z {
            s.extend(f64s(&[z, z]));
            for _ in 0..n {
                s.extend(f64s(&[z]));
            }
        }
        rec(s)
    }

    pub(crate) fn dbf(fields: &[(&str, u8, usize)], rows: &[(bool, Vec<&str>)]) -> Vec<u8> {
        let rec_len = 1 + fields.iter().map(|f| f.2).sum::<usize>();
        let header_len = 32 + fields.len() * 32 + 1;
        let mut d = vec![0u8; 32];
        d[0] = 3;
        d[4..8].copy_from_slice(&(rows.len() as i32).to_le_bytes());
        d[8..10].copy_from_slice(&(header_len as u16).to_le_bytes());
        d[10..12].copy_from_slice(&(rec_len as u16).to_le_bytes());
        for (name, kind, len) in fields {
            let mut f = vec![0u8; 32];
            f[..name.len()].copy_from_slice(name.as_bytes());
            f[11] = *kind;
            f[16] = *len as u8;
            d.extend(f);
        }
        d.push(0x0D);
        for (live, vals) in rows {
            d.push(if *live { b' ' } else { b'*' });
            for (v, (_, _, len)) in vals.iter().zip(fields) {
                let mut cell = v.as_bytes().to_vec();
                cell.resize(*len, b' ');
                d.extend(cell);
            }
        }
        d
    }

    #[test]
    fn points_with_attributes_and_deleted_records() {
        let shp = point_shp(&[(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)]);
        let d = dbf(
            &[("NAME", b'C', 8), ("H", b'N', 6), ("W", b'F', 8), ("OK", b'L', 1), ("DAY", b'D', 8), ("X", b'C', 3)],
            &[
                (true, vec!["Birch", "12", "1.5", "T", "20240131", ""]),
                (false, vec!["Gone", "1", "1", "F", "20240101", "a"]),
                (true, vec!["K\u{e4}pyl\u{e4}", "", "abc", "?", "bad", "x"]),
            ],
        );
        let data = read(&shp, Some(&d)).unwrap();
        assert_eq!(data.features.len(), 2);
        let p = &data.features[0].props;
        assert_eq!(p["NAME"], "Birch");
        assert_eq!(p["H"], 12);
        assert_eq!(p["W"], 1.5);
        assert_eq!(p["OK"], true);
        assert_eq!(p["DAY"], "2024-01-31");
        assert_eq!(p["X"], Value::Null);
        let q = &data.features[1].props;
        assert_eq!(q["NAME"], "K\u{e4}pyl\u{e4}");
        assert_eq!(q["H"], Value::Null);
        assert_eq!(q["W"], Value::Null);
        assert_eq!(q["OK"], Value::Null);
        assert_eq!(q["DAY"], "bad");
        assert_eq!(data.skipped.len(), 1);
        assert_eq!(data.features[1].parts, vec![Part::Point(Pt { x: 5.0, y: 6.0, z: None })]);
    }

    #[test]
    fn latin1_names_and_count_mismatch() {
        let shp = point_shp(&[(1.0, 2.0)]);
        let mut d = dbf(&[("N", b'C', 3)], &[(true, vec!["abc"]), (true, vec!["def"])]);
        let at = d.len() - 8;
        d[at + 1] = 0xE4;
        let data = read(&shp, Some(&d)).unwrap();
        assert_eq!(data.features[0].props["N"], "\u{e4}bc");
        assert!(data.notes[0].contains("2 records"));
    }

    #[test]
    fn polylines_polygons_holes_and_z() {
        let outer: &[(f64, f64)] = &[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0), (0.0, 0.0)];
        let hole: &[(f64, f64)] = &[(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0), (2.0, 2.0)];
        let mut shp = header(5);
        shp.extend(poly_record(5, &[outer, hole], None));
        shp.extend(poly_record(3, &[&[(0.0, 0.0), (1.0, 1.0)], &[(5.0, 5.0), (6.0, 5.0), (7.0, 5.0)]], None));
        shp.extend(poly_record(13, &[&[(0.0, 0.0), (1.0, 1.0)]], Some(7.5)));
        shp.extend(poly_record(15, &[outer], Some(2.0)));
        let data = read(&shp, None).unwrap();
        assert_eq!(data.features.len(), 4);
        let Part::Ring(_, h0) = &data.features[0].parts[0] else { panic!() };
        let Part::Ring(_, h1) = &data.features[0].parts[1] else { panic!() };
        assert!(!h0 && *h1);
        assert_eq!(data.features[1].parts.len(), 2);
        let Part::Line(l) = &data.features[2].parts[0] else { panic!() };
        assert_eq!(l[0].z, Some(7.5));
        let Part::Ring(r, _) = &data.features[3].parts[0] else { panic!() };
        assert_eq!(r[0].z, Some(2.0));
    }

    #[test]
    fn multipoint_z_point_and_special_records() {
        let mut shp = header(8);
        // Multipoint with 2 points.
        let mut s = 8i32.to_le_bytes().to_vec();
        s.extend(f64s(&[0.0; 4]));
        s.extend(2i32.to_le_bytes());
        s.extend(f64s(&[1.0, 2.0, 3.0, 4.0]));
        shp.extend(rec(s));
        // PointZ with a no-data Z.
        let mut s = 11i32.to_le_bytes().to_vec();
        s.extend(f64s(&[5.0, 6.0, -1.0e39, 0.0]));
        shp.extend(rec(s));
        let mut s = 11i32.to_le_bytes().to_vec();
        s.extend(f64s(&[5.0, 6.0, 9.0, 0.0]));
        shp.extend(rec(s));
        // Null, multipatch, unknown.
        for t in [0i32, 31, 99] {
            let mut s = t.to_le_bytes().to_vec();
            s.extend([0u8; 60]);
            shp.extend(rec(s));
        }
        let data = read(&shp, None).unwrap();
        assert_eq!(data.features.len(), 3);
        assert_eq!(data.features[0].parts.len(), 2);
        assert_eq!(data.features[1].parts[0], Part::Point(Pt { x: 5.0, y: 6.0, z: None }));
        assert_eq!(data.features[2].parts[0], Part::Point(Pt { x: 5.0, y: 6.0, z: Some(9.0) }));
        assert_eq!(data.skipped.len(), 3);
    }

    #[test]
    fn malformed_files_do_not_panic() {
        assert!(read(b"", None).is_err());
        assert!(read(&[0u8; 100], None).is_err());
        let mut bad_version = header(1);
        bad_version[28] = 5;
        assert!(read(&bad_version, None).is_err());
        let shp = point_shp(&[(1.0, 2.0), (3.0, 4.0)]);
        for cut in 0..shp.len() {
            let _ = read(&shp[..cut], None);
        }
        let d = dbf(&[("N", b'C', 3)], &[(true, vec!["abc"])]);
        for cut in 0..d.len() {
            let _ = read(&shp, Some(&d[..cut]));
        }
        // Bad record length.
        let mut bad = header(1);
        bad.extend(1i32.to_be_bytes());
        bad.extend(0i32.to_be_bytes());
        let data = read(&bad, None).unwrap();
        assert!(data.notes[0].contains("bad record"));
        // Huge counts.
        let mut s = 3i32.to_le_bytes().to_vec();
        s.extend(f64s(&[0.0; 4]));
        s.extend(i32::MAX.to_le_bytes());
        s.extend(i32::MAX.to_le_bytes());
        let mut shp = header(3);
        shp.extend(rec(s.clone()));
        assert_eq!(read(&shp, None).unwrap().skipped.len(), 1);
        // Negative counts and unordered parts.
        let mut s2 = s.clone();
        s2[36..40].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(read_shape(&s2).is_err());
        let mut s3 = 3i32.to_le_bytes().to_vec();
        s3.extend(f64s(&[0.0; 4]));
        s3.extend(2i32.to_le_bytes());
        s3.extend(2i32.to_le_bytes());
        s3.extend(1i32.to_le_bytes());
        s3.extend(0i32.to_le_bytes());
        s3.extend(f64s(&[0.0; 4]));
        assert!(read_shape(&s3).is_err());
        let mut s4 = s3.clone();
        s4[44..48].copy_from_slice(&0i32.to_le_bytes());
        s4[48..52].copy_from_slice(&9i32.to_le_bytes());
        assert!(read_shape(&s4).is_err());
        let mut s5 = 8i32.to_le_bytes().to_vec();
        s5.extend(f64s(&[0.0; 4]));
        s5.extend(i32::MAX.to_le_bytes());
        assert!(read_shape(&s5).is_err());
        s5[36..40].copy_from_slice(&(-3i32).to_le_bytes());
        assert!(read_shape(&s5).is_err());
        // PolyLineZ whose Z block is cut off.
        let full = poly_record(13, &[&[(0.0, 0.0), (1.0, 1.0)]], Some(1.0));
        assert!(read_shape(&full[8..full.len() - 8]).is_err());
        assert!(read_shape(&[1, 0]).is_err());
        // Header of dbf that is wrong.
        assert!(read(&point_shp(&[(1.0, 1.0)]), Some(&[0u8; 5])).is_err());
        let mut tiny = vec![0u8; 40];
        tiny[8] = 40;
        tiny[10] = 1;
        assert!(read(&point_shp(&[(1.0, 1.0)]), Some(&tiny)).is_ok());
    }

    #[test]
    fn import_through_the_trait() {
        let shp = point_shp(&[(24.9384, 60.1699)]);
        let d = dbf(&[("NAME", b'C', 5)], &[(true, vec!["Oak"])]);
        let prj = "GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]]";
        let src = Source {
            name: "trees".into(),
            data: shp.clone(),
            extra: BTreeMap::from([("dbf".to_string(), d), ("prj".to_string(), prj.as_bytes().to_vec())]),
        };
        let doc = cadektor_core::model::Document::default();
        let r = ShapefileFormat.import(&src, &Context::of(&doc), &ImportOptions::default()).unwrap();
        let mut ed = cadektor_core::command::Editor::new(doc);
        ed.execute(&r.command).unwrap();
        assert_eq!(ed.doc().georef.as_ref().unwrap().crs, "EPSG:32635");
        let o = ed.doc().objects.values().next().unwrap();
        assert_eq!(o.data["NAME"], "Oak");
        assert_eq!(ed.doc().class(o.class).unwrap().name, "trees");
        assert_eq!(ShapefileFormat.format(), Format::Shapefile);
        // Without a name and without prj.
        let src = Source { name: String::new(), data: shp, extra: BTreeMap::new() };
        let r = ShapefileFormat.import(&src, &Context::of(&ed.doc().clone()), &ImportOptions::default()).unwrap();
        assert!(r.report.classes_created.contains(&"Shapefile".to_string()));
    }
}
