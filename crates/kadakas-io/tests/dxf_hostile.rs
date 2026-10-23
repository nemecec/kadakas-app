//! Damaged and hostile DXF files (IOP-15). Each file must be refused with a message or imported
//! with a report that accounts for every entity. It must not panic, and it must finish in time.

mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use kadakas_core::command::Editor;
use kadakas_core::model::Document;
use kadakas_io::{Context, Format, ImportOptions, Source, importer};
use proptest::prelude::*;
use support::{corpus, dxf};

const LIMIT: Duration = Duration::from_secs(30);

/// Imports the bytes. `Ok(None)` is a refusal with a message. `Ok(Some(n))` is an import of n
/// objects whose batch ran. `Err` is a failure of the rules above.
fn run(data: &[u8], opts: &ImportOptions) -> Result<Option<usize>, String> {
    let src = Source {
        name: "hostile".into(),
        data: data.to_vec(),
        extra: Default::default(),
    };
    let doc = Document::with_sequential_ids();
    let start = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        importer(Format::Dxf).import(&src, &Context::of(&doc), opts)
    }))
    .map_err(|_| "panic in the importer".to_string())?;
    if start.elapsed() > LIMIT {
        return Err(format!("took {:?}", start.elapsed()));
    }
    match result {
        Err(e) if e.to_string().trim().is_empty() => Err("a refusal without a message".into()),
        Err(_) => Ok(None),
        Ok(r) => {
            let rep = &r.report;
            if rep.entities_read != rep.entities_imported + rep.entities_skipped {
                return Err(format!("entities are lost: {rep:?}"));
            }
            let mut ed = Editor::new(doc);
            catch_unwind(AssertUnwindSafe(|| ed.execute(&r.command)))
                .map_err(|_| "panic while the batch ran".to_string())?
                .map_err(|e| format!("the batch failed: {e}"))?;
            if !ed
                .doc()
                .objects
                .values()
                .all(|o| o.is_finite() && o.shape.is_in_range())
            {
                return Err("an object has a bad number".into());
            }
            Ok(Some(rep.total()))
        }
    }
}

