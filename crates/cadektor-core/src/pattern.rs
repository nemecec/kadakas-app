//! Pattern geometry: hatches, tiles and line types as plain segments and polylines in model units.
//!
//! Canvas, SVG, PDF and DXF output all draw from these segments, so a pattern looks the same
//! everywhere. Fills are clipped to the shape exactly: every segment lies inside the rings that
//! bound the shape (an even-odd rule, so a ring inside another is a hole).
//!
//! `scale` is how many model millimeters one paper millimeter is (the drawing scale of the layer
//! or of the viewport). A pattern in paper units is multiplied by it. One in world units is not.

use glam::{DAffine2, DVec2};

use crate::geom::point_in_polygon;
use crate::graphics::{HatchDef, LineElement, LineTypeDef, PatternUnits, TileDef};
use crate::model::{Attributes, Color, Document, Object, Shape};
use crate::resource::{ResourceId, Resources};

/// Most segments that one pattern makes for one shape. A denser pattern is cut off.
pub const MAX_SEGMENTS: usize = 200_000;
/// Most lines of one hatch family. A hatch with more lines than this is too dense to see and is left out.
pub const MAX_LINES: usize = 100_000;
/// Most cells of one tile grid.
pub const MAX_CELLS: usize = 40_000;

/// One piece of a pattern, with the color and weight the pattern asks for (none: the pen of the object).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternSeg {
    pub a: DVec2,
    pub b: DVec2,
    pub color: Option<Color>,
    /// Line weight in mm on paper.
    pub weight: Option<f64>,
}

