//! Binary DXF (IOP-01): the same drawing gives the same objects as text, and a damaged file is
//! refused with the position.

mod support;

use kadakas_io::dxf::binary::write_pairs;
use support::{dxf, import, pairs, refusal, shapes};

const SITE: &str = include_str!("fixtures/site.dxf");

fn drawing() -> String {
    dxf(
        "9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n4\n",
        "0\nLINE\n8\nWalls\n10\n0\n20\n0\n11\n1000\n21\n0\n\
         0\nCIRCLE\n8\nTrees\n10\n500\n20\n500\n40\n250\n\
         0\nLWPOLYLINE\n8\nWalls\n90\n3\n70\n1\n10\n0\n20\n0\n42\n0.5\n10\n100\n20\n0\n10\n100\n20\n100\n\
         0\nTEXT\n8\nLabels\n10\n10\n20\n10\n40\n50\n1\nHello\n",
    )
}

#[test]
fn binary_and_text_give_the_same_objects() {
    for text in [drawing(), SITE.to_string()] {
        let (ed, report) = import(text.as_bytes()).unwrap();
        for wide in [false, true] {
            let bytes = write_pairs(&pairs(&text), wide);
            let (bed, brep) = import(&bytes).unwrap();
            assert_eq!(shapes(&ed), shapes(&bed), "wide {wide}");
            assert_eq!(report.objects, brep.objects);
            assert_eq!(report.skipped, brep.skipped);
            assert!(!brep.notes.iter().any(|n| n.contains("ends early")));
        }
    }
}

#[test]
fn a_binary_file_cut_anywhere_never_panics() {
    let bytes = write_pairs(&pairs(&drawing()), true);
    let mut refused = 0;
    for n in 0..bytes.len() {
        match import(&bytes[..n]) {
            Ok((_, rep)) => {
                let _ = rep;
            }
            Err(e) => {
                refused += 1;
                let _ = e.to_string();
            }
        }
    }
    assert!(refused > bytes.len() / 2);
}

#[test]
fn a_damaged_binary_file_says_where() {
    let mut bytes = write_pairs(&pairs(&drawing()), true);
    let n = bytes.len();
    bytes.truncate(n - 11);
    let at = bytes
        .windows(8)
        .position(|w| w == b"Hello\0\x00\x00".as_slice() || w.starts_with(b"Hello"))
        .expect("text value");
    bytes.truncate(at + 3);
    let err = refusal(&bytes);
    assert!(err.contains("binary DXF file is damaged at byte"), "{err}");
    assert!(err.contains("text DXF"), "{err}");
}

/// The binary files of ezdxf and GDAL (MIT) read as the objects of the drawing they hold. The
/// counts are the ones that the ezdxf reader finds in the model space.
#[test]
fn public_binary_files_are_read() {
    use support::corpus;
    let files: Vec<(&str, Option<Vec<u8>>, &str, usize)> = vec![
        ("gdal r12", corpus::gdal("bin_dxf_r12.dxf"), "line", 3),
        ("gdal r2000", corpus::gdal("bin_dxf_r2000.dxf"), "text", 1),
        (
            "ezdxf r12",
            corpus::ezdxf_integration("bin_dxf_r12.dxf"),
            "line",
            3,
        ),
        (
            "ezdxf r13",
            corpus::ezdxf_integration("bin_dxf_r13.dxf"),
            "line",
            3,
        ),
        (
            "ezdxf r14",
            corpus::ezdxf_integration("bin_dxf_r14.dxf"),
            "line",
            3,
        ),
        (
            "ezdxf r2000",
            corpus::ezdxf_integration("bin_dxf_r2000.dxf"),
            "text",
            1,
        ),
    ];
    for (label, data, kind, n) in files {
        let Some(data) = data else { continue };
        assert!(kadakas_io::dxf::binary::is_binary(&data), "{label}");
        let (_, rep) = import(&data).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(
            rep.objects.get(kind),
            Some(&n),
            "{label}: {:?} {:?}",
            rep.objects,
            rep.skipped
        );
        assert_eq!(rep.total(), n, "{label}");
        assert!(
            !rep.notes.iter().any(|n| n.contains("ends early")),
            "{label}: {:?}",
            rep.notes
        );
    }
}
