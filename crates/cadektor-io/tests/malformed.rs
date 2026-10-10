//! Random and damaged input must never panic. If an import succeeds, its batch must run.

use cadektor_core::command::Editor;
use cadektor_core::model::Document;
use cadektor_crs::Crs;
use cadektor_io::{Context, Format, ImportOptions, Source, importer};
use proptest::prelude::*;

const DXF: &[u8] = include_bytes!("fixtures/site.dxf");
const GEOJSON: &[u8] = include_bytes!("fixtures/trees.geojson");

fn shapefile() -> Vec<u8> {
    // One polygon with a hole-like second ring, hand-made little-endian records.
    let mut shp = vec![0u8; 100];
    shp[..4].copy_from_slice(&9994i32.to_be_bytes());
    shp[28..32].copy_from_slice(&1000i32.to_le_bytes());
    shp[32..36].copy_from_slice(&5i32.to_le_bytes());
    let ring = [
        (0.0f64, 0.0f64),
        (0.0, 10.0),
        (10.0, 10.0),
        (10.0, 0.0),
        (0.0, 0.0),
    ];
    let mut s = 5i32.to_le_bytes().to_vec();
    s.extend([0u8; 32]);
    s.extend(1i32.to_le_bytes());
    s.extend((ring.len() as i32).to_le_bytes());
    s.extend(0i32.to_le_bytes());
    for (x, y) in ring {
        s.extend(x.to_le_bytes());
        s.extend(y.to_le_bytes());
    }
    shp.extend(1i32.to_be_bytes());
    shp.extend(((s.len() / 2) as i32).to_be_bytes());
    shp.extend(s);
    shp
}

fn try_import(format: Format, data: Vec<u8>) {
    let src = Source {
        name: "fuzz".into(),
        data,
        extra: Default::default(),
    };
    let doc = Document::default();
    if let Ok(r) = importer(format).import(&src, &Context::of(&doc), &ImportOptions::default()) {
        let mut ed = Editor::new(doc);
        ed.execute(&r.command)
            .expect("a successful import must run");
        assert!(
            ed.doc()
                .objects
                .values()
                .all(|o| o.is_finite() && o.shape.is_in_range())
        );
        if ed.can_undo() {
            assert!(ed.undo());
        }
    }
}

fn all_formats(data: &[u8]) {
    for f in [Format::Dxf, Format::Shapefile, Format::GeoJson] {
        try_import(f, data.to_vec());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn random_bytes(data in proptest::collection::vec(any::<u8>(), 0..2000)) {
        all_formats(&data);
    }

    #[test]
    fn random_text_with_dxf_shape(lines in proptest::collection::vec(("[0-9]{1,3}", "[A-Za-z0-9 .+-]{0,10}"), 0..200)) {
        let mut text = String::from("0\nSECTION\n2\nENTITIES\n");
        for (c, v) in lines {
            text += &format!("{c}\n{v}\n");
        }
        try_import(Format::Dxf, text.into_bytes());
    }

    #[test]
    fn truncated_files(cut in 0.0..1.0f64) {
        for (f, data) in [(Format::Dxf, DXF.to_vec()), (Format::GeoJson, GEOJSON.to_vec()), (Format::Shapefile, shapefile())] {
            let n = (data.len() as f64 * cut) as usize;
            try_import(f, data[..n].to_vec());
        }
    }

    #[test]
    fn mutated_files(pos in 0.0..1.0f64, byte in any::<u8>(), count in 1usize..8) {
        for (f, data) in [(Format::Dxf, DXF.to_vec()), (Format::GeoJson, GEOJSON.to_vec()), (Format::Shapefile, shapefile())] {
            let mut d = data;
            for k in 0..count {
                let i = (((pos + k as f64 * 0.137) % 1.0) * d.len() as f64) as usize;
                let last = d.len() - 1;
                d[i.min(last)] = byte.wrapping_add(k as u8);
            }
            try_import(f, d);
        }
    }

    #[test]
    fn random_crs_text(text in ".{0,200}") {
        let _ = Crs::parse(&text);
    }

    #[test]
    fn random_wkt_like_text(parts in proptest::collection::vec("(PROJCS|GEOGCS|DATUM|SPHEROID|PARAMETER|UNIT|PROJECTION)?[\\[\\]\",0-9a-z. ]{0,12}", 0..30)) {
        let _ = Crs::parse(&parts.concat());
    }
}

#[test]
fn valid_fixtures_import() {
    for (f, d) in [
        (Format::Dxf, DXF.to_vec()),
        (Format::GeoJson, GEOJSON.to_vec()),
        (Format::Shapefile, shapefile()),
    ] {
        let src = Source {
            name: "x".into(),
            data: d,
            extra: Default::default(),
        };
        let r = importer(f)
            .import(
                &src,
                &Context::of(&Document::default()),
                &ImportOptions::default(),
            )
            .unwrap();
        assert!(r.report.total() > 0);
    }
}

#[test]
fn hostile_numbers_are_skipped_not_fatal() {
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n1e300\n20\n0\n11\n0\n21\n0\n0\nCIRCLE\n10\n0\n20\n0\n40\n1e308\n0\nARC\n10\n0\n20\n0\n40\n5\n50\n1e308\n51\n3\n0\nLINE\n10\nnan\n20\ninf\n0\nENDSEC\n0\nEOF\n";
    try_import(Format::Dxf, dxf.as_bytes().to_vec());
    let json = r#"{"type":"Point","coordinates":[1e999, 5]}"#;
    try_import(Format::GeoJson, json.as_bytes().to_vec());
    let json = r#"{"type":"Point","coordinates":[1e300, 5e300]}"#;
    try_import(Format::GeoJson, json.as_bytes().to_vec());
    let json = format!("{}1{}", "[".repeat(5000), "]".repeat(5000));
    try_import(Format::GeoJson, json.into_bytes());
}
