//! Measurements of the drawing of a surface model at the limits of the model (MOD-13): 100,000
//! faces in the plan, the 3D scene, a hidden-line viewport, a section, the sheet outputs and Object
//! Info. They are not run by `cargo test`. The numbers are in `docs/modeler-limits.md`.
//!
//! `cargo test --release -p kadakas-print --test surface_perf -- --ignored --nocapture --test-threads=1`

use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::*;
use kadakas_core::section::{KEY, SectionView};
use kadakas_core::sheets::Viewport;
use kadakas_core::surface::plan::{self, Limits, Part};
use kadakas_core::surface::{Edge, EdgeId, Face, FaceId, Plane, SurfaceModel, VertexId};
use kadakas_print::{PageCache, compose_page, export_pdf, svg};
use serde_json::json;

/// Builds a closed or open triangle mesh as a surface model. The fronts look away from `inside`
/// (a point inside the solid) or up.
fn mesh_model(points: &[DVec3], tris: &[[usize; 3]], inside: Option<DVec3>) -> SurfaceModel {
    let vertices: BTreeMap<VertexId, DVec3> = points
        .iter()
        .enumerate()
        .map(|(i, p)| (VertexId(i as u32 + 1), *p))
        .collect();
    let ebase = points.len() as u32;
    let mut edges = BTreeMap::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut faces = BTreeMap::new();
    let mut next_edge = ebase + 1;
    let fbase = ebase + (tris.len() as u32) * 2;
    for (i, t) in tris.iter().enumerate() {
        let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
        let n = (b - a).cross(c - a);
        let out = match inside {
            Some(o) => n.dot((a + b + c) / 3.0 - o) >= 0.0,
            None => n.z >= 0.0,
        };
        let t = if out { *t } else { [t[0], t[2], t[1]] };
        for k in 0..3 {
            let (p, q) = (t[k], t[(k + 1) % 3]);
            let key = (p.min(q), p.max(q));
            if seen.insert(key) {
                edges.insert(
                    EdgeId(next_edge),
                    Edge {
                        v: [VertexId(p as u32 + 1), VertexId(q as u32 + 1)],
                    },
                );
                next_edge += 1;
            }
        }
        let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
        let plane = Plane::new(a, (b - a).cross(c - a)).unwrap();
        faces.insert(
            FaceId(fbase + i as u32 + 1),
            Face::new(
                plane,
                t.iter().map(|k| VertexId(*k as u32 + 1)).collect(),
                vec![],
            ),
        );
    }
    SurfaceModel::from_parts(vertices, edges, faces, fbase + tris.len() as u32 + 10)
}

/// A bumpy terrain of `n` x `n` squares of 100 mm, each cut in two triangles.
fn terrain(n: usize) -> SurfaceModel {
    let mut points = Vec::new();
    for y in 0..=n {
        for x in 0..=n {
            let (fx, fy) = (x as f64, y as f64);
            let z =
                300.0 * (fx / 9.0).sin() * (fy / 7.0).cos() + 40.0 * ((fx * 13.0 + fy * 7.0).sin());
            points.push(DVec3::new(fx * 100.0, fy * 100.0, z));
        }
    }
    let id = |x: usize, y: usize| y * (n + 1) + x;
    let mut tris = Vec::new();
    for y in 0..n {
        for x in 0..n {
            tris.push([id(x, y), id(x + 1, y), id(x + 1, y + 1)]);
            tris.push([id(x, y), id(x + 1, y + 1), id(x, y + 1)]);
        }
    }
    mesh_model(&points, &tris, None)
}

/// A closed ball of radius 5000 mm with `slices` meridians and `rings` parallels.
fn sphere(slices: usize, rings: usize) -> SurfaceModel {
    let r = 5000.0;
    let mut points = vec![DVec3::new(0.0, 0.0, r), DVec3::new(0.0, 0.0, -r)];
    for i in 1..rings {
        let t = std::f64::consts::PI * i as f64 / rings as f64;
        for k in 0..slices {
            let a = std::f64::consts::TAU * k as f64 / slices as f64;
            points.push(DVec3::new(
                r * t.sin() * a.cos(),
                r * t.sin() * a.sin(),
                r * t.cos(),
            ));
        }
    }
    let ring = |i: usize, k: usize| 2 + (i - 1) * slices + (k % slices);
    let mut tris = Vec::new();
    for k in 0..slices {
        tris.push([0, ring(1, k), ring(1, k + 1)]);
        tris.push([1, ring(rings - 1, k + 1), ring(rings - 1, k)]);
    }
    for i in 1..rings - 1 {
        for k in 0..slices {
            tris.push([ring(i, k), ring(i + 1, k), ring(i + 1, k + 1)]);
            tris.push([ring(i, k), ring(i + 1, k + 1), ring(i, k + 1)]);
        }
    }
    mesh_model(&points, &tris, Some(DVec3::ZERO))
}

