//! Hit testing and box selection against document objects. The code is in `kadakas_annot::hit`,
//! so the agent API can use it without this crate.

pub use kadakas_annot::hit::*;

// What the tests below use of the code that moved to `kadakas_annot::hit`.
#[cfg(test)]
#[allow(unused_imports)]
use {
    crate::annot_pick::Prims,
    glam::DVec2,
    kadakas_core::curve,
    kadakas_core::geom::{dist_to_segment, point_in_polygon},
    kadakas_core::irrigation::IrrigationShape,
    kadakas_core::model::{Document, Object, ObjectId, Shape, arc_sweep},
};

/// Topmost pickable object at `p`.
#[cfg(test)]
pub fn hit_test(doc: &Document, p: DVec2, tol: f64) -> Option<ObjectId> {
    hit_test_in(Prims::default(), doc, p, tol, None)
}

/// Objects selected by a box. `crossing` selects objects that touch the box,
/// otherwise objects must be fully inside. In a group, the members of the group are selected. A group
/// is selected when any of its members is selected this way (crossing) or all of them (inside).
#[cfg(test)]
pub fn box_select(doc: &Document, a: DVec2, b: DVec2, crossing: bool) -> Vec<ObjectId> {
    box_select_in(Prims::default(), doc, a, b, crossing, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri() -> Vec<DVec2> {
        vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(100.0, 0.0),
            DVec2::new(0.0, 100.0),
        ]
    }

    #[test]
    fn points_and_boxes_against_a_polygon() {
        let t = tri();
        assert!(point_in_polygon(DVec2::new(10.0, 10.0), &t));
        assert!(!point_in_polygon(DVec2::new(90.0, 90.0), &t));
        // A box inside, a box across the edge, a box far away, a box that holds the polygon.
        let b = |x0: f64, y0: f64, x1: f64, y1: f64| (DVec2::new(x0, y0), DVec2::new(x1, y1));
        let (lo, hi) = b(5.0, 5.0, 20.0, 20.0);
        assert!(box_in_polygon(lo, hi, &t, false) && box_in_polygon(lo, hi, &t, true));
        let (lo, hi) = b(40.0, 40.0, 80.0, 80.0);
        assert!(!box_in_polygon(lo, hi, &t, false) && box_in_polygon(lo, hi, &t, true));
        let (lo, hi) = b(200.0, 200.0, 300.0, 300.0);
        assert!(!box_in_polygon(lo, hi, &t, true));
        let (lo, hi) = b(-50.0, -50.0, 500.0, 500.0);
        assert!(box_in_polygon(lo, hi, &t, true) && !box_in_polygon(lo, hi, &t, false));
        // A box that the polygon crosses with no corner inside either way: a thin bar.
        let bar = vec![
            DVec2::new(-10.0, 5.0),
            DVec2::new(200.0, 5.0),
            DVec2::new(200.0, 6.0),
            DVec2::new(-10.0, 6.0),
        ];
        let (lo, hi) = b(50.0, 0.0, 60.0, 100.0);
        assert!(box_in_polygon(lo, hi, &bar, true));
    }

    #[test]
    fn a_lasso_selects_inside_or_crossing() {
        use kadakas_core::command::{Command, Editor};
        use kadakas_core::model::Attributes;
        let mut ed = Editor::default();
        let mut add = |shape: Shape| {
            ObjectId(
                ed.execute(&Command::AddObject {
                    shape,
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
            )
        };
        let inside = add(Shape::Circle {
            center: DVec2::new(20.0, 20.0),
            radius: 5.0,
        });
        let across = add(Shape::Line {
            a: DVec2::new(50.0, 40.0),
            b: DVec2::new(200.0, 40.0),
        });
        let outside = add(Shape::Circle {
            center: DVec2::new(400.0, 400.0),
            radius: 5.0,
        });
        let t = tri();
        let inner = lasso_select_in(Prims::default(), ed.doc(), &t, false, None);
        assert_eq!(inner, vec![inside]);
        let touched = lasso_select_in(Prims::default(), ed.doc(), &t, true, None);
        assert!(
            touched.contains(&inside) && touched.contains(&across) && !touched.contains(&outside)
        );
        assert!(lasso_select_in(Prims::default(), ed.doc(), &t[..2], true, None).is_empty());
    }

    use crate::testkit::Session;
    use kadakas_core::command::Command;
    use kadakas_core::model::Attributes;

    fn doc_with(shapes: Vec<Shape>) -> Document {
        let mut s = Session::default();
        for shape in shapes {
            s.editor
                .execute(&Command::AddObject {
                    shape,
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap();
        }
        s.editor.doc().clone()
    }

    #[test]
    fn picks_line_and_topmost() {
        let doc = doc_with(vec![
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(100.0, 0.0),
            },
            Shape::Line {
                a: DVec2::new(50.0, -10.0),
                b: DVec2::new(50.0, 10.0),
            },
        ]);
        let ids: Vec<_> = doc.objects.values().map(|o| o.id).collect();
        assert_eq!(hit_test(&doc, DVec2::new(50.0, 1.0), 2.0), Some(ids[1]));
        assert_eq!(hit_test(&doc, DVec2::new(20.0, 1.0), 2.0), Some(ids[0]));
        assert_eq!(hit_test(&doc, DVec2::new(20.0, 30.0), 2.0), None);
    }

    #[test]
    fn unfilled_rect_needs_edge_but_area_does_not() {
        let doc = doc_with(vec![
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::splat(100.0),
            },
            Shape::Hardscape {
                boundary: kadakas_core::curve::corners(&[
                    DVec2::new(200.0, 0.0),
                    DVec2::new(300.0, 0.0),
                    DVec2::new(300.0, 100.0),
                ]),
                material: "x".into(),
                thickness: None,
                elevation: None,
            },
        ]);
        assert_eq!(hit_test(&doc, DVec2::splat(50.0), 2.0), None);
        assert!(hit_test(&doc, DVec2::new(0.0, 50.0), 2.0).is_some());
        assert!(hit_test(&doc, DVec2::new(280.0, 30.0), 2.0).is_some());
    }

    #[test]
    fn arc_angle_range() {
        let arc = Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start: 0.0,
            end: std::f64::consts::FRAC_PI_2,
        };
        assert!(shape_hit(&arc, false, DVec2::new(7.0, 7.1), 0.5));
        assert!(!shape_hit(&arc, false, DVec2::new(-7.0, -7.0), 0.5));
    }

    #[test]
    fn full_circle_arc_is_hit_all_around_and_zero_sweep_only_at_its_start() {
        let tau = std::f64::consts::TAU;
        let full = Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start: 0.5,
            end: 0.5 + tau,
        };
        for angle in [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0] {
            let p = DVec2::from_angle(angle) * 10.0;
            assert!(shape_hit(&full, false, p, 0.5), "angle {angle}");
        }
        // Not near the circle: no hit.
        assert!(!shape_hit(&full, false, DVec2::new(5.0, 0.0), 0.5));
        // An end angle of 2π below the start is the same full circle.
        let full_neg = Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start: 0.5,
            end: 0.5 - tau,
        };
        assert!(shape_hit(&full_neg, false, DVec2::new(-10.0, 0.0), 0.5));
        // Zero sweep (not valid in a document, but must not panic or hit everywhere).
        let zero = Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start: 0.5,
            end: 0.5,
        };
        assert!(!shape_hit(&zero, false, DVec2::new(-10.0, 0.0), 0.5));
        assert!(shape_hit(&zero, false, DVec2::from_angle(0.5) * 10.0, 0.5));
    }

    #[test]
    fn locked_layer_not_pickable() {
        let mut s = Session::default();
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::X,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        s.editor
            .execute(&Command::ModifyLayer {
                id: kadakas_core::model::FIRST_LAYER,
                name: None,
                visible: None,
                locked: Some(true),
                elevation: None,
                scale: None,
            })
            .unwrap();
        assert_eq!(hit_test(s.editor.doc(), DVec2::new(0.5, 0.0), 1.0), None);
    }

    #[test]
    fn box_modes() {
        let doc = doc_with(vec![
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 10.0,
            },
            Shape::Circle {
                center: DVec2::new(100.0, 0.0),
                radius: 10.0,
            },
        ]);
        let b = (DVec2::new(-20.0, -20.0), DVec2::new(105.0, 20.0));
        assert_eq!(box_select(&doc, b.0, b.1, false).len(), 1);
        assert_eq!(box_select(&doc, b.0, b.1, true).len(), 2);
    }

    #[test]
    fn rounded_corners_are_hit_on_the_curve_not_the_corner() {
        use kadakas_core::model::{Vertex, VertexKind};
        let arc = |x: f64, y: f64| Vertex::new(DVec2::new(x, y), VertexKind::Arc { radius: 100.0 });
        let shape = Shape::Polyline {
            points: vec![
                arc(0.0, 0.0),
                arc(1000.0, 0.0),
                arc(1000.0, 1000.0),
                arc(0.0, 1000.0),
            ],
            closed: true,
        };
        // The sharp corner is cut off: a click on it misses, a click on the arc hits.
        assert!(!shape_hit(&shape, false, DVec2::new(0.0, 0.0), 5.0));
        let on_arc = DVec2::new(100.0, 100.0) - DVec2::new(1.0, 1.0).normalize() * 100.0;
        assert!(shape_hit(&shape, false, on_arc, 5.0));
        assert!(shape_hit(&shape, false, DVec2::new(500.0, 0.0), 5.0));
        // Inside is hit when filled.
        assert!(shape_hit(&shape, true, DVec2::new(500.0, 500.0), 5.0));
        assert!(!shape_hit(&shape, false, DVec2::new(500.0, 500.0), 5.0));
        let area = Shape::Hardscape {
            boundary: shape.vertices().unwrap().clone(),
            material: "m".into(),
            thickness: None,
            elevation: None,
        };
        assert!(shape_hit(&area, false, DVec2::new(500.0, 500.0), 5.0));
        assert!(!shape_hit(&area, false, DVec2::new(-20.0, -20.0), 5.0));
    }

    #[test]
    fn existing_trees_pick_by_canopy_and_tags_by_label_or_leader() {
        use kadakas_core::plants::tree::{TreeAction, TreeCondition};
        let tree = Shape::ExistingTree {
            at: DVec2::ZERO,
            species: String::new(),
            dbh: 500.0,
            spread: 4000.0,
            height: 9000.0,
            condition: TreeCondition::Good,
            action: TreeAction::Retain,
            rpz_factor: None,
            rpz_cap: None,
        };
        assert!(shape_hit(&tree, false, DVec2::new(1900.0, 0.0), 5.0));
        // The root protection zone (6000 mm) does not pick the tree.
        assert!(!shape_hit(&tree, false, DVec2::new(3000.0, 0.0), 5.0));
        let tag = Shape::PlantTag {
            at: DVec2::new(1000.0, 1000.0),
            anchor: DVec2::ZERO,
            style: kadakas_core::resource::ResourceId::from_num(1),
            height: 400.0,
        };
        assert!(
            shape_hit(&tag, false, DVec2::new(1500.0, 1200.0), 5.0),
            "label"
        );
        assert!(
            shape_hit(&tag, false, DVec2::new(500.0, 500.0), 5.0),
            "leader"
        );
        assert!(!shape_hit(&tag, false, DVec2::new(-500.0, 500.0), 5.0));
    }

    #[test]
    fn a_degenerate_segment_still_measures_a_distance() {
        assert_eq!(
            dist_to_segment(DVec2::new(3.0, 4.0), DVec2::ZERO, DVec2::ZERO),
            5.0
        );
    }

    #[test]
    fn irrigation_lines_and_symbols_are_picked() {
        use kadakas_core::irrigation::LineType;
        let line = IrrigationShape::Line {
            points: vec![DVec2::ZERO, DVec2::new(1000.0, 0.0)],
            line_type: LineType::Lateral,
            size: "32".into(),
            material: "PE".into(),
            emitter_spacing: None,
            emitter_flow: None,
        };
        assert!(irrigation_hit(&line, DVec2::new(500.0, 4.0), 5.0));
        assert!(!irrigation_hit(&line, DVec2::new(500.0, 40.0), 5.0));
        let poc = IrrigationShape::Poc {
            at: DVec2::ZERO,
            name: String::new(),
            static_pressure: 1.0,
            available_flow: 1.0,
            supply_loss: None,
            meter_loss: None,
        };
        assert!(irrigation_hit(&poc, DVec2::new(8.0, 0.0), 5.0));
        assert!(!irrigation_hit(&poc, DVec2::new(11.0, 0.0), 5.0));
    }

    #[test]
    fn a_component_instance_is_picked_at_the_box_around_its_definition() {
        use kadakas_core::command::{Command, Editor};
        use kadakas_core::model::Attributes;
        use kadakas_core::surface::Edit;
        let mut ed = Editor::with_sequential_ids();
        let m = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::default()),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let p = [
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
        ];
        ed.execute(&Command::EditSurface {
            id: m,
            edit: Edit::AddEdges {
                edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
            },
        })
        .unwrap();
        let out = ed
            .execute(&Command::MakeComponent {
                name: "Tile".into(),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![m],
                replace: true,
            })
            .unwrap();
        let inst = ObjectId(out.created[1]);
        assert_eq!(
            hit_test(ed.doc(), DVec2::new(500.0, 500.0), 5.0),
            Some(inst)
        );
        assert_eq!(hit_test(ed.doc(), DVec2::new(-3.0, 500.0), 5.0), Some(inst));
        assert_eq!(hit_test(ed.doc(), DVec2::new(1500.0, 500.0), 5.0), None);
        // The shape alone does not know its definition.
        let Shape::Component { symbol, transform } = ed.doc().object(inst).unwrap().shape.clone()
        else {
            panic!()
        };
        assert!(!shape_hit(
            &Shape::Component { symbol, transform },
            true,
            DVec2::new(500.0, 500.0),
            5.0
        ));
    }
}
