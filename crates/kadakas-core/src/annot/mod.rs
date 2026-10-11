//! Annotation: dimensions, styled text, callouts, data tags, legends, north arrows, scale bars and
//! revision clouds. An annotation is an object (`Shape::Annotation`) on a design layer or a sheet
//! layer, or an item in the annotation space of a viewport (`AnnotationItem::Shape`).
//!
//! - [`objects`] has the data, [`style`] the resources (text styles, dimension styles, tag styles
//!   and notes databases), [`dim`] the measuring and drawing of dimensions, [`draw`] the drawing of
//!   all annotations as primitives, and [`sync`] the update of dimensions and tags when objects move.
//! - Sizes are paper mm. On a design layer they are multiplied by the scale of the layer. In a
//!   viewport the positions are paper mm already, and lengths that stand for real distances
//!   (dimension values, the scale bar) use the scale of the viewport (see [`draw::Ctx`]).
//! - Text that needs the worksheet expressions (data tags, legends) comes through the
//!   [`draw::TextSource`] trait. The crate `kadakas-annot` implements it with the worksheet code.

pub mod dim;
pub mod draw;
pub mod objects;
pub mod style;
pub mod sync;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::model::{Document, MAX_COORD, ObjectId, Shape};
use crate::resource::ResourceId;

pub use objects::*;

/// An annotation object. The `type` key in JSON names the variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Annot {
    Dimension(Dimension),
    Text(StyledText),
    Callout(Callout),
    DataTag(DataTag),
    Legend(Legend),
    NorthArrow(NorthArrow),
    ScaleBar(ScaleBar),
    RevisionCloud(RevisionCloud),
}

/// The JSON names of the annotation types, as `Annot::type_name` gives them.
pub const TYPE_NAMES: [&str; 8] = [
    "dimension",
    "text",
    "callout",
    "data_tag",
    "legend",
    "north_arrow",
    "scale_bar",
    "revision_cloud",
];

/// Longest text in an annotation, in characters.
pub const MAX_TEXT_CHARS: usize = 10_000;

