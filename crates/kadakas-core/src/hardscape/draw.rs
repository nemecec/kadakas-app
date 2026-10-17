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
//! that says so ([`PREVIEW_LABEL`]). A group whose units are smaller than `min_unit_mm` (a pixel
//! limit of the screen, a limit of the paper) is hidden, and the drawing says so; the zone edges and
//! the outlines of the courses are always drawn.
//!
//! The bounds are measured (`measure_bounds` in the tests of this module, and in the app and print
//! crates); the numbers are in the documentation of each constant.
//!
//! The laid plan is kept by a content key (the outline, the holes, the pattern, the plan, the
//! tolerance, the unit limit and the objects that a zone or a course follows), so an edit of
//! something else does not lay the plan again. The view is clipped from the kept plan. A plan can
//! be laid in the background ([`PrepareJob`]) and its check can be stopped ([`Prepared::findings_cancellable`]),
//! so a window never waits for them.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// What a drawing says when it hides the units of a group that are too small to see.
pub const HIDDEN_LABEL: &str = "pattern hidden at this zoom (units too small)";

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
/// Most units of a lattice that zones give to the zone of their center, for which the lattice is
/// laid whole and kept (the zones then read their lists from it). A bigger lattice is laid on the
/// window of the view only: a unit still goes to the zone of its center.
pub const OWNER_LATTICE_MAX: usize = 300_000;

/// The most memory the shared cache of laid plans keeps, as estimated by [`Prepared::approx_bytes`].
pub const CACHE_BYTES: usize = 384 * 1024 * 1024;
/// The most plans the shared cache keeps.
pub const CACHE_PLANS: usize = 48;

thread_local! {
    static PATTERN_UNITS: Cell<usize> = const { Cell::new(layout::MAX_UNITS) };
}

/// Sets the most units that a plan may have in the drawings of the calling thread (the courses of a
/// plan are bound by it, as in the report: `SessionLimits::pattern_units`). The window and the API
/// set it from their session at each call; a render can set it in its [`SvgContext`]. Another
/// thread (the background worker) gets the number with its job.
pub fn set_pattern_units(n: usize) {
    PATTERN_UNITS.with(|p| p.set(n.max(1)));
}

/// The unit limit that a plan is laid with now.
pub fn pattern_units() -> usize {
    context()
        .max_units
        .unwrap_or_else(|| PATTERN_UNITS.with(Cell::get))
}