fn ok(data: &[u8]) -> Option<usize> {
    run(data, &ImportOptions::default()).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn the_fuzzer_files_of_gdal_are_refused_cleanly() {
    for name in [
        "clusterfuzz-testcase-minimized-dxf_fuzzer-5400376672124928.dxf",
        "clusterfuzz-testcase-minimized-shape_fuzzer-6126814756995072.dxf",
        "insert-too-many-errors.dxf",
    ] {
        let Some(data) = corpus::gdal(name) else {
            continue;
        };
        let r = ok(&data);
        // The first two are not DXF files: a refusal. The third imports no object.
        if name.starts_with("clusterfuzz") {
            assert!(r.is_none(), "{name}");
        }
    }
}

#[test]
fn huge_counts_in_entities_do_not_make_the_import_run_long() {
    let big = "2000000000";
    let entities = [
        format!("0\nLWPOLYLINE\n90\n{big}\n70\n0\n10\n0\n20\n0\n10\n1\n20\n1\n"),
        format!("0\nHATCH\n70\n1\n91\n{big}\n92\n7\n72\n0\n73\n1\n93\n{big}\n10\n0\n20\n0\n10\n1\n20\n0\n10\n1\n20\n1\n97\n{big}\n"),
        format!("0\nHATCH\n70\n1\n91\n1\n92\n1\n93\n{big}\n72\n4\n94\n3\n73\n0\n74\n0\n95\n{big}\n96\n{big}\n40\n0\n"),
        format!("0\nSPLINE\n70\n8\n71\n{big}\n72\n{big}\n73\n{big}\n74\n{big}\n40\n0\n10\n0\n20\n0\n"),
        String::from("0\nSPLINE\n70\n8\n71\n3\n73\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n10\n0\n20\n0\n10\n1\n20\n1\n10\n2\n20\n0\n10\n3\n20\n1\n42\n1e300\n"),
        // A negative face count, and a count near the largest number.
        "0\nMESH\n92\n3\n10\n0\n20\n0\n30\n0\n10\n1\n20\n0\n30\n0\n10\n0\n20\n1\n30\n0\n93\n2\n90\n-1\n90\n0\n".to_string(),
        "0\nMESH\n92\n3\n10\n0\n20\n0\n30\n0\n10\n1\n20\n0\n30\n0\n10\n0\n20\n1\n30\n0\n93\n2\n90\n9223372036854775807\n90\n0\n".to_string(),
        format!("0\nMESH\n92\n{big}\n10\n0\n20\n0\n30\n0\n93\n{big}\n90\n{big}\n"),
        format!("0\nPOLYLINE\n70\n16\n71\n{big}\n72\n{big}\n0\nVERTEX\n10\n0\n20\n0\n0\nSEQEND\n"),
        format!("0\nPOLYLINE\n70\n64\n71\n{big}\n72\n{big}\n0\nVERTEX\n70\n192\n10\n0\n20\n0\n0\nVERTEX\n70\n128\n71\n{big}\n72\n1\n73\n2\n0\nSEQEND\n"),
        format!("0\nINSERT\n2\nB\n70\n{big}\n71\n{big}\n44\n1\n45\n1\n"),
        "0\nINSERT\n2\nB\n41\n1e300\n42\n1e300\n43\n1e300\n10\n1e300\n20\n1e300\n".to_string(),
        "0\nCIRCLE\n40\n1e308\n10\n1e308\n20\n-1e308\n".to_string(),
        "0\nARC\n40\n1e-300\n50\n0\n51\n1e308\n10\n0\n20\n0\n".to_string(),
        "0\nELLIPSE\n10\n0\n20\n0\n11\n1e308\n21\n1e308\n40\n1e-300\n41\n0\n42\n1e308\n".to_string(),
        "0\nTEXT\n40\n1e308\n10\n1e308\n20\n0\n1\nx\n50\n1e308\n".to_string(),
        "0\nLINE\n10\nnan\n20\ninf\n11\n-inf\n21\n1e999\n".to_string(),
        "0\nWIPEOUT\n11\n1e308\n21\n1e308\n12\n1e308\n22\n1e308\n13\n1e-300\n23\n1e-300\n71\n2\n91\n2000000000\n14\n1\n24\n1\n".to_string(),
        "0\nLEADER\n76\n2000000000\n10\n0\n20\n0\n".to_string(),
        "0\nSOLID\n10\n1e308\n20\n1e308\n11\n-1e308\n21\n1e308\n12\n0\n22\n0\n13\n1\n23\n1\n210\n1\n220\n1\n230\n1\n".to_string(),
        "0\nCIRCLE\n210\n1e308\n220\n1e308\n230\n1e-300\n10\n0\n20\n0\n40\n5\n".to_string(),
    ];
    let blocks = "0\nBLOCK\n2\nB\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n";
    for e in &entities {
        let text = format!(
            "0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{e}0\nENDSEC\n0\nEOF\n"
        );
        let _ = ok(text.as_bytes());
    }
}

#[test]
fn blocks_that_nest_without_end_are_stopped() {
    // A chain of 400 blocks, each one inserting the next one.
    let mut blocks = String::new();
    for i in 0..400 {
        blocks.push_str(&format!("0\nBLOCK\n2\nB{i}\n0\nINSERT\n2\nB{}\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n", i + 1));
    }
    let text = format!(
        "0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nB0\n0\nENDSEC\n0\nEOF\n"
    );
    let n = ok(text.as_bytes());
    assert!(n.is_some());
    // Blocks with ten copies of the next level each: 10^12 copies, stopped by the limits.
    let mut blocks = String::new();
    for i in 0..12 {
        blocks.push_str(&format!("0\nBLOCK\n2\nC{i}\n"));
        for _ in 0..10 {
            blocks.push_str(&format!("0\nINSERT\n2\nC{}\n", i + 1));
        }
        blocks.push_str("0\nENDBLK\n");
    }
    blocks.push_str("0\nBLOCK\n2\nC12\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n");
    let text = format!(
        "0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nC0\n0\nENDSEC\n0\nEOF\n"
    );
    // The import is refused because the blocks make more objects than the limit.
    assert!(ok(text.as_bytes()).is_none());
    // A block that contains itself, and two that contain each other.
    let text = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nINSERT\n2\nA\n0\nENDBLK\n0\nBLOCK\n2\nP\n0\nINSERT\n2\nQ\n0\nENDBLK\n0\nBLOCK\n2\nQ\n0\nINSERT\n2\nP\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n0\nINSERT\n2\nP\n0\nENDSEC\n0\nEOF\n";
    assert!(ok(text.as_bytes()).is_some());
}

#[test]
fn dimensions_that_point_at_their_own_block_are_stopped() {
    let blocks =
        "0\nBLOCK\n2\n*D1\n0\nDIMENSION\n2\n*D1\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n";
    let text = format!(
        "0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nDIMENSION\n2\n*D1\n0\nENDSEC\n0\nEOF\n"
    );
    assert!(ok(text.as_bytes()).is_some());
}

#[test]
fn a_very_long_line_and_a_very_long_file_are_refused_or_read_in_time() {
    // A text value over the limit of one line.
    let long = "a".repeat((1 << 20) + 10);
    let text = dxf("", &format!("0\nTEXT\n1\n{long}\n"));
    assert!(ok(text.as_bytes()).is_none());
    // The option max_bytes refuses a file before it is read.
    let small = ImportOptions {
        max_bytes: 20,
        ..ImportOptions::default()
    };
    assert_eq!(run(dxf("", "0\nLINE\n").as_bytes(), &small), Ok(None));
    // 60,000 short entities in a debug build.
    let mut ents = String::new();
    for i in 0..60_000 {
        ents.push_str(&format!(
            "0\nLINE\n8\nL{}\n10\n{i}\n20\n0\n11\n{i}\n21\n1\n",
            i % 50
        ));
    }
    let n = ok(dxf("", &ents).as_bytes());
    assert_eq!(n, Some(60_000));
}

#[test]
fn paper_space_with_hostile_viewports_does_not_fail_the_batch() {
    let vp = |extra: &str| {
        format!("0\nVIEWPORT\n67\n1\n10\n100\n20\n100\n40\n100\n41\n100\n68\n1\n69\n2\n{extra}")
    };
    let mut ents = String::new();
    for extra in [
        "45\n1e300\n",
        "45\n1e-300\n",
        "45\n50\n16\n1e300\n26\n1e300\n36\n0\n",
        "45\n50\n17\n1e300\n27\n-1e300\n",
        "45\n50\n12\n1e308\n22\n1e308\n",
        "45\n50\n331\nZZZ\n331\n\n340\nFFFF\n",
        "45\n50\n51\nnan\n90\n-1\n",
    ] {
        ents.push_str(&vp(extra));
    }
    // More 3D viewports than a sheet takes.
    for _ in 0..20 {
        ents.push_str(&vp("45\n50\n16\n1\n26\n-1\n36\n1\n"));
    }
    // Shapes that a sheet does not hold.
    ents.push_str("0\nPOINT\n67\n1\n10\n0\n20\n0\n0\n3DFACE\n67\n1\n10\n0\n20\n0\n11\n1\n21\n0\n12\n1\n22\n1\n");
    let text = dxf("", &ents);
    let n = ok(text.as_bytes());
    assert!(n.is_some());
    // Two hundred layouts with odd names.
    let mut blocks = String::new();
    let mut objects = String::from("0\nSECTION\n2\nOBJECTS\n");
    for i in 0..200 {
        blocks.push_str(&format!("0\nBLOCK\n5\n{:X}\n330\n{:X}\n2\n*Paper_Space{i}\n0\nTEXT\n10\n0\n20\n0\n40\n1\n1\nx\n0\nENDBLK\n", 0x1000 + i, 0x2000 + i));
        objects.push_str(&format!("0\nLAYOUT\n5\n{:X}\n1\n\n1\n<>?{i}\\\"\n71\n{i}\n44\n1e300\n45\n-5\n10\n5\n20\n5\n11\n1\n21\n1\n330\n{:X}\n", 0x3000 + i, 0x2000 + i));
    }
    objects.push_str("0\nENDSEC\n");
    let text = format!(
        "0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n{objects}0\nEOF\n"
    );
    assert!(ok(text.as_bytes()).is_some());
}

#[test]
fn a_binary_file_with_a_text_that_never_ends_or_a_huge_pair_count_is_refused() {
    use kadakas_io::dxf::binary::write_pairs;
    let pairs: Vec<(i32, String)> = vec![(0, "SECTION".into()), (2, "ENTITIES".into())];
    let mut data = write_pairs(&pairs, true);
    data.extend_from_slice(&[0, 0]);
    data.extend(std::iter::repeat_n(b'x', 5_000_000));
    assert!(ok(&data).is_none());
    // A pair of the wrong width at the end.
    let mut data = write_pairs(&pairs, true);
    data.extend_from_slice(&[10, 0, 1, 2, 3]);
    assert!(ok(&data).is_none());
    // Many small entities in a binary file, with the update data off (the walk is what counts).
    let mut many = vec![(0, "SECTION".to_string()), (2, "ENTITIES".to_string())];
    for i in 0..100_000 {
        many.push((0, "POINT".into()));
        many.push((10, i.to_string()));
    }
    many.push((0, "ENDSEC".into()));
    many.push((0, "EOF".into()));
    let data = write_pairs(&many, true);
    let off = ImportOptions {
        track_source: false,
        ..ImportOptions::default()
    };
    assert_eq!(run(&data, &off), Ok(Some(100_000)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_bytes_after_the_binary_marker_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400), wide in any::<bool>()) {
        let mut data = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
        if wide { data.extend_from_slice(&[0, 0]); } else { data.push(0); }
        data.extend_from_slice(b"SECTION\0");
        data.extend(bytes);
        let r = run(&data, &ImportOptions::default());
        prop_assert!(r.is_ok(), "{:?}", r);
    }

    #[test]
    fn a_valid_binary_file_cut_or_changed_never_panics(cut in 0usize..2000, flip in 0usize..2000, byte in any::<u8>()) {
        let text = dxf("9\n$ACADVER\n1\nAC1015\n", "0\nLINE\n8\nA\n10\n0\n20\n0\n11\n5\n21\n5\n0\nLWPOLYLINE\n90\n3\n70\n1\n10\n0\n20\n0\n10\n5\n20\n0\n42\n0.5\n10\n5\n20\n5\n0\nTEXT\n10\n0\n20\n0\n40\n3\n1\nHi\n");
        let mut data = kadakas_io::dxf::binary::write_pairs(&support::pairs(&text), true);
        let n = data.len();
        data[flip % n] = byte;
        data.truncate(cut.min(n));
        let r = run(&data, &ImportOptions::default());
        prop_assert!(r.is_ok(), "{:?}", r);
    }
}
