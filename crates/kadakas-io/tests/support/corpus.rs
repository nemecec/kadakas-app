//! Public sample DXF files, fetched at run time (IOP-15).
//!
//! The files are NOT in the repository. A test asks for a file by repository and path. The helper
//! downloads it with `curl` from a fixed commit into `tests/data/dxf-public/` (a folder that git
//! ignores) and reads it from there the next time. When there is no network (or no `curl`), or
//! when the environment variable `KADAKAS_CORPUS=off` is set, `fetch` returns `None` and the test
//! prints `SKIP` and ends. A missing network never fails a test.
//!
//! Sources, both with a clear licence:
//! - ezdxf (https://github.com/mozman/ezdxf), MIT licence: `examples_dxf` and `integration_tests/data`.
//! - GDAL (https://github.com/OSGeo/gdal), MIT-style licence: `autotest/ogr/data/dxf`. Two files there are copies from ezdxf.
//!
//! LibreCAD files are GPL and are never fetched.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// A repository that the corpus comes from, at a fixed commit.
#[derive(Clone, Copy, Debug)]
pub struct Repo {
    pub name: &'static str,
    pub slug: &'static str,
    pub commit: &'static str,
}

pub const EZDXF: Repo = Repo {
    name: "ezdxf",
    slug: "mozman/ezdxf",
    commit: "7b202613e0d689955b8932a22bcbcf68bdd684f0",
};
pub const GDAL: Repo = Repo {
    name: "gdal",
    slug: "OSGeo/gdal",
    commit: "739fcf03bfcc075e300dbd45743187b380a17cdc",
};

/// The folder of the downloaded files (ignored by git).
pub fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/dxf-public")
}

static OFFLINE: AtomicBool = AtomicBool::new(false);

pub fn disabled() -> bool {
    OFFLINE.load(Ordering::Relaxed)
        || std::env::var("KADAKAS_CORPUS").is_ok_and(|v| v.eq_ignore_ascii_case("off"))
}

fn local(repo: Repo, path: &str) -> PathBuf {
    cache_dir()
        .join(repo.name)
        .join(&repo.commit[..8])
        .join(path)
}

/// The bytes of a file of a repository, or `None` when it cannot be had (offline, disabled, or
/// the file does not exist at that commit).
pub fn fetch(repo: Repo, path: &str) -> Option<Vec<u8>> {
    let file = local(repo, path);
    if let Ok(bytes) = std::fs::read(&file) {
        return Some(bytes);
    }
    if disabled() {
        return None;
    }
    std::fs::create_dir_all(file.parent()?).ok()?;
    let url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}",
        repo.slug, repo.commit, path
    );
    let part = file.with_extension("part");
    let status = Command::new("curl")
        .args(["-fsSL", "--connect-timeout", "8", "-m", "90", "-o"])
        .arg(&part)
        .arg(&url)
        .output();
    match status {
        Ok(out) if out.status.success() => {
            std::fs::rename(&part, &file).ok()?;
            std::fs::read(&file).ok()
        }
        Ok(out) => {
            let _ = std::fs::remove_file(&part);
            // Exit code 22: the server answered with an error (no such file). Anything else is
            // the network.
            if out.status.code() != Some(22) {
                OFFLINE.store(true, Ordering::Relaxed);
            }
            None
        }
        Err(_) => {
            OFFLINE.store(true, Ordering::Relaxed);
            None
        }
    }
}

/// Like `fetch`, for a test that needs one file: prints `SKIP` when the file is not there.
pub fn need(repo: Repo, path: &str) -> Option<Vec<u8>> {
    let r = fetch(repo, path);
    if r.is_none() {
        eprintln!(
            "SKIP: {} {path} is not available (offline or KADAKAS_CORPUS=off)",
            repo.name
        );
    }
    r
}

/// Downloads many files at once (8 at a time). Files that cannot be had are left out.
pub fn prefetch(files: &[(Repo, &str)]) {
    if disabled() {
        return;
    }
    let todo: Vec<(Repo, String)> = files
        .iter()
        .filter(|(r, p)| !local(*r, p).exists())
        .map(|(r, p)| (*r, p.to_string()))
        .collect();
    let chunks: Vec<Vec<(Repo, String)>> = todo
        .chunks(todo.len().div_ceil(8).max(1))
        .map(<[_]>::to_vec)
        .collect();
    std::thread::scope(|s| {
        for chunk in &chunks {
            s.spawn(move || {
                for (r, p) in chunk {
                    let _ = fetch(*r, p);
                }
            });
        }
    });
}

/// The GDAL files `autotest/ogr/data/dxf/<name>`.
pub const GDAL_FILES: &[&str] = &[
    "3d.dxf",
    "3dface.dxf",
    "BINARY_wipeout.dxf",
    "LWPOLYLINE-OCS.dxf",
    "additional-entities.dxf",
    "assorted.dxf",
    "attrib-nested.dxf",
    "attrib.dxf",
    "bin_dxf_r12.dxf",
    "bin_dxf_r2000.dxf",
    "block-basepoint.dxf",
    "block-hidden-entities.dxf",
    "block-insert-order.dxf",
    "byblock-bylayer-new.dxf",
    "byblock-bylayer.dxf",
    "circle.dxf",
    "closed_polyline_with_bulge.dxf",
    "clusterfuzz-testcase-minimized-dxf_fuzzer-5400376672124928.dxf",
    "clusterfuzz-testcase-minimized-shape_fuzzer-6126814756995072.dxf",
    "dimension-entities-only.dxf",
    "dimension.dxf",
    "ellipse_z_extrusion_minus_1.dxf",
    "elliptical-arc-hatch-min.dxf",
    "entities_only.dxf",
    "frozen-off.dxf",
    "hatch.dxf",
    "hatch_as_multipolygon.dxf",
    "hatch_pattern_generated_by_gdal.dxf",
    "header_extended.dxf",
    "insert-recursive-pair.dxf",
    "insert-too-many-errors.dxf",
    "insert_only.dxf",
    "insert_only_col_count_zero.dxf",
    "leader-mleader.dxf",
    "linetypes.dxf",
    "lwpolyline_smooth.dxf",
    "mtext-ocs-reduced.dxf",
    "ocs2wcs1.dxf",
    "ocs2wcs2.dxf",
    "ocs2wcs3.dxf",
    "polyline_smooth.dxf",
    "solid-less-than-4-vertices.dxf",
    "solid-vertex-ordering.dxf",
    "solid.dxf",
    "spline_closed.dxf",
    "spline_qcad.dxf",
    "spline_weight.dxf",
    "spline_with_very_close_neg_to_zero_knot.dxf",
    "text-block-transform.dxf",
    "text-fancy.dxf",
    "text.dxf",
    "transparency.dxf",
    "utf-8.dxf",
    "wipeout.dxf",
];

