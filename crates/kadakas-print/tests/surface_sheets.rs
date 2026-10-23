//! Surface models on sheets (MOD-13): the plan viewport draws their faces in the colours of their
//! materials and the edges that are shown, a section cuts a closed solid to a filled outline and an
//! open surface to a line, a hidden-line viewport leaves the soft edges out, and the PDF, SVG and
//! PNG outputs carry them.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh::{SURFACE_BACK, SURFACE_FRONT};
use kadakas_core::model::*;
use kadakas_core::resource::ResourceId;
use kadakas_core::section::{KEY, SectionView};
use kadakas_core::sheets::Viewport;
use kadakas_core::surface::{EdgeId, Edit, PaintSide};
use kadakas_print::compose::hidden_line_prims;
use kadakas_print::display::{PSeg, PathPrim, Prim};
use kadakas_print::{PageCache, compose_page, export_pdf, select_sheets, svg};
use lopdf::content::Content;
use serde_json::json;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn run(ed: &mut Editor, c: Command) -> Vec<kadakas_core::uid::Uid> {
    ed.execute(&c).unwrap().created
}

/// A closed box with its corner at `(x, y, 0)`: a square pulled up, so that the fronts of its faces
/// look out.
fn add_box_in(ed: &mut Editor, x: f64, y: f64, s: f64, h: f64, class: Option<ClassId>) -> ObjectId {
    let id = ObjectId(
        run(
            ed,
            Command::AddObject {
                shape: Shape::SurfaceModel(Box::default()),
                layer: None,
                class,
                attrs: Attributes::default(),
            },
        )[0],
    );
    let p = [
        [x, y, 0.0],
        [x + s, y, 0.0],
        [x + s, y + s, 0.0],
        [x, y + s, 0.0],
    ];
    run(
        ed,
        Command::EditSurface {
            id,
            edit: Edit::AddEdges {
                edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
            },
        },
    );
    let face = model(ed, id).faces().next().unwrap().0;
    run(
        ed,
        Command::EditSurface {
            id,
            edit: Edit::PushPull { face, distance: h },
        },
    );
    id
}

fn add_box(ed: &mut Editor, x: f64, y: f64, s: f64, h: f64) -> ObjectId {
    add_box_in(ed, x, y, s, h, None)
}

fn model(ed: &Editor, id: ObjectId) -> &kadakas_core::surface::SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("{}", other.kind()),
    }
}

