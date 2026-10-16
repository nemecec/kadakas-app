//! The drawing of a paved area: one source for the canvas, `render.svg`, the sheets and the PDF.
//!
//! What is drawn is the pattern plan of the hardscape (zones, courses, their own patterns,
//! origins and angles, units that go to the zone of their center), made by the same engine as
//! `report.unit_pattern` and the DXF export (`layout::plan::plan_groups`). The tile counts that are
//! drawn at full detail are the counts of the report.
//!
//! Level of detail (PAT-12). The part of the plan that is in view, or in the region of a render, is
//! drawn with its real units when it holds at most a bound of units: the work is clipped to the view
//! box (the pattern is laid on the part of the region that is in view, with the origin and the angle
//! of the whole), so a lot of 250,000 units can be zoomed in to the unit. Cut units are marked with
//! a light fill; whole units stay plain. Above the bound the drawing is a preview: the gap lines of
//! the whole plan, with larger units where the pattern is too fine (the old drawing), and a label
//! that says so ([`PREVIEW_LABEL`]).
//!
//! The bounds are measured (`measure_bounds` in the tests of this module, and in the app and print
//! crates); the numbers are in the documentation of each constant.
//!
//! The laid plan is kept by a content key (the outline, the holes, the pattern, the plan, the
//! tolerance and the objects that a zone or a course follows), so an edit of something else does
//! not lay the plan again. The view is clipped from the kept plan.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use glam::DVec2;

use crate::layout::check::{self, CheckOptions};
use crate::layout::gaps::{self, MAX_DRAW_SEGMENTS};
use crate::layout::plan::{self, Body, Group, Planned, Role};
use crate::layout::units::DEFAULT_TRIM;
use crate::layout::{self, Limits, Pattern, Placement, Region, Segment};
use crate::model::{Document, Object};

/// A box in drawing coordinates: lowest and highest corner (mm).
pub type BBox = (DVec2, DVec2);

/// What the canvas and the previews say when they draw a preview.
pub const PREVIEW_LABEL: &str = "preview: zoom in for the real pattern";

/// Most units in the window of the canvas for a drawing of the real units. The window is the
/// visible part and a margin round it (see the app). Measured in the app (`measure_canvas_frames`,
/// release-like build of the egui crates, one core): painting a kept window and tessellating it
/// costs 1.9 ms at 10,000 line segments, 15 ms at 30,000 and 39 ms at 100,000. A running bond or a
/// stack has about one segment per unit, so 30,000 units are the most for 60 frames a second.
pub const CANVAS_BOUND: usize = 30_000;
/// Most gap lines in the window of the canvas (see [`CANVAS_BOUND`]). A herringbone has about three
/// lines per unit, so its window holds a third of the units before the preview.
pub const CANVAS_MAX_SEGMENTS: usize = 30_000;
/// Most units in the region of a render (`render.svg`) for the real units. Measured
/// (`measure_bounds`): a 1,250 m2 lot of 200 x 100 mm units (60,000 units) is an SVG of 5.5 MB made
/// in 0.28 s; the 5,000 m2 lot (233,000 units) is 21.8 MB and 1.0 s, too much for an answer of the
/// API. Above the bound the render is a preview of 0.2 to 0.7 MB.
pub const SVG_BOUND: usize = 60_000;
/// Most units in the window of a viewport on a sheet for the real units, for a PDF, an SVG or a PNG
/// of a sheet. Measured (`measure_pdf` in the print crate): 60,000 units are a PDF of 0.9 MB made in
/// 1.1 s (the page in 48 ms, an SVG of the page 5.3 MB); the 5,000 m2 lot (233,000 units) is a PDF of
/// 3.5 MB in 4.3 s and an SVG of 21 MB. Above the bound the sheet has the gap lines and a note.
pub const PRINT_BOUND: usize = 60_000;
/// Most units of a lattice that zones give to the zone of their center. The lattice is laid whole
/// (and kept) because a unit goes to the zone of its center; a bigger lattice is drawn as a
/// preview.
pub const OWNER_LATTICE_MAX: usize = 300_000;

