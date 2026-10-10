//! Sheets show the grading graphics: the contours and the analysis colors of the site model,
//! walls with their tags, grade arrows and labels, spot elevations, section ends and the legend.

use cadektor_core::command::{Command, Editor};
use cadektor_core::curve::corners;
use cadektor_core::model::*;
use cadektor_core::sheets::*;
use cadektor_print::compose_page;
use cadektor_print::display::*;
use glam::{DVec2, DVec3};
use serde_json::json;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn add(
    ed: &mut Editor,
    shape: Shape,
    layer: Option<LayerId>,
    data: Option<(&str, serde_json::Value)>,
) -> ObjectId {
    let out = ed
        .execute(&Command::AddObject {
            shape,
            layer,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let id = ObjectId(out.created[0]);
    if let Some((key, value)) = data {
        ed.execute(&Command::SetData {
            id,
            set: [(key.to_string(), value)].into(),
            remove: vec![],
        })
        .unwrap();
    }
    id
}

/// A site model with a ramp (10 percent) and a pad, with every grading object, and a sheet with a plan viewport.
fn project() -> (Editor, LayerId) {
    let mut ed = Editor::default();
    let points: Vec<DVec3> = (0..11)
        .flat_map(|i| {
            (0..11).map(move |j| DVec3::new(i as f64 * 1000.0, j as f64 * 1000.0, i as f64 * 100.0))
        })
        .collect();
    add(
        &mut ed,
        Shape::SiteModel {
            points,
            lines: vec![],
            settings: Default::default(),
        },
        None,
        Some((
            "site_analysis",
            json!({"mode": "slope", "surface": "existing", "breaks": [5.0, 20.0], "colors": [[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]]}),
        )),
    );
    add(
        &mut ed,
        Shape::SiteModifier {
            modifier: ModifierKind::Pad {
                elevation: 900.0,
                slope: None,
            },
            boundary: corners(&[
                v(6000.0, 6000.0),
                v(8000.0, 6000.0),
                v(8000.0, 8000.0),
                v(6000.0, 8000.0),
            ]),
        },
        None,
        None,
    );
    add(
        &mut ed,
        Shape::Polyline {
            points: corners(&[v(1000.0, 1000.0), v(5000.0, 1000.0), v(5000.0, 3000.0)]),
            closed: false,
        },
        None,
        Some((
            "landscape_wall",
            json!({"top": {"mode": "terrain", "offset": 600.0}}),
        )),
    );
    add(
        &mut ed,
        Shape::Polyline {
            points: corners(&[
                v(1000.0, 8000.0),
                v(4000.0, 8000.0),
                v(4000.0, 9500.0),
                v(1000.0, 9500.0),
            ]),
            closed: true,
        },
        None,
        Some((
            "landscape_wall",
            json!({"top": {"mode": "fixed", "elevation": 1200.0}}),
        )),
    );
    add(
        &mut ed,
        Shape::Line {
            a: v(1000.0, 5000.0),
            b: v(6000.0, 5000.0),
        },
        None,
        Some(("grade_object", json!({"snap": "existing"}))),
    );
    add(
        &mut ed,
        Shape::Circle {
            center: v(7000.0, 7000.0),
            radius: 100.0,
        },
        None,
        Some(("spot_elevation", json!({"prefix": "TC"}))),
    );
    add(
        &mut ed,
        Shape::Line {
            a: v(0.0, 4000.0),
            b: v(10_000.0, 4000.0),
        },
        None,
        Some(("site_section", json!({"name": "B"}))),
    );
    add(
        &mut ed,
        Shape::Rect {
            min: v(10_500.0, 0.0),
            max: v(13_000.0, 4000.0),
        },
        None,
        Some(("analysis_legend", json!({"show_areas": true}))),
    );
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "Grading".into(),
            paper: Some(PaperSize::A3),
            orientation: None,
            number: Some("L-301".into()),
            title: Some("Grading plan".into()),
        })
        .unwrap()
        .created[0],
    );
    let mut vp = Viewport::new(
        v(20.0, 40.0),
        v(400.0, 250.0),
        v(6500.0, 4800.0),
        50.0,
        vec![FIRST_LAYER],
    );
    vp.fit(ed.doc());
    add(&mut ed, Shape::Viewport(Box::new(vp)), Some(sheet), None);
    (ed, sheet)
}

fn texts(prims: &[Prim]) -> Vec<(String, f64, Align)> {
    prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text(t) => Some((t.text.clone(), t.height, t.align)),
            _ => None,
        })
        .collect()
}