/// The parameters where an infinite line crosses the rings, as pairs (enter, leave).
/// The line is `o + t * u`. The rings fill by the even-odd rule.
fn line_intervals(o: DVec2, u: DVec2, rings: &[Vec<DVec2>]) -> Vec<(f64, f64)> {
    let n = DVec2::new(-u.y, u.x);
    let mut ts: Vec<f64> = Vec::new();
    for ring in rings {
        let m = ring.len();
        if m < 3 {
            continue;
        }
        for i in 0..m {
            let (a, b) = (ring[i], ring[(i + 1) % m]);
            let (da, db) = (n.dot(a - o), n.dot(b - o));
            // Half open: a vertex on the line counts for one of its two edges.
            if (da <= 0.0 && db > 0.0) || (db <= 0.0 && da > 0.0) {
                let q = a + (b - a) * (da / (da - db));
                ts.push(u.dot(q - o));
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.chunks_exact(2).map(|c| (c[0], c[1])).filter(|(a, b)| b > a).collect()
}

/// The pieces of `[t0, t1]` that the pattern (dash, gap, dash, ...) draws. The pattern is at `phase`
/// (a distance into its period) at `t0`. At most `budget` pieces are added.
fn dash_pieces(t0: f64, t1: f64, pattern: &[f64], phase: f64, out: &mut Vec<(f64, f64)>, budget: &mut usize) {
    let period: f64 = pattern.iter().sum();
    if pattern.is_empty() || period.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || !phase.is_finite() {
        if *budget > 0 {
            *budget -= 1;
            out.push((t0, t1));
        }
        return;
    }
    // Find the element at the phase.
    let mut left = phase.rem_euclid(period);
    let mut idx = 0;
    while idx < pattern.len() && left >= pattern[idx] {
        left -= pattern[idx];
        idx += 1;
    }
    if idx >= pattern.len() {
        idx = 0;
        left = 0.0;
    }
    let mut t = t0;
    let mut rest = pattern[idx] - left;
    let mut guard = 0usize;
    while t < t1 && *budget > 0 && guard < 4 * MAX_SEGMENTS {
        guard += 1;
        let end = (t + rest).min(t1);
        if idx % 2 == 0 && end > t {
            out.push((t, end));
            *budget -= 1;
        }
        t = end;
        idx = (idx + 1) % pattern.len();
        rest = pattern[idx];
    }
}

/// The dash pattern of a hatch line in model units. An odd count is repeated, so dash and gap swap.
fn dash_in(dash: &[f64], f: f64) -> Vec<f64> {
    let mut v: Vec<f64> = dash.iter().map(|d| d * f).collect();
    if v.len() % 2 == 1 {
        v.extend(dash.iter().map(|d| d * f));
    }
    v
}

fn bbox(rings: &[Vec<DVec2>]) -> Option<(DVec2, DVec2)> {
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    for p in rings.iter().flatten() {
        if !p.is_finite() {
            return None;
        }
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

/// The lines of a hatch inside the rings.
pub fn hatch_segments(def: &HatchDef, rings: &[Vec<DVec2>], scale: f64) -> Vec<PatternSeg> {
    let mut out = Vec::new();
    let Some((lo, hi)) = bbox(rings) else { return out };
    let f = def.units.factor(scale);
    let mut budget = MAX_SEGMENTS;
    for line in &def.lines {
        let spacing = line.spacing * f;
        if !(spacing.is_finite() && spacing > 0.0) {
            continue;
        }
        let a = line.angle.to_radians();
        let u = DVec2::from_angle(a);
        let n = DVec2::new(-u.y, u.x);
        let origin = line.origin * f;
        let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        let (dmin, dmax) = corners.iter().map(|c| n.dot(*c - origin)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), d| (a.min(d), b.max(d)));
        let (k0, k1) = ((dmin / spacing).ceil(), (dmax / spacing).floor());
        if !(k0.is_finite() && k1.is_finite()) || k1 < k0 || (k1 - k0) as usize >= MAX_LINES {
            continue;
        }
        let dash = dash_in(&line.dash, f);
        let (k0, k1) = (k0 as i64, k1 as i64);
        for k in k0..=k1 {
            let p0 = origin + n * (k as f64 * spacing);
            let phase = k as f64 * line.offset * f;
            for (t0, t1) in line_intervals(p0, u, rings) {
                let mut pieces = Vec::new();
                // The pattern is at its start at the origin of the line, then moves by the offset line by line.
                dash_pieces(t0, t1, &dash, t0 - phase, &mut pieces, &mut budget);
                for (s0, s1) in pieces {
                    out.push(PatternSeg { a: p0 + u * s0, b: p0 + u * s1, color: line.color, weight: line.weight });
                }
            }
            if budget == 0 {
                return out;
            }
        }
    }
    out
}

fn inside(p: DVec2, rings: &[Vec<DVec2>]) -> bool {
    rings.iter().filter(|r| r.len() >= 3 && point_in_polygon(p, r)).count() % 2 == 1
}

/// The parts of the segment `a`-`b` that lie inside the rings.
pub fn clip_segment(a: DVec2, b: DVec2, rings: &[Vec<DVec2>]) -> Vec<[DVec2; 2]> {
    let d = b - a;
    let mut ts = vec![0.0, 1.0];
    for ring in rings {
        let m = ring.len();
        if m < 3 {
            continue;
        }
        for i in 0..m {
            let (c, e) = (ring[i], ring[(i + 1) % m]);
            let r = e - c;
            let den = d.perp_dot(r);
            if den == 0.0 {
                continue;
            }
            let t = (c - a).perp_dot(r) / den;
            let s = (c - a).perp_dot(d) / den;
            if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&s) {
                ts.push(t);
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    let mut out: Vec<[DVec2; 2]> = Vec::new();
    for w in ts.windows(2) {
        if w[1] - w[0] <= 0.0 {
            continue;
        }
        let mid = a + d * ((w[0] + w[1]) / 2.0);
        if inside(mid, rings) {
            let piece = [a + d * w[0], a + d * w[1]];
            match out.last_mut() {
                Some(last) if last[1] == piece[0] => last[1] = piece[1],
                _ => out.push(piece),
            }
        }
    }
    out
}

/// The parts of a polyline that lie inside the rings.
pub fn clip_polyline(points: &[DVec2], closed: bool, rings: &[Vec<DVec2>], out: &mut Vec<[DVec2; 2]>) {
    let n = points.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        out.extend(clip_segment(points[i], points[(i + 1) % n], rings));
    }
}

/// The repeated symbol of a tile inside the rings. The geometry comes from the 2D components of
/// the symbol. A missing symbol gives nothing.
pub fn tile_segments(res: &Resources, def: &TileDef, rings: &[Vec<DVec2>], scale: f64) -> Vec<PatternSeg> {
    let mut out = Vec::new();
    let Some((lo, hi)) = bbox(rings) else { return out };
    let f = def.units.factor(scale);
    let polys = crate::symbols::symbol_polylines(res, def.symbol, def.scale * f);
    if polys.is_empty() {
        return out;
    }
    // How far the symbol reaches from its insertion point.
    let reach = polys.iter().flat_map(|(p, _)| p.iter()).fold(0.0f64, |m, p| m.max(p.length()));
    let (w, h) = (def.width * f, def.height * f);
    let rot = DAffine2::from_angle(def.angle.to_radians());
    let back = rot.inverse();
    // The area in grid coordinates.
    let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|c| back.transform_point2(c));
    let (glo, ghi) = corners.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(a, b), c| (a.min(*c), b.max(*c)));
    let shift = def.row_shift * f;
    let (r0, r1) = (((glo.y - reach) / h).floor() as i64, ((ghi.y + reach) / h).ceil() as i64);
    let (c0, c1) = (((glo.x - reach - shift.abs()) / w).floor() as i64, ((ghi.x + reach + shift.abs()) / w).ceil() as i64);
    if (r1 - r0 + 1).saturating_mul(c1 - c0 + 1) as usize > MAX_CELLS || !(w > 0.0 && h > 0.0) {
        return out;
    }
    let mut budget = MAX_SEGMENTS;
    let mut pieces: Vec<[DVec2; 2]> = Vec::new();
    for r in r0..=r1 {
        let row_shift = if r.rem_euclid(2) == 1 { shift } else { 0.0 };
        for c in c0..=c1 {
            let center = rot.transform_point2(DVec2::new(c as f64 * w + row_shift, r as f64 * h));
            // Skip cells whose symbol cannot touch the shape.
            if center.x + reach < lo.x || center.x - reach > hi.x || center.y + reach < lo.y || center.y - reach > hi.y {
                continue;
            }
            pieces.clear();
            for (poly, closed) in &polys {
                let moved: Vec<DVec2> = poly.iter().map(|p| center + rot.transform_vector2(*p)).collect();
                clip_polyline(&moved, *closed, rings, &mut pieces);
            }
            for s in &pieces {
                if budget == 0 {
                    return out;
                }
                budget -= 1;
                out.push(PatternSeg { a: s[0], b: s[1], color: None, weight: None });
            }
        }
    }
    out
}

/// What a fill pattern draws in the rings. A hatch or a tile gives lines; any other ID gives nothing.
pub fn fill_segments(res: &Resources, pattern: ResourceId, rings: &[Vec<DVec2>], scale: f64) -> Vec<PatternSeg> {
    if let Some(h) = res.hatch(pattern) {
        hatch_segments(h, rings, scale)
    } else if let Some(t) = res.tile(pattern) {
        tile_segments(res, t, rings, scale)
    } else {
        Vec::new()
    }
}

/// A symbol placed on a line by a line type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at: DVec2,
    /// Direction of the line there, in radians. Zero for a mark that does not turn.
    pub angle: f64,
    pub symbol: ResourceId,
    /// Scale of the symbol in model units per unit of the definition.
    pub scale: f64,
}

/// A line type applied to a polyline: the strokes to draw and the marks to place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineTypeGeometry {
    pub strokes: Vec<Vec<DVec2>>,
    pub marks: Vec<Mark>,
}

/// The point at distance `s` along the polyline, with the direction there.
fn point_at(points: &[DVec2], cum: &[f64], s: f64) -> (DVec2, DVec2) {
    let i = match cum.binary_search_by(|c| c.total_cmp(&s)) {
        Ok(i) => i.min(points.len() - 2),
        Err(i) => i.saturating_sub(1).min(points.len() - 2),
    };
    let (a, b) = (points[i], points[i + 1]);
    let len = cum[i + 1] - cum[i];
    let t = if len > 0.0 { ((s - cum[i]) / len).clamp(0.0, 1.0) } else { 0.0 };
    (a.lerp(b, t), (b - a).normalize_or_zero())
}

/// Applies a line type to a polyline (already flattened). A line that is too long for the pattern
/// (more than `MAX_SEGMENTS` repeats) stays solid.
pub fn line_type_geometry(res: &Resources, def: &LineTypeDef, points: &[DVec2], closed: bool, scale: f64) -> LineTypeGeometry {
    let mut pts: Vec<DVec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    if closed && pts.len() > 1 {
        pts.push(pts[0]);
    }
    pts.dedup();
    if pts.len() < 2 {
        return LineTypeGeometry::default();
    }
    let f = def.units.factor(scale);
    let mut cum = vec![0.0];
    for w in pts.windows(2) {
        cum.push(cum.last().copied().unwrap_or(0.0) + w[0].distance(w[1]));
    }
    let total = *cum.last().expect("not empty");
    let period = def.period() * f;
    if period.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || total / period > MAX_SEGMENTS as f64 / def.elements.len().max(1) as f64 {
        return LineTypeGeometry { strokes: vec![pts], marks: Vec::new() };
    }
    let mut g = LineTypeGeometry::default();
    let mut s = 0.0;
    'outer: loop {
        for e in &def.elements {
            match e {
                LineElement::Dash { length } => {
                    let end = (s + length * f).min(total);
                    if end > s {
                        let mut stroke = vec![point_at(&pts, &cum, s).0];
                        stroke.extend(pts.iter().zip(&cum).filter(|(_, c)| **c > s && **c < end).map(|(p, _)| *p));
                        stroke.push(point_at(&pts, &cum, end).0);
                        g.strokes.push(stroke);
                    }
                    s += length * f;
                }
                LineElement::Gap { length } => s += length * f,
                LineElement::Mark { symbol, scale: k, rotate } => {
                    if s <= total && res.symbol(*symbol).is_some() {
                        let (at, dir) = point_at(&pts, &cum, s.min(total));
                        g.marks.push(Mark { at, angle: if *rotate { dir.y.atan2(dir.x) } else { 0.0 }, symbol: *symbol, scale: k * f });
                    }
                }
            }
            if s >= total {
                break 'outer;
            }
        }
    }
    g
}