fn add_sheet(ed: &mut Editor, vp: Viewport) -> (LayerId, ObjectId) {
    let sheet = LayerId(
        run(
            ed,
            Command::AddSheet {
                name: "S".into(),
                paper: None,
                orientation: None,
                number: Some("1".into()),
                title: None,
            },
        )[0],
    );
    let vp = ObjectId(
        run(
            ed,
            Command::AddObject {
                shape: Shape::Viewport(Box::new(vp)),
                layer: Some(sheet),
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    );
    (sheet, vp)
}

fn plan_viewport(ed: &mut Editor, scale: f64) -> (LayerId, ObjectId) {
    let vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(500.0, 500.0),
        scale,
        vec![FIRST_LAYER],
    );
    add_sheet(ed, vp)
}

fn set_viewport(ed: &mut Editor, id: ObjectId, f: impl FnOnce(&mut Viewport)) {
    let Shape::Viewport(vp) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    let mut vp = vp.clone();
    f(&mut vp);
    ed.execute(&Command::ModifyObject {
        id,
        shape: Some(Shape::Viewport(vp)),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
}

fn paths(prims: &[Prim]) -> Vec<&PathPrim> {
    prims
        .iter()
        .filter_map(|p| if let Prim::Path(p) = p { Some(p) } else { None })
        .collect()
}

fn points(p: &PathPrim) -> Vec<DVec2> {
    p.segs
        .iter()
        .filter_map(|s| match s {
            PSeg::Move(q) | PSeg::Line(q) => Some(*q),
            _ => None,
        })
        .collect()
}

fn bounds(pts: &[DVec2]) -> (DVec2, DVec2) {
    pts.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(l, h), p| (l.min(*p), h.max(*p)),
    )
}

/// The filled paths with the colour, and the stroked lines (edges), of a plan.
fn fills(prims: &[Prim], color: [u8; 4]) -> Vec<&PathPrim> {
    paths(prims)
        .into_iter()
        .filter(|p| p.fill == Some(color))
        .collect()
}

fn edge_paths(prims: &[Prim]) -> Vec<&PathPrim> {
    paths(prims)
        .into_iter()
        .filter(|p| p.fill.is_none() && !p.closed && p.stroke.is_some() && p.segs.len() >= 2)
        .collect()
}

/// The edges of the page as separate segments.
fn edge_segments(prims: &[Prim]) -> usize {
    edge_paths(prims)
        .iter()
        .map(|p| p.segs.iter().filter(|s| matches!(s, PSeg::Move(_))).count())
        .sum()
}

fn material(ed: &mut Editor, name: &str, color: [u8; 4]) -> ResourceId {
    ResourceId(
        run(
            ed,
            Command::AddResource {
                name: name.into(),
                body: kadakas_core::resource::ResourceBody::Render(Box::new(
                    kadakas_core::present::RenderResource::Material(
                        kadakas_core::present::Material {
                            base_color: Color(color),
                            ..Default::default()
                        },
                    ),
                )),
            },
        )[0],
    )
}

fn top_face(ed: &Editor, id: ObjectId) -> kadakas_core::surface::FaceId {
    model(ed, id)
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(f, _)| f)
        .unwrap()
}

fn vertical_edges(ed: &Editor, id: ObjectId) -> Vec<EdgeId> {
    let m = model(ed, id);
    m.edges()
        .filter(|(_, e)| m.vertex(e.v[0]).unwrap().z != m.vertex(e.v[1]).unwrap().z)
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn the_plan_viewport_draws_the_top_of_a_box_with_its_edges_to_scale() {
    let mut ed = Editor::default();
    add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let (sheet, _) = plan_viewport(&mut ed, 100.0);
    let page = compose_page(ed.doc(), sheet).unwrap();
    // The top is a square of 1000 mm: 10 mm on paper at 1:100, with the center of the
    // viewport (500, 500) at the center of the frame (120, 70).
    let tops = fills(&page.prims, SURFACE_FRONT);
    assert_eq!(tops.len(), 1);
    let (lo, hi) = bounds(&points(tops[0]));
    assert!((lo - v(115.0, 65.0)).length() < 1e-6, "{lo}");
    assert!((hi - v(125.0, 75.0)).length() < 1e-6, "{hi}");
    // The bottom is seen from its back, under the top.
    let bottoms = fills(&page.prims, SURFACE_BACK);
    assert_eq!(bottoms.len(), 1);
    let at = |color: [u8; 4]| {
        page.prims
            .iter()
            .position(|p| matches!(p, Prim::Path(q) if q.fill == Some(color)))
            .unwrap()
    };
    assert!(at(SURFACE_BACK) < at(SURFACE_FRONT));
    // 8 edges: the 4 vertical ones are points in the plan.
    assert_eq!(edge_segments(&page.prims), 8);
}

#[test]
fn a_material_colours_the_face_in_the_plan_and_in_the_outputs() {
    let mut ed = Editor::default();
    let b = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let brick = material(&mut ed, "Brick", [180, 60, 40, 255]);
    let top = top_face(&ed, b);
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::SetFaceMaterial {
                faces: vec![top],
                side: PaintSide::Front,
                material: Some(brick.to_string()),
            },
        },
    );
    let (sheet, _) = plan_viewport(&mut ed, 100.0);
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert_eq!(fills(&page.prims, [180, 60, 40, 255]).len(), 1);
    assert!(fills(&page.prims, SURFACE_FRONT).is_empty());
    // The SVG carries the colour and the PDF fills with it.
    let text = svg::page_svg(&page);
    assert!(
        text.contains("rgb(180,60,40)") || text.contains("#b43c28") || text.contains("180,60,40"),
        "{text}"
    );
    let mut cache = PageCache::default();
    let bytes = export_pdf(
        ed.doc(),
        ed.revision(),
        &mut cache,
        &select_sheets(ed.doc(), None).unwrap(),
    )
    .unwrap();
    let pdf = lopdf::Document::load_mem(&bytes).unwrap();
    let id = pdf.get_pages()[&1];
    let ops = Content::decode(&pdf.get_page_content(id))
        .unwrap()
        .operations;
    let want = [180.0 / 255.0, 60.0 / 255.0, 40.0 / 255.0];
    assert!(
        ops.iter().any(|op| {
            op.operator == "rg"
                && op.operands.len() == 3
                && op
                    .operands
                    .iter()
                    .zip(want)
                    .all(|(o, w)| o.as_float().is_ok_and(|f| (f64::from(f) - w).abs() < 0.01))
        }),
        "no fill with the colour of the material"
    );
}

