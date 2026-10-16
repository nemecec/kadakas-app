//! The drawing hooks of the grading objects. `draw::svg_ext` and `draw::scene_ext` call these for
//! objects that `kadakas_core::site::carries_extra` accepts. The canvas and the print module use
//! `extras` and `site_layers`, so every output shows the same graphics.

use glam::DVec2;

use kadakas_core::mesh::SceneSink;
use kadakas_core::model::{Document, Object, Shape};

use super::draw::{Drawn, Label, Poly};
use super::{SiteGround, analysis, grade, host, section, spot, wall};
use crate::model::{SiteCache, Surface};

fn scale_of(doc: &Document, obj: &Object) -> f64 {
    doc.layer(obj.layer).map_or(100.0, |l| l.scale)
}

/// What a wall, grade, spot elevation, section line or legend adds to a drawing, at the scale of its layer.
pub fn extras(doc: &Document, cache: &SiteCache, revision: u64, obj: &Object) -> Drawn {
    extras_at(doc, cache, revision, obj, scale_of(doc, obj))
}

/// Like `extras`, with the drawing scale given (model mm per paper mm), for a viewport at its own scale.
pub fn extras_at(
    doc: &Document,
    cache: &SiteCache,
    revision: u64,
    obj: &Object,
    scale: f64,
) -> Drawn {
    if let Some(d) = wall::data(obj) {
        let site = host(doc, cache, revision, obj.layer, d.site);
        let ground = site.as_ref().map(|s| SiteGround::new(s));
        let dz = site_dz(doc, site.as_deref(), obj);
        if let Some(w) = wall::evaluate_at(
            doc,
            obj,
            ground.as_ref().map(|g| g as super::Ground<'_>),
            dz,
        ) {
            return wall::draw(&w, scale);
        }
    } else if let Some(d) = grade::data(obj) {
        let site = d
            .snap
            .and_then(|_| host(doc, cache, revision, obj.layer, d.site));
        let ground = site.as_ref().map(|s| SiteGround::new(s));
        if let Some((d, g)) = grade::evaluate(obj, ground.as_ref().map(|g| g as super::Ground<'_>))
        {
            return grade::draw(obj.id, &d, &g, scale);
        }
    } else if let (Some(d), Shape::Circle { center, radius }) = (spot::data(obj), &obj.shape) {
        let text = match host(doc, cache, revision, obj.layer, d.site) {
            Some(site) => spot::label(
                &d,
                &spot::read_at(&site, *center),
                *center,
                doc.georef.as_ref(),
            ),
            None => "no site model".to_string(),
        };
        return spot::draw(obj.id, *center, *radius, &d, &text, scale);
    } else if let (Some(d), Shape::Line { a, b }) = (section::data(obj), &obj.shape) {
        return section_ends(obj, &d.name, *a, *b, scale);
    } else if let (Some(d), Shape::Rect { min, max }) = (analysis::legend_of(obj), &obj.shape) {
        let site_obj = d.site.and_then(|id| doc.object(id)).or_else(|| {
            doc.objects.values().find(|o| {
                o.layer == obj.layer
                    && matches!(o.shape, Shape::SiteModel { .. })
                    && analysis::spec_of(o).is_some_and(|s| s.mode != analysis::Mode::Off)
            })
        });
        let result = site_obj.and_then(|so| {
            let site = cache.get(revision, doc, so.id).ok()?;
            analysis::for_site(cache, &site, &analysis::spec_of(so)?)
        });
        if let Some(a) = result {
            return analysis::legend_draw(obj.id, min.min(*max), min.max(*max), &d, &a);
        }
    }
    Drawn::default()
}

/// The height of the layer of `obj` above the layer of the site model.
fn site_dz(doc: &Document, site: Option<&crate::SiteResult>, obj: &Object) -> f64 {
    let elev = |layer| doc.layer(layer).map_or(0.0, |l| l.elevation);
    site.and_then(|s| doc.object(s.id))
        .map_or(0.0, |so| elev(obj.layer) - elev(so.layer))
}

/// What a site model adds to a drawing besides its outline: the analysis colors and the contours.
/// The SVG of the plan draws the contours itself, so it asks only for the analysis (`contours` false).
pub fn site_layers(
    doc: &Document,
    cache: &SiteCache,
    revision: u64,
    obj: &Object,
    scale: f64,
    contours: bool,
) -> Drawn {
    let mut out = Drawn::default();
    let Ok(site) = cache.get(revision, doc, obj.id) else {
        return out;
    };
    if let Some(a) = analysis::spec_of(obj).and_then(|s| analysis::for_site(cache, &site, &s)) {
        out.extend(analysis::draw(obj.id, &a));
    }
    if contours {
        for which in crate::draw::shown_surfaces(&site, site.settings.display_2d) {
            let color = match which {
                Surface::Existing => site
                    .settings
                    .existing_color
                    .map_or([150, 104, 60, 255], |c| c.0),
                Surface::Proposed => site
                    .settings
                    .proposed_color
                    .map_or([30, 120, 50, 255], |c| c.0),
            };
            for c in site.contours(which).unwrap_or(&[]) {
                let weight = if c.major { 0.5 } else { 0.18 } * scale;
                out.polys.push(
                    Poly::new("contour", obj.id, vec![c.points.clone()], c.closed)
                        .stroked(color, weight),
                );
            }
        }
    }
    out
}

/// The ends of a section line: a tick across the line and the name of the section.
fn section_ends(obj: &Object, name: &str, a: DVec2, b: DVec2, scale: f64) -> Drawn {
    let mut out = Drawn::default();
    let len = a.distance(b);
    if len <= 0.0 {
        return out;
    }
    let dir = (b - a) / len;
    let n = DVec2::new(-dir.y, dir.x);
    let tick = 4.0 * scale;
    for p in [a, b] {
        out.polys.push(
            Poly::new(
                "section-end",
                obj.id,
                vec![vec![p - n * tick, p + n * tick]],
                false,
            )
            .stroked([0, 0, 0, 255], 0.5 * scale),
        );
        let mut label = Label::new(
            "section-name",
            obj.id,
            p + n * tick * 1.4,
            name.to_string(),
            3.5 * scale,
        );
        label.center = true;
        out.labels.push(label);
    }
    out
}

/// Adds the SVG of the extras of one object (after its plain shape).
pub fn svg(doc: &Document, cache: &SiteCache, revision: u64, obj: &Object, out: &mut String) {
    out.push_str(&extras(doc, cache, revision, obj).svg());
}

/// A wall object computed on the site model of its layer (or on flat ground without one). `None`
/// for an object that is not a wall. The 3D scene and the IFC export use it, so they agree.
pub fn wall_of(
    doc: &Document,
    cache: &SiteCache,
    revision: u64,
    obj: &Object,
) -> Option<wall::Wall> {
    let d = wall::data(obj)?;
    let site = host(doc, cache, revision, obj.layer, d.site);
    let ground = site.as_ref().map(|s| SiteGround::new(s));
    let dz = site_dz(doc, site.as_deref(), obj);
    wall::evaluate_at(
        doc,
        obj,
        ground.as_ref().map(|g| g as super::Ground<'_>),
        dz,
    )
}

/// Adds the 3D solid of a wall to a scene sink.
pub fn scene(
    doc: &Document,
    cache: &SiteCache,
    revision: u64,
    obj: &Object,
    sink: &mut SceneSink<'_>,
) {
    if let Some(w) = wall_of(doc, cache, revision, obj) {
        wall::emit(&w, sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SiteResult;
    use crate::grading::testutil::*;
    use crate::model::tests::grid_site;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::mesh;
    use kadakas_core::svg as core_svg;
    use serde_json::json;

    fn render(ed: &Editor) -> String {
        let cache = SiteCache::default();
        let ext = crate::draw::svg_ext(&cache, ed.revision());
        core_svg::render_with(ed.doc(), 800, Some(&ext))
    }

    #[test]
    fn every_extra_is_drawn_in_plan() {
        let mut ed = Editor::default();
        let sid = add_with(
            &mut ed,
            grid_site(11, 1000.0, |x, _| x / 10.0),
            analysis::KEY,
            json!({"mode": "slope", "surface": "existing"}),
        );
        add_with(
            &mut ed,
            path(&[(1000.0, 1000.0), (6000.0, 1000.0)]),
            wall::KEY,
            json!({"top": {"mode": "terrain", "offset": 600.0}}),
        );
        add_with(
            &mut ed,
            Shape::Line {
                a: DVec2::new(1000.0, 3000.0),
                b: DVec2::new(6000.0, 3000.0),
            },
            grade::KEY,
            json!({"snap": "existing"}),
        );
        add_with(
            &mut ed,
            Shape::Line {
                a: DVec2::new(1000.0, 4000.0),
                b: DVec2::new(6000.0, 4000.0),
            },
            grade::KEY,
            json!({"elevation_a": 1000.0, "slope": {"percent": 2.0}}),
        );
        add_with(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(3000.0, 5000.0),
                radius: 100.0,
            },
            spot::KEY,
            json!({}),
        );
        add_with(
            &mut ed,
            Shape::Line {
                a: DVec2::new(0.0, 8000.0),
                b: DVec2::new(10_000.0, 8000.0),
            },
            section::KEY,
            json!({"name": "B"}),
        );
        add_with(
            &mut ed,
            Shape::Rect {
                min: DVec2::new(11_000.0, 0.0),
                max: DVec2::new(14_000.0, 6000.0),
            },
            analysis::LEGEND_KEY,
            json!({"show_areas": true}),
        );
        let svg = render(&ed);
        for kind in [
            "landscape_wall",
            "wall-tag",
            "grade-arrow",
            "grade-label",
            "grade-tag",
            "spot-marker",
            "spot-label",
            "section-end",
            "section-name",
            "legend-swatch",
            r#"data-kind="analysis""#,
        ] {
            assert!(svg.contains(kind), "missing {kind}");
        }
        assert!(
            svg.contains("TW 0.70"),
            "the top of the wall is 600 mm above the ground (100 at x = 1000)"
        );
        assert!(svg.contains(">0.30<") || svg.contains("0.30"));
        assert!(svg.contains(">B<"));
        // Without the extension only the plain shapes are drawn.
        assert!(!core_svg::render(ed.doc(), 800).contains("wall-tag"));
        // A legend without an analysis, and a spot without a site model.
        let mut ed2 = Editor::default();
        add_with(
            &mut ed2,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(100.0, 100.0),
            },
            analysis::LEGEND_KEY,
            json!({}),
        );
        add_with(
            &mut ed2,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 10.0,
            },
            spot::KEY,
            json!({}),
        );
        let s2 = render(&ed2);
        assert!(!s2.contains("legend-swatch") && s2.contains("no site model"));
        // A legend that names a site model.
        let site2 = add_with(
            &mut ed2,
            grid_site(4, 1000.0, |x, _| x / 5.0),
            analysis::KEY,
            json!({"mode": "elevation"}),
        );
        add_with(
            &mut ed2,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(100.0, 100.0),
            },
            analysis::LEGEND_KEY,
            json!({"site": site2.0}),
        );
        assert!(render(&ed2).contains("legend-swatch"));
        let _ = sid;
    }

    #[test]
    fn analysis_colors_the_3d_terrain() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(6, 1000.0, |x, _| x / 10.0));
        let cache = SiteCache::default();
        let colors = |ed: &Editor| {
            let ext = crate::draw::scene_ext(&cache, ed.revision());
            mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext))
                .mesh
                .colors
                .clone()
        };
        let plain = colors(&ed);
        assert!(plain.windows(2).all(|w| w[0] == w[1]));
        ed.execute(&Command::SetData { id, set: [(analysis::KEY.to_string(), json!({"mode": "slope", "breaks": [5.0], "colors": [[1, 2, 3, 255], [9, 8, 7, 255]]}))].into(), remove: vec![] }).unwrap();
        let colored = colors(&ed);
        assert!(
            colored.contains(&[9, 8, 7, 255]) && !colored.contains(&[1, 2, 3, 255]),
            "10 percent is above the limit of 5"
        );
        // Showing contours only still shows the analysis colors.
        let site = SiteResult::compute(ed.doc(), id).unwrap();
        let a = analysis::for_site(
            &SiteCache::default(),
            &site,
            &analysis::spec_of(ed.doc().object(id).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(a.bands.len(), 2);
        let _ = a;
    }

    #[test]
    fn moved_end_points_change_the_grade_label() {
        let mut ed = Editor::default();
        let id = add_with(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(10_000.0, 0.0),
            },
            grade::KEY,
            json!({"elevation_a": 0.0, "elevation_b": 500.0}),
        );
        assert!(render(&ed).contains("5.0% (1:20)"));
        ed.execute(&Command::ModifyObject {
            id,
            shape: Some(Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(20_000.0, 0.0),
            }),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        assert!(render(&ed).contains("2.5% (1:40)"));
        // A zero-length section draws no ends.
        add_with(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::ZERO,
            },
            section::KEY,
            json!({}),
        );
        assert!(!render(&ed).contains("section-end"));
    }

    #[test]
    fn walls_draw_in_3d_and_analysis_colors_the_terrain() {
        let mut ed = Editor::default();
        add(&mut ed, grid_site(11, 1000.0, |x, _| x / 10.0));
        let w = add_with(
            &mut ed,
            path(&[(1000.0, 1000.0), (6000.0, 1000.0)]),
            wall::KEY,
            json!({"top": {"mode": "terrain", "offset": 600.0}, "thickness": 300.0}),
        );
        let cache = SiteCache::default();
        let ext = crate::draw::scene_ext(&cache, ed.revision());
        let scene = mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext));
        let (lo, hi) = scene.object_bounds(w).unwrap();
        let (lo, hi) = (scene.to_world(lo), scene.to_world(hi));
        assert!(
            lo.z < 100.5 && (hi.z - 600.0 - 600.0).abs() < 1.0,
            "{lo:?} {hi:?}"
        );
        // A wall without a site model is drawn from 0.
        let mut ed2 = Editor::default();
        let w2 = add_with(
            &mut ed2,
            path(&[(0.0, 0.0), (3000.0, 0.0)]),
            wall::KEY,
            json!({"top": {"mode": "fixed", "elevation": 1000.0}}),
        );
        let ext2 = crate::draw::scene_ext(&cache, ed2.revision());
        let scene2 = mesh::build_with(ed2.doc(), glam::DVec3::ZERO, Some(&ext2));
        let (lo, hi) = scene2.object_bounds(w2).unwrap();
        assert!(
            (scene2.to_world(hi).z - 1000.0).abs() < 1e-3 && scene2.to_world(lo).z.abs() < 1e-3
        );
    }
}
