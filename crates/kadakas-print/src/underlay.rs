//! Map underlays on sheets: the raster as an embedded image, with its resolution capped.

use std::sync::Arc;

use kadakas_core::underlay::Underlay;

use crate::display::ImagePrim;
use crate::plan::Xf;

/// Longest side of an embedded underlay image in pixels (about 4 megapixels per image).
pub const MAX_SIDE_PX: u32 = 2048;

/// The underlay as an image on paper. Clear parts and the opacity blend with white paper,
/// because sheet images have no alpha channel.
pub fn image_prim(u: &Underlay, xf: &Xf) -> Option<ImagePrim> {
    let img = u.decode(MAX_SIDE_PX)?;
    let a = xf.p(u.min);
    let b = xf.p(u.max);
    let rgb: Vec<u8> = img
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let alpha = f64::from(p[3]) / 255.0 * u.opacity;
            [0, 1, 2].map(|c| (f64::from(p[c]) * alpha + 255.0 * (1.0 - alpha)).round() as u8)
        })
        .collect();
    Some(ImagePrim {
        min: a.min(b),
        max: a.max(b),
        width: img.width,
        height: img.height,
        rgb: Arc::new(rgb),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::DVec2;
    use kadakas_core::underlay::{PngData, Rgba};

    fn underlay(opacity: f64) -> Underlay {
        let img = Rgba {
            width: 2,
            height: 1,
            data: vec![255, 0, 0, 255, 0, 0, 255, 0],
        };
        Underlay {
            name: String::new(),
            min: DVec2::new(1000.0, 2000.0),
            max: DVec2::new(5000.0, 3000.0),
            width: 2,
            height: 1,
            png: PngData::new(img.to_png()),
            opacity,
            source: String::new(),
        }
    }

    #[test]
    fn image_is_placed_on_paper_and_blended_with_white() {
        let xf = Xf {
            model_center: DVec2::new(3000.0, 2500.0),
            paper_center: DVec2::new(100.0, 100.0),
            turn: DVec2::X,
            frame: None,
            k: 0.01,
            window: None,
        };
        let i = image_prim(&underlay(1.0), &xf).unwrap();
        assert_eq!(i.min, DVec2::new(80.0, 95.0));
        assert_eq!(i.max, DVec2::new(120.0, 105.0));
        // Opaque red, then a clear pixel that shows the paper.
        assert_eq!(&i.rgb[..6], &[255, 0, 0, 255, 255, 255]);
        let half = image_prim(&underlay(0.5), &xf).unwrap();
        assert_eq!(&half.rgb[..3], &[255, 128, 128]);
        let mut bad = underlay(1.0);
        bad.png = PngData::new(vec![1, 2, 3]);
        assert!(image_prim(&bad, &xf).is_none());
    }
}
