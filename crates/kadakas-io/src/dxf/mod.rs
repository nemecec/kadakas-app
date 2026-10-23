//! DXF import and export (text and binary DXF).

mod aci;
#[cfg(test)]
mod base_tests;
pub mod binary;
pub mod block;
pub mod compare;
mod export;
pub use export::patterns::{
    MAX_EXPORT_GAP_LINES, MAX_EXPORT_UNITS, layer_names as pattern_layer_names,
};
pub mod hash;
mod import;
pub mod link;
pub mod parse;
pub mod revision;
mod source;
pub mod text;
pub mod units;
pub mod update;
pub mod version;
pub mod views;

use kadakas_core::model::{Color, Document};

use crate::{
    Context, ExportOptions, ExportResult, Exporter, Format, ImportOptions, ImportResult, Importer,
    IoError, Source,
};

pub struct DxfFormat;

impl Importer for DxfFormat {
    fn format(&self) -> Format {
        Format::Dxf
    }

    fn import(
        &self,
        src: &Source,
        ctx: &Context,
        opts: &ImportOptions,
    ) -> Result<ImportResult, IoError> {
        import::import(src, ctx, opts, None)
    }
}

/// Like the DXF importer, with the link to the original file that `base: "link"` stores on the layer
/// source (the caller made it from the file, see [`kadakas_core::provenance::FileLink`]). Without
/// a link, `base: "link"` is an error: the importer cannot make it from bytes.
pub fn import_linked(
    src: &Source,
    ctx: &Context,
    opts: &ImportOptions,
    link: Option<&kadakas_core::provenance::FileLink>,
) -> Result<ImportResult, IoError> {
    import::import(src, ctx, opts, link)
}

impl Exporter for DxfFormat {
    fn format(&self) -> Format {
        Format::Dxf
    }

    fn export(&self, doc: &Document, opts: &ExportOptions) -> Result<ExportResult, IoError> {
        export::export(doc, opts)
    }
}

/// RGB of an AutoCAD Color Index (1 to 255). Index 7 is shown as black, because
/// it is "white or black, whichever shows on the background" and drawings are on white.
pub fn aci_rgb(index: u8) -> Option<[u8; 3]> {
    match index {
        0 => None,
        7 => Some([0, 0, 0]),
        i => Some(aci::ACI_TABLE[usize::from(i)]),
    }
}

/// The ACI index whose color is nearest to `rgb`.
pub fn nearest_aci(rgb: [u8; 3]) -> u8 {
    let dist = |a: [u8; 3]| -> i32 { (0..3).map(|i| (a[i] as i32 - rgb[i] as i32).pow(2)).sum() };
    (1..=255u8)
        .min_by_key(|i| dist(aci_rgb(*i).expect("1..=255 have colors")))
        .expect("range is not empty")
}

/// A color from a true-color value (`0xRRGGBB`).
pub fn true_color(v: u32) -> Color {
    Color([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aci_table_matches_known_values() {
        assert_eq!(aci_rgb(1), Some([255, 0, 0]));
        assert_eq!(aci_rgb(7), Some([0, 0, 0]));
        assert_eq!(aci_rgb(10), Some([255, 0, 0]));
        assert_eq!(aci_rgb(11), Some([255, 127, 127]));
        assert_eq!(aci_rgb(12), Some([165, 0, 0]));
        assert_eq!(aci_rgb(20), Some([255, 63, 0]));
        assert_eq!(aci_rgb(40), Some([255, 191, 0]));
        assert_eq!(aci_rgb(90), Some([0, 255, 0]));
        assert_eq!(aci_rgb(130), Some([0, 255, 255]));
        assert_eq!(aci_rgb(250), Some([51, 51, 51]));
        assert_eq!(aci_rgb(255), Some([255, 255, 255]));
        assert_eq!(aci_rgb(0), None);
        for i in 1..=255u8 {
            assert!(aci_rgb(i).is_some());
        }
    }

    #[test]
    fn nearest_color() {
        assert_eq!(nearest_aci([255, 0, 0]), 1);
        assert_eq!(nearest_aci([0, 0, 255]), 5);
        assert_eq!(nearest_aci([250, 250, 250]), 255);
        assert_eq!(true_color(0x112233), Color([0x11, 0x22, 0x33, 255]));
    }
}
