use super::*;
use crate::model::{ClassId, LayerId};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

#[test]
fn slugs_are_layer_safe() {
    assert_eq!(slug("Zone 1"), "ZONE-1");
    assert_eq!(slug("  Stalls / north  "), "STALLS-NORTH");
    assert_eq!(slug("%%%"), "X");
    assert_eq!(slug("Kerb course"), "KERB-COURSE");
}

#[test]
fn corner_unit_names_tell_the_shape() {
    let sq = [v(0.0, 0.0), v(200.0, 0.0), v(200.0, 200.0), v(0.0, 200.0)];
    assert_eq!(corner_name(&sq), "corner 200x200");
    let rh = [v(0.0, 0.0), v(200.0, 0.0), v(300.0, 150.0), v(100.0, 150.0)];
    assert!(corner_name(&rh).contains("degrees"), "{}", corner_name(&rh));
    assert_eq!(number_text(2.0), "2");
    assert_eq!(number_text(2.04), "2");
    assert_eq!(number_text(2.26), "2.3");
}

#[test]
fn the_plan_is_checked() {
    let mut p = Plan::default();
    assert!(p.is_empty() && p.validate().is_ok());
    p.tolerance = 0.0;
    assert!(p.validate().is_err());
    let mut p = Plan::default();
    p.zones.push(ZoneSpec {
        seed: v(f64::NAN, 0.0),
        ..Default::default()
    });
    assert!(p.validate().unwrap_err().contains("seed"));
    let mut p = Plan::default();
    p.zones.push(ZoneSpec {
        align_to_split: Some(3),
        ..Default::default()
    });
    assert!(p.validate().unwrap_err().contains("align_to_split"));
    let mut p = Plan::default();
    p.courses.push(CourseDef {
        name: "a".into(),
        path: vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::corner(v(10_000.0, 0.0)),
        ],
        ..Default::default()
    });
    assert!(p.validate().is_ok());
    p.courses.push(p.courses[0].clone());
    assert!(p.validate().unwrap_err().contains("two courses"));
    p.courses.pop();
    p.courses[0].name = " ".into();
    assert!(p.validate().unwrap_err().contains("name"));
    p.courses[0].name = "a".into();
    p.courses[0].along = 1.0;
    assert!(p.validate().unwrap_err().contains("course 'a'"));
    // A course that follows a drawing needs no copy of the path.
    p.courses[0].along = 100.0;
    p.courses[0].path.clear();
    p.courses[0].source = Some(ObjectId::from_num(5));
    assert!(p.validate().is_ok());
    let mut p = Plan::default();
    p.splits.push(SplitLine {
        source: None,
        points: vec![Vertex::corner(v(f64::INFINITY, 0.0))],
    });
    assert!(p.validate().is_err());
}

#[test]
fn the_plan_moves_with_the_object() {
    let mut o = Object::new(
        ObjectId::from_num(1),
        LayerId::from_num(1),
        ClassId::from_num(0),
        Shape::Hardscape {
            boundary: curve::corners(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)]),
            material: String::new(),
            thickness: None,
            elevation: None,
        },
    );
    let mut p = Plan::default();
    p.splits.push(SplitLine {
        source: None,
        points: vec![Vertex::corner(v(1.0, 1.0)), Vertex::corner(v(2.0, 2.0))],
    });
    p.zones.push(ZoneSpec {
        seed: v(5.0, 5.0),
        origin: Some(v(1.0, 2.0)),
        ..Default::default()
    });
    p.courses.push(CourseDef {
        name: "k".into(),
        path: vec![Vertex::corner(v(0.0, 0.0)), Vertex::corner(v(0.0, 9.0))],
        ..Default::default()
    });
    o.data.insert(KEY.into(), serde_json::to_value(&p).unwrap());
    translate_data(&mut o, v(100.0, 50.0));
    let q = data(&o).unwrap();
    assert_eq!(q.splits[0].points[0].p, v(101.0, 51.0));
    assert_eq!(q.zones[0].seed, v(105.0, 55.0));
    assert_eq!(q.zones[0].origin, Some(v(101.0, 52.0)));
    assert_eq!(q.courses[0].path[1].p, v(100.0, 59.0));
    // Objects without a plan, and other shapes, stay as they are.
    let mut line = o.clone();
    line.shape = Shape::Line {
        a: v(0.0, 0.0),
        b: v(1.0, 0.0),
    };
    assert!(data(&line).is_none());
    translate_data(&mut line, v(1.0, 1.0));
    o.data.insert(KEY.into(), serde_json::json!({"splits": 5}));
    assert!(has_unreadable_data(&o) && data(&o).is_none());
    translate_data(&mut o, v(1.0, 1.0));
    assert!(has_unreadable_data(&o));
}

#[test]
fn paths_of_drawn_objects() {
    assert_eq!(
        path_of(&Shape::Line {
            a: v(0.0, 0.0),
            b: v(1.0, 0.0)
        })
        .unwrap()
        .0
        .len(),
        2
    );
    let (arc, closed) = path_of(&Shape::Arc {
        center: v(0.0, 0.0),
        radius: 10.0,
        start: 0.0,
        end: std::f64::consts::PI,
    })
    .unwrap();
    assert_eq!(arc.len(), 3);
    assert!(!closed);
    assert!((arc[1].p.y - 10.0).abs() < 1e-9);
    assert!(
        path_of(&Shape::Arc {
            center: v(0.0, 0.0),
            radius: 10.0,
            start: 0.0,
            end: std::f64::consts::TAU
        })
        .is_none()
    );
    assert!(
        path_of(&Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0
        })
        .is_none()
    );
}
