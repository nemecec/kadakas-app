//! Massing models and image underlays in the core: commands, files, 2D (SVG) and 3D (mesh).

use std::sync::Arc;

use glam::DVec2;
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::curve::corners;
use kadakas_core::massing::{Massing, RoofStyle};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::svg;
use kadakas_core::underlay::{PngData, Rgba, Underlay};

fn footprint(w: f64, d: f64) -> Vec<Vertex> {
    corners(&[
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, d),
        DVec2::new(0.0, d),
    ])
}

fn add(ed: &mut Editor, shape: Shape) -> Result<ObjectId, CommandError> {
    ed.execute(&Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .map(|o| ObjectId(o.created[0]))
}

fn house(roof: RoofStyle) -> Shape {
    let mut m = Massing::new(footprint(10_000.0, 6000.0), 6000.0);
    m.roof = roof;
    Shape::Massing(Box::new(m))
}

fn underlay(min: DVec2, max: DVec2) -> Shape {
    let img = Rgba {
        width: 2,
        height: 2,
        data: [200, 40, 40, 255].repeat(4),
    };
    Shape::Underlay(Box::new(Underlay {
        name: "map".into(),
        min,
        max,
        width: 2,
        height: 2,
        png: PngData(Arc::new(img.to_png())),
        opacity: 0.5,
        source: "test".into(),
    }))
}

#[test]
fn commands_check_the_values() {
    let mut ed = Editor::default();
    assert!(add(&mut ed, house(RoofStyle::Hip)).is_ok());
    let mut bad = Massing::new(footprint(1000.0, 1000.0), 3000.0);
    bad.pitch_deg = 120.0;
    let e = add(&mut ed, Shape::Massing(Box::new(bad.clone()))).unwrap_err();
    assert!(e.to_string().contains("pitch"), "{e}");
    bad.pitch_deg = 30.0;
    bad.height = f64::NAN;
    assert!(add(&mut ed, Shape::Massing(Box::new(bad))).is_err());
    let mut far = Massing::new(footprint(1000.0, 1000.0), 3000.0);
    far.base = Some(1.0e12);
    assert!(add(&mut ed, Shape::Massing(Box::new(far))).is_err());
    // An underlay needs a size and a PNG.
    assert!(add(&mut ed, underlay(DVec2::ZERO, DVec2::ZERO)).is_err());
    let Shape::Underlay(mut u) = underlay(DVec2::ZERO, DVec2::splat(100.0)) else {
        panic!()
    };
    u.png = PngData(Arc::new(vec![1, 2, 3, 4, 5, 6, 7, 8, 9]));
    assert!(
        add(&mut ed, Shape::Underlay(u))
            .unwrap_err()
            .to_string()
            .contains("PNG")
    );
    assert!(add(&mut ed, underlay(DVec2::ZERO, DVec2::splat(100.0))).is_ok());
}

#[test]
fn files_round_trip_and_bad_files_are_refused() {
    let mut ed = Editor::default();
    let m = add(&mut ed, house(RoofStyle::Gable)).unwrap();
    let u = add(&mut ed, underlay(DVec2::ZERO, DVec2::new(5000.0, 3000.0))).unwrap();
    let text = serde_json::to_string(ed.doc()).unwrap();
    let back = Document::from_json(&text).unwrap();
    assert_eq!(
        back.object(m).unwrap().shape,
        ed.doc().object(m).unwrap().shape
    );
    assert_eq!(
        back.object(u).unwrap().shape,
        ed.doc().object(u).unwrap().shape
    );
    // The file holds the picture as text, and the shape kinds as agents write them.
    assert!(text.contains("\"kind\":\"massing\"") && text.contains("\"kind\":\"underlay\""));
    // A file with a broken value does not open.
    let broken = text.replace("\"pitch_deg\":30.0", "\"pitch_deg\":300.0");
    assert!(Document::from_json(&broken).is_err());
    // A massing model written by hand needs only the footprint.
    let minimal = text.replace("\"roof\":\"gable\",", "");
    assert!(Document::from_json(&minimal).is_ok());
    // Move and copy work like for other shapes.
    ed.execute(&Command::MoveObjects {
        ids: vec![m, u],
        delta: DVec2::new(100.0, 200.0),
    })
    .unwrap();
    let Shape::Underlay(moved) = &ed.doc().object(u).unwrap().shape else {
        panic!()
    };
    assert_eq!(moved.min, DVec2::new(100.0, 200.0));
    let Shape::Massing(mm) = &ed.doc().object(m).unwrap().shape else {
        panic!()
    };
    assert_eq!(mm.footprint[0].p, DVec2::new(100.0, 200.0));
    // A massing model turns (with its ridge); an underlay does not.
    ed.execute(&Command::RotateObjects {
        ids: vec![m],
        center: DVec2::ZERO,
        angle: 0.5,
    })
    .unwrap();
    let e = ed
        .execute(&Command::RotateObjects {
            ids: vec![u],
            center: DVec2::ZERO,
            angle: 0.5,
        })
        .unwrap_err();
    assert!(e.to_string().contains("cannot be turned"));
}

#[test]
fn rotation_turns_the_ridge() {
    let mut ed = Editor::default();
    let mut model = Massing::new(footprint(10_000.0, 6000.0), 3000.0);
    model.ridge_angle = Some(0.0);
    model.roof = RoofStyle::Gable;
    let m = add(&mut ed, Shape::Massing(Box::new(model))).unwrap();
    ed.execute(&Command::RotateObjects {
        ids: vec![m],
        center: DVec2::ZERO,
        angle: std::f64::consts::FRAC_PI_2,
    })
    .unwrap();
    let Shape::Massing(r) = &ed.doc().object(m).unwrap().shape else {
        panic!()
    };
    assert!((r.ridge_angle.unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((r.footprint[1].p - DVec2::new(0.0, 10_000.0)).length() < 1e-6);
    // The volume does not change when the model is turned.
    let v0 = Massing::new(footprint(10_000.0, 6000.0), 3000.0)
        .quantities()
        .footprint_area;
    assert!((r.quantities().footprint_area - v0).abs() < 1e-3);
}

#[test]
fn svg_draws_underlays_first_and_massing_with_roof_lines() {
    let mut ed = Editor::default();
    // The house is added first, the underlay last: the picture is still at the bottom.
    let h = add(&mut ed, house(RoofStyle::Gable)).unwrap();
    let u = add(
        &mut ed,
        underlay(DVec2::new(-1000.0, -1000.0), DVec2::new(12_000.0, 8000.0)),
    )
    .unwrap();
    let s = svg::render(ed.doc(), 800);
    let image = s.find("<image ").expect("the picture is drawn");
    let polygon = s
        .find(&format!("data-id=\"{h}\""))
        .expect("the house is drawn");
    assert!(image < polygon, "the underlay is below the vector objects");
    assert!(s.contains("data:image/png;base64,"));
    assert!(
        s.contains("data-kind=\"massing_roof\""),
        "a gable roof shows its ridge"
    );
    assert!(s.contains("opacity=\"0.5\""));
    // Y is flipped: the top of the picture is at -max.y.
    assert!(s.contains("y=\"-8000\""));
    let _ = u;
    // A flat roof has no roof lines.
    let mut flat = Editor::default();
    add(&mut flat, house(RoofStyle::Flat)).unwrap();
    assert!(!svg::render(flat.doc(), 400).contains("massing_roof"));
    // The drawing bounds include the picture.
    let (lo, hi) = svg::drawing_bounds(ed.doc()).unwrap();
    assert!(lo.x <= -1000.0 && hi.y >= 8000.0);
}

#[test]
fn massing_is_a_closed_solid_in_the_3d_scene_and_underlays_are_not() {
    let mut ed = Editor::default();
    let h = add(&mut ed, house(RoofStyle::Hip)).unwrap();
    let u = add(&mut ed, underlay(DVec2::ZERO, DVec2::splat(20_000.0))).unwrap();
    let scene = mesh::build(ed.doc());
    let (lo, hi) = scene.object_bounds(h).expect("the house has geometry");
    assert!(scene.objects().any(|o| o == h));
    assert!(
        !scene.objects().any(|o| o == u),
        "an underlay has no 3D form"
    );
    // The base is at the layer elevation (0) and the top at the ridge.
    let rise = 3000.0 * 30f64.to_radians().tan();
    assert!(
        ((hi.z - lo.z) as f64 - (6000.0 + rise)).abs() < 1.0,
        "{lo:?} {hi:?}"
    );
    assert!(scene.mesh.triangle_count() > 10, "walls and roof");
    assert!(
        scene.lines.segment_count() >= 8,
        "base, eave and roof lines"
    );
    // A model with its own base height stands there, above a layer with elevation.
    let mut ed = Editor::default();
    ed.execute(&Command::ModifyLayer {
        id: FIRST_LAYER,
        name: None,
        visible: None,
        locked: None,
        elevation: Some(1000.0),
        scale: None,
    })
    .unwrap();
    let mut m = Massing::new(footprint(5000.0, 5000.0), 3000.0);
    m.base = Some(500.0);
    add(&mut ed, Shape::Massing(Box::new(m))).unwrap();
    let scene = mesh::build(ed.doc());
    let (lo, hi) = scene.bounds.unwrap();
    assert!((scene.origin.z + f64::from(lo.z) - 1500.0).abs() < 1.0);
    assert!((f64::from(hi.z - lo.z) - 3000.0).abs() < 1.0);
}

#[test]
fn properties_for_the_data_manager() {
    use kadakas_core::props;
    let mut ed = Editor::default();
    let mut m = Massing::new(footprint(10_000.0, 8000.0), 3000.0);
    m.floors = Some(4);
    m.floor_height = 3000.0;
    m.name = "Hall".into();
    let id = add(&mut ed, Shape::Massing(Box::new(m))).unwrap();
    let o = ed.doc().object(id).unwrap();
    let get = |name: &str| props::property(ed.doc(), o, &[name]);
    assert_eq!(get("floors"), props::Prop::Value(4.0.into()));
    assert_eq!(get("kind"), props::Prop::Value("massing".into()));
    let gfa = match get("grossfloorarea") {
        props::Prop::Value(v) => v.as_f64().unwrap(),
        other => panic!("{other:?}"),
    };
    assert!((gfa - 320.0).abs() < 1e-9);
    assert!(matches!(get("volume"), props::Prop::Value(_)));
    assert!(matches!(get("area"), props::Prop::Value(_)));
    assert!(matches!(get("perimeter"), props::Prop::Value(_)));
    assert_eq!(get("name"), props::Prop::Value("Hall".into()));
    assert_eq!(get("height"), props::Prop::Value(12_000.0.into()));
    assert_eq!(get("roof"), props::Prop::Value("flat".into()));
    assert_eq!(get("grossarea"), props::Prop::NotApplicable);
}
