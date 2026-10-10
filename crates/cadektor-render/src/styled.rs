//! Renders a document scene with a render style: the glue between the document (sun, materials,
//! render style resources) and the renderers.

use cadektor_core::mesh::Scene;
use cadektor_core::model::Document;
use cadektor_core::present::{self, RenderStyle};
use cadektor_core::resource::ResourceId;

use crate::camera::Camera;
use crate::hlr::{HiddenLines, HlOptions, hidden_lines};
use crate::look::Look;
use crate::raster::Image;
use crate::shade::{Light, ShadeOptions, render_scene};

/// The render style to use: the one asked for, else the active style of the document, else the
/// built-in shaded style.
pub fn resolve_style(doc: &Document, wanted: Option<ResourceId>) -> RenderStyle {
    wanted
        .or(doc.drawing.presentation.active_style)
        .and_then(|id| present::render_style(doc, id))
        .cloned()
        .unwrap_or_default()
}

/// The light of a style in a document: the sun of the document when the style uses it.
pub fn light_for(doc: &Document, style: &RenderStyle) -> Light {
    if !style.sun {
        return Light::Fixed;
    }
    match doc.drawing.presentation.sun {
        None => Light::Fixed,
        Some(s) => match s.light_direction() {
            Some(d) => Light::Sun([d[0] as f32, d[1] as f32, d[2] as f32]),
            None => Light::Night,
        },
    }
}

/// Options for the CPU renderer from a style.
pub fn shade_options(
    doc: &Document,
    style: &RenderStyle,
    width: u32,
    height: u32,
    samples: u32,
) -> ShadeOptions {
    let light = light_for(doc, style);
    ShadeOptions {
        width,
        height,
        mode: style.mode,
        edges: style.edges,
        shadows: style.shadows && matches!(light, Light::Sun(_)),
        light,
        background: style.background.0,
        line_px: (style.line_weight as f32).max(0.5),
        samples,
    }
}

/// Draws the scene of a document with a render style.
pub fn render_styled(
    doc: &Document,
    scene: &Scene,
    camera: &Camera,
    style: &RenderStyle,
    width: u32,
    height: u32,
    samples: u32,
) -> Image {
    let look = Look::build(doc, scene, style.textures);
    render_scene(
        scene,
        &look,
        camera,
        &shade_options(doc, style, width, height, samples),
    )
}

/// The vector lines of the scene for a hidden-line style. For other styles it returns `None`.
pub fn vector_lines(
    scene: &Scene,
    camera: &Camera,
    style: &RenderStyle,
    width: u32,
    height: u32,
) -> Option<HiddenLines> {
    style.mode.is_hidden_line().then(|| {
        hidden_lines(
            scene,
            camera,
            width,
            height,
            &HlOptions {
                include_hidden: style.mode == present::RenderMode::DashedHiddenLine,
                mesh_edges: true,
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::Color;
    use cadektor_core::present::{RenderMode, RenderResource, SunSettings};
    use cadektor_core::resource::ResourceBody;

    fn add_style(ed: &mut Editor, s: RenderStyle) -> ResourceId {
        ResourceId(
            ed.execute(&Command::AddResource {
                name: format!("s{}", ed.doc().resources.len()),
                body: ResourceBody::Render(Box::new(RenderResource::RenderStyle(s))),
            })
            .unwrap()
            .created[0],
        )
    }

    #[test]
    fn style_resolution_prefers_the_request_then_the_active_style() {
        let mut ed = Editor::default();
        assert_eq!(resolve_style(ed.doc(), None), RenderStyle::default());
        let a = add_style(
            &mut ed,
            RenderStyle {
                mode: RenderMode::Wireframe,
                ..Default::default()
            },
        );
        let b = add_style(
            &mut ed,
            RenderStyle {
                mode: RenderMode::HiddenLine,
                ..Default::default()
            },
        );
        ed.execute(&Command::SetRenderStyle { style: Some(a) })
            .unwrap();
        assert_eq!(resolve_style(ed.doc(), None).mode, RenderMode::Wireframe);
        assert_eq!(
            resolve_style(ed.doc(), Some(b)).mode,
            RenderMode::HiddenLine
        );
        // An ID that is not a render style falls back to the default.
        assert_eq!(
            resolve_style(ed.doc(), Some(ResourceId::from_num(999))),
            RenderStyle::default()
        );
    }

    #[test]
    fn light_follows_the_sun_of_the_document() {
        let mut ed = Editor::default();
        let style = RenderStyle {
            sun: true,
            shadows: true,
            ..Default::default()
        };
        // No sun set: the fixed light.
        assert_eq!(light_for(ed.doc(), &style), Light::Fixed);
        ed.execute(&Command::SetSun {
            sun: Some(SunSettings::default()),
        })
        .unwrap();
        assert!(matches!(light_for(ed.doc(), &style), Light::Sun(d) if d[2] > 0.5));
        assert_eq!(
            light_for(
                ed.doc(),
                &RenderStyle {
                    sun: false,
                    ..style.clone()
                }
            ),
            Light::Fixed
        );
        ed.execute(&Command::SetSun {
            sun: Some(SunSettings {
                hour: 1.0,
                ..Default::default()
            }),
        })
        .unwrap();
        assert_eq!(light_for(ed.doc(), &style), Light::Night);
        // Shadows need a sun.
        let o = shade_options(ed.doc(), &style, 10, 10, 1);
        assert!(!o.shadows);
        ed.execute(&Command::SetSun {
            sun: Some(SunSettings::default()),
        })
        .unwrap();
        let o = shade_options(ed.doc(), &style, 10, 10, 1);
        assert!(o.shadows && o.mode == RenderMode::Shaded && o.background == style.background.0);
    }

    #[test]
    fn render_styled_and_vector_lines() {
        let mut ed = Editor::default();
        add(&mut ed, square(500.0), Some([200, 30, 30, 255]));
        let scene = scene_of(&ed);
        let cam = top_ortho(1000.0);
        let img = render_styled(ed.doc(), &scene, &cam, &RenderStyle::default(), 50, 50, 1);
        assert_eq!((img.width, img.height), (50, 50));
        assert_ne!(img.pixel(25, 25), crate::raster::BACKGROUND);
        assert!(vector_lines(&scene, &cam, &RenderStyle::default(), 50, 50).is_none());
        let hl = RenderStyle {
            mode: RenderMode::HiddenLine,
            background: Color([255, 255, 255, 255]),
            ..Default::default()
        };
        let lines = vector_lines(&scene, &cam, &hl, 50, 50).unwrap();
        assert_eq!(lines.visible_edge_count(), 4);
    }
}