/// What to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawOptions {
    /// The gap lines of the patterns (and the edges of the course units).
    pub joints: bool,
    /// The outline of every whole unit.
    pub whole_units: bool,
    /// The outline of every cut unit.
    pub cut_units: bool,
    /// Most units in the view for the real units; above it the drawing is a preview.
    pub bound: usize,
    /// Most gap lines for the real units; above it the drawing is a preview.
    pub max_segments: usize,
}

impl DrawOptions {
    /// Gap lines and cut units: the canvas.
    pub fn canvas() -> Self {
        DrawOptions {
            joints: true,
            whole_units: false,
            cut_units: true,
            bound: CANVAS_BOUND,
            max_segments: CANVAS_MAX_SEGMENTS,
        }
    }

    /// Every unit as an outline: SVG and sheets.
    pub fn units(bound: usize) -> Self {
        DrawOptions {
            joints: false,
            whole_units: true,
            cut_units: true,
            bound,
            max_segments: bound.saturating_mul(6),
        }
    }
}

/// A cut unit: its outline, then the holes in it.
pub type CutUnit = Vec<Vec<DVec2>>;

/// What one group (the field, a zone, a course) draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawnGroup {
    pub name: String,
    pub role: Option<Role>,
    /// Gap lines (a preview, or `DrawOptions::joints`), and the edges of course units.
    pub joints: Vec<Segment>,
    /// The outlines of the whole units in view (when asked).
    pub whole: Vec<Vec<DVec2>>,
    /// The cut units in view (when asked).
    pub cut: Vec<CutUnit>,
    /// The whole and the cut units in view, also when their outlines were not asked for. Only at
    /// full detail.
    pub whole_count: usize,
    pub cut_count: usize,
}

/// How much was drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    /// The real units of the view.
    Full,
    /// The gap lines of the whole plan: the view holds more units than the bound.
    Preview {
        units_in_view: u64,
        bound: usize,
        /// The pattern is too fine for the preview too: some joints are left out.
        truncated: bool,
    },
}

/// A drawing of a plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub detail: Detail,
    pub groups: Vec<DrawnGroup>,
    /// The edges of the zones (closed rings), for a dashed line.
    pub zone_edges: Vec<Vec<DVec2>>,
    /// The view this was drawn for (`None`: the whole plan).
    pub view: Option<BBox>,
}

impl Drawn {
    pub fn is_preview(&self) -> bool {
        matches!(self.detail, Detail::Preview { .. })
    }

    /// The whole and the cut units drawn, over all groups.
    pub fn unit_counts(&self) -> (usize, usize) {
        self.groups
            .iter()
            .fold((0, 0), |(w, c), g| (w + g.whole_count, c + g.cut_count))
    }

    /// All segments, in all groups.
    pub fn segment_count(&self) -> usize {
        self.groups.iter().map(|g| g.joints.len()).sum()
    }
}

#[derive(Debug, Default)]
struct Preview {
    groups: Vec<DrawnGroup>,
    truncated: bool,
}

/// A hardscape with a joint pattern, laid as the report lays it, ready to be drawn for any view.
#[derive(Debug)]
pub struct Prepared {
    pub key: u64,
    /// The groups, as the report and the export have them.
    pub planned: Planned,
    /// The whole area that the pattern goes in (the outline less the kerb border, less the holes).
    pub region: Region,
    /// The lattices of centered zones that are too big to lay whole.
    lattice_too_big: bool,
    preview: OnceLock<Preview>,
    findings: OnceLock<Result<check::Report, String>>,
}

// ---------------------------------------------------------------------------
// The key and the cache