#[test]
fn soft_hidden_and_covered_things_are_not_drawn_in_the_plan() {
    let mut ed = Editor::default();
    let b = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let (sheet, _) = plan_viewport(&mut ed, 100.0);
    let edges_of = |ed: &Editor| edge_segments(&compose_page(ed.doc(), sheet).unwrap().prims);
    assert_eq!(edges_of(&ed), 8);
    // Soften one top edge and hide one bottom edge.
    let (top, bottom): (Vec<EdgeId>, Vec<EdgeId>) = {
        let m = model(&ed, b);
        let at = |z: f64| -> Vec<EdgeId> {
            m.edges()
                .filter(|(_, e)| {
                    m.vertex(e.v[0]).unwrap().z == z && m.vertex(e.v[1]).unwrap().z == z
                })
                .map(|(id, _)| id)
                .collect()
        };
        (at(500.0), at(0.0))
    };
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::SetEdgeStyle {
                edges: vec![top[0]],
                soft: Some(true),
                smooth: None,
            },
        },
    );
    assert_eq!(edges_of(&ed), 7);
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::SetHidden {
                faces: vec![],
                edges: vec![bottom[0]],
                hidden: true,
            },
        },
    );
    assert_eq!(edges_of(&ed), 6);
    // A hidden top face leaves the bottom.
    let face = top_face(&ed, b);
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::SetHidden {
                faces: vec![face],
                edges: vec![],
                hidden: true,
            },
        },
    );
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert!(fills(&page.prims, SURFACE_FRONT).is_empty());
    assert_eq!(fills(&page.prims, SURFACE_BACK).len(), 1);
}

#[test]
fn a_class_and_a_layer_that_the_viewport_hides_hide_the_model() {
    let mut ed = Editor::default();
    let class = ClassId(
        run(
            &mut ed,
            Command::AddClass {
                name: "Shed".into(),
                attrs: Attributes::default(),
            },
        )[0],
    );
    add_box_in(&mut ed, 0.0, 0.0, 1000.0, 500.0, Some(class));
    let (sheet, vp) = plan_viewport(&mut ed, 100.0);
    let n = |ed: &Editor| fills(&compose_page(ed.doc(), sheet).unwrap().prims, SURFACE_FRONT).len();
    assert_eq!(n(&ed), 1);
    set_viewport(&mut ed, vp, |v| {
        v.class_overrides.entry(class).or_default().visible = Some(false)
    });
    assert_eq!(n(&ed), 0);
    set_viewport(&mut ed, vp, |v| {
        v.class_overrides.clear();
        v.layers.clear();
    });
    assert_eq!(n(&ed), 0);
}

#[test]
fn an_instance_of_a_component_draws_the_definition_in_the_plan() {
    let mut ed = Editor::default();
    let b = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let out = run(
        &mut ed,
        Command::MakeComponent {
            name: "Block".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![b],
            replace: true,
        },
    );
    let inst = ObjectId(out[1]);
    // Move the instance 3000 mm to the right.
    run(
        &mut ed,
        Command::MoveObjects {
            ids: vec![inst],
            delta: v(3000.0, 0.0),
        },
    );
    let vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(3500.0, 500.0),
        100.0,
        vec![FIRST_LAYER],
    );
    let (sheet, _) = add_sheet(&mut ed, vp);
    let page = compose_page(ed.doc(), sheet).unwrap();
    let tops = fills(&page.prims, SURFACE_FRONT);
    assert_eq!(tops.len(), 1);
    let (lo, hi) = bounds(&points(tops[0]));
    assert!((lo - v(115.0, 65.0)).length() < 1e-6, "{lo}");
    assert!((hi - v(125.0, 75.0)).length() < 1e-6, "{hi}");
    assert_eq!(edge_segments(&page.prims), 8);
}

