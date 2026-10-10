//! Geoimages from files: PNG and JPEG with world files, and GeoTIFF.

use std::io::Cursor;

use kadakas_core::DVec2;
use kadakas_core::georef::Georeference;
use kadakas_core::underlay::Rgba;
use kadakas_gis::geoimage::{load_file, place, read_geotiff};
use tiff::encoder::{TiffEncoder, colortype};
use tiff::tags::Tag;

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kadakas-gis-img-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn red(w: u32, h: u32) -> Rgba {
    Rgba {
        width: w,
        height: h,
        data: [220, 20, 20, 255].repeat((w * h) as usize),
    }
}

fn georef() -> Georeference {
    Georeference {
        crs: "EPSG:3067".into(),
        origin: DVec2::new(385_000.0, 6_672_000.0),
        rotation: 0.0,
        user_origin: DVec2::ZERO,
    }
}

const WORLD: &str = "2\n0\n0\n-2\n385001\n6672009\n";

#[test]
fn png_with_a_world_file_and_prj() {
    let d = temp("png");
    std::fs::write(d.join("site.png"), red(10, 10).to_png()).unwrap();
    std::fs::write(d.join("site.pgw"), WORLD).unwrap();
    let img = load_file(&d.join("site.png"), None).unwrap();
    assert_eq!(img.name, "site");
    assert_eq!(img.crs, None);
    let placed = place(&img, Some(&georef()), 256).unwrap();
    assert!(placed.notes[0].contains("assumed"));
    assert!((placed.underlay.max.x - 20_000.0).abs() < 1e-6);
    // A .prj next to the picture gives the CRS; an argument overrides it.
    std::fs::write(
        d.join("site.prj"),
        "PROJCS[\"ETRS_1989_TM35FIN\",GEOGCS[\"GCS_ETRS_1989\",DATUM[\"D_ETRS_1989\",SPHEROID[\"GRS_1980\",6378137.0,298.257222101]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],PARAMETER[\"False_Easting\",500000.0],PARAMETER[\"False_Northing\",0.0],PARAMETER[\"Central_Meridian\",27.0],PARAMETER[\"Scale_Factor\",0.9996],PARAMETER[\"Latitude_Of_Origin\",0.0],UNIT[\"Meter\",1.0]]",
    )
    .unwrap();
    let img = load_file(&d.join("site.png"), None).unwrap();
    assert!(img.crs.as_deref().unwrap().starts_with("PROJCS"));
    let placed = place(&img, Some(&georef()), 256).unwrap();
    assert!(placed.notes.is_empty());
    let img = load_file(&d.join("site.png"), Some("EPSG:3067")).unwrap();
    assert_eq!(img.crs.as_deref(), Some("EPSG:3067"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn jpeg_with_a_jgw_and_missing_world_files() {
    let d = temp("jpg");
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
        .encode_image(&image::RgbImage::from_pixel(
            10,
            10,
            image::Rgb([20, 200, 20]),
        ))
        .unwrap();
    std::fs::write(d.join("a.jpg"), &jpg).unwrap();
    // No world file: the error names what was looked for.
    let e = load_file(&d.join("a.jpg"), None).unwrap_err().to_string();
    assert!(e.contains("no world file") && e.contains(".jgw"), "{e}");
    std::fs::write(d.join("a.jgw"), WORLD).unwrap();
    let img = load_file(&d.join("a.jpg"), Some("EPSG:3067")).unwrap();
    let placed = place(&img, Some(&georef()), 256).unwrap();
    let px = placed.underlay.decode(256).unwrap();
    assert!(px.data[1] > 150 && px.data[3] == 255);
    assert!(load_file(&d.join("missing.png"), None).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

fn geotiff(w: u32, h: u32, with_tags: bool, point: bool) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    let mut enc = TiffEncoder::new(&mut buf).unwrap();
    let mut img = enc.new_image::<colortype::RGB8>(w, h).unwrap();
    if with_tags {
        img.encoder()
            .write_tag(Tag::Unknown(33550), &[2.0f64, 2.0, 0.0][..])
            .unwrap();
        img.encoder()
            .write_tag(
                Tag::Unknown(33922),
                &[0.0f64, 0.0, 0.0, 385_000.0, 6_672_010.0, 0.0][..],
            )
            .unwrap();
        let mut keys: Vec<u16> = vec![1, 1, 0, 3, 1024, 0, 1, 1, 3072, 0, 1, 3067];
        keys.extend([1025, 0, 1, if point { 2 } else { 1 }]);
        img.encoder()
            .write_tag(Tag::Unknown(34735), &keys[..])
            .unwrap();
    }
    let data: Vec<u8> = [10u8, 120, 220].repeat((w * h) as usize);
    img.write_data(&data).unwrap();
    buf.into_inner()
}

#[test]
fn geotiff_with_tiepoint_scale_and_keys() {
    let g = read_geotiff("t", &geotiff(10, 10, true, false)).unwrap();
    assert_eq!(g.crs.as_deref(), Some("EPSG:3067"));
    assert_eq!(
        g.raster.affine.apply(0.0, 0.0),
        DVec2::new(385_000.0, 6_672_010.0)
    );
    assert_eq!(
        g.raster.affine.apply(10.0, 10.0),
        DVec2::new(385_020.0, 6_671_990.0)
    );
    assert_eq!(&g.raster.image.data[..4], &[10, 120, 220, 255]);
    // Pixel is point: the tiepoint is the center of the first pixel.
    let p = read_geotiff("t", &geotiff(10, 10, true, true)).unwrap();
    assert_eq!(
        p.raster.affine.apply(0.0, 0.0),
        DVec2::new(384_999.0, 6_672_011.0)
    );
    // A TIFF without georeference is refused with a hint.
    let e = read_geotiff("t", &geotiff(4, 4, false, false))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("no georeference") && e.contains("world file"),
        "{e}"
    );
    assert!(read_geotiff("t", b"not a tiff").is_err());
    // Through a file: GeoTIFF alone, and a world file wins over the tags.
    let d = temp("tif");
    std::fs::write(d.join("m.tif"), geotiff(10, 10, true, false)).unwrap();
    let img = load_file(&d.join("m.tif"), None).unwrap();
    let placed = place(&img, Some(&georef()), 256).unwrap();
    assert!((placed.underlay.max.y - 10_000.0).abs() < 1e-6);
    std::fs::write(d.join("m.tfw"), "1\n0\n0\n-1\n385000.5\n6672009.5\n").unwrap();
    let img = load_file(&d.join("m.tif"), None).unwrap();
    assert_eq!(img.raster.affine.a, 1.0);
    // A plain TIFF with a world file works too.
    std::fs::write(d.join("n.tif"), geotiff(5, 5, false, false)).unwrap();
    std::fs::write(d.join("n.tfw"), WORLD).unwrap();
    assert!(load_file(&d.join("n.tif"), Some("EPSG:3067")).is_ok());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn big_geoimages_are_reduced_to_the_cap() {
    let d = temp("big");
    std::fs::write(d.join("b.png"), red(300, 200).to_png()).unwrap();
    std::fs::write(d.join("b.pgw"), "1\n0\n0\n-1\n385000.5\n6672199.5\n").unwrap();
    let img = load_file(&d.join("b.png"), Some("EPSG:3067")).unwrap();
    let placed = place(&img, Some(&georef()), 100).unwrap();
    assert_eq!((placed.underlay.width, placed.underlay.height), (100, 67));
    let _ = std::fs::remove_dir_all(&d);
}

fn tiff_with<C: colortype::ColorType>(w: u32, h: u32, data: &[C::Inner], matrix: bool) -> Vec<u8>
where
    [C::Inner]: tiff::encoder::TiffValue,
{
    let mut buf = Cursor::new(Vec::new());
    let mut enc = TiffEncoder::new(&mut buf).unwrap();
    let mut img = enc.new_image::<C>(w, h).unwrap();
    if matrix {
        let m = [
            2.0f64,
            0.0,
            0.0,
            385_000.0,
            0.0,
            -2.0,
            0.0,
            6_672_010.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        img.encoder()
            .write_tag(Tag::Unknown(34264), &m[..])
            .unwrap();
    }
    img.write_data(data).unwrap();
    buf.into_inner()
}

#[test]
fn other_sample_formats_and_the_model_transformation() {
    // 8 bit gray: the value is copied into all channels.
    let g = read_geotiff(
        "g",
        &tiff_with::<colortype::Gray8>(2, 2, &[10, 20, 30, 40], true),
    )
    .unwrap();
    assert_eq!(
        &g.raster.image.data[..8],
        &[10, 10, 10, 255, 20, 20, 20, 255]
    );
    // The model transformation matrix gives the place.
    assert_eq!(
        g.raster.affine.apply(0.0, 0.0),
        DVec2::new(385_000.0, 6_672_010.0)
    );
    assert_eq!(
        g.raster.affine.apply(1.0, 1.0),
        DVec2::new(385_002.0, 6_672_008.0)
    );
    // 16 bit gray: the high byte is kept.
    let g16 = read_geotiff(
        "g",
        &tiff_with::<colortype::Gray16>(2, 1, &[0x1234, 0xFF00], true),
    )
    .unwrap();
    assert_eq!(
        &g16.raster.image.data[..8],
        &[0x12, 0x12, 0x12, 255, 0xFF, 0xFF, 0xFF, 255]
    );
    // RGBA keeps its alpha.
    let rgba = read_geotiff(
        "g",
        &tiff_with::<colortype::RGBA8>(1, 1, &[1, 2, 3, 4], true),
    )
    .unwrap();
    assert_eq!(rgba.raster.image.data, vec![1, 2, 3, 4]);
    // CMYK is not supported, and the message says what is.
    let e = read_geotiff(
        "g",
        &tiff_with::<colortype::CMYK8>(1, 1, &[1, 2, 3, 4], true),
    )
    .unwrap_err()
    .to_string();
    assert!(
        e.contains("not supported") && e.contains("gray, RGB or RGBA"),
        "{e}"
    );
    // 32 bit float samples are not supported either.
    let e = read_geotiff(
        "g",
        &tiff_with::<colortype::Gray32Float>(1, 1, &[1.5], true),
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("not 8 or 16 bit"), "{e}");
}
