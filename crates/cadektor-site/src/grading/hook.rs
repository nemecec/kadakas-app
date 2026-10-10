//! The drawing hooks of the grading objects. `draw::svg_ext` and `draw::scene_ext` call these for
//! objects that `cadektor_core::site::carries_extra` accepts.

use glam::DVec2;

use cadektor_core::mesh::SceneSink;
use cadektor_core::model::{Document, Object, Shape};

use super::{analysis, grade, ground_of, host, section, spot, wall};
use crate::model::SiteCache;

fn scale_of(doc: &Document, obj: &Object) -> f64 {
    doc.layer(obj.layer).map_or(100.0, |l| l.scale)
}

/// Adds the SVG of the extras of one object (after its plain shape).
pub fn svg(doc: &Document, cache: &SiteCache, revision: u64, obj: &Object, out: &mut String) {
    let scale = scale_of(doc, obj);
    if let Some(d) = wall::data(obj) {
        let site = host(doc, cache, revision, obj, d.site);
        let ground = site.as_ref().map(|s| ground_of(s));
        if let Some(w) = wall::evaluate(doc, obj, ground.as_ref().map(|g| g as super::Ground<'_>)) {
            out.push_str(&wall::svg(&w, scale));
        }
    } else if let Some(d) = grade::data(obj) {
        let site = d.snap.and_then(|_| host(doc, cache, revision, obj, d.site));
        let ground = site.as_ref().map(|s| ground_of(s));
        if let Some((d, g)) = grade::evaluate(obj, ground.as_ref().map(|g| g as super::Ground<'_>)) {
            out.push_str(&grade::svg(obj.id, &d, &g, scale));
        }
    } else if let (Some(d), Shape::Circle { center, radius }) = (spot::data(obj), &obj.shape) {
        let text = match host(doc, cache, revision, obj, d.site) {
            Some(site) => spot::label(&d, &spot::read_at(&site, *center), *center, doc.georef.as_ref()),
            None => "no site model".to_string(),
        };
        out.push_str(&spot::svg(obj.id, *center, *radius, &d, &text, scale));
    } else if let (Some(d), Shape::Line { a, b }) = (section::data(obj), &obj.shape) {
        out.push_str(&section_svg(obj, &d.name, *a, *b, scale));
    } else if let (Some(d), Shape::Rect { min, max }) = (analysis::legend_of(obj), &obj.shape) {
        let site_obj = d.site.and_then(|id| doc.object(id)).or_else(|| {
            doc.objects.values().find(|o| o.layer == obj.layer && matches!(o.shape, Shape::SiteModel { .. }) && analysis::spec_of(o).is_some_and(|s| s.mode != analysis::Mode::Off))
        });
        let result = site_obj.and_then(|so| {
            let site = cache.get(revision, doc, so.id).ok()?;
            analysis::for_site(&site, &analysis::spec_of(so)?)
        });
        if let Some(a) = result {
            out.push_str(&analysis::legend_svg(obj.id, min.min(*max), min.max(*max), &d, &a));
        }
    }
}

/// The ends of a section line: a tick across the line and the name of the section.
fn section_svg(obj: &Object, name: &str, a: DVec2, b: DVec2, scale: f64) -> String {
    use std::fmt::Write;
    let len = a.distance(b);
    if len <= 0.0 {
        return String::new();
    }
    let dir = (b - a) / len;
    let n = DVec2::new(-dir.y, dir.x);
    let tick = 4.0 * scale;
    let mut out = String::new();
    for p in [a, b] {
        let (p0, p1) = (p - n * tick, p + n * tick);
        let _ = writeln!(out, r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="black" stroke-width="{}" data-kind="section-end" data-section="{}"/>"#, p0.x, -p0.y, p1.x, -p1.y, 0.5 * scale, obj.id);
        let t = p + n * tick * 1.4;
        let _ = writeln!(
            out,
            r#"<text x="{}" y="{}" font-size="{}" font-family="sans-serif" text-anchor="middle" fill="black" data-kind="section-name" data-section="{}">{}</text>"#,
            t.x,
            -t.y,
            3.5 * scale,
            obj.id,
            super::esc(name)
        );
    }
    out
}

/// Adds the 3D solid of a wall to a scene sink.
pub fn scene(doc: &Document, cache: &SiteCache, revision: u64, obj: &Object, sink: &mut SceneSink<'_>) {
    let Some(d) = wall::data(obj) else { return };
    let site = host(doc, cache, revision, obj, d.site);
    let ground = site.as_ref().map(|s| ground_of(s));
    if let Some(w) = wall::evaluate(doc, obj, ground.as_ref().map(|g| g as super::Ground<'_>)) {
        wall::emit(&w, sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grading::testutil::*;
    use crate::model::tests::grid_site;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::mesh;
    use cadektor_core::svg as core_svg;
    use serde_json::json;

    fn render(ed: &Editor) -> String {
        let cache = SiteCache::default();
        let ext = crate::draw::svg_ext(&cache, ed.revision());
        core_svg::render_with(ed.doc(), 800, Some(&ext))
    }

    #[test]
    fn every_extra_is_drawn_in_plan() {
        let mut ed = Editor::default();
        let sid = add_with(&mut ed, grid_site(11, 1000.0, |x, _| x / 10.0), analysis::KEY, json!({"mode": "slope", "surface": "existing"}));
        add_with(&mut ed, path(&[(1000.0, 1000.0), (6000.0, 1000.0)]), wall::KEY, json!({"top": {"mode": "terrain", "offset": 600.0}}));
        add_with(&mut ed, Shape::Line { a: DVec2::new(1000.0, 3000.0), b: DVec2::new(6000.0, 3000.0) }, grade::KEY, json!({"snap": "existing"}));
        add_with(&mut ed, Shape::Line { a: DVec2::new(1000.0, 4000.0), b: DVec2::new(6000.0, 4000.0) }, grade::KEY, json!({"elevation_a": 1000.0, "slope": {"percent": 2.0}}));
        add_with(&mut ed, Shape::Circle { center: DVec2::new(3000.0, 5000.0), radius: 100.0 }, spot::KEY, json!({}));
        add_with(&mut ed, Shape::Line { a: DVec2::new(0.0, 8000.0), b: DVec2::new(10_000.0, 8000.0) }, section::KEY, json!({"name": "B"}));
        add_with(&mut ed, Shape::Rect { min: DVec2::new(11_000.0, 0.0), max: DVec2::new(14_000.0, 6000.0) }, analysis::LEGEND_KEY, json!({"show_areas": true}));
        let svg = render(&ed);
        for kind in ["landscape_wall", "wall-tag", "grade-arrow", "grade-label", "grade-tag", "spot-marker", "spot-label", "section-end", "section-name", "legend-swatch", r#"data-kind="analysis""#] {
            assert!(svg.contains(kind), "missing {kind}");
        }
        assert!(svg.contains("TW 0.70"), "the top of the wall is 600 mm above the ground (100 at x = 1000)");
        assert!(svg.contains(">0.30<") || svg.contains("0.30"));
        assert!(svg.contains(">B<"));
        // Without the extension only the plain shapes are drawn.
        assert!(!core_svg::render(ed.doc(), 800).contains("wall-tag"));
        // A legend without an analysis, and a spot without a site model.
        let mut ed2 = Editor::default();
        add_with(&mut ed2, Shape::Rect { min: DVec2::ZERO, max: DVec2::new(100.0, 100.0) }, analysis::LEGEND_KEY, json!({}));
        add_with(&mut ed2, Shape::Circle { center: DVec2::ZERO, radius: 10.0 }, spot::KEY, json!({}));
        let s2 = render(&ed2);
        assert!(!s2.contains("legend-swatch") && s2.contains("no site model"));
        // A legend that names a site model.
        let site2 = add_with(&mut ed2, grid_site(4, 1000.0, |x, _| x / 5.0), analysis::KEY, json!({"mode": "elevation"}));
        add_with(&mut ed2, Shape::Rect { min: DVec2::ZERO, max: DVec2::new(100.0, 100.0) }, analysis::LEGEND_KEY, json!({"site": site2.0}));
        assert!(render(&ed2).contains("legend-swatch"));
        let _ = sid;
    }

    #[test]
    fn moved_end_points_change_the_grade_label() {
        let mut ed = Editor::default();
        let id = add_with(&mut ed, Shape::Line { a: DVec2::ZERO, b: DVec2::new(10_000.0, 0.0) }, grade::KEY, json!({"elevation_a": 0.0, "elevation_b": 500.0}));
        assert!(render(&ed).contains("5.0% (1:20)"));
        ed.execute(&Command::ModifyObject { id, shape: Some(Shape::Line { a: DVec2::ZERO, b: DVec2::new(20_000.0, 0.0) }), layer: None, class: None, attrs: None }).unwrap();
        assert!(render(&ed).contains("2.5% (1:40)"));
        // A zero-length section draws no ends.
        add_with(&mut ed, Shape::Line { a: DVec2::ZERO, b: DVec2::ZERO }, section::KEY, json!({}));
        assert!(!render(&ed).contains("section-end"));
    }

    #[test]
    fn walls_draw_in_3d_and_analysis_colors_the_terrain() {
        let mut ed = Editor::default();
        add(&mut ed, grid_site(11, 1000.0, |x, _| x / 10.0));
        let w = add_with(&mut ed, path(&[(1000.0, 1000.0), (6000.0, 1000.0)]), wall::KEY, json!({"top": {"mode": "terrain", "offset": 600.0}, "thickness": 300.0}));
        let cache = SiteCache::default();
        let ext = crate::draw::scene_ext(&cache, ed.revision());
        let scene = mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext));
        let (lo, hi) = scene.object_bounds(w).unwrap();
        let (lo, hi) = (scene.to_world(lo), scene.to_world(hi));
        assert!(lo.z < 100.5 && (hi.z - 600.0 - 600.0).abs() < 1.0, "{lo:?} {hi:?}");
        // A wall without a site model is drawn from 0.
        let mut ed2 = Editor::default();
        let w2 = add_with(&mut ed2, path(&[(0.0, 0.0), (3000.0, 0.0)]), wall::KEY, json!({"top": {"mode": "fixed", "elevation": 1000.0}}));
        let ext2 = crate::draw::scene_ext(&cache, ed2.revision());
        let scene2 = mesh::build_with(ed2.doc(), glam::DVec3::ZERO, Some(&ext2));
        let (lo, hi) = scene2.object_bounds(w2).unwrap();
        assert!((scene2.to_world(hi).z - 1000.0).abs() < 1e-3 && scene2.to_world(lo).z.abs() < 1e-3);
    }
}