/// A section line across y = 500 from x = -1000 to x = 2000, looking toward +y.
fn section_line(ed: &mut Editor) -> ObjectId {
    let line = ObjectId(
        run(
            ed,
            Command::AddObject {
                shape: Shape::Line {
                    a: v(-1000.0, 500.0),
                    b: v(2000.0, 500.0),
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    );
    run(
        ed,
        Command::SetData {
            id: line,
            set: [(KEY.to_string(), json!({"name": "A", "look": "left"}))].into(),
            remove: vec![],
        },
    );
    line
}

fn section_sheet(ed: &mut Editor, line: ObjectId) -> (LayerId, Viewport) {
    let mut vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(1500.0, 500.0),
        20.0,
        vec![FIRST_LAYER],
    );
    vp.view = "section".into();
    vp.section = Some(SectionView::new(line));
    vp.snapshot_visibility(ed.doc());
    let (sheet, _) = add_sheet(ed, vp.clone());
    (sheet, vp)
}

fn drawing(ed: &Editor, vp: &Viewport, line: ObjectId) -> kadakas_x3d::section::SectionDrawing {
    kadakas_print::section::drawing(
        ed.doc(),
        vp,
        vp.section.as_ref().unwrap(),
        &kadakas_site::SiteCache::default(),
        1,
    )
    .unwrap_or_else(|| panic!("no drawing for {line}"))
}

#[test]
fn the_cut_of_a_closed_box_is_its_outline_with_the_right_area_and_a_poche() {
    let mut ed = Editor::default();
    add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let line = section_line(&mut ed);
    let (sheet, vp) = section_sheet(&mut ed, line);
    let d = drawing(&ed, &vp, line);
    // The plane y = 500 cuts the box 1000 mm wide and 500 mm high.
    assert_eq!(d.faces.len(), 1);
    assert_eq!(d.faces[0].rings.len(), 1);
    assert!((d.cut_area() - 500_000.0).abs() < 1.0, "{}", d.cut_area());
    let ring = &d.faces[0].rings[0];
    let (lo, hi) = ring.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(l, h), p| (l.min(*p), h.max(*p)),
    );
    // u counts from the start of the line (x = -1000).
    assert!((lo - v(1000.0, 0.0)).length() < 0.2, "{lo}");
    assert!((hi - v(2000.0, 500.0)).length() < 0.2, "{hi}");
    assert!(d.profiles.is_empty());
    // On the sheet the cut is filled (the poche) and outlined, 50 x 25 mm at 1:20.
    let page = compose_page(ed.doc(), sheet).unwrap();
    let poche: Vec<&PathPrim> = paths(&page.prims)
        .into_iter()
        .filter(|p| p.fill == Some([214, 214, 210, 255]))
        .collect();
    assert_eq!(poche.len(), 1);
    let (plo, phi) = bounds(&points(poche[0]));
    assert!((phi.x - plo.x - 50.0).abs() < 0.02 && (phi.y - plo.y - 25.0).abs() < 0.02);
}

#[test]
fn a_surface_that_is_not_closed_is_cut_to_a_line_and_not_filled() {
    let mut ed = Editor::default();
    // A box without a lid: five faces.
    let b = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let top = top_face(&ed, b);
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::EraseFaces { faces: vec![top] },
        },
    );
    let line = section_line(&mut ed);
    let (sheet, vp) = section_sheet(&mut ed, line);
    let d = drawing(&ed, &vp, line);
    assert!(d.faces.is_empty());
    assert!(d.cut_area().abs() < 1e-9);
    // The cut is one open line: the U of the two walls and the floor. The top is missing.
    assert_eq!(d.profiles.len(), 1);
    let line_points = &d.profiles[0].points;
    assert_eq!(line_points.len(), 4, "{line_points:?}");
    assert_ne!(line_points.first(), line_points.last(), "not closed");
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert!(
        paths(&page.prims)
            .iter()
            .all(|p| p.fill != Some([214, 214, 210, 255]))
    );
}