/// The key of what the drawing depends on: the outline, the holes, the pattern, the plan, the
/// settings and the tolerance, and the objects that a zone or a course follows. Two objects with
/// the same key are drawn the same.
pub fn content_key(doc: &Document, obj: &Object, tolerance: f64) -> u64 {
    let mut h = DefaultHasher::new();
    format!("{:?}", obj.shape).hash(&mut h);
    format!("{:?}", super::data(obj)).hash(&mut h);
    let plan = plan::data(obj);
    format!("{plan:?}").hash(&mut h);
    plan::has_unreadable_data(obj).hash(&mut h);
    if let Some((_, style)) = super::data::style_of(doc, obj) {
        format!("{:?}|{:?}", style.joints, style.border).hash(&mut h);
    }
    tolerance.to_bits().hash(&mut h);
    if let Some(p) = &plan {
        let followed = p
            .splits
            .iter()
            .filter_map(|s| s.source)
            .chain(p.courses.iter().filter_map(|c| c.source));
        for id in followed {
            id.hash(&mut h);
            if let Some(o) = doc.object(id) {
                format!("{:?}", o.shape).hash(&mut h);
            }
        }
    }
    h.finish()
}

/// How many laid plans the cache keeps.
const CACHE_SIZE: usize = 12;

type CacheEntry = (u64, Result<Arc<Prepared>, String>);

/// Laid plans by content key, the last used kept. A plan that cannot be laid is kept too, with its
/// message.
pub struct PlanCache {
    entries: Mutex<Vec<CacheEntry>>,
    capacity: usize,
    builds: std::sync::atomic::AtomicU64,
}