pub fn gdal(name: &str) -> Option<Vec<u8>> {
    need(GDAL, &format!("autotest/ogr/data/dxf/{name}"))
}

/// The ezdxf files in `examples_dxf`.
pub const EZDXF_EXAMPLES: &[&str] = &[
    "3dface.dxf",
    "Minimal_DXF_AC1006.dxf",
    "Minimal_DXF_AC1009.dxf",
    "Minimal_DXF_AC1021.dxf",
    "VP4.dxf",
    "acad_table_simple.dxf",
    "acad_table_with_blk_ref.dxf",
    "caret_encoding.dxf",
    "circle_radius_le_0.dxf",
    "clockwise_arcs_hatch.dxf",
    "colors.dxf",
    "columns_R2007.dxf",
    "columns_R2018.dxf",
    "dimension_in_block.dxf",
    "dimension_in_nested_blocks.dxf",
    "hatch_issues.dxf",
    "hatch_text_box_in_fill.dxf",
    "hatch_text_box_in_hole.dxf",
    "hatch_text_box_overlapping_fill_and_hole.dxf",
    "hatches_1.dxf",
    "hatches_2.dxf",
    "image/images.dxf",
    "image/viewports.dxf",
    "insert_bricscad_level_1.dxf",
    "mtext_columns_R2000.dxf",
    "mtext_columns_R2007.dxf",
    "mtext_columns_R2018.dxf",
    "mtext_complex_inline_codes.dxf",
    "multi_insert_with_attribs.dxf",
    "text.dxf",
    "text_alignments.dxf",
    "text_fonts.dxf",
    "text_in_viewports.dxf",
    "text_mirror_shx_font.dxf",
    "text_mirror_true_type_font.dxf",
    "text_negative_extrusion.dxf",
    "text_oblique_rotate.dxf",
    "text_stacked_shx_font.dxf",
    "text_stacked_true_type_font.dxf",
    "transform_hatch_pattern.dxf",
    "uncommon.dxf",
    "valid_lineweights.dxf",
    "viewport_frozen_layers.dxf",
    "visibility.dxf",
    "wipeout_door.dxf",
    "wipeout_random.dxf",
    "xclip.dxf",
];

pub fn ezdxf(name: &str) -> Option<Vec<u8>> {
    need(EZDXF, &format!("examples_dxf/{name}"))
}

/// The ezdxf files in `integration_tests/data`.
pub const EZDXF_INTEGRATION: &[&str] = &[
    "AC1003_LINE_Example.dxf",
    "ASCII_R12.dxf",
    "Leica_Disto_S910.dxf",
    "MODEL.dxf",
    "POLI-ALL210_12.DXF",
    "R12_with_trash_beyond_EOF.dxf",
    "bin_dxf_r12.dxf",
    "bin_dxf_r13.dxf",
    "bin_dxf_r14.dxf",
    "bin_dxf_r2000.dxf",
    "cc_dxflib.dxf",
    "custom_blocks.dxf",
    "duplicate_handles.dxf",
    "dxf_unicode.dxf",
    "empty_handles.dxf",
    "groups.dxf",
    "issue1106.dxf",
    "issue_1359_1.dxf",
    "issue_1359_2.dxf",
    "layout_broken_links.dxf",
    "layout_broken_links_2.dxf",
    "layout_missing_block_definition.dxf",
    "layout_missing_block_record.dxf",
    "mtext_framed_columns.dxf",
    "mtext_text_frame.dxf",
    "no_layouts.dxf",
    "r12_blocks_with_no_names.dxf",
    "r2000_blocks_with_no_name_and_no_block_record.dxf",
    "r2000_blocks_with_no_names.dxf",
    "recover01.dxf",
    "recover02.dxf",
    "small_r13.dxf",
    "small_r14.dxf",
];

pub fn ezdxf_integration(name: &str) -> Option<Vec<u8>> {
    need(EZDXF, &format!("integration_tests/data/{name}"))
}

/// Every file of the corpus: (label, repository, path).
pub fn all() -> Vec<(String, Repo, String)> {
    let mut v = Vec::new();
    for n in GDAL_FILES {
        v.push((
            format!("gdal/{n}"),
            GDAL,
            format!("autotest/ogr/data/dxf/{n}"),
        ));
    }
    for n in EZDXF_EXAMPLES {
        v.push((
            format!("ezdxf/examples/{n}"),
            EZDXF,
            format!("examples_dxf/{n}"),
        ));
    }
    for n in EZDXF_INTEGRATION {
        v.push((
            format!("ezdxf/integration/{n}"),
            EZDXF,
            format!("integration_tests/data/{n}"),
        ));
    }
    v
}