/// A section line across `y` from x = -1000 to `x1`, looking toward +y.
fn section_line_at(ed: &mut Editor, y: f64, x1: f64) -> ObjectId {
    let line = ObjectId(
        run(
            ed,
            Command::AddObject {
                shape: Shape::Line {
                    a: v(-1000.0, y),
                    b: v(x1, y),
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    );
    run(
        ed,
        Command::SetData {
            id: line,
            set: [(KEY.to_string(), json!({"name": "A", "look": "left"}))].into(),
            remove: vec![],
        },
    );
    line
}

#[test]
fn a_box_with_a_hole_through_it_is_cut_to_a_ring_with_a_hole_and_the_area_of_the_walls() {
    let mut ed = Editor::default();
    // A box of 2000 x 2000 x 1000 mm with a hole of 1000 x 500 mm through it along y.
    let b = add_box(&mut ed, 0.0, 0.0, 2000.0, 1000.0);
    let p = [
        [500.0, 0.0, 250.0],
        [1500.0, 0.0, 250.0],
        [1500.0, 0.0, 750.0],
        [500.0, 0.0, 750.0],
    ];
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::AddEdges {
                edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
            },
        },
    );
    // The small face on the front (y = 0), pushed through to the back.
    let inner = {
        let m = model(&ed, b);
        m.faces()
            .filter(|(_, f)| f.plane.normal.y < -0.9)
            .min_by(|a, c| {
                m.face_area(a.0)
                    .unwrap()
                    .total_cmp(&m.face_area(c.0).unwrap())
            })
            .map(|(f, _)| f)
            .unwrap()
    };
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::PushPull {
                face: inner,
                distance: -2000.0,
            },
        },
    );
    assert!(model(&ed, b).check().is_closed(), "still a closed solid");
    let line = section_line_at(&mut ed, 1000.0, 3000.0);
    let mut vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(1500.0, 500.0),
        20.0,
        vec![FIRST_LAYER],
    );
    vp.view = "section".into();
    vp.section = Some(SectionView::new(line));
    vp.snapshot_visibility(ed.doc());
    let (sheet, _) = add_sheet(&mut ed, vp.clone());
    let d = drawing(&ed, &vp, line);
    // One cut face with two rings: the outside 2000 x 1000 and the hole 1000 x 500.
    assert_eq!(d.faces.len(), 1);
    assert_eq!(d.faces[0].rings.len(), 2, "{:?}", d.faces[0].rings);
    assert!((d.cut_area() - 1_500_000.0).abs() < 1.0, "{}", d.cut_area());
    let areas: Vec<f64> = d.faces[0]
        .rings
        .iter()
        .map(|r| kadakas_core::geom::signed_area(r))
        .collect();
    assert!(areas.iter().any(|a| (a - 2_000_000.0).abs() < 1.0));
    assert!(
        areas.iter().any(|a| (a + 500_000.0).abs() < 1.0),
        "{areas:?}"
    );
    // The poche on the page has the hole: one path with two sub-paths.
    let page = compose_page(ed.doc(), sheet).unwrap();
    let poche: Vec<&PathPrim> = paths(&page.prims)
        .into_iter()
        .filter(|p| p.fill == Some([214, 214, 210, 255]))
        .collect();
    assert_eq!(poche.len(), 1);
    assert_eq!(
        poche[0]
            .segs
            .iter()
            .filter(|s| matches!(s, PSeg::Move(_)))
            .count(),
        2
    );
}

#[test]
fn a_box_without_a_wall_parallel_to_the_cut_is_cut_to_a_line_though_the_ring_closes() {
    let mut ed = Editor::default();
    let b = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let wall = model(&ed, b)
        .faces()
        .find(|(_, f)| f.plane.normal.y > 0.9)
        .map(|(f, _)| f)
        .unwrap();
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::EraseFaces { faces: vec![wall] },
        },
    );
    let line = section_line(&mut ed);
    let (_, vp) = section_sheet(&mut ed, line);
    let d = drawing(&ed, &vp, line);
    // The four sides of the cut close a ring, but the model has no inside.
    assert!(
        d.faces.is_empty() && d.cut_area().abs() < 1e-9,
        "{}",
        d.cut_area()
    );
    assert_eq!(d.profiles.len(), 1);
    let ring = &d.profiles[0].points;
    assert_eq!(ring.first(), ring.last(), "the line closes on itself");
}

#[test]
fn each_part_of_an_instance_is_cut_by_its_own_state() {
    let mut ed = Editor::default();
    let closed = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let open = add_box(&mut ed, 2000.0, 0.0, 1000.0, 500.0);
    let wall = model(&ed, open)
        .faces()
        .find(|(_, f)| f.plane.normal.y > 0.9)
        .map(|(f, _)| f)
        .unwrap();
    run(
        &mut ed,
        Command::EditSurface {
            id: open,
            edit: Edit::EraseFaces { faces: vec![wall] },
        },
    );
    let out = run(
        &mut ed,
        Command::MakeComponent {
            name: "Pair".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![closed, open],
            replace: true,
        },
    );
    let inst = ObjectId(out[1]);
    let line = section_line_at(&mut ed, 500.0, 4000.0);
    let (_, vp) = section_sheet(&mut ed, line);
    let d = drawing(&ed, &vp, line);
    // One instance: the closed box is filled, the other one is a line.
    assert_eq!(d.faces.len(), 1, "{:?}", d.faces);
    assert_eq!(d.faces[0].object, inst);
    assert!((d.cut_area() - 500_000.0).abs() < 1.0, "{}", d.cut_area());
    assert_eq!(d.profiles.len(), 1);
    assert_eq!(d.profiles[0].object, inst);
}