/// What to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// A group whose smallest unit side is less than this (mm) is hidden: it draws no units and no
    /// lines, and is not counted for the bound. 0: nothing is hidden.
    pub min_unit_mm: f64,
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
            min_unit_mm: 0.0,
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
            min_unit_mm: 0.0,
        }
    }

    /// The same, hiding the groups with units smaller than `mm`.
    pub fn hiding_units_under(self, mm: f64) -> Self {
        DrawOptions {
            min_unit_mm: mm,
            ..self
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
    /// The width of the joints of the group, in mm.
    pub gap: f64,
    /// The smaller side of the unit of the group (the smallest of its unit shapes), in mm.
    pub unit_mm: f64,
    /// The units of the group are smaller than `DrawOptions::min_unit_mm`: no units, no lines.
    pub hidden: bool,
    /// Gap lines (a preview, or `DrawOptions::joints`), and the edges of course units.
    pub joints: Vec<Segment>,
    /// The outline of a course (the area it takes), always drawn.
    pub outline: Vec<Vec<DVec2>>,
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

    /// Some group is hidden because its units are too small (see `DrawOptions::min_unit_mm`).
    pub fn has_hidden(&self) -> bool {
        self.groups.iter().any(|g| g.hidden)
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
    preview: OnceLock<Preview>,
    findings: OnceLock<Result<check::Report, String>>,
}

// ---------------------------------------------------------------------------
// The key and the cache

/// The key of what the drawing depends on: the outline, the holes, the pattern, the plan, the
/// settings, the tolerance and the unit limit, and the objects that a zone or a course follows. Two
/// objects with the same key are drawn the same.
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
    pattern_units().hash(&mut h);
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

type CacheEntry = (u64, Result<Arc<Prepared>, String>, usize);

/// Laid plans by content key, the last used kept, bound by their number and by their estimated
/// memory. A plan that cannot be laid is kept too, with its message.
pub struct PlanCache {
    entries: Mutex<Vec<CacheEntry>>,
    capacity: usize,
    bytes: usize,
    builds: std::sync::atomic::AtomicU64,
}

impl PlanCache {
    pub const fn new(capacity: usize, bytes: usize) -> Self {
        PlanCache {
            entries: Mutex::new(Vec::new()),
            capacity,
            bytes,
            builds: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// How often a plan was laid (a hit does not count). Tests use it to check the work done.
    pub fn builds(&self) -> u64 {
        self.builds.load(Ordering::Relaxed)
    }

    /// How many plans the cache holds, and their estimated memory.
    pub fn size(&self) -> (usize, usize) {
        let c = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        (c.len(), c.iter().map(|e| e.2).sum())
    }

    /// The plan for a key, if it is in the cache (it becomes the most recent).
    pub fn cached(&self, key: u64) -> Option<Result<Arc<Prepared>, String>> {
        let mut c = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let i = c.iter().position(|(k, ..)| *k == key)?;
        let hit = c.remove(i);
        let out = hit.1.clone();
        c.push(hit);
        Some(out)
    }

    /// Puts a plan in the cache. The oldest plans go when there are too many or too big.
    pub fn insert(&self, key: u64, plan: Result<Arc<Prepared>, String>) {
        let size = plan.as_ref().map_or(64, |p| p.approx_bytes());
        let mut c = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        c.retain(|(k, ..)| *k != key);
        c.push((key, plan, size));
        while c.len() > 1
            && (c.len() > self.capacity || c.iter().map(|e| e.2).sum::<usize>() > self.bytes)
        {
            drop(c.remove(0));
        }
    }

    /// The laid plan for a key that [`content_key`] made. It is laid here when it is not in the
    /// cache.
    pub fn get(
        &self,
        doc: &Document,
        obj: &Object,
        key: u64,
    ) -> Option<Result<Arc<Prepared>, String>> {
        layout::object::pattern_of(doc, obj)?;
        if let Some(hit) = self.cached(key) {
            return Some(hit);
        }
        self.builds.fetch_add(1, Ordering::Relaxed);
        let built = Prepared::new(doc, obj, layout::TOLERANCE, key, pattern_units()).map(Arc::new);
        self.insert(key, built.clone());
        Some(built)
    }

    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

static CACHE: PlanCache = PlanCache::new(CACHE_PLANS, CACHE_BYTES);

/// How many plans the shared cache laid since the start of the process (a hit does not count).
pub fn builds() -> u64 {
    CACHE.builds()
}

/// The shared cache of laid plans (tests look at its size).
pub fn cache() -> &'static PlanCache {
    &CACHE
}

/// The plan for a key from the shared cache; nothing is laid.
pub fn cached(key: u64) -> Option<Result<Arc<Prepared>, String>> {
    CACHE.cached(key)
}

/// The laid plan of a hardscape, from the cache when nothing that it depends on changed. `None`
/// if the object has no joint pattern; `Err` says why the plan cannot be laid (the message of the
/// report). This lays the plan on the calling thread when it is not in the cache: a window uses
/// [`prepare_job`] instead.
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

/// The work of laying one plan, with what it needs from the document, so that it can run on
/// another thread. The document that it holds has the styles, the object and the objects that a
/// zone or a course follows, and nothing else.
pub struct PrepareJob {
    doc: Document,
    obj: Object,
    key: u64,
    max_units: usize,
}

impl PrepareJob {
    /// The content key of the plan that the job lays.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// Lays the plan and puts it in the shared cache.
    pub fn run(self) -> Result<Arc<Prepared>, String> {
        if let Some(hit) = CACHE.cached(self.key) {
            return hit;
        }
        CACHE.builds.fetch_add(1, Ordering::Relaxed);
        let built = Prepared::new(
            &self.doc,
            &self.obj,
            layout::TOLERANCE,
            self.key,
            self.max_units,
        )
        .map(|p| {
            // What the first drawing needs is made here, not in the window.
            p.warm();
            Arc::new(p)
        });
        CACHE.insert(self.key, built.clone());
        built
    }
}

/// A job that lays the plan of an object, for the key that [`content_key`] made. `None` if the
/// object has no joint pattern.
pub fn prepare_job(doc: &Document, obj: &Object, key: u64) -> Option<PrepareJob> {
    layout::object::pattern_of(doc, obj)?;
    // The document of the job holds the style of the object and the objects that are followed,
    // not every resource of the drawing.
    let mut small = Document::default();
    if let Some(style) = super::style_id(obj).and_then(|id| doc.resources.get(id)) {
        small.resources.push(style.clone());
    }
    if let Some(p) = plan::data(obj) {
        let followed = p
            .splits
            .iter()
            .filter_map(|s| s.source)
            .chain(p.courses.iter().filter_map(|c| c.source));
        for id in followed {
            if let Some(o) = doc.object(id) {
                small.objects.insert(id, o.clone());
            }
        }
    }
    Some(PrepareJob {
        doc: small,
        obj: obj.clone(),
        key,
        max_units: pattern_units(),
    })
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

/// The smaller side of the unit of a group, in mm: the smallest side of the cells of a pattern, the
/// smaller of the length and the depth of a course.
fn unit_mm_of(g: &Group) -> f64 {
    match &g.body {
        Body::Pattern { pattern, .. } => pattern
            .compile()
            .map(|c| {
                c.kinds
                    .iter()
                    .map(|k| (k.hi - k.lo).min_element())
                    .fold(f64::INFINITY, f64::min)
            })
            .ok()
            .filter(|m| m.is_finite())
            .unwrap_or(0.0),
        Body::Course(c) => c.along.min(c.depth),
    }
}

fn new_group(g: &Group, min_unit_mm: f64) -> DrawnGroup {
    let unit_mm = unit_mm_of(g);
    let outline = match &g.body {
        Body::Course(c) => c.band.iter().flatten().cloned().collect(),
        Body::Pattern { .. } => Vec::new(),
    };
    DrawnGroup {
        name: g.name.clone(),
        role: Some(g.role),
        gap: g.gap(),
        unit_mm,
        hidden: unit_mm < min_unit_mm,
        outline,
        ..Default::default()
    }
}

fn ring_bytes(r: &Region) -> usize {
    r.rings().map(|x| x.len() * 16).sum()
}

impl Prepared {
    /// Lays the plan of an object. `Err`: the message of the report. The object must have a joint
    /// pattern. `max_units` bounds the units of the courses, as in the report.
    pub fn new(
        doc: &Document,
        obj: &Object,
        tolerance: f64,
        key: u64,
        max_units: usize,
    ) -> Result<Prepared, String> {
        let (pattern, place) = layout::object::pattern_of(doc, obj)
            .ok_or_else(|| "the object has no joint pattern".to_string())?;
        let region = layout::object::region_of(doc, obj, tolerance)?;
        let planned = plan::plan_groups(doc, obj, region.clone(), pattern, place, max_units)?;
        Ok(Prepared {
            key,
            planned,
            region,
            preview: OnceLock::new(),
            findings: OnceLock::new(),
        })
    }

    /// About how much memory the plan holds, in bytes: the rings of the regions and the pieces of
    /// the courses, and the laid lattice of the zones that give their units by center.
    pub fn approx_bytes(&self) -> usize {
        let mut n = ring_bytes(&self.region) + 1024;
        for g in &self.planned.groups {
            n += ring_bytes(&g.region);
            match &g.body {
                Body::Course(c) => {
                    n += c
                        .pieces
                        .iter()
                        .map(|p| p.poly.len() * 16 + 64)
                        .sum::<usize>()
                }
                Body::Pattern { pattern, .. } => {
                    if g.owner.is_some() {
                        // A laid unit costs about 200 bytes.
                        n += (estimate(pattern, g.region.area) as usize)
                            .min(OWNER_LATTICE_MAX)
                            .saturating_mul(200);
                    }
                }
            }
        }
        n
    }

    /// About how many units the plan has in `view` (the whole plan for `None`), without the groups
    /// whose units are smaller than `min_unit_mm`. Exact for the units of courses; for the patterns
    /// it is the area of the region in view over the area of a cell, as the engine estimates its
    /// work.
    pub fn units_in(&self, view: Option<BBox>, min_unit_mm: f64) -> u64 {
        let mut total = 0u64;
        for g in &self.planned.groups {
            if unit_mm_of(g) < min_unit_mm {
                continue;
            }
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

    /// Makes what a drawing needs and that takes time: the preview (the gap lines of the whole plan)
    /// and the lattices of the zones that give their units by center, when they are kept. The
    /// background job calls it; a drawing that comes after finds both ready.
    pub fn warm(&self) {
        self.preview();
        let mut seen: Vec<*const Vec<Region>> = Vec::new();
        for g in &self.planned.groups {
            let (Some(o), Some((pattern, place))) = (&g.owner, resolved(g)) else {
                continue;
            };
            let ptr = Arc::as_ptr(&o.zones);
            if seen.contains(&ptr) || estimate(pattern, g.region.area) > OWNER_LATTICE_MAX as u64 {
                continue;
            }
            seen.push(ptr);
            let _ = o.warm(&g.region, pattern, &place, &Limits::default());
        }
    }

    /// The box of the whole plan.
    pub fn bounds(&self) -> BBox {
        (self.region.lo, self.region.hi)
    }

    /// Draws the plan for a view: the real units when `view` holds at most `opts.bound` units,
    /// else the preview. `None` is the whole plan.
    pub fn draw(&self, view: Option<BBox>, opts: &DrawOptions) -> Drawn {
        let est = self.units_in(view, opts.min_unit_mm);
        if est <= opts.bound as u64
            && let Some(d) = self.draw_full(view, opts)
        {
            return d;
        }
        let p = self.preview();
        let groups = p
            .groups
            .iter()
            .map(|g| {
                let mut g = g.clone();
                if g.unit_mm < opts.min_unit_mm {
                    g.hidden = true;
                    g.joints.clear();
                }
                g
            })
            .collect();
        Drawn {
            detail: Detail::Preview {
                units_in_view: est,
                bound: opts.bound,
                truncated: p.truncated,
            },
            groups,
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
                let mut dg = new_group(g, 0.0);
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
        // The window laid one time for all zones of a lattice that is too big to keep.
        let mut windows: Vec<(*const Vec<Region>, Vec<Vec<layout::Unit>>)> = Vec::new();
        for g in &self.planned.groups {
            let mut dg = new_group(g, opts.min_unit_mm);
            if let Some(v) = view {
                dg.outline.retain(|r| boxes_meet(box_of_ring(r), v));
            }
            if dg.hidden {
                groups.push(dg);
                continue;
            }
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
                            Some(o)
                                if estimate(pattern, g.region.area) > OWNER_LATTICE_MAX as u64 =>
                            {
                                // A lattice too big to keep: lay the window, and give each unit to
                                // the zone of its center.
                                view?;
                                let ptr = Arc::as_ptr(&o.zones);
                                if !windows.iter().any(|w| w.0 == ptr) {
                                    let per_zone =
                                        o.units_near_all(&window, pattern, &place, &limits).ok()?;
                                    windows.push((ptr, per_zone));
                                }
                                if let Some((_, per_zone)) = windows.iter().find(|w| w.0 == ptr) {
                                    for u in &per_zone[o.index] {
                                        take(u, &mut dg, false);
                                    }
                                }
                            }
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

    /// The findings of the pattern check (`unit_pattern.check` with its default options), laid
    /// once. This waits for the check; a window uses [`Prepared::findings_cancellable`] on another
    /// thread and [`Prepared::findings_ready`].
    pub fn findings(&self) -> &Result<check::Report, String> {
        self.findings.get_or_init(|| {
            check::check(&self.planned.groups, &self.region, &CheckOptions::default())
                .map_err(|e| e.to_string())
        })
    }

    /// The findings, if they were made already.
    pub fn findings_ready(&self) -> Option<&Result<check::Report, String>> {
        self.findings.get()
    }

    /// Makes the findings, and stops when `cancel` is set (`None` then, nothing is kept).
    pub fn findings_cancellable(
        &self,
        cancel: &AtomicBool,
    ) -> Option<&Result<check::Report, String>> {
        if let Some(r) = self.findings.get() {
            return Some(r);
        }
        let made = check::check_cancellable(
            &self.planned.groups,
            &self.region,
            &CheckOptions::default(),
            cancel,
        );
        match made {
            Err(layout::LayoutError::Invalid(m)) if m == check::CANCELLED => None,
            other => {
                let _ = self.findings.set(other.map_err(|e| e.to_string()));
                self.findings.get()
            }
        }
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

/// What the findings of the check say about their number: the total of each kind (exact), how many
/// are listed, and the kinds whose list was cut (at most `CheckOptions::max_findings`, 200, of one
/// kind are listed).
pub fn findings_summary(r: &check::Report) -> (usize, usize, Vec<String>) {
    (
        r.counts.values().sum(),
        r.findings.len(),
        r.truncated.clone(),
    )
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
    /// Pixels of the picture for a mm of the model (0: not known). The notes in the picture are
    /// 12 px high.
    pub px_per_mm: f64,
    /// The limit of units of a plan (`SessionLimits::pattern_units`); `None`: the limit set with
    /// [`set_pattern_units`].
    pub max_units: Option<usize>,
}

impl Default for SvgContext {
    fn default() -> Self {
        SvgContext {
            view: None,
            show_check: false,
            bound: SVG_BOUND,
            px_per_mm: 0.0,
            max_units: None,
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
        check::Kind::JointTooNarrow | check::Kind::JointTooWide | check::Kind::ZoneEdgeJoint => {
            "rgb(235,130,20)"
        }
        check::Kind::SmallPiece | check::Kind::WrongSize => "rgb(160,40,200)",
        check::Kind::Coverage => "rgb(20,110,220)",
    }
}

/// The height of a note in the picture, in model mm: 12 px when the scale of the picture is known,
/// else 3.5 mm of paper.
fn note_height(ctx: &SvgContext, scale: f64) -> f64 {
    if ctx.px_per_mm > 0.0 {
        12.0 / ctx.px_per_mm
    } else {
        3.5 * scale
    }
}

/// A note in the picture, at the top left of the part of the box `bounds` that is in view.
#[allow(clippy::too_many_arguments)]
fn note(
    out: &mut String,
    ctx: &SvgContext,
    scale: f64,
    bounds: BBox,
    color: &str,
    part: &str,
    attrs: &str,
    line: usize,
    text: &str,
) {
    let h = note_height(ctx, scale);
    let (lo, hi) = ctx
        .view
        .map_or(bounds, |v| (bounds.0.max(v.0), bounds.1.min(v.1)));
    let (lo, hi) = if lo.x <= hi.x && lo.y <= hi.y {
        (lo, hi)
    } else {
        bounds
    };
    let _ = writeln!(
        out,
        r#"<text x="{}" y="{}" font-size="{}" font-family="sans-serif" fill="{color}" data-part="{part}"{attrs}>{}</text>"#,
        num(lo.x + 0.5 * h),
        num(-hi.y + 1.3 * h * (line as f64 + 1.0)),
        num(h),
        escape(text)
    );
}

/// The SVG of the pattern plan of a hardscape. `scale` is the drawing scale of the layer (1 mm of
/// line weight on paper is `scale` mm of the model), `stroke` the pen of the object, `color` the
/// color of the pattern lines (the pen when none).
///
/// Per group a `<g data-part="group">` holds `data-whole` and `data-cut` (the units drawn), the
/// whole units as the subpaths of one `<path data-part="units-whole">` and each cut unit as a
/// `<path data-part="unit-cut">` with a light fill (18% of the pen). A course has its outline as
/// `<path data-part="course-outline">`. A preview has the gap lines as `<path data-part="joints">`,
/// `data-detail="preview"` on the outer group and a text with [`PREVIEW_LABEL`] that is 12 px high
/// in the picture. A plan that cannot be laid has a text with the reason.
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
    let color = color.unwrap_or_else(|| stroke.to_string());
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            // Nothing to draw, and the reason is in the picture for a person who looks.
            let (lo, hi) = doc.object_bounds(obj);
            if lo.is_finite() && hi.is_finite() {
                let _ = writeln!(
                    out,
                    r#"<g data-part="pattern" data-of="{}" data-detail="none" data-error="{}">"#,
                    obj.id,
                    escape(&e)
                );
                note(
                    &mut out,
                    &ctx,
                    scale,
                    (lo, hi),
                    &color,
                    "pattern-error",
                    "",
                    0,
                    &format!("Pattern not drawn: {e}"),
                );
                out.push_str("</g>\n");
            }
            return out;
        }
    };
    let drawn = prepared.draw(ctx.view, &DrawOptions::units(ctx.bound));
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
        // The units show a course; its band is drawn when the units are hidden.
        if g.hidden && !g.outline.is_empty() {
            let mut d = String::new();
            for ring in &g.outline {
                ring_d(ring, &mut d);
            }
            let _ = writeln!(
                out,
                r#"<path d="{}" fill="none" stroke="{color}" stroke-width="{hair}" data-part="course-outline" data-of="{}"/>"#,
                d.trim_end(),
                obj.id
            );
        }
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
            let width = g.gap.max(hair);
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
        note(
            &mut out,
            &ctx,
            scale,
            prepared.bounds(),
            &color,
            "preview-note",
            &format!(
                r#" data-units-in-view="{units_in_view}" data-bound="{bound}" data-truncated="{truncated}""#
            ),
            0,
            PREVIEW_LABEL,
        );
    }
    if ctx.show_check {
        out.push_str(&check_svg(&prepared, ctx.view, scale, obj));
    }
    out.push_str("</g>\n");
    out
}

/// The markers of the findings of the check, in view. The group says how many findings there are
/// (`data-findings`, exact), how many are listed (`data-listed`, at most 200 of each kind), the kinds
/// whose list was cut (`data-cut-kinds`) and the options of the check (`data-options="default"`:
/// the defaults of `unit_pattern.check`, not the options of an earlier call).
pub fn check_svg(prepared: &Prepared, view: Option<BBox>, scale: f64, obj: &Object) -> String {
    let mut out = String::new();
    match prepared.findings() {
        Err(e) => {
            let _ = writeln!(
                out,
                r#"<g data-part="check" data-of="{}" data-options="default" data-error="{}"/>"#,
                obj.id,
                escape(e)
            );
        }
        Ok(r) => {
            let (total, listed, cut) = findings_summary(r);
            let _ = writeln!(
                out,
                r#"<g data-part="check" data-of="{}" data-options="default" data-findings="{total}" data-listed="{listed}" data-cut-kinds="{}">"#,
                obj.id,
                cut.join(",")
            );
            let _ = writeln!(
                out,
                "<title>Pattern check with the default options: {total} findings, {listed} marked{}</title>",
                if cut.is_empty() {
                    String::new()
                } else {
                    format!(" (the list of {} was cut)", cut.join(", "))
                }
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
