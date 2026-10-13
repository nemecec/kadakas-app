//! DXF import and export (ASCII).

mod export;
pub mod hash;
mod import;
pub mod parse;
pub mod revision;
mod source;

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
        import::import(src, ctx, opts)
    }
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
    const BASIC: [[u8; 3]; 9] = [
        [255, 0, 0],
        [255, 255, 0],
        [0, 255, 0],
        [0, 255, 255],
        [0, 0, 255],
        [255, 0, 255],
        [0, 0, 0],
        [128, 128, 128],
        [192, 192, 192],
    ];
    match index {
        0 => None,
        1..=9 => Some(BASIC[index as usize - 1]),
        10..=249 => {
            let n = (index - 10) as usize;
            let hue = (n / 10) as f64 * 15.0;
            let (sat, val) = [
                (1.0, 1.0),
                (0.5, 1.0),
                (1.0, 0.65),
                (0.5, 0.65),
                (1.0, 0.5),
                (0.5, 0.5),
                (1.0, 0.3),
                (0.5, 0.3),
                (1.0, 0.15),
                (0.5, 0.15),
            ][n % 10];
            Some(hsv(hue, sat, val))
        }
        250..=255 => {
            let g = [51, 91, 132, 173, 214, 255][index as usize - 250];
            Some([g, g, g])
        }
    }
}

fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|u| ((u + m) * 255.0).round() as u8)
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
        assert_eq!(aci_rgb(11), Some([255, 128, 128]));
        assert_eq!(aci_rgb(12), Some([166, 0, 0]));
        assert_eq!(aci_rgb(20), Some([255, 64, 0]));
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