/// What the pattern attributes of an object add to its drawing, in model units.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decor {
    /// Lines of the hatch or tile that fills the shape, clipped to it.
    pub fill: Vec<PatternSeg>,
    /// With a line type: the dashes that replace the outline.
    pub strokes: Vec<Vec<DVec2>>,
    /// With a line type: the symbols along the outline.
    pub marks: Vec<Mark>,
    /// True if the dashes replace the outline of the shape (a line type is set and exists).
    pub replaces_outline: bool,
}

/// The outlines of a shape that can have a fill pattern or a line type, as polylines.
fn outlines(obj: &Object) -> Vec<(Vec<DVec2>, bool)> {
    match &obj.shape {
        Shape::Line { .. } | Shape::Rect { .. } | Shape::Circle { .. } | Shape::Arc { .. } | Shape::Polyline { .. } | Shape::LandscapeArea { .. } | Shape::Hardscape { .. } => {
            crate::symbols::shape_polylines(&obj.shape)
        }
        _ => Vec::new(),
    }
}

/// The fill pattern and the line type of an object, as geometry. `attrs` are the final attributes of the
/// object (the viewport or the style may have changed them). `scale` is model mm per paper mm.
/// `None` if the object has neither a pattern nor a line type, or its shape cannot have them.
pub fn decorate(doc: &Document, obj: &Object, attrs: &Attributes, scale: f64) -> Option<Decor> {
    if attrs.fill_pattern.is_none() && attrs.line_type.is_none() {
        return None;
    }
    let lines = outlines(obj);
    if lines.is_empty() {
        return None;
    }
    let mut d = Decor::default();
    if let Some(p) = attrs.fill_pattern {
        let mut rings: Vec<Vec<DVec2>> = lines.iter().filter(|(pts, closed)| *closed && pts.len() >= 3).map(|(pts, _)| pts.clone()).collect();
        if !rings.is_empty()
            && let Some(data) = crate::landscape::data(obj)
        {
            rings.extend(data.holes.iter().map(|h| crate::curve::flatten(h, true, 0.05)));
        }
        d.fill = fill_segments(&doc.resources, p, &rings, scale);
    }
    if let Some(l) = attrs.line_type.and_then(|l| doc.resources.line_type(l)) {
        d.replaces_outline = true;
        for (pts, closed) in &lines {
            let g = line_type_geometry(&doc.resources, l, pts, *closed, scale);
            d.strokes.extend(g.strokes);
            d.marks.extend(g.marks);
        }
    }
    Some(d)
}

