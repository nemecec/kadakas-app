//! Drawing site models: SVG for 2D and mesh and lines for 3D. Core calls these through its
//! extension hooks, so core does not depend on this crate.

use std::fmt::Write;

use glam::DVec3;

use kadakas_core::ground::GroundSource;
use kadakas_core::mesh::SceneSink;
use kadakas_core::model::{Color, Document, Object, Shape};
use kadakas_core::site::{Display3d, SurfaceDisplay};

use crate::contour::Contour;
use crate::grading::{self, analysis::AnalysisResult};
use crate::model::{SiteCache, SiteResult, Surface};

/// Default colors: existing ground is tan, proposed ground is green.
const EXISTING_MESH: [u8; 4] = [176, 150, 112, 255];
const PROPOSED_MESH: [u8; 4] = [120, 168, 100, 255];
const EXISTING_LINE: Color = Color([150, 104, 60, 255]);
const PROPOSED_LINE: Color = Color([30, 120, 50, 255]);

/// The surfaces that a display choice shows. The proposed surface is left out when it equals
/// the existing one.
pub fn shown_surfaces(site: &SiteResult, display: SurfaceDisplay) -> Vec<Surface> {
    let mut v = Vec::new();
    if matches!(display, SurfaceDisplay::Existing | SurfaceDisplay::Both)
        || (site.proposed.is_none() && display == SurfaceDisplay::Proposed)
    {
        v.push(Surface::Existing);
    }
    if site.proposed.is_some() && matches!(display, SurfaceDisplay::Proposed | SurfaceDisplay::Both)
    {
        v.push(Surface::Proposed);
    }
    v
}

fn line_color(site: &SiteResult, which: Surface) -> Color {
    match which {
        Surface::Existing => site.settings.existing_color.unwrap_or(EXISTING_LINE),
        Surface::Proposed => site.settings.proposed_color.unwrap_or(PROPOSED_LINE),
    }
}

fn mesh_color(site: &SiteResult, which: Surface) -> [u8; 4] {
    match which {
        Surface::Existing => site.settings.existing_color.map_or(EXISTING_MESH, |c| c.0),
        Surface::Proposed => site.settings.proposed_color.map_or(PROPOSED_MESH, |c| c.0),
    }
}

/// Adds the 3D geometry of a site model: a terrain mesh, contour lines, or both.
pub fn draw_3d(site: &SiteResult, sink: &mut SceneSink<'_>) {
    draw_3d_with(site, None, sink);
}

/// Like `draw_3d`. With an analysis, the mesh of the analyzed surface has the colors of its bands.
pub fn draw_3d_with(
    site: &SiteResult,
    analysis: Option<&AnalysisResult>,
    sink: &mut SceneSink<'_>,
) {
    let display = site.settings.display_3d;
    let colored = analysis.map(|a| {
        if site.proposed.is_none() {
            Surface::Existing
        } else {
            a.surface
        }
    });
    for which in shown_surfaces(site, site.settings.surface_3d) {
        if let (Some(a), true) = (analysis, colored == Some(which)) {
            let tin = site.surface(which);
            for band in a.bands.iter().filter(|b| b.triangles > 0) {
                sink.mesh(
                    &tin.vertices,
                    &grading::analysis::band_indices(tin, a, band.index),
                    band.color,
                );
            }
        } else if matches!(display, Display3d::Mesh | Display3d::Both) {
            let tin = site.surface(which);
            let mut idx: Vec<u32> = tin.triangles.iter().flatten().copied().collect();
            // Vertical pad edges are drawn as walls.
            let mut vertices = tin.vertices.clone();
            for q in tin.walls() {
                let n = vertices.len() as u32;
                vertices.extend(q);
                idx.extend([n, n + 1, n + 2, n, n + 2, n + 3]);
            }
            sink.mesh(&vertices, &idx, mesh_color(site, which));
        }
        if matches!(display, Display3d::Contours | Display3d::Both) {
            let color = line_color(site, which).0;
            for c in site.contours(which).unwrap_or(&[]) {
                let pts: Vec<DVec3> = c.points.iter().map(|p| p.extend(c.level)).collect();
                sink.polyline(&pts, c.closed, color);
            }
        }
    }
}