fn add(ed: &mut Editor, m: SurfaceModel) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(m)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn time<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("{what:<58} {:>9.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    r
}

fn sheet(ed: &mut Editor, vp: Viewport) -> LayerId {
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "S".into(),
            paper: None,
            orientation: None,
            number: Some("1".into()),
            title: None,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::AddObject {
        shape: Shape::Viewport(Box::new(vp)),
        layer: Some(sheet),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    sheet
}

fn viewport(center: DVec2, scale: f64, view: &str, mode: &str) -> Viewport {
    let mut vp = Viewport::new(
        DVec2::new(10.0, 10.0),
        DVec2::new(410.0, 287.0),
        center,
        scale,
        vec![FIRST_LAYER],
    );
    vp.view = view.into();
    vp.mode = mode.into();
    vp
}

fn report(name: &str, m: SurfaceModel, center: DVec2, scale: f64, cut_at: f64) {
    println!(
        "\n== {name}: {} vertices, {} edges, {} faces ==",
        m.vertex_count(),
        m.edge_count(),
        m.face_count()
    );
    let mut ed = Editor::default();
    let id = time("add the object (check of the model)", || add(&mut ed, m));
    let doc = ed.doc().clone();
    let obj = doc.object(id).unwrap();
    let Shape::SurfaceModel(model) = &obj.shape else {
        panic!()
    };

    // The plan: the parts to draw, the drawing within the limits, and the full drawing.
    let part = [Part {
        model,
        xf: model.transform,
        fill: None,
    }];
    let d = time("plan: outline (the limits of the plan)", || {
        plan::drawing(&part, None)
    });
    println!(
        "   -> {:?}, {} faces, {} edges drawn",
        d.detail,
        d.faces(),
        d.edges()
    );
    for (faces, label) in [
        (10_000, "10k"),
        (20_000, "20k"),
        (50_000, "50k"),
        (usize::MAX, "all"),
    ] {
        if faces > model.face_count() && faces != usize::MAX {
            continue;
        }
        let lim = Limits {
            faces,
            edges: usize::MAX,
            outline: usize::MAX,
        };
        // The full drawing of the first `faces` faces: a model of that size.
        let sub = if faces == usize::MAX || faces >= model.face_count() {
            None
        } else {
            Some(faces)
        };
        let _ = sub;
        let d = time(&format!("plan: full drawing, limit {label} faces"), || {
            plan::drawing_with(&part, None, lim)
        });
        println!(
            "   -> {:?}, {} faces, {} edges drawn",
            d.detail,
            d.faces(),
            d.edges()
        );
    }
    time("plan: hit test (a click)", || {
        plan::hit(&part, center, 10.0)
    });
    time("plan: snap points near the pointer", || {
        plan::snap_geometry(&part, Some((center, 400.0)), 20_000)
    });
    time("info: counts only (Check::Never)", || {
        kadakas_core::surface::info::of_object(&doc, obj, kadakas_core::surface::info::Check::Never)
    });
    let i = time("info: with the closed-solid check (Check::Always)", || {
        kadakas_core::surface::info::of_object(
            &doc,
            obj,
            kadakas_core::surface::info::Check::Always,
        )
    });
    println!("   -> {:?}", i.map(|i| i.solid));

    // 3D scene.
    let scene = time("3D scene: mesh::build", || kadakas_core::mesh::build(&doc));
    println!(
        "   -> {} triangles, {} line segments, {} soft",
        scene.mesh.triangle_count(),
        scene.lines.segment_count(),
        scene.soft_edges.len()
    );

    // Sheets.
    let mut ed2 = ed;
    let plan_sheet = sheet(&mut ed2, viewport(center, scale, "plan", "shaded"));
    let page = time("sheet: plan viewport (compose_page)", || {
        compose_page(ed2.doc(), plan_sheet).unwrap()
    });
    println!("   -> {} primitives", page.prims.len());
    time("sheet: plan viewport as SVG", || svg::page_svg(&page).len());
    let mut cache = PageCache::default();
    let bytes = time("sheet: plan viewport as PDF", || {
        export_pdf(ed2.doc(), ed2.revision(), &mut cache, &[plan_sheet]).unwrap()
    });
    println!("   -> {} KB", bytes.len() / 1024);

    for (view, label) in [
        ("top", "top"),
        ("right_isometric", "isometric"),
        ("front", "front"),
    ] {
        let mut ed3 = Editor::default();
        add(
            &mut ed3,
            doc.object(id)
                .map(|o| match &o.shape {
                    Shape::SurfaceModel(m) => (**m).clone(),
                    _ => unreachable!(),
                })
                .unwrap(),
        );
        let s = sheet(&mut ed3, viewport(center, scale, view, "hidden_line"));
        let page = time(&format!("sheet: hidden-line viewport, {label}"), || {
            compose_page(ed3.doc(), s).unwrap()
        });
        println!("   -> {} primitives", page.prims.len());
    }

    // Section across the model.
    let mut ed4 = Editor::default();
    add(
        &mut ed4,
        match &doc.object(id).unwrap().shape {
            Shape::SurfaceModel(m) => (**m).clone(),
            _ => unreachable!(),
        },
    );
    let line = ObjectId(
        ed4.execute(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::new(-1000.0, cut_at),
                b: DVec2::new(center.x * 2.0 + 1000.0, cut_at),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed4.execute(&Command::SetData {
        id: line,
        set: [(KEY.to_string(), json!({"name": "A", "look": "left"}))].into(),
        remove: vec![],
    })
    .unwrap();
    let mut vp = viewport(DVec2::new(center.x, 0.0), scale, "section", "shaded");
    vp.section = Some(SectionView::new(line));
    vp.snapshot_visibility(ed4.doc());
    let d = time("section: drawing (cut and the edges beyond)", || {
        kadakas_print::section::drawing(
            ed4.doc(),
            &vp,
            vp.section.as_ref().unwrap(),
            &kadakas_site::SiteCache::default(),
            1,
        )
        .unwrap()
    });
    println!(
        "   -> {} cut faces, {} profiles, {} edges beyond, cut area {:.0} mm2, truncated {}",
        d.faces.len(),
        d.profiles.len(),
        d.edges.len(),
        d.cut_area(),
        d.truncated
    );
    let s = sheet(&mut ed4, vp);
    time("sheet: section viewport (compose_page)", || {
        compose_page(ed4.doc(), s).unwrap()
    });
}

#[test]
#[ignore = "a measurement"]
fn terrain_of_100k_faces() {
    let m = time("build the terrain", || terrain(223));
    report(
        "terrain",
        m,
        DVec2::new(11_150.0, 11_150.0),
        100.0,
        11_150.0,
    );
}

#[test]
#[ignore = "a measurement"]
fn ball_of_100k_faces() {
    let m = time("build the ball", || sphere(160, 313));
    report("ball", m, DVec2::new(0.0, 0.0), 40.0, 0.0);
}

#[test]
#[ignore = "a measurement"]
fn terrain_of_20k_faces() {
    let m = time("build the terrain", || terrain(100));
    report("terrain", m, DVec2::new(5_000.0, 5_000.0), 30.0, 5_000.0);
}

/// Where the time of a hidden-line pass goes: the line list (the edges of the model), the edges
/// that the mesh adds (silhouettes and creases) and the triangles that cover.
fn breakdown(name: &str, m: SurfaceModel, view: kadakas_render::views::StandardView) {
    use kadakas_render::camera::Camera;
    use kadakas_render::hlr::{HlOptions, hidden_lines};
    use kadakas_render::views::Projection;
    println!("\n== hidden-line breakdown: {name}, {view:?} ==");
    let mut ed = Editor::default();
    add(&mut ed, m);
    let scene = kadakas_core::mesh::build(ed.doc());
    let (lo, hi) = scene.bounds.unwrap();
    let size = (hi - lo).max_element();
    let mut cam = Camera::standard(view, Projection::Orthographic);
    cam.target = (lo + hi) / 2.0;
    cam.distance = size / 2.0 / (kadakas_render::camera::FOV_Y / 2.0).tan() * 1.1;
    let (w, h) = (8000u32, 5600u32);
    let run = |what: &str, scene: &kadakas_core::mesh::Scene, opts: HlOptions| {
        let hl = time(what, || hidden_lines(scene, &cam, w, h, &opts));
        println!("   -> {} visible segments", hl.visible_edge_count());
    };
    run(
        "all: lines + mesh edges + covers",
        &scene,
        HlOptions {
            include_hidden: false,
            mesh_edges: true,
        },
    );
    run(
        "lines only (mesh_edges false)",
        &scene,
        HlOptions {
            include_hidden: false,
            mesh_edges: false,
        },
    );
    let mut no_lines = scene.clone();
    no_lines.lines = Default::default();
    run(
        "mesh edges only (no line list)",
        &no_lines,
        HlOptions {
            include_hidden: false,
            mesh_edges: true,
        },
    );
}

#[test]
#[ignore = "a measurement"]
fn hidden_line_breakdown() {
    use kadakas_render::views::StandardView;
    breakdown("terrain 100k", terrain(223), StandardView::Front);
    breakdown("terrain 100k", terrain(223), StandardView::Top);
    breakdown("ball 100k", sphere(160, 313), StandardView::Front);
}