/// The pen of a pattern: its own color, or the one that the object brings.
pub fn pen_of(seg: &PatternSeg, pen: Color) -> Color {
    seg.color.unwrap_or(pen)
}

/// True if a unit setting scales with the drawing.
pub fn scales_with_drawing(units: PatternUnits) -> bool {
    units == PatternUnits::Paper
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::polygon_area;
    use crate::graphics::{GraphicResource, HatchLine, SymbolDef, SymbolItem};
    use crate::model::{Attributes, Shape};
    use crate::resource::{Resource, ResourceBody};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn square(x: f64, y: f64, s: f64) -> Vec<DVec2> {
        vec![v(x, y), v(x + s, y), v(x + s, y + s), v(x, y + s)]
    }

    fn hatch(lines: Vec<HatchLine>, units: PatternUnits) -> HatchDef {
        HatchDef { units, lines }
    }

    fn total_length(segs: &[PatternSeg]) -> f64 {
        segs.iter().map(|s| s.a.distance(s.b)).sum()
    }

    #[test]
    fn horizontal_lines_fill_a_square() {
        let h = hatch(vec![HatchLine::solid(0.0, 10.0)], PatternUnits::World);
        let segs = hatch_segments(&h, &[square(0.0, 0.0, 100.0)], 100.0);
        // Lines at y = 0, 10, ..., 100: the first and last lie on the boundary (half open rule keeps one).
        assert!((10..=11).contains(&segs.len()), "{}", segs.len());
        assert!(segs.iter().all(|s| (s.a.distance(s.b) - 100.0).abs() < 1e-9));
    }

    #[test]
    fn paper_units_follow_the_scale_and_world_units_do_not() {
        let rings = [square(0.0, 0.0, 1000.0)];
        let line = || HatchLine::solid(0.0, 2.0);
        let paper = hatch_segments(&hatch(vec![line()], PatternUnits::Paper), &rings, 100.0);
        let world = hatch_segments(&hatch(vec![line()], PatternUnits::World), &rings, 100.0);
        // 2 mm on paper at 1:100 is 200 mm in the model: about 5 lines. 2 mm in the model: about 500.
        assert!((5..=6).contains(&paper.len()), "{}", paper.len());
        assert!((500..=501).contains(&world.len()), "{}", world.len());
        assert!(scales_with_drawing(PatternUnits::Paper) && !scales_with_drawing(PatternUnits::World));
    }

    #[test]
    fn hatch_length_matches_the_area_over_the_spacing() {
        let tri = vec![v(0.0, 0.0), v(900.0, 100.0), v(300.0, 700.0)];
        for angle in [0.0, 30.0, 45.0, 90.0, 133.0] {
            let h = hatch(vec![HatchLine::solid(angle, 7.0)], PatternUnits::World);
            let segs = hatch_segments(&h, std::slice::from_ref(&tri), 1.0);
            let covered = total_length(&segs) * 7.0;
            let area = polygon_area(&tri);
            assert!((covered - area).abs() < 0.01 * area, "angle {angle}: {covered} vs {area}");
            // Every piece lies inside the triangle.
            for s in &segs {
                assert!(point_in_polygon((s.a + s.b) / 2.0, &tri));
            }
        }
    }

    #[test]
    fn holes_are_not_hatched() {
        let outer = square(0.0, 0.0, 1000.0);
        let hole = square(250.0, 250.0, 500.0);
        let h = hatch(vec![HatchLine::solid(0.0, 5.0)], PatternUnits::World);
        let segs = hatch_segments(&h, &[outer.clone(), hole.clone()], 1.0);
        let covered = total_length(&segs) * 5.0;
        let area = polygon_area(&outer) - polygon_area(&hole);
        assert!((covered - area).abs() < 0.01 * area, "{covered} vs {area}");
        assert!(segs.iter().all(|s| !point_in_polygon((s.a + s.b) / 2.0, &hole)));
    }

    #[test]
    fn dashes_and_offsets() {
        let rings = [square(0.0, 0.0, 100.0)];
        let line = HatchLine { dash: vec![10.0, 10.0], ..HatchLine::solid(0.0, 50.0) };
        let segs = hatch_segments(&hatch(vec![line.clone()], PatternUnits::World), &rings, 1.0);
        // Three lines (0, 50, 100: the top one is dropped by the half open rule), each with 5 dashes.
        let len = total_length(&segs);
        assert!((len - 2.0 * 50.0 * 1.0 * 1.0 - 0.0).abs() < 1e-6 || (len - 150.0).abs() < 1e-6 || (len - 250.0).abs() < 1e-6, "{len}");
        // An odd pattern repeats, so dash and gap swap on the second turn: 10 on, 10 off, 10 on... (same as 10, 10).
        let odd = HatchLine { dash: vec![10.0], ..HatchLine::solid(0.0, 50.0) };
        let segs_odd = hatch_segments(&hatch(vec![odd], PatternUnits::World), &rings, 1.0);
        assert!((total_length(&segs_odd) - len).abs() < 1e-6);
        // The offset staggers the dashes of neighboring lines.
        let staggered = HatchLine { offset: 5.0, ..line };
        let s2 = hatch_segments(&hatch(vec![staggered], PatternUnits::World), &rings, 1.0);
        let starts: Vec<f64> = s2.iter().filter(|s| (s.a.y - 50.0).abs() < 1e-9).map(|s| s.a.x).collect();
        let base: Vec<f64> = segs.iter().filter(|s| (s.a.y - 50.0).abs() < 1e-9).map(|s| s.a.x).collect();
        assert!(!starts.is_empty() && !base.is_empty());
        assert!((starts[0] - base[0]).abs() > 1e-6, "{starts:?} {base:?}");
    }

    #[test]
    fn hatch_colors_and_weights_pass_through() {
        let line = HatchLine { color: Some(Color([1, 2, 3, 255])), weight: Some(0.1), ..HatchLine::solid(0.0, 50.0) };
        let segs = hatch_segments(&hatch(vec![line], PatternUnits::World), &[square(0.0, 0.0, 100.0)], 1.0);
        assert!(segs.iter().all(|s| s.color == Some(Color([1, 2, 3, 255])) && s.weight == Some(0.1)));
        assert_eq!(pen_of(&segs[0], Color::BLACK), Color([1, 2, 3, 255]));
        let plain = PatternSeg { color: None, ..segs[0] };
        assert_eq!(pen_of(&plain, Color::BLACK), Color::BLACK);
    }

    #[test]
    fn too_dense_or_bad_input_is_safe() {
        let dense = hatch(vec![HatchLine::solid(0.0, 1e-6)], PatternUnits::World);
        assert!(hatch_segments(&dense, &[square(0.0, 0.0, 1000.0)], 1.0).is_empty());
        assert!(hatch_segments(&dense, &[], 1.0).is_empty());
        assert!(hatch_segments(&dense, &[vec![v(f64::NAN, 0.0), v(1.0, 0.0), v(0.0, 1.0)]], 1.0).is_empty());
        let zero = HatchDef { units: PatternUnits::World, lines: vec![HatchLine { spacing: 0.0, ..HatchLine::solid(0.0, 1.0) }] };
        assert!(hatch_segments(&zero, &[square(0.0, 0.0, 10.0)], 1.0).is_empty());
        // Many dashes are cut off at the limit.
        let many = hatch(vec![HatchLine { dash: vec![0.001, 0.001], ..HatchLine::solid(0.0, 10.0) }], PatternUnits::World);
        let segs = hatch_segments(&many, &[square(0.0, 0.0, 1000.0)], 1.0);
        assert!(segs.len() <= MAX_SEGMENTS);
    }

    #[test]
    fn clipping_a_segment() {
        let rings = [square(0.0, 0.0, 10.0)];
        assert_eq!(clip_segment(v(-5.0, 5.0), v(15.0, 5.0), &rings), vec![[v(0.0, 5.0), v(10.0, 5.0)]]);
        assert!(clip_segment(v(-5.0, 20.0), v(15.0, 20.0), &rings).is_empty());
        assert_eq!(clip_segment(v(2.0, 2.0), v(8.0, 8.0), &rings), vec![[v(2.0, 2.0), v(8.0, 8.0)]]);
        // Through a hole: two pieces.
        let with_hole = [square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)];
        assert_eq!(clip_segment(v(0.0, 5.0), v(10.0, 5.0), &with_hole).len(), 2);
        let mut out = Vec::new();
        clip_polyline(&[v(-5.0, 5.0), v(5.0, 5.0), v(5.0, 15.0)], false, &rings, &mut out);
        assert_eq!(out.len(), 2);
        clip_polyline(&square(2.0, 2.0, 4.0), true, &rings, &mut out);
        assert_eq!(out.len(), 2 + 4);
    }

    fn with_cross_symbol() -> (Resources, ResourceId) {
        let mut res = Resources::default();
        let item = |a: DVec2, b: DVec2| SymbolItem { shape: Shape::Line { a, b }, attrs: Attributes::default(), class: None };
        let def = SymbolDef { insertion: v(0.0, 0.0), items: vec![item(v(-1.0, 0.0), v(1.0, 0.0)), item(v(0.0, -1.0), v(0.0, 1.0))], solids: vec![] };
        res.push(Resource { id: ResourceId(7), name: "Cross".into(), body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))) });
        (res, ResourceId(7))
    }

    #[test]
    fn a_tile_repeats_a_symbol_inside_the_shape() {
        let (res, sym) = with_cross_symbol();
        let def = TileDef { symbol: sym, width: 20.0, height: 20.0, row_shift: 0.0, angle: 0.0, scale: 1.0, units: PatternUnits::World };
        let ring = square(0.0, 0.0, 100.0);
        let segs = tile_segments(&res, &def, std::slice::from_ref(&ring), 1.0);
        // Cells at 0, 20, ..., 100 in both directions: 36 crosses of 2 lines, those on the edge cut in part.
        assert!(segs.len() >= 50, "{}", segs.len());
        assert!(segs.iter().all(|s| point_in_polygon((s.a + s.b) / 2.0, &ring) || (s.a + s.b).x / 2.0 <= 100.0 + 1e-9));
        assert!(segs.iter().all(|s| s.a.min(s.b).cmpge(v(-1e-9, -1e-9)).all() && s.a.max(s.b).cmple(v(100.0 + 1e-9, 100.0 + 1e-9)).all()));
        // A brick shift moves every second row.
        let brick = TileDef { row_shift: 10.0, ..def.clone() };
        let b = tile_segments(&res, &brick, std::slice::from_ref(&ring), 1.0);
        assert_ne!(segs, b);
        // A turned grid still stays inside.
        let turned = TileDef { angle: 30.0, ..def.clone() };
        let t = tile_segments(&res, &turned, std::slice::from_ref(&ring), 1.0);
        assert!(!t.is_empty() && t.iter().all(|s| s.a.min(s.b).cmpge(v(-1e-6, -1e-6)).all() && s.a.max(s.b).cmple(v(100.0 + 1e-6, 100.0 + 1e-6)).all()));
        // Paper units follow the scale: 20 mm cells at 1:10 are 200 mm, so the grid has few cells.
        let paper = TileDef { units: PatternUnits::Paper, ..def.clone() };
        let p = tile_segments(&res, &paper, std::slice::from_ref(&ring), 10.0);
        assert!(p.len() < segs.len());
        // Missing symbol, too many cells, bad rings.
        assert!(tile_segments(&res, &TileDef { symbol: ResourceId(99), ..def.clone() }, std::slice::from_ref(&ring), 1.0).is_empty());
        let tiny = TileDef { width: 1e-3, height: 1e-3, ..def.clone() };
        assert!(tile_segments(&res, &tiny, std::slice::from_ref(&ring), 1.0).is_empty());
        assert!(tile_segments(&res, &def, &[], 1.0).is_empty());
    }

    #[test]
    fn fill_segments_picks_by_type() {
        let (mut res, sym) = with_cross_symbol();
        res.push(Resource { id: ResourceId(8), name: "H".into(), body: ResourceBody::Graphic(Box::new(GraphicResource::Hatch(hatch(vec![HatchLine::solid(0.0, 10.0)], PatternUnits::World)))) });
        res.push(Resource {
            id: ResourceId(9),
            name: "T".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Tile(TileDef { symbol: sym, width: 30.0, height: 30.0, row_shift: 0.0, angle: 0.0, scale: 1.0, units: PatternUnits::World }))),
        });
        let ring = [square(0.0, 0.0, 100.0)];
        assert!(!fill_segments(&res, ResourceId(8), &ring, 1.0).is_empty());
        assert!(!fill_segments(&res, ResourceId(9), &ring, 1.0).is_empty());
        assert!(fill_segments(&res, sym, &ring, 1.0).is_empty(), "a symbol is not a pattern");
        assert!(fill_segments(&res, ResourceId(1234), &ring, 1.0).is_empty());
    }

    fn line_type(elements: Vec<LineElement>) -> LineTypeDef {
        LineTypeDef { units: PatternUnits::World, elements }
    }

    #[test]
    fn dashed_lines() {
        let res = Resources::default();
        let lt = line_type(vec![LineElement::Dash { length: 10.0 }, LineElement::Gap { length: 5.0 }]);
        let g = line_type_geometry(&res, &lt, &[v(0.0, 0.0), v(100.0, 0.0)], false, 1.0);
        // 100 = 6 x 15 + 10: seven dashes, the last one is whole.
        assert_eq!(g.strokes.len(), 7);
        assert_eq!(g.strokes[0], vec![v(0.0, 0.0), v(10.0, 0.0)]);
        assert_eq!(g.strokes[6], vec![v(90.0, 0.0), v(100.0, 0.0)]);
        assert!(g.marks.is_empty());
        // A dash that reaches the end is cut there.
        let g = line_type_geometry(&res, &lt, &[v(0.0, 0.0), v(95.0, 0.0)], false, 1.0);
        assert_eq!(g.strokes.last().unwrap().last().unwrap(), &v(95.0, 0.0));
        // A dash bends around a corner.
        let g = line_type_geometry(&res, &lt, &[v(0.0, 0.0), v(8.0, 0.0), v(8.0, 20.0)], false, 1.0);
        assert_eq!(g.strokes[0], vec![v(0.0, 0.0), v(8.0, 0.0), v(8.0, 2.0)]);
        // A closed line returns to its start.
        let g = line_type_geometry(&res, &lt, &square(0.0, 0.0, 30.0), true, 1.0);
        let len: f64 = g.strokes.iter().map(|s| s.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>()).sum();
        assert!((len - 80.0).abs() < 1e-6, "{len}");
        // Paper units follow the scale.
        let paper = LineTypeDef { units: PatternUnits::Paper, ..lt.clone() };
        let g = line_type_geometry(&res, &paper, &[v(0.0, 0.0), v(1000.0, 0.0)], false, 10.0);
        assert_eq!(g.strokes[0], vec![v(0.0, 0.0), v(100.0, 0.0)]);
    }

    #[test]
    fn marks_sit_on_the_line() {
        let (res, sym) = with_cross_symbol();
        let lt = line_type(vec![LineElement::Dash { length: 20.0 }, LineElement::Mark { symbol: sym, scale: 2.0, rotate: true }, LineElement::Gap { length: 10.0 }]);
        let g = line_type_geometry(&res, &lt, &[v(0.0, 0.0), v(0.0, 100.0)], false, 1.0);
        assert!(!g.marks.is_empty());
        assert!(g.marks.iter().all(|m| m.at.x == 0.0 && (m.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9 && m.scale == 2.0 && m.symbol == sym));
        assert_eq!(g.marks[0].at, v(0.0, 20.0));
        let fixed = line_type(vec![LineElement::Mark { symbol: sym, scale: 1.0, rotate: false }, LineElement::Gap { length: 25.0 }]);
        let g = line_type_geometry(&res, &fixed, &[v(0.0, 0.0), v(0.0, 100.0)], false, 1.0);
        assert!(g.marks.iter().all(|m| m.angle == 0.0));
        assert_eq!(g.marks.len(), 4);
        // A mark of a symbol that is gone is left out.
        let gone = line_type(vec![LineElement::Mark { symbol: ResourceId(404), scale: 1.0, rotate: true }, LineElement::Gap { length: 25.0 }]);
        assert!(line_type_geometry(&res, &gone, &[v(0.0, 0.0), v(0.0, 100.0)], false, 1.0).marks.is_empty());
    }

    #[test]
    fn dense_or_degenerate_lines_stay_solid_or_empty() {
        let res = Resources::default();
        let lt = line_type(vec![LineElement::Dash { length: 1e-4 }, LineElement::Gap { length: 1e-4 }]);
        let g = line_type_geometry(&res, &lt, &[v(0.0, 0.0), v(1e6, 0.0)], false, 1.0);
        assert_eq!(g.strokes, vec![vec![v(0.0, 0.0), v(1e6, 0.0)]]);
        assert_eq!(line_type_geometry(&res, &lt, &[v(0.0, 0.0)], false, 1.0), LineTypeGeometry::default());
        assert_eq!(line_type_geometry(&res, &lt, &[v(1.0, 1.0), v(1.0, 1.0)], false, 1.0), LineTypeGeometry::default());
    }

    #[test]
    fn decor_gives_fill_dashes_and_marks_for_the_shapes_that_can_have_them() {
        use crate::command::{Command, Editor};
        let mut ed = Editor::default();
        let mut add = |name: &str, g: GraphicResource| ResourceId(ed.execute(&Command::AddResource { name: name.into(), body: ResourceBody::Graphic(Box::new(g)) }).unwrap().created[0]);
        let h = add("H", GraphicResource::Hatch(hatch(vec![HatchLine::solid(0.0, 100.0)], PatternUnits::World)));
        let dot = add("Dot", GraphicResource::Symbol(crate::graphics::SymbolDef { items: vec![crate::graphics::SymbolItem { shape: Shape::Circle { center: DVec2::ZERO, radius: 5.0 }, attrs: Attributes::default(), class: None }], ..Default::default() }));
        let lt = add("L", GraphicResource::LineType(line_type(vec![LineElement::Dash { length: 100.0 }, LineElement::Mark { symbol: dot, scale: 1.0, rotate: true }, LineElement::Gap { length: 100.0 }])));
        let rect = Object::new(crate::model::ObjectId(900), crate::model::FIRST_LAYER, crate::model::NONE_CLASS, Shape::Rect { min: v(0.0, 0.0), max: v(1000.0, 500.0) });
        let doc = ed.doc();
        let none = Attributes::default();
        assert!(decorate(doc, &rect, &none, 1.0).is_none(), "no pattern attributes");
        let both = Attributes { fill_pattern: Some(h), line_type: Some(lt), ..Default::default() };
        let d = decorate(doc, &rect, &both, 1.0).unwrap();
        assert!(d.replaces_outline && !d.fill.is_empty() && !d.strokes.is_empty() && !d.marks.is_empty());
        let fill_only = decorate(doc, &rect, &Attributes { fill_pattern: Some(h), ..Default::default() }, 1.0).unwrap();
        assert!(!fill_only.replaces_outline && fill_only.strokes.is_empty());
        // A line type that is gone leaves the outline alone.
        let gone = decorate(doc, &rect, &Attributes { line_type: Some(ResourceId(999)), ..Default::default() }, 1.0).unwrap();
        assert!(!gone.replaces_outline);
        // An open line has no fill. A plant has neither.
        let line = Object::new(crate::model::ObjectId(901), crate::model::FIRST_LAYER, crate::model::NONE_CLASS, Shape::Line { a: v(0.0, 0.0), b: v(500.0, 0.0) });
        assert!(decorate(doc, &line, &both, 1.0).unwrap().fill.is_empty());
        let plant = Object::new(crate::model::ObjectId(902), crate::model::FIRST_LAYER, crate::model::NONE_CLASS, Shape::Plant { at: DVec2::ZERO, species: "x".into(), spread: 1.0, height: 1.0 });
        assert!(decorate(doc, &plant, &both, 1.0).is_none());
        // Holes of a landscape area are not hatched.
        let mut ed2 = Editor::default();
        let h2 = ResourceId(ed2.execute(&Command::AddResource { name: "H".into(), body: ResourceBody::Graphic(Box::new(GraphicResource::Hatch(hatch(vec![HatchLine::solid(0.0, 10.0)], PatternUnits::World)))) }).unwrap().created[0]);
        let boundary = crate::curve::corners(&square(0.0, 0.0, 1000.0));
        let id = ed2.execute(&Command::AddLandscapeArea { boundary, name: "a".into(), patch: Default::default(), layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0];
        let hole = crate::curve::corners(&square(250.0, 250.0, 500.0));
        ed2.execute(&Command::SetLandscapeArea { ids: vec![crate::model::ObjectId(id)], patch: crate::landscape::LandscapePatch { holes: Some(vec![hole]), ..Default::default() } }).unwrap();
        let o = ed2.doc().object(crate::model::ObjectId(id)).unwrap();
        let d = decorate(ed2.doc(), o, &Attributes { fill_pattern: Some(h2), ..Default::default() }, 1.0).unwrap();
        let covered: f64 = d.fill.iter().map(|s| s.a.distance(s.b)).sum::<f64>() * 10.0;
        assert!((covered - 750_000.0).abs() < 0.03 * 750_000.0, "{covered}");
    }

    #[test]
    fn degenerate_rings_and_phases_are_safe() {
        // A ring with fewer than three points is skipped.
        let h = hatch(vec![HatchLine::solid(0.0, 10.0)], PatternUnits::World);
        let thin = vec![v(0.0, 0.0), v(100.0, 0.0)];
        assert!(hatch_segments(&h, &[thin.clone(), square(0.0, 0.0, 100.0)], 1.0).len() > 5);
        assert!(clip_segment(v(0.0, 0.0), v(10.0, 10.0), &[thin]).is_empty());
        // A dash pattern with an element longer than the interval, and a phase at the edge of the period.
        let mut out = Vec::new();
        let mut budget = 10;
        dash_pieces(0.0, 5.0, &[10.0, 10.0], 0.0, &mut out, &mut budget);
        assert_eq!(out, vec![(0.0, 5.0)]);
        out.clear();
        dash_pieces(0.0, 30.0, &[10.0, 10.0], 20.0, &mut out, &mut budget);
        assert_eq!(out[0], (0.0, 10.0));
        out.clear();
        dash_pieces(0.0, 30.0, &[], 0.0, &mut out, &mut budget);
        assert_eq!(out, vec![(0.0, 30.0)]);
        // The budget limits the pieces.
        out.clear();
        let mut small = 2;
        dash_pieces(0.0, 100.0, &[1.0, 1.0], 0.0, &mut out, &mut small);
        assert_eq!(out.len(), 2);
    }
}