impl Annot {
    pub fn type_name(&self) -> &'static str {
        match self {
            Annot::Dimension(_) => TYPE_NAMES[0],
            Annot::Text(_) => TYPE_NAMES[1],
            Annot::Callout(_) => TYPE_NAMES[2],
            Annot::DataTag(_) => TYPE_NAMES[3],
            Annot::Legend(_) => TYPE_NAMES[4],
            Annot::NorthArrow(_) => TYPE_NAMES[5],
            Annot::ScaleBar(_) => TYPE_NAMES[6],
            Annot::RevisionCloud(_) => TYPE_NAMES[7],
        }
    }

    /// Calls `f` with every number of the annotation.
    pub fn numbers(&self, f: &mut dyn FnMut(f64)) {
        let mut v = |p: &DVec2| {
            f(p.x);
            f(p.y);
        };
        match self {
            Annot::Dimension(d) => {
                d.points.iter().for_each(&mut v);
                f(d.offset);
                d.area.into_iter().for_each(&mut *f);
            }
            Annot::Text(t) => v(&t.at),
            Annot::Callout(c) => {
                v(&c.at);
                c.leaders.iter().flat_map(|l| &l.points).for_each(&mut v);
                f(c.shoulder_mm);
            }
            Annot::DataTag(t) => {
                v(&t.offset);
                v(&t.at);
                v(&t.anchor);
            }
            Annot::Legend(l) => {
                v(&l.at);
                f(l.swatch_mm);
            }
            Annot::NorthArrow(n) => {
                v(&n.at);
                f(n.size_mm);
                f(n.rotation_deg);
            }
            Annot::ScaleBar(s) => {
                v(&s.at);
                f(s.segment);
                f(s.height_mm);
                s.scale.into_iter().for_each(&mut *f);
            }
            Annot::RevisionCloud(r) => {
                r.points.iter().for_each(&mut v);
                f(r.bump_mm);
            }
        }
    }

    pub fn is_finite(&self) -> bool {
        let mut ok = true;
        self.numbers(&mut |x| ok &= x.is_finite());
        ok
    }

    pub fn is_in_range(&self) -> bool {
        let mut ok = true;
        self.numbers(&mut |x| ok &= x.abs() <= MAX_COORD);
        ok
    }

    /// Moves the annotation. Anchors stay. The commands tie anchored points to their objects again.
    pub fn translate(&mut self, d: DVec2) {
        match self {
            Annot::Dimension(x) => x.points.iter_mut().for_each(|p| *p += d),
            Annot::Text(t) => t.at += d,
            Annot::Callout(c) => {
                c.at += d;
                c.leaders
                    .iter_mut()
                    .flat_map(|l| l.points.iter_mut())
                    .for_each(|p| *p += d);
            }
            Annot::DataTag(t) => {
                t.at += d;
                t.anchor += d;
            }
            Annot::Legend(l) => l.at += d,
            Annot::NorthArrow(n) => n.at += d,
            Annot::ScaleBar(s) => s.at += d,
            Annot::RevisionCloud(r) => r.points.iter_mut().for_each(|p| *p += d),
        }
    }

    /// Moves every stored model point through `f` (a change of the CRS, for example). Paper
    /// sizes and offsets stay. A dimension measures its points again when it is drawn, so its text
    /// follows; a typed text stays as it was typed.
    pub fn map_points(&mut self, f: &mut dyn FnMut(DVec2) -> DVec2) {
        match self {
            Annot::Dimension(x) => x.points.iter_mut().for_each(|p| *p = f(*p)),
            Annot::Text(t) => t.at = f(t.at),
            Annot::Callout(c) => {
                c.at = f(c.at);
                c.leaders
                    .iter_mut()
                    .flat_map(|l| l.points.iter_mut())
                    .for_each(|p| *p = f(*p));
            }
            Annot::DataTag(t) => {
                t.at = f(t.at);
                t.anchor = f(t.anchor);
            }
            Annot::Legend(l) => l.at = f(l.at),
            Annot::NorthArrow(n) => n.at = f(n.at),
            Annot::ScaleBar(s) => s.at = f(s.at),
            Annot::RevisionCloud(r) => r.points.iter_mut().for_each(|p| *p = f(*p)),
        }
    }

    /// The box around the stored points. Text and sizes in paper mm are not counted, because they
    /// depend on the scale (see `draw::prims` for the real extent).
    pub fn bounds(&self) -> (DVec2, DVec2) {
        let boxed = |pts: &mut dyn Iterator<Item = DVec2>| {
            pts.fold(
                (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(p), hi.max(p)),
            )
        };
        let (lo, hi) = match self {
            Annot::Dimension(d) => boxed(&mut d.points.iter().copied()),
            Annot::Text(t) => boxed(&mut std::iter::once(t.at)),
            Annot::Callout(c) => boxed(
                &mut std::iter::once(c.at)
                    .chain(c.leaders.iter().flat_map(|l| l.points.iter().copied())),
            ),
            Annot::DataTag(t) => boxed(&mut [t.at, t.anchor].into_iter()),
            Annot::Legend(l) => boxed(&mut std::iter::once(l.at)),
            Annot::NorthArrow(n) => boxed(&mut std::iter::once(n.at)),
            Annot::ScaleBar(s) => boxed(&mut std::iter::once(s.at)),
            Annot::RevisionCloud(r) => boxed(&mut r.points.iter().copied()),
        };
        if lo.is_finite() && hi.is_finite() {
            (lo, hi)
        } else {
            (DVec2::ZERO, DVec2::ZERO)
        }
    }

    /// Why the annotation is not valid, or `None`. This checks numbers and counts, not references.
    pub fn problem(&self) -> Option<String> {
        if !self.is_finite() {
            return Some("numbers must be finite".into());
        }
        let long = |s: &str, max: usize| s.chars().count() > max;
        match self {
            Annot::Dimension(d) => {
                let (lo, hi) = d.mode.point_range();
                if d.points.len() < lo || d.points.len() > hi {
                    return Some(if lo == hi {
                        format!("a {} dimension needs {lo} points", d.mode.name())
                    } else {
                        format!("a {} dimension needs {lo} to {hi} points", d.mode.name())
                    });
                }
                if d.anchors.len() > d.points.len() {
                    return Some("a dimension has at most one anchor per point".into());
                }
                if matches!(d.mode, DimMode::Radial | DimMode::Diameter)
                    && d.points[0] == d.points[1]
                {
                    return Some("the radius must be positive".into());
                }
                if d.mode == DimMode::Area && d.area.is_some_and(|a| a < 0.0) {
                    return Some("the area cannot be negative".into());
                }
                if d.text.as_deref().is_some_and(|t| long(t, 1000)) {
                    return Some("dimension text is too long".into());
                }
            }
            Annot::Text(t) if long(&t.text, MAX_TEXT_CHARS) => {
                return Some("text is too long".into());
            }
            Annot::Callout(c) => {
                if c.leaders.is_empty() || c.leaders.len() > 20 {
                    return Some("a callout has 1 to 20 leaders".into());
                }
                if c.leaders
                    .iter()
                    .any(|l| l.points.is_empty() || l.points.len() > 20)
                {
                    return Some("a leader has 1 to 20 points".into());
                }
                if !(0.0..=100.0).contains(&c.shoulder_mm) {
                    return Some("the shoulder is 0 to 100 mm".into());
                }
                if long(&c.text, MAX_TEXT_CHARS) || c.note.as_deref().is_some_and(|n| long(n, 200))
                {
                    return Some("text is too long".into());
                }
            }
            Annot::Legend(l) => {
                if !(1.0..=50.0).contains(&l.swatch_mm) {
                    return Some("the swatch size is 1 to 50 mm".into());
                }
                if long(&l.criteria, 2000) || long(&l.group_by, 2000) || long(&l.title, 500) {
                    return Some("legend text is too long".into());
                }
            }
            Annot::NorthArrow(n) if !(2.0..=300.0).contains(&n.size_mm) => {
                return Some("the north arrow size is 2 to 300 mm".into());
            }
            Annot::ScaleBar(s) => {
                if s.segment <= 0.0 || s.segments == 0 || s.segments > 20 {
                    return Some(
                        "a scale bar needs a segment length above zero and 1 to 20 segments".into(),
                    );
                }
                if !(0.5..=50.0).contains(&s.height_mm) {
                    return Some("the scale bar height is 0.5 to 50 mm".into());
                }
                if s.scale.is_some_and(|x| x <= 0.0 || x > 1.0e6) {
                    return Some("the scale must be a positive denominator".into());
                }
            }
            Annot::RevisionCloud(r) => {
                if r.points.len() < 3 || r.points.len() > 1000 {
                    return Some("a revision cloud needs 3 to 1000 points".into());
                }
                if !(1.0..=100.0).contains(&r.bump_mm) {
                    return Some("the bump size is 1 to 100 mm".into());
                }
                if [&r.revision, &r.date, &r.description, &r.author]
                    .iter()
                    .any(|s| long(s, 500))
                {
                    return Some("revision text is too long".into());
                }
            }
            _ => {}
        }
        None
    }

    /// The objects that this annotation follows.
    pub fn depends_on(&self) -> Vec<ObjectId> {
        match self {
            Annot::Dimension(d) => d.anchors.iter().flatten().map(|a| a.object).collect(),
            Annot::Callout(c) => c
                .leaders
                .iter()
                .filter_map(|l| l.anchor)
                .map(|a| a.object)
                .collect(),
            Annot::DataTag(t) => vec![t.object],
            _ => Vec::new(),
        }
    }

    /// True if the annotation refers to the resource.
    pub fn uses_resource(&self, id: ResourceId) -> bool {
        match self {
            Annot::Dimension(d) => d.style == Some(id),
            Annot::Text(t) => t.style == Some(id),
            Annot::Callout(c) => c.style == Some(id) || c.notes_db == Some(id),
            Annot::DataTag(t) => t.style == id,
            Annot::Legend(l) => l.style == Some(id),
            Annot::ScaleBar(s) => s.style == Some(id),
            Annot::NorthArrow(_) | Annot::RevisionCloud(_) => false,
        }
    }

    /// Makes the annotation use the resource `to` where it used `from`.
    pub fn replace_resource(&mut self, from: ResourceId, to: ResourceId) {
        let swap = |s: &mut Option<ResourceId>| {
            if *s == Some(from) {
                *s = Some(to);
            }
        };
        match self {
            Annot::Dimension(d) => swap(&mut d.style),
            Annot::Text(t) => swap(&mut t.style),
            Annot::Callout(c) => {
                swap(&mut c.style);
                swap(&mut c.notes_db);
            }
            Annot::DataTag(t) if t.style == from => t.style = to,
            Annot::DataTag(_) => {}
            Annot::Legend(l) => swap(&mut l.style),
            Annot::ScaleBar(s) => swap(&mut s.style),
            Annot::NorthArrow(_) | Annot::RevisionCloud(_) => {}
        }
    }

    /// Checks that the resources, the tagged object and the anchors exist, and that the
    /// resources have the right type.
    pub fn check_refs(&self, doc: &Document) -> Result<(), String> {
        let res = &doc.resources;
        let need = |ok: bool, id: ResourceId, what: &str| {
            if ok {
                Ok(())
            } else if res.get(id).is_some() {
                Err(format!("resource {id} is not a {what}"))
            } else {
                Err(format!("resource {id} does not exist"))
            }
        };
        match self {
            Annot::Dimension(d) => {
                if let Some(s) = d.style {
                    need(res.dimension_style(s).is_some(), s, "dimension style")?;
                }
            }
            Annot::Text(t) => {
                if let Some(s) = t.style {
                    need(res.text_style(s).is_some(), s, "text style")?;
                }
            }
            Annot::Callout(c) => {
                if let Some(s) = c.style {
                    need(res.text_style(s).is_some(), s, "text style")?;
                }
                if let Some(s) = c.notes_db {
                    need(res.notes_db(s).is_some(), s, "notes database")?;
                }
                if c.note.is_some() && c.notes_db.is_none() {
                    return Err("a callout with a note key needs a notes database".into());
                }
            }
            Annot::DataTag(t) => {
                need(res.tag_style(t.style).is_some(), t.style, "tag style")?;
                match doc.object(t.object) {
                    None => return Err(format!("object {} does not exist", t.object)),
                    Some(o)
                        if matches!(
                            o.shape,
                            Shape::Annotation(_) | Shape::Viewport(_) | Shape::TitleBlock(_)
                        ) =>
                    {
                        return Err("this object cannot be tagged".into());
                    }
                    Some(_) => {}
                }
            }
            Annot::Legend(l) => {
                if let Some(s) = l.style {
                    need(res.text_style(s).is_some(), s, "text style")?;
                }
            }
            Annot::ScaleBar(b) => {
                if let Some(s) = b.style {
                    need(res.text_style(s).is_some(), s, "text style")?;
                }
            }
            Annot::NorthArrow(_) | Annot::RevisionCloud(_) => {}
        }
        for id in self.depends_on() {
            if self.is_dimension_or_callout() && doc.object(id).is_none() {
                return Err(format!("object {id} does not exist"));
            }
        }
        Ok(())
    }

    fn is_dimension_or_callout(&self) -> bool {
        matches!(self, Annot::Dimension(_) | Annot::Callout(_))
    }
}

