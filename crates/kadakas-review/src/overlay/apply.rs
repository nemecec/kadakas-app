//! Putting an overlay on what the renderers draw: the SVG of the plan, and the 3D scene.
//!
//! The overlay model and the style stay free of renderers. These functions use only types of
//! `kadakas-core` (the SVG overlay trait, the scene), so any renderer built on them shows the
//! same tints: the SVG, the CPU renderer, the plan canvas and the 3D view of the app.
//!
//! A ghost is drawn against the document of the old side (`old`), with its resources (symbols,
//! styles, materials), so that a removed symbol or styled plant has its own geometry. Whether it
//! is shown follows the layers and classes of the new drawing, or of the old one where the new
//! drawing has no such layer or class.

use std::sync::Arc;

use glam::DVec2;
use kadakas_core::mesh::{self, Scene, SceneExt};
use kadakas_core::model::{Document, Object, ObjectId, Shape};
use kadakas_core::svg_overlay::{SvgGhost, SvgLegendRow, SvgLook, SvgOverlay, SvgPattern, SvgTint};

use super::style::{CompareStyle, Pattern, UnchangedMode};
use super::{CompareOverlay, GhostReport, Status};

/// True if a shape draws something in the plan. A picture (underlay), a group, a shape of a kind
/// that this version does not know and the parts of a sheet make no ghost: a ghost never covers
/// the plan with a picture. Annotations and site models do: they are drawn against the document
/// of the old side, with its worksheets and site data.
fn has_geometry(shape: &Shape) -> bool {
    !matches!(
        shape,
        Shape::Underlay(_)
            | Shape::Group { .. }
            | Shape::Unknown { .. }
            | Shape::Viewport(_)
            | Shape::TitleBlock(_)
    )
}

/// True if the object shows: its layer and class are visible in the new drawing, or in the old
/// one if the new one has no such layer or class.
fn shows(new: &Document, old: &Document, o: &Object) -> bool {
    let layer = new
        .layer(o.layer)
        .or_else(|| old.layer(o.layer))
        .is_none_or(|l| l.visible && l.kind == kadakas_core::model::LayerKind::Design);
    let class = new
        .class(o.class)
        .or_else(|| old.class(o.class))
        .is_none_or(|c| c.visible);
    layer && class
}

/// The ghosts of an overlay for a picture, and what became of every removed object.
#[derive(Clone, Debug, Default)]
pub struct Ghosts {
    /// The objects to draw: removed objects (kind `removed`), and, with `show_previous`, the old
    /// shapes of changed ones (kind `previous`, to draw behind the live objects).
    pub items: Vec<(Object, &'static str)>,
    pub report: GhostReport,
}

/// The ghosts of an overlay for the plan: see [`Ghosts`]. `new` is the drawing that is shown and
/// `old` the old side. Pass the same document twice when there is only one.
pub fn ghosts_in(
    new: &Document,
    old: &Document,
    overlay: &CompareOverlay,
    style: &CompareStyle,
) -> Ghosts {
    let mut out = Ghosts {
        report: GhostReport {
            capped: overlay.omitted_ghosts(),
            not_drawable: overlay.unreadable_ghosts(),
            ..GhostReport::default()
        },
        ..Ghosts::default()
    };
    if style.show_ghosts {
        for o in overlay.ghosts() {
            if !has_geometry(&o.shape) {
                out.report.not_drawable += 1;
            } else if !shows(new, old, o) {
                out.report.hidden += 1;
            } else {
                let mut g = o.clone();
                g.parent = None;
                out.items.push((g, "removed"));
                out.report.drawn += 1;
            }
        }
    }
    if style.show_previous {
        for o in overlay.previous() {
            if has_geometry(&o.shape) && shows(new, old, o) {
                let mut g = o.clone();
                g.parent = None;
                out.items.push((g, "previous"));
            }
        }
    }
    out
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
    legend: Vec<SvgLegendRow>,
    report: GhostReport,
    view: Option<(DVec2, DVec2)>,
}

impl<'a> SvgCompare<'a> {
    /// The overlay for the plan of `doc` (the new side of the comparison). Ghosts are drawn
    /// against `old`, the document of the old side, if there is one.
    pub fn new(
        doc: &Document,
        old: Option<&Arc<Document>>,
        overlay: &'a CompareOverlay,
        style: CompareStyle,
    ) -> SvgCompare<'a> {
        let removed = SvgTint {
            opacity: f64::from(style.ghost),
            fill_alpha: 0.3,
            ..svg_tint(&style, Status::Removed)
        };
        let before = SvgTint {
            opacity: f64::from(style.ghost) * 0.6,
            fill_alpha: 0.1,
            symbol: None,
            ..svg_tint(&style, Status::Changed)
        };
        let found = ghosts_in(doc, old.map_or(doc, |o| o.as_ref()), overlay, &style);
        let report = found.report;
        let ghosts = found
            .items
            .into_iter()
            .map(|(object, kind)| SvgGhost {
                object,
                tint: if kind == "removed" {
                    removed.clone()
                } else {
                    before.clone()
                },
                kind,
                doc: old.cloned(),
                below: kind == "previous",
            })
            .collect();
        // The legend: the count of each status, and a note where the overlay is not complete.
        let counts = overlay.counts();
        let mut legend: Vec<SvgLegendRow> = Status::ALL
            .iter()
            .map(|s| SvgLegendRow {
                tint: Some(svg_tint(&style, *s)),
                text: format!(
                    "{} {}",
                    s.name(),
                    match s {
                        Status::Added => counts.added,
                        Status::Removed => counts.removed,
                        Status::Changed => counts.changed,
                    }
                ),
            })
            .collect();
        let note = |text: String| SvgLegendRow { tint: None, text };
        if report.capped > 0 {
            legend.push(note(format!(
                "ghosts: {} of {} kept",
                overlay.ghost_count(),
                overlay.ghost_count() + report.capped
            )));
        }
        if report.not_drawable > 0 {
            legend.push(note(format!(
                "ghosts not drawable: {}",
                report.not_drawable
            )));
        }
        if report.hidden > 0 {
            legend.push(note(format!("ghosts on hidden layers: {}", report.hidden)));
        }
        if !overlay.exact() {
            legend.push(note(
                "partial: the work limit stopped the comparison".into(),
            ));
        }
        SvgCompare {
            overlay,
            style,
            ghosts,
            legend,
            report,
            view: None,
        }
    }