#[test]
fn soft_edges_are_left_out_of_the_hidden_line_and_the_section_beyond() {
    let mut ed = Editor::default();
    let b = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let (_, vpo) = add_sheet(
        &mut ed,
        Viewport::new(
            v(20.0, 20.0),
            v(220.0, 120.0),
            v(500.0, 500.0),
            20.0,
            vec![FIRST_LAYER],
        ),
    );
    set_viewport(&mut ed, vpo, |v| v.view = "right_isometric".into());
    let Shape::Viewport(vp) = ed.doc().object(vpo).unwrap().shape.clone() else {
        panic!()
    };
    let style = kadakas_core::present::RenderStyle {
        mode: kadakas_core::present::RenderMode::HiddenLine,
        ..Default::default()
    };
    let count = |ed: &Editor| {
        let mut out = Vec::new();
        hidden_line_prims(ed.doc(), &vp, &style, 10_000, &mut out);
        out.iter().filter(|p| matches!(p, Prim::Path(_))).count()
    };
    // A box in an isometric view: 9 of its 12 edges are visible.
    assert_eq!(count(&ed), 9);
    // Soften the vertical edge nearest to the viewer (right isometric looks at x = 1000, y = 0):
    // it is no crease of the mesh any more.
    let near = {
        let m = model(&ed, b);
        vertical_edges(&ed, b)
            .into_iter()
            .find(|e| {
                let p = m.vertex(m.edge(*e).unwrap().v[0]).unwrap();
                p.x == 1000.0 && p.y == 0.0
            })
            .unwrap()
    };
    run(
        &mut ed,
        Command::EditSurface {
            id: b,
            edit: Edit::SetEdgeStyle {
                edges: vec![near],
                soft: Some(true),
                smooth: None,
            },
        },
    );
    assert_eq!(count(&ed), 8);
}

#[test]
fn the_outputs_of_a_sheet_with_a_model_open_and_agree() {
    let mut ed = Editor::default();
    add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let (sheet, _) = plan_viewport(&mut ed, 100.0);
    let page = compose_page(ed.doc(), sheet).unwrap();
    let text = svg::page_svg(&page);
    assert!(text.contains("<path"));
    let png = kadakas_print::png::page_png(&page, 50.0).unwrap();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    // The same page twice gives the same display list.
    assert_eq!(page.prims, compose_page(ed.doc(), sheet).unwrap().prims);
}

#[test]
fn a_shaded_viewport_draws_the_model_and_the_sheet_background_stays_white() {
    let mut ed = Editor::default();
    let id = add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let brick = material(&mut ed, "Brick", [200, 40, 30, 255]);
    let top = top_face(&ed, id);
    run(
        &mut ed,
        Command::EditSurface {
            id,
            edit: Edit::SetFaceMaterial {
                faces: vec![top],
                side: PaintSide::Front,
                material: Some(brick.to_string()),
            },
        },
    );
    let mut vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(500.0, 500.0),
        10.0,
        vec![FIRST_LAYER],
    );
    vp.view = "top".into();
    vp.mode = "shaded".into();
    vp.dpi = 30.0;
    let (sheet, _) = add_sheet(&mut ed, vp);
    let page = compose_page(ed.doc(), sheet).unwrap();
    let img = page
        .prims
        .iter()
        .find_map(|p| {
            if let Prim::Image(i) = p {
                Some(i)
            } else {
                None
            }
        })
        .expect("a picture of the viewport");
    let px = |x: u32, y: u32| {
        let i = ((y * img.width + x) * 3) as usize;
        [img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]]
    };
    // The middle of the frame is the middle of the model: the top, red from its material.
    let mid = px(img.width / 2, img.height / 2);
    assert!(
        mid[0] > mid[1] + 60 && mid[0] > mid[2] + 60,
        "the top of the box is red: {mid:?}"
    );
    // A corner of the frame is white paper.
    assert_eq!(px(2, 2), [255, 255, 255]);
}

#[test]
fn an_elevation_shows_the_edges_of_the_model_and_cuts_nothing() {
    let mut ed = Editor::default();
    add_box(&mut ed, 0.0, 0.0, 1000.0, 500.0);
    let line = section_line(&mut ed);
    run(
        &mut ed,
        Command::SetData {
            id: line,
            set: [(
                KEY.to_string(),
                json!({"name": "E", "look": "right", "mode": "elevation"}),
            )]
            .into(),
            remove: vec![],
        },
    );
    // Looking to the right of the line (toward -y) from the line at y = 500 sees the front face
    // of the box (y = 0 side), from the inside out: the box is in front of the line.
    let (_, vp) = section_sheet(&mut ed, line);
    let d = drawing(&ed, &vp, line);
    assert!(d.faces.is_empty() && d.profiles.is_empty());
    assert!(d.edges.len() >= 4, "{}", d.edges.len());
    let (lo, hi) = d.bounds.unwrap();
    // Right of the line the u axis runs the other way: from the end (x = 2000).
    assert!((hi.y - 500.0).abs() < 0.5 && lo.y.abs() < 0.5, "{lo} {hi}");
    assert!(((hi.x - lo.x) - 1000.0).abs() < 0.5, "{lo} {hi}");
}