// ---------------------------------------------------------------- anchors

/// The vertices of a shape that an anchor can use (see `AnchorTarget::Vertex`).
pub fn shape_vertices(shape: &Shape) -> Vec<DVec2> {
    use crate::curve;
    match shape {
        Shape::Line { a, b } => vec![*a, *b],
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
        }
        Shape::Circle { center, radius } => vec![
            *center + DVec2::X * *radius,
            *center + DVec2::Y * *radius,
            *center - DVec2::X * *radius,
            *center - DVec2::Y * *radius,
        ],
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = crate::model::arc_sweep(*start, *end);
            vec![
                *center + DVec2::from_angle(*start) * *radius,
                *center + DVec2::from_angle(*start + sweep) * *radius,
                *center + DVec2::from_angle(*start + sweep / 2.0) * *radius,
            ]
        }
        Shape::Polyline { points, .. } => curve::positions(points),
        Shape::LandscapeArea { boundary, .. }
        | Shape::Hardscape { boundary, .. }
        | Shape::SiteModifier { boundary, .. } => curve::positions(boundary),
        Shape::Polyline3d { points, .. } => points.iter().map(|p| p.truncate()).collect(),
        Shape::Plant { at, .. } | Shape::ExistingTree { at, .. } | Shape::Text { at, .. } => {
            vec![*at]
        }
        Shape::Locus3d { p } => vec![p.truncate()],
        _ => Vec::new(),
    }
}

