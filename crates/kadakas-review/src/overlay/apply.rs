//! Putting an overlay on what the renderers draw: the SVG of the plan, and the 3D scene.
//!
//! The overlay model and the style stay free of renderers. These functions use only types of
//! `kadakas-core` (the SVG overlay trait, the scene), so any renderer built on them shows the
//! same tints: the SVG, the CPU renderer, the plan canvas and the 3D view of the app.

use glam::DVec2;
use kadakas_core::mesh::{self, Scene};
use kadakas_core::model::{Document, FIRST_LAYER, NONE_CLASS, Object, ObjectId};
use kadakas_core::svg_overlay::{SvgGhost, SvgLook, SvgOverlay, SvgPattern, SvgTint};

use super::style::{CompareStyle, Pattern, UnchangedMode};
use super::{CompareOverlay, Status};

/// The ghost as the new drawing can show it: on its layer and class if the new drawing has them,
/// else on the first layer and in the None class. `None`: the layer is not visible, so the ghost
/// is not drawn.
fn placed(doc: &Document, o: &Object) -> Option<Object> {
    let mut g = o.clone();
    if doc.layer(g.layer).is_none() {
        g.layer = FIRST_LAYER;
    }
    if doc.class(g.class).is_none() {
        g.class = NONE_CLASS;
    }
    g.parent = None;
    doc.is_visible(&g).then_some(g)
}

/// The objects to draw as ghosts of an overlay in a document: the removed objects, and, with
/// `show_previous`, the old shapes of the changed ones.
fn ghost_objects<'a>(
    doc: &'a Document,
    overlay: &'a CompareOverlay,
    style: &CompareStyle,
) -> impl Iterator<Item = (Object, &'static str)> + 'a {
    let removed = style.show_ghosts;
    let previous = style.show_previous;
    removed
        .then(|| overlay.ghosts().filter_map(|o| placed(doc, o)))
        .into_iter()
        .flatten()
        .map(|o| (o, "removed"))
        .chain(
            previous
                .then(|| overlay.previous().filter_map(|o| placed(doc, o)))
                .into_iter()
                .flatten()
                .map(|o| (o, "previous")),
        )
}

fn pattern(p: Pattern) -> SvgPattern {
    match p {
        Pattern::Solid => SvgPattern::Solid,
        Pattern::Dashed => SvgPattern::Dashed,
        Pattern::Dotted => SvgPattern::Dotted,
    }
}

/// The tint of a status for the SVG.
fn svg_tint(style: &CompareStyle, status: Status) -> SvgTint {
    let t = style.tint(status);
    SvgTint {
        rgb: t.rgb,
        pattern: pattern(t.pattern),
        weight_factor: f64::from(t.weight),
        fill_alpha: 0.35,
        opacity: 1.0,
        symbol: Some(t.symbol),
        status: status.name(),
    }
}

/// An overlay for the SVG of a document (see `kadakas_core::svg::render_overlay`).
pub struct SvgCompare<'a> {
    overlay: &'a CompareOverlay,
    style: CompareStyle,
    ghosts: Vec<SvgGhost>,
    view: Option<(DVec2, DVec2)>,
}

impl<'a> SvgCompare<'a> {
    /// The overlay for the plan of `doc` (the new side of the comparison).
    pub fn new(doc: &Document, overlay: &'a CompareOverlay, style: CompareStyle) -> SvgCompare<'a> {
        let removed = SvgTint {
            opacity: f64::from(style.ghost),
            fill_alpha: 0.15,
            ..svg_tint(&style, Status::Removed)
        };
        let before = SvgTint {
            opacity: f64::from(style.ghost) * 0.6,
            fill_alpha: 0.1,
            symbol: None,
            ..svg_tint(&style, Status::Changed)
        };
        let ghosts = ghost_objects(doc, overlay, &style)
            .map(|(object, kind)| SvgGhost {
                object,
                tint: if kind == "removed" {
                    removed.clone()
                } else {
                    before.clone()
                },
                kind,
            })
            .collect();
        SvgCompare {
            overlay,
            style,
            ghosts,
            view: None,
        }
    }

    /// Fixes the part of the model to show, so that two renders line up.
    pub fn with_view(mut self, view: (DVec2, DVec2)) -> SvgCompare<'a> {
        self.view = Some(view);
        self
    }
}

impl SvgOverlay for SvgCompare<'_> {
    fn look(&self, id: ObjectId) -> SvgLook {
        match self.overlay.status_of(id) {
            Some(status) => SvgLook::Tint(svg_tint(&self.style, status)),
            None => match self.style.unchanged {
                UnchangedMode::Show => SvgLook::Keep,
                UnchangedMode::Dim => SvgLook::Dim(f64::from(self.style.dim)),
                UnchangedMode::Hide => SvgLook::Hide,
            },
        }
    }

    fn ghosts(&self) -> &[SvgGhost] {
        &self.ghosts
    }

    fn view(&self) -> Option<(DVec2, DVec2)> {
        self.view
    }
}

/// A document with the ghosts of an overlay as its objects, on the layers and classes of `doc`.
/// Building the 3D scene of it gives the ghost geometry.
pub fn ghost_document(doc: &Document, overlay: &CompareOverlay, style: &CompareStyle) -> Document {
    let mut out = Document::default();
    out.layers = doc.layers.clone();
    out.classes = doc.classes.clone();
    for (o, kind) in ghost_objects(doc, overlay, style) {
        // The old shape of a changed object has the ID of the live object: 3D shows only the
        // ghosts of removed objects.
        if kind == "removed" {
            out.objects.insert(o.id, o);
        }
    }
    out
}

/// Mixes a color toward the paper. `keep` is the part of the color that stays (0 to 1).
fn fade(c: [u8; 4], paper: [u8; 3], keep: f32) -> [u8; 4] {
    let mix = |a: u8, b: u8| (f32::from(a) * keep + f32::from(b) * (1.0 - keep)).round() as u8;
    [
        mix(c[0], paper[0]),
        mix(c[1], paper[1]),
        mix(c[2], paper[2]),
        c[3],
    ]
}

/// The 3D scene with the overlay: the changed objects in their tints, the unchanged ones dimmed
/// or left out, and the ghosts of removed objects added. Returns the scene and the IDs of the
/// ghosts, which a renderer draws see-through (`kadakas_render::look::Look::translucent`).
pub fn tint_scene(
    scene: &Scene,
    doc: &Document,
    overlay: &CompareOverlay,
    style: &CompareStyle,
) -> (Scene, Vec<ObjectId>) {
    let mut out = if style.unchanged == UnchangedMode::Hide {
        scene.retain(&|id| overlay.status_of(id).is_some())
    } else {
        scene.clone()
    };
    let paper = style.paper();
    out.recolor(&|id, c| match overlay.status_of(id) {
        Some(status) => {
            let [r, g, b] = style.color(status);
            Some([r, g, b, 255])
        }
        None if style.unchanged == UnchangedMode::Dim => Some(fade(c, paper, style.dim)),
        None => None,
    });
    let mut ghosts = Vec::new();
    if style.show_ghosts {
        let gdoc = ghost_document(doc, overlay, style);
        if !gdoc.objects.is_empty() {
            let mut gscene = mesh::build_at(&gdoc, scene.origin);
            let [r, g, b] = style.color(Status::Removed);
            gscene.recolor(&|_, _| Some([r, g, b, 255]));
            ghosts = gscene.objects().collect();
            out.append(&gscene);
        }
    }
    (out, ghosts)
}