fn fills(prims: &[Prim]) -> Vec<Rgba> {
    prims
        .iter()
        .filter_map(|p| match p {
            Prim::Path(p) => p.fill,
            _ => None,
        })
        .collect()
}

#[test]
fn a_sheet_shows_the_grading_graphics() {
    let (ed, sheet) = project();
    let page = compose_page(ed.doc(), sheet).unwrap();
    let t = texts(&page.prims);
    let has = |s: &str| t.iter().any(|(x, ..)| x.contains(s));
    // Tags of the walls (the ground at x = 1000 is 100 mm, so the top is 700), and of the closed wall.
    assert!(has("TW 0.70 BW 0.10"), "{t:?}");
    assert!(has("TW 1.20 BW 0.10"));
    // The grade between x = 1000 (100) and x = 6000 (600): 10 percent.
    assert!(has("10.0% (1:10)"), "{t:?}");
    assert!(has("0.10") && has("0.60"));
    // The spot elevation on the pad, with its existing and proposed heights.
    assert!(has("TC EX 0.70 / PR 0.90"), "{t:?}");
    // The ends of the section line.
    assert!(t.iter().filter(|(x, ..)| x == "B").count() == 2);
    // The legend: a title, a row per band with its area, the highest band first.
    assert!(has("Slope") && has("20 % and above") && has("below 5 %"));
    assert!(has("m2)"));
    // Text is 2.5 mm on paper (3.5 for section names), wherever the viewport scale is.
    let tag = t.iter().find(|(x, ..)| x.starts_with("TW 0.70")).unwrap();
    assert!((tag.1 - 2.5).abs() < 1e-9, "{tag:?}");
    let name = t.iter().find(|(x, ..)| x == "B").unwrap();
    assert!((name.1 - 3.5).abs() < 1e-9 && name.2 == Align::Center);
    // The ramp is 10 percent: every triangle of the existing surface is in the middle band, in its color.
    let f = fills(&page.prims);
    assert!(
        f.iter().any(|c| c[..3] == [4, 5, 6]),
        "the analysis color is on the sheet"
    );
    // The legend swatches show every band color, also the one without triangles.
    for color in [[1u8, 2, 3], [4, 5, 6], [7, 8, 9]] {
        assert!(f.iter().any(|c| c[..3] == color), "{color:?}");
    }
    // Contours of the site model are on the sheet too: paths without fill, in the proposed color.
    let contour = [30u8, 120, 50];
    assert!(page.prims.iter().any(|p| matches!(p, Prim::Path(p) if p.fill.is_none() && p.stroke.is_some_and(|c| c[..3] == contour))));
}

#[test]
fn the_ring_of_a_closed_wall_stays_open_in_the_middle() {
    let (ed, sheet) = project();
    let page = compose_page(ed.doc(), sheet).unwrap();
    // The footprint of the closed wall has two rings in one path, and they run in opposite directions.
    let two_rings: Vec<&PathPrim> = page
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Path(p) if p.fill.is_some_and(|c| c[..3] == [168, 160, 150]) => Some(p),
            _ => None,
        })
        .filter(|p| p.segs.iter().filter(|s| matches!(s, PSeg::Move(_))).count() == 2)
        .collect();
    assert_eq!(two_rings.len(), 1);
    let pts: Vec<DVec2> = two_rings[0]
        .segs
        .iter()
        .map(|s| match s {
            PSeg::Move(p) | PSeg::Line(p) => *p,
            PSeg::Cubic(.., p) => *p,
        })
        .collect();
    let split = two_rings[0]
        .segs
        .iter()
        .skip(1)
        .position(|s| matches!(s, PSeg::Move(_)))
        .unwrap()
        + 1;
    let signed = |r: &[DVec2]| {
        (0..r.len())
            .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
            .sum::<f64>()
    };
    assert!(
        signed(&pts[..split]) * signed(&pts[split..]) < 0.0,
        "opposite directions"
    );
}

#[test]
fn a_site_without_analysis_has_no_band_fills_and_other_sheets_are_unchanged() {
    let (mut ed, sheet) = project();
    let site = ed
        .doc()
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::SiteModel { .. }))
        .unwrap()
        .id;
    ed.execute(&Command::SetData {
        id: site,
        set: [("site_analysis".to_string(), json!({"mode": "off"}))].into(),
        remove: vec![],
    })
    .unwrap();
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert!(!fills(&page.prims).iter().any(|c| c[..3] == [4, 5, 6]));
    // The legend has nothing to show.
    assert!(
        !texts(&page.prims)
            .iter()
            .any(|(x, ..)| x.contains("20 % and above"))
    );
}