/// A box whose model is in the class "Parts", made a component. The instance is in the default
/// class, so only the part follows the class.
fn instance_with_part_class(ed: &mut Editor) -> (ClassId, ObjectId) {
    let part_class = ClassId(
        run(
            ed,
            Command::AddClass {
                name: "Parts".into(),
                attrs: Attributes::default(),
            },
        )[0],
    );
    let b = add_box_in(ed, 0.0, 0.0, 1000.0, 500.0, Some(part_class));
    let out = run(
        ed,
        Command::MakeComponent {
            name: "Block".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![b],
            replace: true,
        },
    );
    let inst = ObjectId(out[1]);
    let class = ed.doc().active_class;
    run(
        ed,
        Command::ModifyObject {
            id: inst,
            shape: None,
            layer: None,
            class: Some(class),
            attrs: None,
        },
    );
    (part_class, inst)
}

#[test]
fn a_viewport_that_hides_the_class_of_a_part_hides_the_part_in_the_plan_in_3d_and_in_a_section() {
    let mut ed = Editor::default();
    let (part_class, _) = instance_with_part_class(&mut ed);
    // The plan.
    let (sheet, vpo) = plan_viewport(&mut ed, 100.0);
    let tops =
        |ed: &Editor| fills(&compose_page(ed.doc(), sheet).unwrap().prims, SURFACE_FRONT).len();
    assert_eq!(tops(&ed), 1);
    set_viewport(&mut ed, vpo, |v| {
        v.class_overrides.entry(part_class).or_default().visible = Some(false)
    });
    assert_eq!(tops(&ed), 0, "the plan follows the viewport");
    // The class is still on in the document: another viewport shows it.
    assert!(ed.doc().class(part_class).unwrap().visible);
    set_viewport(&mut ed, vpo, |v| v.class_overrides.clear());
    assert_eq!(tops(&ed), 1);

    // A hidden-line viewport.
    let mut hl = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(500.0, 500.0),
        20.0,
        vec![FIRST_LAYER],
    );
    hl.view = "right_isometric".into();
    let style = kadakas_core::present::RenderStyle {
        mode: kadakas_core::present::RenderMode::HiddenLine,
        ..Default::default()
    };
    let lines = |vp: &Viewport| {
        let mut out = Vec::new();
        hidden_line_prims(ed.doc(), vp, &style, 10_000, &mut out);
        out.len()
    };
    assert_eq!(lines(&hl), 9);
    hl.class_overrides.entry(part_class).or_default().visible = Some(false);
    assert_eq!(lines(&hl), 0, "the 3D viewport follows the viewport");

    // A section.
    let line = section_line(&mut ed);
    let (_, mut vp) = section_sheet(&mut ed, line);
    assert!((drawing(&ed, &vp, line).cut_area() - 500_000.0).abs() < 1.0);
    vp.class_overrides.entry(part_class).or_default().visible = Some(false);
    let d = drawing(&ed, &vp, line);
    assert!(
        d.faces.is_empty() && d.edges.is_empty(),
        "the section follows the viewport"
    );
}

/// A flat grid of `n` x `n` squares of 10 mm cut in two, as a file has it.
fn flat_grid(n: usize) -> kadakas_core::surface::SurfaceModel {
    use kadakas_core::surface::{Edge, Face, FaceId, Plane, SurfaceModel, VertexId};
    use std::collections::{BTreeMap, HashSet};
    let id_of = |x: usize, y: usize| VertexId((y * (n + 1) + x) as u32 + 1);
    let mut vertices = BTreeMap::new();
    for y in 0..=n {
        for x in 0..=n {
            vertices.insert(
                id_of(x, y),
                glam::DVec3::new(x as f64 * 10.0, y as f64 * 10.0, 0.0),
            );
        }
    }
    let (mut edges, mut faces) = (BTreeMap::new(), BTreeMap::new());
    let (mut seen, mut e, mut f) = (HashSet::new(), 0u32, 0u32);
    let up = Plane::new(glam::DVec3::ZERO, glam::DVec3::Z).unwrap();
    for y in 0..n {
        for x in 0..n {
            let (a, b, c, d) = (
                id_of(x, y),
                id_of(x + 1, y),
                id_of(x + 1, y + 1),
                id_of(x, y + 1),
            );
            for tri in [[a, b, c], [a, c, d]] {
                for k in 0..3 {
                    let (p, q) = (tri[k], tri[(k + 1) % 3]);
                    if seen.insert((p.min(q), p.max(q))) {
                        e += 1;
                        edges.insert(
                            kadakas_core::surface::EdgeId(1_000_000 + e),
                            Edge { v: [p, q] },
                        );
                    }
                }
                f += 1;
                faces.insert(FaceId(2_000_000 + f), Face::new(up, tri.to_vec(), vec![]));
            }
        }
    }
    SurfaceModel::from_parts(vertices, edges, faces, 3_000_000)
}

