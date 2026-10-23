//! Public sample files (IOP-15). Every file of the ezdxf and GDAL test data goes through the
//! importer: it must not panic, it must finish in time, and the report must account for every
//! entity. The files are fetched at run time (see `support::corpus`) and the test skips when
//! there is no network.

mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use kadakas_core::command::Editor;
use kadakas_core::model::Document;
use kadakas_io::{Context, Format, ImportOptions, Source, importer};
use support::corpus;

/// The longest time that one file may take in a debug build.
const LIMIT: Duration = Duration::from_secs(60);

/// What happened to one file.
enum Outcome {
    /// Imported: objects, skipped entities, entities read, and the skipped lines and notes for the
    /// verbose output.
    Imported(usize, usize, usize, Vec<String>),
    /// Refused with a message.
    Refused(String),
}

fn run(data: &[u8], opts: &ImportOptions) -> Result<Outcome, String> {
    let src = Source {
        name: "corpus".into(),
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
        Err(e) => Ok(Outcome::Refused(e.to_string())),
        Ok(r) => {
            let rep = &r.report;
            if rep.entities_read != rep.entities_imported + rep.entities_skipped {
                return Err(format!(
                    "entities are lost: read {} imported {} skipped {}",
                    rep.entities_read, rep.entities_imported, rep.entities_skipped
                ));
            }
            if rep.entities_skipped > rep.skipped_total() {
                return Err(format!(
                    "{} entities skipped but only {} lines in the report",
                    rep.entities_skipped,
                    rep.skipped_total()
                ));
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
            let lines = rep
                .skipped
                .iter()
                .map(|s| format!("  skipped {} x {}: {}", s.count, s.entity, s.reason))
                .chain(rep.notes.iter().map(|n| format!("  note: {n}")))
                .collect();
            Ok(Outcome::Imported(
                rep.total(),
                rep.entities_skipped,
                rep.entities_read,
                lines,
            ))
        }
    }
}

#[test]
fn every_public_file_imports_or_is_refused_cleanly_and_is_accounted_for() {
    let files = corpus::all();
    let wanted: Vec<_> = files.iter().map(|(_, r, p)| (*r, p.as_str())).collect();
    corpus::prefetch(&wanted);
    let mut bad: Vec<String> = Vec::new();
    let (mut imported, mut refused, mut skipped_files) = (0, 0, 0);
    for (label, repo, path) in &files {
        let Some(data) = corpus::fetch(*repo, path) else {
            skipped_files += 1;
            continue;
        };
        match run(&data, &ImportOptions::default()) {
            Ok(Outcome::Imported(objects, skipped, read, lines)) => {
                imported += 1;
                eprintln!("{label}: {objects} objects, {skipped} of {read} entities skipped");
                if std::env::var_os("KADAKAS_CORPUS_VERBOSE").is_some() {
                    for l in lines {
                        eprintln!("{l}");
                    }
                }
            }
            Ok(Outcome::Refused(why)) => {
                refused += 1;
                eprintln!("{label}: refused: {why}");
                if why.trim().is_empty() {
                    bad.push(format!("{label}: refused without a message"));
                }
            }
            Err(why) => bad.push(format!("{label}: {why}")),
        }
    }
    eprintln!("corpus: {imported} imported, {refused} refused, {skipped_files} not available");
    if skipped_files == files.len() {
        eprintln!("SKIP: no corpus file is available (offline or KADAKAS_CORPUS=off)");
        return;
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The filled area of solid hatches equals the area that the ezdxf reader finds (outer paths
/// minus islands), and the number of boundary paths is the same. The numbers come from
/// `ezdxf.path.from_hatch_boundary_path` on the same files.
#[test]
fn hatch_areas_equal_the_ezdxf_reader() {
    use kadakas_core::model::Shape;
    // (file, solid, boundary paths, net area)
    let cases: [(&str, bool, usize, f64); 4] = [
        ("hatches_1.dxf", true, 19, 852.256),
        ("hatches_2.dxf", true, 2, 2537.118),
        ("clockwise_arcs_hatch.dxf", true, 2, 2820.533),
        ("hatch_issues.dxf", false, 8, 150.663),
    ];
    for (name, solid, loops, area) in cases {
        let Some(data) = corpus::ezdxf(name) else {
            continue;
        };
        let (ed, _) = support::import_with(
            &data,
            &ImportOptions {
                origin: kadakas_io::OriginMode::File,
                unit: Some(kadakas_io::Unit::Mm),
                ..ImportOptions::default()
            },
        )
        .unwrap();
        let hatch: Vec<_> = ed
            .doc()
            .objects
            .values()
            .filter(|o| o.data.get("import").and_then(|t| t["kind"].as_str()) == Some("HATCH"))
            .collect();
        assert_eq!(hatch.len(), loops, "{name}: boundary paths");
        // The unit is forced to 1 mm, so the areas are in drawing units, like those of ezdxf.
        let net: f64 = hatch
            .iter()
            .map(|o| match &o.shape {
                Shape::Polyline { points, .. } => {
                    let a = kadakas_core::curve::area(points).abs();
                    if o.data.contains_key("hatch_island") {
                        -a
                    } else {
                        a
                    }
                }
                _ => 0.0,
            })
            .sum();
        if solid {
            assert!(
                (net - area).abs() / area < 0.005,
                "{name}: {net} against {area}"
            );
            assert!(hatch.iter().all(|o| o.attrs.fill.is_some()), "{name}");
        } else {
            // A pattern hatch has no fill and no island marks: the loops are outlines.
            assert!(
                hatch.iter().all(|o| !o.data.contains_key("hatch_island")),
                "{name}"
            );
        }
    }
}

/// The reader finds the same entities as the ezdxf reader: the entities of the model space, with
/// the attributes of block references (our reader lists an ATTRIB as an entity of its own), the
/// INSERTs, and the entities of paper space. The numbers are in
/// `fixtures/dxf_public_counts.json`, made by `tests/data/make_public_counts.py`.
#[test]
fn the_reader_finds_the_entities_that_the_ezdxf_reader_finds() {
    let counts: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/dxf_public_counts.json")).unwrap();
    let files = corpus::all();
    let wanted: Vec<_> = files.iter().map(|(_, r, p)| (*r, p.as_str())).collect();
    corpus::prefetch(&wanted);
    let (mut checked, mut bad) = (0, Vec::new());
    for (label, repo, path) in &files {
        let key = format!(
            "{}:{}",
            repo.name,
            path.trim_start_matches("autotest/ogr/data/dxf/")
        );
        let key = if repo.name == "gdal" {
            format!(
                "gdal:autotest/ogr/data/dxf/{}",
                path.rsplit('/').next().unwrap()
            )
        } else {
            key
        };
        // MTEXT with columns: ezdxf merges the entities of the columns into one. A DIMENSION
        // without its block is dropped by ezdxf.
        if label.contains("columns") || label.contains("mtext_") || label.ends_with("assorted.dxf")
        {
            continue;
        }
        let Some(expected) = counts.get(&key).filter(|v| !v.is_null()) else {
            continue;
        };
        let Some(data) = corpus::fetch(*repo, path) else {
            continue;
        };
        let Ok(dxf) = kadakas_io::dxf::parse::parse(&data) else {
            continue;
        };
        checked += 1;
        let top = |paper: bool| {
            dxf.entities
                .iter()
                .filter(|e| (e.int(67) == Some(1)) == paper)
                .count() as u64
        };
        let want = |k: &str| expected[k].as_u64().unwrap();
        let model = top(false);
        if model != want("model") + want("attribs") {
            bad.push(format!(
                "{label}: model {model}, ezdxf {} + {} attributes",
                want("model"),
                want("attribs")
            ));
        }
        let inserts = dxf
            .entities
            .iter()
            .filter(|e| e.kind == "INSERT" && e.int(67) != Some(1))
            .count() as u64;
        if inserts != want("inserts") {
            bad.push(format!(
                "{label}: {inserts} INSERTs, ezdxf {}",
                want("inserts")
            ));
        }
    }
    eprintln!("{checked} files compared with the ezdxf reader");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// One file for each entity of the list of research gap 3: each gives objects or a line with the
/// name of the entity in the report, never a silent loss.
#[test]
fn the_entities_of_research_gap_3_are_imported_or_named() {
    let imp = |data: Vec<u8>| support::import(&data).unwrap().1;
    if let Some(d) = corpus::gdal("wipeout.dxf") {
        let rep = imp(d);
        assert!(
            rep.notes.iter().any(|n| n.contains("WIPEOUT")),
            "{:?}",
            rep.notes
        );
        assert!(rep.objects.get("polyline").copied().unwrap_or(0) >= 1);
    }
    if let Some(d) = corpus::gdal("BINARY_wipeout.dxf") {
        let rep = imp(d);
        assert!(
            rep.notes.iter().any(|n| n.contains("WIPEOUT")),
            "{:?}",
            rep.notes
        );
    }
    if let Some(d) = corpus::gdal("dimension.dxf") {
        let rep = imp(d);
        assert!(rep.total() >= 15, "{:?}", rep.objects);
        assert!(
            !rep.skipped.iter().any(|s| s.entity == "DIMENSION"),
            "{:?}",
            rep.skipped
        );
    }
    if let Some(d) = corpus::gdal("dimension-entities-only.dxf") {
        // Dimensions without blocks: named in the report.
        let rep = imp(d);
        let s = rep
            .skipped
            .iter()
            .find(|s| s.entity == "DIMENSION")
            .expect("DIMENSION is named");
        assert_eq!(s.count, 3);
    }
    if let Some(d) = corpus::gdal("leader-mleader.dxf") {
        let rep = imp(d);
        let names: Vec<_> = rep.skipped.iter().map(|s| s.entity.as_str()).collect();
        assert!(
            names.contains(&"MULTILEADER") || names.contains(&"MLEADER"),
            "{names:?}"
        );
        assert!(
            rep.notes.iter().any(|n| n.contains("LEADER")),
            "{:?}",
            rep.notes
        );
    }
    if let Some(d) = corpus::gdal("solid.dxf") {
        let (ed, _) = support::import(&d).unwrap();
        let o = ed.doc().objects.values().next().unwrap();
        assert!(
            o.attrs.fill.is_some_and(|f| f.0[3] > 0),
            "a SOLID is filled"
        );
    }
    if let Some(d) = corpus::ezdxf("image/images.dxf") {
        let rep = imp(d);
        let image = rep
            .skipped
            .iter()
            .filter(|s| s.entity == "IMAGE")
            .collect::<Vec<_>>();
        assert!(!image.is_empty());
        assert!(image.iter().all(|s| s.reason.contains("image")));
        assert!(
            image.iter().any(|s| s.reason.contains("image1.jpg")),
            "the file of the image is named"
        );
    }
    if let Some(d) = corpus::gdal("insert-recursive-pair.dxf") {
        let rep = imp(d);
        assert!(
            rep.skipped
                .iter()
                .any(|s| s.reason.contains("nested too deep")),
            "{:?}",
            rep.skipped
        );
    }
}

/// Paper space: the entities and the viewports that the sheets get are the ones that the ezdxf
/// reader finds in the layouts. A layout that holds only the viewport of the paper itself is not
/// imported, so the totals differ by at most that one entity for each layout.
#[test]
fn the_sheets_hold_the_paper_space_entities_that_the_ezdxf_reader_finds() {
    let counts: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/dxf_public_counts.json")).unwrap();
    let files = corpus::all();
    let wanted: Vec<_> = files.iter().map(|(_, r, p)| (*r, p.as_str())).collect();
    corpus::prefetch(&wanted);
    let (mut checked, mut bad) = (0, Vec::new());
    for (label, repo, path) in &files {
        let key = if repo.name == "gdal" {
            format!(
                "gdal:autotest/ogr/data/dxf/{}",
                path.rsplit('/').next().unwrap()
            )
        } else {
            format!("{}:{}", repo.name, path)
        };
        let Some(expected) = counts.get(&key).filter(|v| !v.is_null()) else {
            continue;
        };
        let Some(data) = corpus::fetch(*repo, path) else {
            continue;
        };
        let Ok((_, rep)) = support::import(&data) else {
            continue;
        };
        let (mut entities, mut viewports, mut sheets) = (0usize, 0usize, 0usize);
        for n in &rep.notes {
            let Some(rest) = n.strip_prefix("layout '") else {
                continue;
            };
            let Some((_, tail)) = rest.split_once("sheet layer with ") else {
                continue;
            };
            let nums: Vec<usize> = tail
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|t| t.parse().ok())
                .collect();
            sheets += 1;
            entities += nums[0];
            viewports += nums[1];
        }
        let paper = expected["paper"].as_u64().unwrap() as usize;
        let layouts = expected["layouts"].as_array().unwrap().len();
        if sheets > 0 || paper > 0 {
            checked += 1;
            if entities > paper || paper - entities > layouts {
                bad.push(format!("{label}: sheets hold {entities} entities, ezdxf finds {paper} in {layouts} layouts"));
            }
            let vp = expected["viewports"].as_u64().unwrap() as usize;
            // A viewport that is turned off, or has no size, is named in `skipped`.
            let named: usize = rep
                .skipped
                .iter()
                .filter(|s| s.entity == "VIEWPORT")
                .map(|s| s.count)
                .sum();
            let viewports = viewports + named;
            if viewports > vp || vp - viewports > layouts {
                bad.push(format!(
                    "{label}: sheets hold {viewports} viewports, ezdxf finds {vp}"
                ));
            }
        }
    }
    eprintln!("{checked} files with paper space compared with the ezdxf reader");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// `viewport_frozen_layers.dxf` (ezdxf): one viewport freezes Layer1, another Layer2, and the
/// layer Layer3 is frozen in the drawing. The layers stay hidden in the viewports that freeze
/// them and nowhere else. The scale is the ratio of the view height to the paper height.
#[test]
fn frozen_layers_stay_hidden_in_their_viewports() {
    let Some(data) = corpus::ezdxf("viewport_frozen_layers.dxf") else {
        return;
    };
    let (ed, rep) = support::import_with(
        &data,
        &ImportOptions {
            unit: Some(kadakas_io::Unit::Mm),
            ..ImportOptions::default()
        },
    )
    .unwrap();
    let doc = ed.doc();
    let sheet = doc
        .layers
        .iter()
        .find(|l| l.kind == kadakas_core::model::LayerKind::Sheet)
        .expect("a sheet layer");
    let vps: Vec<_> = doc.viewports_of(sheet.id).map(|(_, v)| v).collect();
    assert_eq!(vps.len(), 3, "{:?} {:?}", rep.notes, rep.skipped);
    let class = |n: &str| doc.classes.iter().find(|c| c.name == n).map(|c| c.id);
    let hidden = |v: &kadakas_core::sheets::Viewport, n: &str| {
        class(n).is_some_and(|c| {
            v.class_overrides
                .get(&c)
                .is_some_and(|o| o.visible == Some(false))
        })
    };
    // The viewports in the file are 2, 4 and 5 (the paper itself is 1).
    assert!(!hidden(vps[0], "Layer1") && !hidden(vps[0], "Layer2"));
    assert!(hidden(vps[1], "Layer1") && !hidden(vps[1], "Layer2"));
    assert!(hidden(vps[2], "Layer2") && !hidden(vps[2], "Layer1"));
    // Layer3 is frozen in the drawing: its class is off, and every viewport hides it.
    if let Some(c) = doc.classes.iter().find(|c| c.name == "Layer3") {
        assert!(!c.visible);
        assert!(vps.iter().all(|v| hidden(v, "Layer3")));
    }
    // 486.29 / 45.72 = 10.63 (ezdxf: the scale is 0.0940).
    assert!(
        (vps[1].scale - 1.0 / 0.0940248363471966).abs() < 1e-6,
        "{}",
        vps[1].scale
    );
    assert!(vps.iter().all(|v| v.is_plan()));
}

/// Words of a text, so that the comparison ignores the line breaks and the spaces. The control
/// characters that a caret code makes are left out: a drawing text has no place for them.
fn words(s: &str) -> Vec<String> {
    let clean: String = s
        .chars()
        .filter(|c| {
            c.is_whitespace() && *c != '\u{b}' && *c != '\u{c}' && *c != '\r' || !c.is_control()
        })
        .collect();
    clean.split_whitespace().map(str::to_string).collect()
}

/// The plain text of the MTEXT entities equals the plain text that the ezdxf reader makes
/// (`MText.plain_text`), for the files with inline codes, escapes and caret codes.
#[test]
fn mtext_codes_give_the_text_that_the_ezdxf_reader_gives() {
    let counts: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/dxf_public_counts.json")).unwrap();
    let files = corpus::all();
    let wanted: Vec<_> = files.iter().map(|(_, r, p)| (*r, p.as_str())).collect();
    corpus::prefetch(&wanted);
    let (mut checked, mut bad) = (0, Vec::new());
    for (label, repo, path) in &files {
        // Columns: ezdxf joins the entities of the columns, we read them one by one.
        if label.contains("column") || label.contains("mtext_framed") {
            continue;
        }
        let key = if repo.name == "gdal" {
            format!(
                "gdal:autotest/ogr/data/dxf/{}",
                path.rsplit('/').next().unwrap()
            )
        } else {
            format!("{}:{}", repo.name, path)
        };
        let Some(expected) = counts.get(&key).filter(|v| !v.is_null()) else {
            continue;
        };
        let Some(data) = corpus::fetch(*repo, path) else {
            continue;
        };
        let Ok(dxf) = kadakas_io::dxf::parse::parse(&data) else {
            continue;
        };
        let ours: Vec<String> = dxf
            .entities
            .iter()
            .filter(|e| e.kind == "MTEXT" && e.int(67) != Some(1))
            .map(|e| {
                let raw: String = e
                    .items
                    .iter()
                    .filter(|(c, _)| *c == 3)
                    .map(|(_, v)| v.as_str())
                    .chain(e.get(1))
                    .collect();
                // The ezdxf reader decodes a caret and a small letter, we do not (x^n).
                if raw
                    .as_bytes()
                    .windows(2)
                    .any(|w| w[0] == b'^' && w[1].is_ascii_lowercase())
                {
                    return "\u{1}skip".to_string();
                }
                kadakas_io::dxf::text::clean_mtext(&raw)
            })
            .collect();
        let want: Vec<String> = expected["mtext"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap().to_string())
            .collect();
        if ours.len() != want.len() {
            bad.push(format!(
                "{label}: {} MTEXT entities, ezdxf {}",
                ours.len(),
                want.len()
            ));
            continue;
        }
        checked += 1;
        for (i, (a, b)) in ours.iter().zip(&want).enumerate() {
            if a == "\u{1}skip" {
                continue;
            }
            // ezdxf keeps the # of a diagonal fraction, we write a slash.
            let (wa, wb) = (words(&a.replace('#', "/")), words(&b.replace('#', "/")));
            if wa != wb {
                let at = wa
                    .iter()
                    .zip(&wb)
                    .position(|(x, y)| x != y)
                    .unwrap_or(wa.len().min(wb.len()));
                bad.push(format!(
                    "{label} #{i}: word {at}: ours {:?} ezdxf {:?}",
                    &wa[at.saturating_sub(1)..(at + 3).min(wa.len())],
                    &wb[at.saturating_sub(1)..(at + 3).min(wb.len())]
                ));
            }
        }
    }
    eprintln!("{checked} files with MTEXT compared with the ezdxf reader");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The cut splines of the GDAL files stay within the tolerance of the curve that the ezdxf
/// reader makes (it flattens within 0.01), and a closed spline stays closed.
#[test]
fn splines_stay_within_the_tolerance_of_the_ezdxf_curve() {
    use kadakas_core::model::Shape;
    let counts: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/dxf_public_counts.json")).unwrap();
    let tol = 0.05;
    for name in ["spline_closed.dxf", "spline_qcad.dxf", "spline_weight.dxf"] {
        let Some(data) = corpus::gdal(name) else {
            continue;
        };
        let key = format!("gdal:autotest/ogr/data/dxf/{name}");
        let want = counts[&key]["splines"].as_array().unwrap();
        let (ed, rep) = support::import_with(
            &data,
            &ImportOptions {
                origin: kadakas_io::OriginMode::File,
                unit: Some(kadakas_io::Unit::Mm),
                tolerance: tol,
                ..ImportOptions::default()
            },
        )
        .unwrap();
        let ours: Vec<(Vec<kadakas_core::DVec2>, bool)> = ed
            .doc()
            .objects
            .values()
            .filter_map(|o| match &o.shape {
                Shape::Polyline { points, closed } => {
                    Some((kadakas_core::curve::flatten(points, *closed, tol), *closed))
                }
                _ => None,
            })
            .collect();
        assert_eq!(ours.len(), want.len(), "{name}: {:?}", rep.skipped);
        for (i, (pts, closed)) in ours.iter().enumerate() {
            let dense: Vec<[f64; 2]> = serde_json::from_value(want[i].clone()).unwrap();
            let worst = dense
                .iter()
                .map(|c| {
                    let c = kadakas_core::DVec2::new(c[0], c[1]);
                    let ring = pts.len();
                    (0..ring - 1 + usize::from(*closed))
                        .map(|k| {
                            let (a, b) = (pts[k], pts[(k + 1) % ring]);
                            let ab = b - a;
                            let t = if ab.length_squared() < 1e-300 {
                                0.0
                            } else {
                                ((c - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
                            };
                            (c - (a + ab * t)).length()
                        })
                        .fold(f64::MAX, f64::min)
                })
                .fold(0.0, f64::max);
            assert!(worst <= tol + 0.02, "{name} #{i}: deviation {worst}");
            if name == "spline_closed.dxf" {
                assert!(*closed, "{name} #{i} is closed in the file");
            }
        }
    }
}

/// Block clips and transparencies are named in the notes, never silent.
#[test]
fn clips_and_transparencies_are_named() {
    if let Some(d) = corpus::ezdxf("xclip.dxf") {
        let rep = support::import(&d).unwrap().1;
        assert!(
            rep.notes.iter().any(|n| n.contains("clip (XCLIP)")),
            "{:?}",
            rep.notes
        );
    }
    if let Some(d) = corpus::gdal("transparency.dxf") {
        let rep = support::import(&d).unwrap().1;
        assert!(
            rep.notes.iter().any(|n| n.contains("transparency")),
            "{:?}",
            rep.notes
        );
    }
}