/// The edges of a shape: edge `i` runs from vertex `i` to vertex `i + 1`. Closed shapes wrap around.
pub fn shape_edges(shape: &Shape) -> Vec<(DVec2, DVec2)> {
    let closed = matches!(
        shape,
        Shape::Rect { .. }
            | Shape::LandscapeArea { .. }
            | Shape::Hardscape { .. }
            | Shape::SiteModifier { .. }
            | Shape::Polyline { closed: true, .. }
            | Shape::Polyline3d { closed: true, .. }
    );
    if matches!(shape, Shape::Circle { .. } | Shape::Arc { .. }) {
        return Vec::new();
    }
    let v = shape_vertices(shape);
    let n = v.len();
    if n < 2 {
        return Vec::new();
    }
    let count = if closed { n } else { n - 1 };
    (0..count).map(|i| (v[i], v[(i + 1) % n])).collect()
}

/// The reference point of an object: the center of a circle, an arc or a rectangle, the position
/// of a plant, the middle of a line, or the centroid of an outline. Data tags and anchors use it.
pub fn ref_point(shape: &Shape) -> Option<DVec2> {
    match shape {
        Shape::Circle { center, .. } | Shape::Arc { center, .. } => Some(*center),
        Shape::Rect { min, max } => Some((*min + *max) / 2.0),
        Shape::Line { a, b } => Some((*a + *b) / 2.0),
        Shape::Plant { at, .. }
        | Shape::ExistingTree { at, .. }
        | Shape::Text { at, .. }
        | Shape::PlantTag { at, .. } => Some(*at),
        Shape::Locus3d { p } => Some(p.truncate()),
        Shape::Polyline { .. }
        | Shape::LandscapeArea { .. }
        | Shape::Hardscape { .. }
        | Shape::SiteModifier { .. }
        | Shape::Polyline3d { .. } => {
            let v = shape_vertices(shape);
            if v.is_empty() {
                return None;
            }
            let closed = shape.area().is_some();
            Some(if closed {
                centroid(&v)
            } else {
                v.iter().copied().sum::<DVec2>() / v.len() as f64
            })
        }
        Shape::Annotation(a) => Some(a.bounds().0),
        _ => None,
    }
}

/// The centroid of a polygon (the average of the corners when it has no area).
pub fn centroid(p: &[DVec2]) -> DVec2 {
    let n = p.len();
    let mut a2 = 0.0;
    let mut c = DVec2::ZERO;
    for i in 0..n {
        let (s, t) = (p[i], p[(i + 1) % n]);
        let cross = s.perp_dot(t);
        a2 += cross;
        c += (s + t) * cross;
    }
    if a2.abs() < 1e-9 {
        p.iter().copied().sum::<DVec2>() / n.max(1) as f64
    } else {
        c / (3.0 * a2)
    }
}

/// The position of an anchor in the document, if the object and the part still exist.
pub fn anchor_point(doc: &Document, a: &Anchor) -> Option<DVec2> {
    let shape = &doc.object(a.object)?.shape;
    match a.target {
        AnchorTarget::Vertex { index } => shape_vertices(shape).get(index).copied(),
        AnchorTarget::Edge { index, t } => shape_edges(shape)
            .get(index)
            .map(|(p, q)| *p + (*q - *p) * t.clamp(0.0, 1.0)),
        AnchorTarget::Center => ref_point(shape),
    }
}

#[cfg(test)]
mod tests;