    /// Fixes the part of the model to show, so that two renders line up.
    pub fn with_view(mut self, view: (DVec2, DVec2)) -> SvgCompare<'a> {
        self.view = Some(view);
        self
    }

    /// What became of the ghosts of removed objects.
    pub fn report(&self) -> GhostReport {
        self.report
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

    fn legend(&self) -> &[SvgLegendRow] {
        &self.legend
    }

    fn dark(&self) -> bool {
        self.style.dark
    }
}

/// A document with the ghosts of removed objects as its objects: the old side with only these
/// objects, all layers and classes made visible (what is hidden was left out already). Building
/// the 3D scene of it gives the ghost geometry with the resources, the drawing settings and the
/// site models of the old side.
pub fn ghost_document(
    new: &Document,
    old: &Document,
    overlay: &CompareOverlay,
    style: &CompareStyle,
) -> (Document, GhostReport) {
    let found = ghosts_in(new, old, overlay, style);
    let mut out = old.clone();
    out.objects.clear();
    for l in &mut out.layers {
        l.visible = true;
    }
    for c in &mut out.classes {
        c.visible = true;
    }
    for (o, kind) in found.items {
        // The old shape of a changed object has the ID of the live object: 3D shows only the
        // ghosts of removed objects.
        if kind == "removed" {
            out.objects.insert(o.id, o);
        }
    }
    (out, found.report)
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

/// The 3D scene of a compare.
pub struct Tinted {
    pub scene: Scene,
    /// The ghosts that made geometry. A renderer draws them see-through
    /// (`kadakas_render::look::Look::ghosted`).
    pub ghosts: Vec<ObjectId>,
    pub report: GhostReport,
}

/// The 3D scene with the overlay: the changed objects in their tints, the unchanged ones dimmed or
/// left out, and the ghosts of removed objects added. `old` is the document of the old side and
/// `ext` draws its site models. A ghost that makes no geometry is reported as not drawable.
pub fn tint_scene(
    scene: &Scene,
    new: &Document,
    old: &Document,
    overlay: &CompareOverlay,
    style: &CompareStyle,
    ext: Option<SceneExt<'_>>,
) -> Tinted {
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
    let mut report = GhostReport::default();
    if style.show_ghosts {
        let (gdoc, mut r) = ghost_document(new, old, overlay, style);
        if !gdoc.objects.is_empty() {
            let mut gscene = mesh::build_with(&gdoc, scene.origin, ext);
            let [cr, cg, cb] = style.color(Status::Removed);
            gscene.recolor(&|_, _| Some([cr, cg, cb, 255]));
            ghosts = gscene.objects().collect();
            out.append(&gscene);
        }
        // A ghost without geometry (a group, an empty shape) was counted as drawn.
        r.not_drawable += r.drawn.saturating_sub(ghosts.len());
        r.drawn = ghosts.len();
        report = r;
    } else {
        report.capped = overlay.omitted_ghosts();
        report.not_drawable = overlay.unreadable_ghosts();
    }
    Tinted {
        scene: out,
        ghosts,
        report,
    }
}
