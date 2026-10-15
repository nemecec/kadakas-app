//! `image_pdf`: a PDF page that holds a picture (the compare of two drawings, IOP-24).

use kadakas_print::image_pdf;

fn grey(w: u32, h: u32) -> Vec<u8> {
    vec![128; (w * h * 3) as usize]
}

#[test]
fn the_page_has_the_size_of_the_picture_and_holds_it() {
    // 300 x 150 pixels at 150 dpi: 2 x 1 inches.
    let pdf = image_pdf(grey(300, 150), 300, 150, 150.0, "Compare").unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 1"), "{text}");
    assert!(text.contains("/Image"), "{text}");
    assert!(
        text.contains("/Width 300") && text.contains("/Height 150"),
        "{text}"
    );
    // 2 x 1 inch = 144 x 72 points.
    assert!(text.contains("144") && text.contains("72"), "{text}");
    assert!(text.contains("Compare"), "{text}");
}

#[test]
fn a_picture_that_does_not_fit_is_refused() {
    assert!(image_pdf(grey(10, 10), 0, 10, 150.0, "x").is_none());
    assert!(image_pdf(grey(10, 10), 10, 0, 150.0, "x").is_none());
    assert!(image_pdf(grey(10, 10), 10, 10, 0.0, "x").is_none());
    assert!(image_pdf(grey(10, 10), 10, 10, f64::NAN, "x").is_none());
    // The pixels do not match the size.
    assert!(image_pdf(vec![0; 5], 10, 10, 150.0, "x").is_none());
}