/// Draws the whole 3D part of one object, if it is a site model. For `mesh::build_with`.
pub fn scene_ext<'a>(
    cache: &'a SiteCache,
    revision: u64,
) -> impl Fn(&Document, &Object, &mut SceneSink<'_>) + 'a {
    move |doc, obj, sink| {
        if kadakas_core::site::carries_extra(obj) {
            grading::hook::scene(doc, cache, revision, obj, sink);
        } else if let Shape::SiteModel { .. } = obj.shape {
            if let Ok(site) = cache.get(revision, doc, obj.id) {
                let analysis = grading::analysis::spec_of(obj)
                    .and_then(|s| grading::analysis::for_site(cache, &site, &s));
                draw_3d_with(&site, analysis.as_deref(), sink);
            }
        } else if matches!(
            obj.shape,
            Shape::Hardscape { .. } | Shape::LandscapeArea { .. }
        ) {
            // Core sends only the objects that drape. They follow the site models on their layer.
            let terrain = crate::terrain::SiteTerrain::new(cache, revision, doc);
            let at = |p| terrain.elevation(obj, p);
            // Fully outside the site models there is no ground to follow.
            let ring = kadakas_core::curve::flatten(
                obj.shape.vertices().map_or(&[][..], |v| v.as_slice()),
                true,
                2.0,
            );
            let ground: Option<kadakas_core::hardscape::Terrain<'_>> = ring
                .iter()
                .step_by((ring.len() / 64).max(1))
                .any(|p| terrain.covered(obj, *p))
                .then_some(&at);
            if matches!(obj.shape, Shape::Hardscape { .. }) {
                kadakas_core::hardscape::emit_object(doc, obj, ground, sink);
            } else {
                kadakas_core::landscape::emit_object(doc, obj, ground, sink);
            }
        } else if let Shape::Massing(m) = &obj.shape {
            // Core sends a massing model here when it stands on the site model.
            let terrain = crate::terrain::SiteTerrain::new(cache, revision, doc);
            let attrs = doc.resolved_attrs(obj);
            let fill = attrs
                .fill
                .filter(|c| c.0[3] > 0)
                .map_or(kadakas_core::massing::DEFAULT_COLOR, |c| c.0);
            let edge = attrs
                .pen
                .filter(|c| c.0[3] > 0)
                .map_or([40, 40, 40, 255], |c| c.0);
            let base = terrain.massing_base(obj, m);
            kadakas_core::massing::emit(m, base, fill, edge, sink);
        } else if let Shape::SiteModifier { boundary, .. } = &obj.shape {
            // The outline of a modifier, at the height of the proposed surface where it can be found.
            let ring = kadakas_core::site::boundary_points(boundary);
            let host = doc
                .objects
                .values()
                .filter(|o| o.layer == obj.layer && matches!(o.shape, Shape::SiteModel { .. }))
                .find_map(|o| cache.get(revision, doc, o.id).ok());
            let pts: Vec<DVec3> = ring
                .iter()
                .map(|p| {
                    p.extend(
                        host.as_ref()
                            .and_then(|h| h.elevation_at(*p))
                            .unwrap_or(0.0),
                    )
                })
                .collect();
            sink.polyline(&pts, true, [90, 90, 90, 255]);
        }
    }
}

fn contour_svg(
    out: &mut String,
    doc: &Document,
    obj: &Object,
    which: Surface,
    color: Color,
    c: &Contour,
    label: bool,
) {
    let scale = doc.layer(obj.layer).map_or(100.0, |l| l.scale);
    let width = if c.major { 0.5 } else { 0.18 } * scale;
    let tag = if c.closed { "polygon" } else { "polyline" };
    let pts: Vec<String> = c
        .points
        .iter()
        .map(|p| format!("{},{}", p.x, -p.y))
        .collect();
    let dash = if which == Surface::Existing {
        format!(r#" stroke-dasharray="{} {}""#, 3.0 * scale, 1.5 * scale)
    } else {
        String::new()
    };
    let _ = writeln!(
        out,
        r#"<{tag} points="{}" fill="none" stroke="{}" stroke-width="{width}"{dash} data-kind="contour" data-surface="{}" data-level="{}" data-major="{}" data-site="{}"/>"#,
        pts.join(" "),
        color.to_css(),
        if which == Surface::Existing {
            "existing"
        } else {
            "proposed"
        },
        c.level,
        c.major,
        obj.id
    );
    if label && c.major && !c.points.is_empty() {
        let mid = c.points[c.points.len() / 2];
        let _ = writeln!(
            out,
            r#"<text x="{}" y="{}" font-size="{}" font-family="sans-serif" fill="{}" data-kind="contour-label" data-site="{}">{}</text>"#,
            mid.x,
            -mid.y,
            2.5 * scale,
            color.to_css(),
            obj.id,
            format_level(c.level)
        );
    }
}

/// An elevation in mm as meters, for a label.
pub fn format_level(mm: f64) -> String {
    let s = format!("{:.2}", mm / 1000.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Adds the contours of a site model to the SVG of the drawing. For `svg::render_with`.
pub fn svg_ext<'a>(
    cache: &'a SiteCache,
    revision: u64,
) -> impl Fn(&Document, &Object, &str, &mut String) + 'a {
    move |doc, obj, _style, out| {
        if kadakas_core::site::carries_extra(obj) {
            grading::hook::svg(doc, cache, revision, obj, out);
            return;
        }
        if !matches!(obj.shape, Shape::SiteModel { .. }) {
            return;
        }
        let Ok(site) = cache.get(revision, doc, obj.id) else {
            return;
        };
        out.push_str(&grading::hook::site_layers(doc, cache, revision, obj, 1.0, false).svg());
        for which in shown_surfaces(&site, site.settings.display_2d) {
            let color = line_color(&site, which);
            for c in site.contours(which).unwrap_or(&[]) {
                contour_svg(out, doc, obj, which, color, c, site.settings.label_contours);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{add, grid_site};
    use glam::DVec2;
    use kadakas_core::command::Editor;
    use kadakas_core::mesh;
    use kadakas_core::model::ModifierKind;
    use kadakas_core::svg;

    fn site_with_pad() -> (Editor, kadakas_core::model::ObjectId) {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(9, 1000.0, |x, _| x / 10.0));
        let b = kadakas_core::curve::corners(&[
            DVec2::new(2000.0, 2000.0),
            DVec2::new(4000.0, 2000.0),
            DVec2::new(4000.0, 4000.0),
            DVec2::new(2000.0, 4000.0),
        ]);
        add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 300.0,
                    slope: None,
                },
                boundary: b,
            },
        );
        (ed, id)
    }

    fn set_settings(
        ed: &mut Editor,
        id: kadakas_core::model::ObjectId,
        f: impl Fn(&mut kadakas_core::model::SiteSettings),
    ) {
        let mut shape = ed.doc().object(id).unwrap().shape.clone();
        if let Shape::SiteModel { settings, .. } = &mut shape {
            f(settings);
        }
        ed.execute(&kadakas_core::command::Command::ModifyObject {
            id,
            shape: Some(shape),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
    }

    #[test]
    fn labels_format_meters() {
        assert_eq!(format_level(2500.0), "2.5");
        assert_eq!(format_level(3000.0), "3");
        assert_eq!(format_level(-1250.0), "-1.25");
    }

    #[test]
    fn svg_has_existing_and_proposed_contours_by_choice() {
        let (mut ed, id) = site_with_pad();
        let cache = SiteCache::default();
        let render = |ed: &Editor| {
            let ext = svg_ext(&cache, ed.revision());
            svg::render_with(ed.doc(), 800, Some(&ext))
        };
        let count = |svg: &str, what: &str| svg.matches(what).count();
        let proposed = render(&ed);
        assert!(
            count(&proposed, r#"data-surface="proposed""#) > 0
                && count(&proposed, r#"data-surface="existing""#) == 0
        );
        set_settings(&mut ed, id, |s| s.display_2d = SurfaceDisplay::Both);
        let both = render(&ed);
        assert!(
            count(&both, r#"data-surface="existing""#) > 0
                && count(&both, r#"data-surface="proposed""#) > 0
        );
        assert!(both.contains("stroke-dasharray"));
        assert!(!both.contains("contour-label"));
        set_settings(&mut ed, id, |s| {
            s.display_2d = SurfaceDisplay::Existing;
            s.label_contours = true;
            s.minor_interval = 100.0;
            s.major_interval = 200.0;
        });
        let labeled = render(&ed);
        assert!(labeled.contains("contour-label"));
        assert!(labeled.contains(r#"data-major="true""#));
        // Without the extension only the outline is drawn.
        assert!(!svg::render(ed.doc(), 800).contains("data-kind=\"contour\""));
        assert!(svg::render(ed.doc(), 800).contains(r#"data-kind="site_model""#));
    }

    #[test]
    fn site_without_a_surface_draws_only_its_outline() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::SiteModel {
                points: vec![glam::DVec3::ZERO],
                lines: vec![],
                settings: Default::default(),
            },
        );
        let cache = SiteCache::default();
        let ext = svg_ext(&cache, ed.revision());
        assert!(!svg::render_with(ed.doc(), 400, Some(&ext)).contains("contour"));
        let ext3 = scene_ext(&cache, ed.revision());
        assert!(mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext3)).is_empty());
    }

    #[test]
    fn scene_has_mesh_or_contours_by_setting() {
        let (mut ed, id) = site_with_pad();
        let cache = SiteCache::default();
        let build = |ed: &Editor| {
            let ext = scene_ext(&cache, ed.revision());
            mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext))
        };
        let scene = build(&ed);
        assert!(scene.mesh.triangle_count() > 100);
        assert!(scene.mesh.tri_object.iter().all(|o| *o == id));
        // The pad outline is a line of its own.
        let outline_only = scene.lines.segment_count();
        assert!(outline_only >= 4);
        set_settings(&mut ed, id, |s| s.display_3d = Display3d::Contours);
        let scene = build(&ed);
        assert_eq!(scene.mesh.triangle_count(), 0);
        assert!(scene.lines.segment_count() > outline_only);
        // Contours are at their level height.
        let zs: Vec<f32> = scene.lines.positions.iter().map(|p| p.z).collect();
        assert!(zs.iter().any(|z| (*z - 250.0).abs() < 1e-3
            || (*z - 300.0).abs() < 1e-3
            || (*z - 500.0).abs() < 1e-3));
        set_settings(&mut ed, id, |s| {
            s.display_3d = Display3d::Both;
            s.surface_3d = SurfaceDisplay::Both;
            s.existing_color = Some(Color([1, 2, 3, 255]));
            s.proposed_color = Some(Color([4, 5, 6, 255]));
        });
        let scene = build(&ed);
        assert!(
            scene.mesh.colors.contains(&[1, 2, 3, 255])
                && scene.mesh.colors.contains(&[4, 5, 6, 255])
        );
        // Core alone draws only the modifier outline.
        let bare = mesh::build_with(ed.doc(), glam::DVec3::ZERO, None);
        assert_eq!(bare.mesh.triangle_count(), 0);
        assert_eq!(bare.lines.segment_count(), 4);
    }

    #[test]
    fn shown_surfaces_skips_the_duplicate() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(3, 1000.0, |x, _| x));
        let site = SiteResult::compute(ed.doc(), id).unwrap();
        assert_eq!(
            shown_surfaces(&site, SurfaceDisplay::Proposed),
            vec![Surface::Existing]
        );
        assert_eq!(
            shown_surfaces(&site, SurfaceDisplay::Both),
            vec![Surface::Existing]
        );
        assert_eq!(
            shown_surfaces(&site, SurfaceDisplay::Existing),
            vec![Surface::Existing]
        );
        let (ed, id) = site_with_pad();
        let site = SiteResult::compute(ed.doc(), id).unwrap();
        assert_eq!(
            shown_surfaces(&site, SurfaceDisplay::Both),
            vec![Surface::Existing, Surface::Proposed]
        );
        assert_eq!(
            shown_surfaces(&site, SurfaceDisplay::Proposed),
            vec![Surface::Proposed]
        );
    }
}