impl PlanCache {
    pub const fn new(capacity: usize) -> Self {
        PlanCache {
            entries: Mutex::new(Vec::new()),
            capacity,
            builds: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// How often a plan was laid (a hit does not count). Tests use it to check the work done.
    pub fn builds(&self) -> u64 {
        self.builds.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The laid plan for a key that [`content_key`] made.
    pub fn get(
        &self,
        doc: &Document,
        obj: &Object,
        key: u64,
    ) -> Option<Result<Arc<Prepared>, String>> {
        layout::object::pattern_of(doc, obj)?;
        {
            let mut c = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(i) = c.iter().position(|(k, _)| *k == key) {
                let hit = c.remove(i);
                let out = hit.1.clone();
                c.push(hit);
                return Some(out);
            }
        }
        self.builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let built = Prepared::new(doc, obj, layout::TOLERANCE, key).map(Arc::new);
        let mut c = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if c.len() >= self.capacity {
            drop(c.remove(0));
        }
        c.push((key, built.clone()));
        Some(built)
    }

    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

static CACHE: PlanCache = PlanCache::new(CACHE_SIZE);

/// How many plans the shared cache laid since the start of the process (a hit does not count).
pub fn builds() -> u64 {
    CACHE.builds()
}

/// The laid plan of a hardscape, from the cache when nothing that it depends on changed. `None`
/// if the object has no joint pattern; `Err` says why the plan cannot be laid (the message of the
/// report).
pub fn prepared(doc: &Document, obj: &Object) -> Option<Result<Arc<Prepared>, String>> {
    let key = content_key(doc, obj, layout::TOLERANCE);
    CACHE.get(doc, obj, key)
}

/// Like [`prepared`], for a key that the caller made with [`content_key`] (the canvas keeps the key
/// of an object for a revision of the document, so it does not hash the object at every frame).
pub fn prepared_with_key(
    doc: &Document,
    obj: &Object,
    key: u64,
) -> Option<Result<Arc<Prepared>, String>> {
    CACHE.get(doc, obj, key)
}

fn estimate(pattern: &Pattern, area: f64) -> u64 {
    match pattern.compile() {
        Ok(c) => {
            let pieces: usize = c.kinds.iter().map(|k| k.pieces.len()).sum();
            (area / c.det.abs().max(1e-9) * pieces as f64).ceil() as u64
        }
        Err(_) => 0,
    }
}

fn box_of_ring(ring: &[DVec2]) -> BBox {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in ring {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo, hi)
}

fn boxes_meet(a: BBox, b: BBox) -> bool {
    a.1.x >= b.0.x && a.0.x <= b.1.x && a.1.y >= b.0.y && a.0.y <= b.1.y
}

fn region_area_in(region: &Region, view: Option<BBox>) -> f64 {
    match view {
        None => region.area,
        Some((lo, hi)) => {
            if lo.x <= region.lo.x
                && lo.y <= region.lo.y
                && hi.x >= region.hi.x
                && hi.y >= region.hi.y
            {
                region.area
            } else {
                region.clipped_to_box(lo, hi).map_or(0.0, |r| r.area)
            }
        }
    }
}

/// The place a group starts its pattern: the group's own, with the origin of the whole region where
/// it has none.
fn resolved(g: &Group) -> Option<(&Pattern, Placement)> {
    match &g.body {
        Body::Pattern { pattern, place } => {
            let (origin, angle) = place.resolve(&g.region);
            Some((
                pattern,
                Placement {
                    origin: Some(origin),
                    angle,
                },
            ))
        }
        Body::Course(_) => None,
    }
}

/// The region of a group for counting: the zone, for a unit that goes to the zone of its center.
fn zone_region(g: &Group) -> &Region {
    match &g.owner {
        Some(o) => &o.zones[o.index],
        None => &g.region,
    }
}

fn edges_of(poly: &[DVec2], out: &mut Vec<Segment>) {
    let n = poly.len();
    for i in 0..n {
        out.push((poly[i], poly[(i + 1) % n]));
    }
}

impl Prepared {
    /// Lays the plan of an object. `Err`: the message of the report. The object must have a joint
    /// pattern.
    pub fn new(doc: &Document, obj: &Object, tolerance: f64, key: u64) -> Result<Prepared, String> {
        let (pattern, place) = layout::object::pattern_of(doc, obj)
            .ok_or_else(|| "the object has no joint pattern".to_string())?;
        let region = layout::object::region_of(doc, obj, tolerance)?;
        let planned =
            plan::plan_groups(doc, obj, region.clone(), pattern, place, layout::MAX_UNITS)?;
        let mut lattice_too_big = false;
        for g in planned.groups.iter().filter(|g| g.owner.is_some()) {
            if let Body::Pattern { pattern, .. } = &g.body
                && estimate(pattern, g.region.area) > OWNER_LATTICE_MAX as u64
            {
                lattice_too_big = true;
            }
        }
        Ok(Prepared {
            key,
            planned,
            region,
            lattice_too_big,
            preview: OnceLock::new(),
            findings: OnceLock::new(),
        })
    }

    /// About how many units the plan has in `view` (the whole plan for `None`). Exact for the
    /// units of courses; for the patterns it is the area of the region in view over the area of a
    /// cell, as the engine estimates its work.
    pub fn units_in(&self, view: Option<BBox>) -> u64 {
        let mut total = 0u64;
        for g in &self.planned.groups {
            match &g.body {
                Body::Pattern { pattern, .. } => {
                    total += estimate(pattern, region_area_in(zone_region(g), view));
                }
                Body::Course(c) => {
                    total += c
                        .pieces
                        .iter()
                        .filter(|p| view.is_none_or(|v| boxes_meet(box_of_ring(&p.poly), v)))
                        .count() as u64;
                }
            }
        }
        total
    }

    /// The box of the whole plan.
    pub fn bounds(&self) -> BBox {
        (self.region.lo, self.region.hi)
    }

    /// Draws the plan for a view: the real units when `view` holds at most `opts.bound` units,
    /// else the preview. `None` is the whole plan.
    pub fn draw(&self, view: Option<BBox>, opts: &DrawOptions) -> Drawn {
        let est = self.units_in(view);
        if est <= opts.bound as u64
            && !self.lattice_too_big
            && let Some(d) = self.draw_full(view, opts)
        {
            return d;
        }
        let p = self.preview();
        Drawn {
            detail: Detail::Preview {
                units_in_view: est,
                bound: opts.bound,
                truncated: p.truncated,
            },
            groups: p.groups.clone(),
            zone_edges: self.zone_edges(),
            view,
        }
    }

    fn zone_edges(&self) -> Vec<Vec<DVec2>> {
        self.planned
            .groups
            .iter()
            .filter(|g| g.role == Role::Zone)
            .flat_map(|g| zone_region(g).rings().cloned().collect::<Vec<_>>())
            .collect()
    }

    /// The gap lines of the whole plan, with larger units where it is too fine.
    fn preview(&self) -> &Preview {
        self.preview.get_or_init(|| {
            let mut out = Preview::default();
            let mut left = MAX_DRAW_SEGMENTS;
            let mut lattices: Vec<*const Vec<Region>> = Vec::new();
            for g in &self.planned.groups {
                let mut dg = DrawnGroup {
                    name: g.name.clone(),
                    role: Some(g.role),
                    ..Default::default()
                };
                match (&g.body, resolved(g)) {
                    (Body::Pattern { .. }, Some((pattern, place))) => {
                        // The zones of one lattice draw its lines once.
                        let first = match &g.owner {
                            Some(o) => {
                                let p = Arc::as_ptr(&o.zones);
                                let new = !lattices.contains(&p);
                                lattices.push(p);
                                new
                            }
                            None => true,
                        };
                        if first {
                            let gaps =
                                gaps::gap_lines(&g.region, pattern, &place, left.max(1000), true);
                            out.truncated |= gaps.truncated;
                            left = left.saturating_sub(gaps.segments.len());
                            dg.joints = gaps.segments;
                        }
                    }
                    (Body::Course(c), _) => {
                        for p in &c.pieces {
                            if left == 0 {
                                out.truncated = true;
                                break;
                            }
                            let before = dg.joints.len();
                            edges_of(&p.poly, &mut dg.joints);
                            left = left.saturating_sub(dg.joints.len() - before);
                        }
                    }
                    _ => {}
                }
                out.groups.push(dg);
            }
            out
        })
    }

    /// The real units in `view`. `None` if the work is too big after all (the estimate was low).
    fn draw_full(&self, view: Option<BBox>, opts: &DrawOptions) -> Option<Drawn> {
        let limits = Limits {
            max_units: opts.bound.saturating_mul(4).max(1000),
            trim: DEFAULT_TRIM,
        };
        let mut groups = Vec::with_capacity(self.planned.groups.len());
        let mut lattices: Vec<*const Vec<Region>> = Vec::new();
        for g in &self.planned.groups {
            let mut dg = DrawnGroup {
                name: g.name.clone(),
                role: Some(g.role),
                ..Default::default()
            };
            let in_view = |ring: &[DVec2]| view.is_none_or(|v| boxes_meet(box_of_ring(ring), v));
            let take = |u: &layout::Unit, dg: &mut DrawnGroup, course: bool| {
                let Some(outer) = u.rings.first() else { return };
                if !in_view(&outer.points) {
                    return;
                }
                if u.whole {
                    dg.whole_count += 1;
                    if opts.whole_units {
                        dg.whole.push(outer.points.clone());
                    } else if course && opts.joints {
                        edges_of(&outer.points, &mut dg.joints);
                    }
                } else {
                    dg.cut_count += 1;
                    if opts.cut_units || opts.whole_units {
                        dg.cut
                            .push(u.rings.iter().map(|r| r.points.clone()).collect());
                    }
                    if course && opts.joints && !opts.whole_units {
                        edges_of(&outer.points, &mut dg.joints);
                    }
                }
            };
            match &g.body {
                Body::Course(_) => {
                    g.for_each_unit(&limits_for_course(&limits, g), &mut |u| {
                        take(u, &mut dg, true)
                    })
                    .ok()?;
                }
                Body::Pattern { .. } => {
                    let (pattern, place) = resolved(g)?;
                    // The pattern is laid on the part of the region that is near the view.
                    let margin = unit_extent(pattern) * 1.5 + pattern.gap;
                    let window = match view {
                        Some((lo, hi)) => g.region.clipped_to_box(lo - margin, hi + margin),
                        None => Some(g.region.clone()),
                    };
                    let Some(window) = window else {
                        groups.push(dg);
                        continue;
                    };
                    if opts.joints {
                        let first = match &g.owner {
                            Some(o) => {
                                let p = Arc::as_ptr(&o.zones);
                                let new = !lattices.contains(&p);
                                lattices.push(p);
                                new
                            }
                            None => true,
                        };
                        if first {
                            let max = opts.max_segments.max(2000);
                            let gl = gaps::gap_lines(&window, pattern, &place, max, false);
                            if gl.truncated {
                                return None;
                            }
                            dg.joints = gl
                                .segments
                                .into_iter()
                                .filter(|(a, b)| {
                                    view.is_none_or(|v| boxes_meet((a.min(*b), a.max(*b)), v))
                                })
                                .collect();
                        }
                    }
                    if opts.whole_units || opts.cut_units {
                        match &g.owner {
                            Some(_) => {
                                g.for_each_unit(&Limits::default(), &mut |u| {
                                    take(u, &mut dg, false)
                                })
                                .ok()?;
                            }
                            None => {
                                layout::for_each_unit(
                                    &window,
                                    pattern,
                                    &place,
                                    &limits,
                                    &mut |u| take(u, &mut dg, false),
                                )
                                .ok()?;
                            }
                        }
                    }
                }
            }
            groups.push(dg);
        }
        Some(Drawn {
            detail: Detail::Full,
            groups,
            zone_edges: self.zone_edges(),
            view,
        })
    }

    /// The findings of the pattern check (`unit_pattern.check` with its defaults), laid once.
    pub fn findings(&self) -> &Result<check::Report, String> {
        self.findings.get_or_init(|| {
            check::check(&self.planned.groups, &self.region, &CheckOptions::default())
                .map_err(|e| e.to_string())
        })
    }
}

fn limits_for_course(l: &Limits, g: &Group) -> Limits {
    match &g.body {
        Body::Course(c) => Limits {
            max_units: l.max_units.max(c.pieces.len() + 1),
            trim: l.trim,
        },
        _ => *l,
    }
}

fn unit_extent(pattern: &Pattern) -> f64 {
    pattern.compile().map_or(1000.0, |c| {
        c.kinds
            .iter()
            .map(|k| (k.hi - k.lo).max_element())
            .fold(0.0, f64::max)
    })
}

// ---------------------------------------------------------------------------
// SVG

/// What a render tells the drawing of the paved areas: the region it shows and the options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgContext {
    /// The region of the drawing in the render (`None`: the whole plan).
    pub view: Option<BBox>,
    /// Mark the findings of the pattern check.
    pub show_check: bool,
    /// Most units in the region for the real units.
    pub bound: usize,
}

impl Default for SvgContext {
    fn default() -> Self {
        SvgContext {
            view: None,
            show_check: false,
            bound: SVG_BOUND,
        }
    }
}

thread_local! {
    static CONTEXT: Cell<Option<SvgContext>> = const { Cell::new(None) };
}

/// The context of the render that runs now on this thread (the default outside a render).
pub fn context() -> SvgContext {
    CONTEXT.with(|c| c.get()).unwrap_or_default()
}

/// Runs `f` with a context. The context before is back afterwards.
pub fn with_context<R>(ctx: SvgContext, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<SvgContext>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CONTEXT.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(CONTEXT.with(|c| c.replace(Some(ctx))));
    f()
}

/// A number for an SVG path: millimeters to 0.001, without trailing zeros.
fn num(x: f64) -> String {
    let s = format!("{x:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

fn ring_d(ring: &[DVec2], d: &mut String) {
    for (i, p) in ring.iter().enumerate() {
        let _ = write!(
            d,
            "{}{} {} ",
            if i == 0 { "M " } else { "L " },
            num(p.x),
            num(-p.y)
        );
    }
    d.push_str("Z ");
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The color of a finding on the plan.
pub fn finding_color(kind: check::Kind) -> &'static str {
    match kind {
        check::Kind::Overlap | check::Kind::OutsideArea => "rgb(220,30,30)",
        check::Kind::JointTooNarrow | check::Kind::JointTooWide => "rgb(235,130,20)",
        check::Kind::SmallPiece | check::Kind::WrongSize => "rgb(160,40,200)",
        check::Kind::Coverage => "rgb(20,110,220)",
    }
}

/// The SVG of the pattern plan of a hardscape. `scale` is the drawing scale of the layer (1 mm of
/// line weight on paper is `scale` mm of the model), `stroke` the pen of the object, `color` the
/// color of the pattern lines (the pen when none).
///
/// Per group a `<g data-part="group">` holds `data-whole` and `data-cut` (the units drawn), the
/// whole units as the subpaths of one `<path data-part="units-whole">` and each cut unit as a
/// `<path data-part="unit-cut">` with a light fill (18% of the pen). A preview has the gap lines
/// as `<path data-part="joints">`, `data-detail="preview"` on the outer group and a text with
/// [`PREVIEW_LABEL`].
pub fn svg(
    doc: &Document,
    obj: &Object,
    scale: f64,
    stroke: &str,
    color: Option<String>,
    pattern_id: &str,
) -> String {
    let ctx = context();
    let Some(prepared) = prepared(doc, obj) else {
        return String::new();
    };
    let mut out = String::new();
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            // Nothing to draw, and the reason is in the picture for a person who looks.
            let _ = writeln!(
                out,
                r#"<g data-part="pattern" data-of="{}" data-detail="none" data-error="{}"/>"#,
                obj.id,
                escape(&e)
            );
            return out;
        }
    };
    let drawn = prepared.draw(ctx.view, &DrawOptions::units(ctx.bound));
    let color = color.unwrap_or_else(|| stroke.to_string());
    let hair = 0.1 * scale;
    let detail = if drawn.is_preview() {
        "preview"
    } else {
        "full"
    };
    let _ = writeln!(
        out,
        r#"<g data-part="pattern" data-of="{}" data-pattern="{pattern_id}" data-detail="{detail}">"#,
        obj.id
    );
    for g in &drawn.groups {
        let role = g.role.map_or("field", |r| match r {
            Role::Field => "field",
            Role::Zone => "zone",
            Role::Course => "course",
        });
        let _ = writeln!(
            out,
            r#"<g data-part="group" data-group="{}" data-role="{role}" data-whole="{}" data-cut="{}">"#,
            escape(&g.name),
            g.whole_count,
            g.cut_count
        );
        if !g.joints.is_empty() {
            let mut d = String::new();
            for (a, b) in &g.joints {
                let _ = write!(
                    d,
                    "M {} {} L {} {} ",
                    num(a.x),
                    num(-a.y),
                    num(b.x),
                    num(-b.y)
                );
            }
            // The width of a joint is the width of its line in the preview, as it was.
            let width = joint_width(&prepared, g).max(hair);
            let _ = writeln!(
                out,
                r#"<path d="{}" fill="none" stroke="{color}" stroke-width="{width}" data-part="joints" data-of="{}" data-pattern="{pattern_id}"/>"#,
                d.trim_end(),
                obj.id
            );
        }
        if !g.whole.is_empty() {
            let mut d = String::new();
            for ring in &g.whole {
                ring_d(ring, &mut d);
            }
            let _ = writeln!(
                out,
                r#"<path d="{}" fill="none" stroke="{color}" stroke-width="{hair}" data-part="units-whole" data-of="{}"/>"#,
                d.trim_end(),
                obj.id
            );
        }
        for u in &g.cut {
            let mut d = String::new();
            for ring in u {
                ring_d(ring, &mut d);
            }
            let _ = writeln!(
                out,
                r#"<path d="{}" fill="{color}" fill-opacity="0.18" fill-rule="evenodd" stroke="{color}" stroke-width="{hair}" data-part="unit-cut" data-of="{}"/>"#,
                d.trim_end(),
                obj.id
            );
        }
        out.push_str("</g>\n");
    }
    if !drawn.zone_edges.is_empty() {
        let mut d = String::new();
        for ring in &drawn.zone_edges {
            ring_d(ring, &mut d);
        }
        let _ = writeln!(
            out,
            r#"<path d="{}" fill="none" stroke="{color}" stroke-width="{}" stroke-dasharray="{} {}" data-part="zone-edges" data-of="{}"/>"#,
            d.trim_end(),
            hair,
            4.0 * scale,
            2.0 * scale,
            obj.id
        );
    }
    if let Detail::Preview {
        units_in_view,
        bound,
        truncated,
    } = &drawn.detail
    {
        let (lo, hi) = ctx.view.map_or(prepared.bounds(), |v| {
            let (a, b) = prepared.bounds();
            (a.max(v.0), b.min(v.1))
        });
        let (lo, hi) = if lo.x <= hi.x && lo.y <= hi.y {
            (lo, hi)
        } else {
            prepared.bounds()
        };
        let _ = writeln!(
            out,
            r#"<text x="{}" y="{}" font-size="{}" font-family="sans-serif" fill="{color}" data-part="preview-note" data-units-in-view="{units_in_view}" data-bound="{bound}" data-truncated="{truncated}">{}</text>"#,
            num(lo.x + 2.0 * scale),
            num(-hi.y + 5.0 * scale),
            3.5 * scale,
            PREVIEW_LABEL
        );
    }
    if ctx.show_check {
        out.push_str(&check_svg(&prepared, ctx.view, scale, obj));
    }
    out.push_str("</g>\n");
    out
}

/// The width of the joint line of a group in a preview: the joint of its pattern.
fn joint_width(prepared: &Prepared, g: &DrawnGroup) -> f64 {
    prepared
        .planned
        .groups
        .iter()
        .find(|p| p.name == g.name)
        .map_or(0.0, Group::gap)
}

/// The markers of the findings of the check, in view.
pub fn check_svg(prepared: &Prepared, view: Option<BBox>, scale: f64, obj: &Object) -> String {
    let mut out = String::new();
    match prepared.findings() {
        Err(e) => {
            let _ = writeln!(
                out,
                r#"<g data-part="check" data-of="{}" data-error="{}"/>"#,
                obj.id,
                escape(e)
            );
        }
        Ok(r) => {
            let _ = writeln!(
                out,
                r#"<g data-part="check" data-of="{}" data-findings="{}">"#,
                obj.id,
                r.findings.len()
            );
            let radius = 4.0 * scale;
            for f in &r.findings {
                let at = DVec2::new(f.at[0], f.at[1]);
                if view.is_some_and(|(lo, hi)| {
                    at.x < lo.x || at.x > hi.x || at.y < lo.y || at.y > hi.y
                }) {
                    continue;
                }
                let color = finding_color(f.kind);
                let _ = writeln!(
                    out,
                    r#"<circle cx="{}" cy="{}" r="{radius}" fill="none" stroke="{color}" stroke-width="{}" data-part="finding" data-kind="{}"><title>{}</title></circle>"#,
                    num(at.x),
                    num(-at.y),
                    0.35 * scale,
                    f.kind.id(),
                    escape(&f.message)
                );
            }
            out.push_str("</g>\n");
        }
    }
    out
}

#[cfg(test)]
mod tests;