fn add_shape(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        run(
            ed,
            Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    )
}

#[test]
fn a_big_model_in_a_plan_viewport_says_so_on_the_page_in_the_svg_and_the_pdf() {
    let mut ed = Editor::default();
    // 101 x 101 squares: 20,402 faces and 30,603 edges, over the limits of the plan.
    add_shape(&mut ed, Shape::SurfaceModel(Box::new(flat_grid(101))));
    let mut vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(505.0, 505.0),
        10.0,
        vec![FIRST_LAYER],
    );
    vp.name = "Plan".into();
    let (sheet, _) = add_sheet(&mut ed, vp);
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert_eq!(page.notes.len(), 1, "{:?}", page.notes);
    assert!(page.notes[0].contains("outline") && page.notes[0].contains("20402 faces"));
    // Only the border is drawn: 4 sides of 101 edges, no fill.
    assert_eq!(edge_segments(&page.prims), 4 * 101);
    let text = svg::page_svg(&page);
    assert!(
        text.contains("<desc>") && text.contains("drawn as its outline"),
        "{text}"
    );
    let mut cache = PageCache::default();
    let bytes = export_pdf(
        ed.doc(),
        ed.revision(),
        &mut cache,
        &select_sheets(ed.doc(), None).unwrap(),
    )
    .unwrap();
    let pdf = lopdf::Document::load_mem(&bytes).unwrap();
    let info = pdf.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let subject = pdf
        .get_dictionary(info)
        .unwrap()
        .get(b"Subject")
        .unwrap()
        .as_str()
        .unwrap();
    let subject = String::from_utf8_lossy(subject).to_string();
    assert!(
        subject.contains("Page 1:") && subject.contains("outline"),
        "{subject}"
    );
    // A page with nothing to say has no note.
    let mut small = Editor::default();
    add_box(&mut small, 0.0, 0.0, 1000.0, 500.0);
    let (s2, _) = plan_viewport(&mut small, 100.0);
    let page = compose_page(small.doc(), s2).unwrap();
    assert!(page.notes.is_empty());
    assert!(!svg::page_svg(&page).contains("<desc>"));
}

#[test]
fn a_hidden_line_viewport_and_a_section_say_when_they_leave_edges_out() {
    let mut ed = Editor::default();
    // 130 x 130 squares: 50,960 edges, over the 50,000 lines of one object.
    add_shape(&mut ed, Shape::SurfaceModel(Box::new(flat_grid(130))));
    let mut vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(650.0, 650.0),
        20.0,
        vec![FIRST_LAYER],
    );
    vp.view = "top".into();
    vp.mode = "hidden_line".into();
    vp.name = "Top".into();
    let (sheet, _) = add_sheet(&mut ed, vp);
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert!(
        page.notes
            .iter()
            .any(|n| n.contains("hidden-line limit") && n.contains("Viewport Top")),
        "{:?}",
        page.notes
    );
    let mut ed2 = Editor::default();
    add_shape(&mut ed2, Shape::SurfaceModel(Box::new(flat_grid(130))));
    // The whole grid is beyond the cut plane, in front of the grid.
    let line = section_line_at(&mut ed2, -10.0, 1400.0);
    let mut sv = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(500.0, 0.0),
        10.0,
        vec![FIRST_LAYER],
    );
    sv.view = "section".into();
    sv.name = "Cut".into();
    sv.section = Some(SectionView::new(line));
    sv.snapshot_visibility(ed2.doc());
    let (s2, _) = add_sheet(&mut ed2, sv);
    let page = compose_page(ed2.doc(), s2).unwrap();
    assert!(
        page.notes.iter().any(|n| n.contains("Viewport Cut")),
        "{:?}",
        page.notes
    );
}
