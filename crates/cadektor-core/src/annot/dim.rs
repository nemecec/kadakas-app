//! Dimensions: measuring, text and drawing.
//!
//! `values` measures the stored points, so a value always agrees with the geometry. The
//! lengths are in the units of the layer (or paper mm in a viewport). `Ctx::real` turns them
//! into real millimeters before the text is formatted.

use std::f64::consts::TAU;

use glam::DVec2;

use super::draw::{Ctx, Fill, Prim, circle_points, text_width};
use super::style::{DimensionStyle, LengthUnit, Terminator};
use super::{Axis, DimMode, Dimension};

/// The measured values of a dimension: lengths in layer units (angles in degrees), one per
/// dimension line (a chain has one per segment, a baseline and an ordinate one per measured point).
pub fn values(d: &Dimension) -> Vec<f64> {
    let p = &d.points;
    let axis_len = |a: DVec2, b: DVec2, u: DVec2| (b - a).dot(u).abs();
    match d.mode {
        DimMode::Linear { axis } => {
            let u = direction(axis, p);
            vec![axis_len(p[0], p[1], u)]
        }
        DimMode::Chain { axis } => {
            let u = direction(axis, p);
            p.windows(2).map(|w| axis_len(w[0], w[1], u)).collect()
        }
        DimMode::Baseline { axis } | DimMode::Ordinate { axis } => {
            let u = direction(axis, p);
            p[1..].iter().map(|q| axis_len(p[0], *q, u)).collect()
        }
        DimMode::Angular => {
            let (a1, a2) = ((p[1] - p[0]).to_angle(), (p[2] - p[0]).to_angle());
            vec![(a2 - a1).rem_euclid(TAU).to_degrees()]
        }
        DimMode::Radial => vec![p[0].distance(p[1])],
        DimMode::Diameter => vec![2.0 * p[0].distance(p[1])],
        DimMode::ArcLength => {
            let sweep = ((p[2] - p[0]).to_angle() - (p[1] - p[0]).to_angle()).rem_euclid(TAU);
            vec![p[0].distance(p[1]) * sweep]
        }
    }
}

/// The measuring direction of a group of points.
fn direction(axis: Axis, p: &[DVec2]) -> DVec2 {
    match axis {
        Axis::Horizontal => DVec2::X,
        Axis::Vertical => DVec2::Y,
        Axis::Aligned => {
            let d = p[p.len() - 1] - p[0];
            if d.length() > 0.0 { d.normalize() } else { DVec2::X }
        }
    }
}

/// The offset in paper mm that puts the dimension line of a linear, chain, baseline or ordinate
/// dimension through the point `through`. `p` are the points that are known so far. Other modes give 0.
pub fn offset_for(mode: DimMode, p: &[DVec2], through: DVec2, paper: f64) -> f64 {
    let (DimMode::Linear { axis } | DimMode::Chain { axis } | DimMode::Baseline { axis } | DimMode::Ordinate { axis }) = mode else { return 0.0 };
    if p.is_empty() || paper <= 0.0 {
        return 0.0;
    }
    (through - p[0]).dot(direction(axis, p).perp()) / paper
}

/// Rounds to a number of decimals, halves away from zero (2.5 gives 3, -2.5 gives -3).
pub fn round_half_away(x: f64, decimals: usize) -> f64 {
    let k = 10f64.powi(decimals as i32);
    let r = (x * k).round() / k;
    if r.is_finite() { r } else { x }
}

/// Puts the two ray points of an angular dimension in the order that measures the angle the user
/// meant, like the click order in a CAD program: clicks that turn clockwise measure the
/// same (smaller) angle as clicks that turn counterclockwise. The angle is at most 180 degrees.
pub fn angular_order(vertex: DVec2, p1: DVec2, p2: DVec2) -> (DVec2, DVec2) {
    let sweep = ((p2 - vertex).to_angle() - (p1 - vertex).to_angle()).rem_euclid(TAU);
    if sweep > std::f64::consts::PI { (p2, p1) } else { (p1, p2) }
}

fn trim_number(s: String) -> String {
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') { s[1..].to_string() } else { s }
}

/// A length in real mm as text in the units of the style.
pub fn format_length(mm: f64, style: &DimensionStyle) -> String {
    let p = style.precision as usize;
    let mm = match style.round_to {
        Some(inc) if inc > 0.0 => (mm / inc).round() * inc,
        _ => mm,
    };
    match style.unit {
        LengthUnit::FtIn => {
            let total = mm / 25.4;
            let sign = if total < 0.0 { "-" } else { "" };
            let total = total.abs();
            let mut feet = (total / 12.0).floor();
            let mut inches = total - feet * 12.0;
            inches = round_half_away(inches, p);
            if inches >= 12.0 {
                feet += 1.0;
                inches -= 12.0;
            }
            if feet == 0.0 { format!("{sign}{inches:.p$}\"") } else { format!("{sign}{feet:.0}'-{inches:.p$}\"") }
        }
        unit => {
            let k = match unit {
                LengthUnit::Mm => 1.0,
                LengthUnit::Cm => 10.0,
                LengthUnit::M => 1000.0,
                LengthUnit::In => 25.4,
                _ => 304.8,
            };
            let n = trim_number(format!("{:.p$}", round_half_away(mm / k, p)));
            if style.show_unit { format!("{n}{}", unit.suffix()) } else { n }
        }
    }
}

/// An angle in degrees as text.
pub fn format_angle(deg: f64, style: &DimensionStyle) -> String {
    format!("{}\u{b0}", trim_number(format!("{:.*}", style.angle_precision as usize, round_half_away(deg, style.angle_precision as usize))))
}

fn tolerance_text(style: &DimensionStyle) -> String {
    let Some(t) = style.tolerance else { return String::new() };
    let num = |x: f64| trim_number(format!("{:.*}", style.precision as usize, round_half_away(x, style.precision as usize)));
    if t.plus == t.minus { format!(" \u{b1}{}", num(t.plus)) } else { format!(" +{}/-{}", num(t.plus), num(t.minus)) }
}

/// The text of each dimension line. `ctx.real` turns layer units into real mm. The text of the
/// dimension replaces the value of a single dimension.
pub fn texts(d: &Dimension, style: &DimensionStyle, ctx: &Ctx) -> Vec<String> {
    let vals = values(d);
    let mut out: Vec<String> = vals
        .iter()
        .map(|v| {
            let body = match d.mode {
                DimMode::Angular => format_angle(*v, style),
                _ => format_length(v / ctx.real, style) + &tolerance_text(style),
            };
            let lead = match d.mode {
                DimMode::Radial => "R",
                DimMode::Diameter => "\u{d8}",
                DimMode::ArcLength => "Arc ",
                _ => "",
            };
            format!("{lead}{}{body}{}", style.prefix, style.suffix)
        })
        .collect();
    if let (Some(t), 1) = (&d.text, out.len()) {
        out[0] = t.clone();
    }
    out
}

// ---------------------------------------------------------------- drawing

fn thin_line(out: &mut Vec<Prim>, a: DVec2, b: DVec2) {
    out.push(Prim::line(a, b));
}

/// A terminator at `at`. `dir` is the unit direction in which it points (the tip is at `at`).
pub fn terminator(out: &mut Vec<Prim>, kind: Terminator, size: f64, at: DVec2, dir: DVec2) {
    let n = dir.perp();
    match kind {
        Terminator::None => {}
        Terminator::Arrow => out.push(Prim::poly(vec![at, at - dir * size + n * size * 0.17, at - dir * size - n * size * 0.17], true, Fill::Ink)),
        Terminator::OpenArrow => out.push(Prim::poly(vec![at - dir * size + n * size * 0.25, at, at - dir * size - n * size * 0.25], false, Fill::None)),
        Terminator::Tick => {
            let t = (dir + n).normalize() * size * 0.5;
            out.push(Prim::line(at - t, at + t));
        }
        Terminator::Dot => out.push(Prim::poly(circle_points(at, size * 0.25, 12), true, Fill::Ink)),
    }
}

/// The side of a line on which text sits: the normal that points up, or right for a vertical line.
fn text_side(n: DVec2) -> DVec2 {
    if n.y > 1e-9 || (n.y.abs() <= 1e-9 && n.x > 0.0) { n } else { -n }
}

/// Text next to a line: the middle `m` of the line and its unit normal.
fn text_beside(out: &mut Vec<Prim>, text: &str, style: &DimensionStyle, ctx: &Ctx, m: DVec2, normal: DVec2) {
    let h = style.text_size_mm * ctx.paper;
    let w = text_width(text, h);
    let s = text_side(normal);
    let ext = s.x.abs() * w / 2.0 + s.y.abs() * h / 2.0;
    let center = m + s * (style.text_gap_mm * ctx.paper + ext);
    out.push(Prim::text(center - DVec2::new(0.0, h * 0.35), text, h, true, style.color));
}

/// One dimension line between two measured points `p` and `q`, on the line `c` (the coordinate
/// along the normal `n`).
#[allow(clippy::too_many_arguments)]
fn linear(out: &mut Vec<Prim>, p: DVec2, q: DVec2, u: DVec2, c: f64, text: &str, style: &DimensionStyle, ctx: &Ctx) {
    let n = u.perp();
    let (sp, sq) = (p.dot(u), q.dot(u));
    let (lp, lq) = (u * sp + n * c, u * sq + n * c);
    let gap = style.ext_gap_mm * ctx.paper;
    let over = style.ext_overshoot_mm * ctx.paper;
    for (m, l) in [(p, lp), (q, lq)] {
        let d = l - m;
        let dist = d.length();
        if dist > gap + 1e-9 {
            let dir = d / dist;
            thin_line(out, m + dir * gap, l + dir * over);
        }
    }
    let len = (sq - sp).abs();
    if len > 1e-12 {
        let dir = u * (sq - sp).signum();
        let size = style.terminator_size_mm * ctx.paper;
        if len >= 2.0 * size {
            thin_line(out, lp, lq);
            terminator(out, style.terminator, size, lp, -dir);
            terminator(out, style.terminator, size, lq, dir);
        } else {
            // Too short for the ends: they go outside and point inward.
            thin_line(out, lp - dir * size * 1.5, lq + dir * size * 1.5);
            terminator(out, style.terminator, size, lp, dir);
            terminator(out, style.terminator, size, lq, -dir);
        }
    }
    text_beside(out, text, style, ctx, (lp + lq) / 2.0, n);
}

fn arc_points(center: DVec2, r: f64, start: f64, sweep: f64) -> Vec<DVec2> {
    let n = ((sweep.abs() / 0.1).ceil() as usize).clamp(4, 400);
    (0..=n).map(|i| center + DVec2::from_angle(start + sweep * i as f64 / n as f64) * r).collect()
}

/// The lines, terminators and text of a dimension. `style` is the dimension style or the default.
pub fn prims(d: &Dimension, style: &DimensionStyle, ctx: &Ctx) -> Vec<Prim> {
    let mut out = Vec::new();
    let p = &d.points;
    let texts = texts(d, style, ctx);
    let off = d.offset * ctx.paper;
    let size = style.terminator_size_mm * ctx.paper;
    match d.mode {
        DimMode::Linear { axis } | DimMode::Chain { axis } | DimMode::Baseline { axis } => {
            let u = direction(axis, p);
            let n = u.perp();
            let c0 = p[0].dot(n) + off;
            match d.mode {
                DimMode::Linear { .. } => linear(&mut out, p[0], p[1], u, c0, &texts[0], style, ctx),
                DimMode::Chain { .. } => {
                    for (i, w) in p.windows(2).enumerate() {
                        linear(&mut out, w[0], w[1], u, c0, &texts[i], style, ctx);
                    }
                }
                _ => {
                    let step = style.baseline_spacing_mm * ctx.paper * if off < 0.0 { -1.0 } else { 1.0 };
                    for (i, q) in p[1..].iter().enumerate() {
                        linear(&mut out, p[0], *q, u, c0 + step * i as f64, &texts[i], style, ctx);
                    }
                }
            }
        }
        DimMode::Ordinate { axis } => {
            let along_x = axis != Axis::Vertical;
            // The datum mark.
            let h = 1.5 * ctx.paper;
            thin_line(&mut out, p[0] - DVec2::X * h, p[0] + DVec2::X * h);
            thin_line(&mut out, p[0] - DVec2::Y * h, p[0] + DVec2::Y * h);
            let gap = style.ext_gap_mm * ctx.paper;
            for (i, q) in p[1..].iter().enumerate() {
                let sign = if off < 0.0 { -1.0 } else { 1.0 };
                if along_x {
                    // A leader goes up (or down) from the point to the text.
                    let y = p[0].y + off;
                    thin_line(&mut out, *q + DVec2::new(0.0, gap * sign), DVec2::new(q.x, y));
                    let hh = style.text_size_mm * ctx.paper;
                    let yc = y + sign * (style.text_gap_mm * ctx.paper + hh / 2.0);
                    out.push(Prim::text(DVec2::new(q.x, yc - hh * 0.35), &texts[i], hh, true, style.color));
                } else {
                    let x = p[0].x + off;
                    thin_line(&mut out, *q + DVec2::new(gap * sign, 0.0), DVec2::new(x, q.y));
                    let hh = style.text_size_mm * ctx.paper;
                    let w = text_width(&texts[i], hh);
                    let xc = x + sign * (style.text_gap_mm * ctx.paper + w / 2.0);
                    out.push(Prim::text(DVec2::new(xc, q.y - hh * 0.35), &texts[i], hh, true, style.color));
                }
            }
        }
        DimMode::Angular => {
            let v = p[0];
            let (a1, a2) = ((p[1] - v).to_angle(), (p[2] - v).to_angle());
            let sweep = (a2 - a1).rem_euclid(TAU);
            let r = if d.offset.abs() > 0.0 { d.offset.abs() * ctx.paper } else { 0.6 * (p[1] - v).length().min((p[2] - v).length()) };
            if r > 0.0 && sweep > 1e-9 {
                let gap = style.ext_gap_mm * ctx.paper;
                let over = style.ext_overshoot_mm * ctx.paper;
                for (q, a) in [(p[1], a1), (p[2], a2)] {
                    let dir = DVec2::from_angle(a);
                    let reach = r.max((q - v).length());
                    if reach + over > gap {
                        thin_line(&mut out, v + dir * gap.min(reach), v + dir * (reach.max(r) + over));
                    }
                }
                out.push(Prim::poly(arc_points(v, r, a1, sweep), false, Fill::None));
                let tip = |a: f64, sign: f64| DVec2::from_angle(a).perp() * sign;
                terminator(&mut out, style.terminator, size, v + DVec2::from_angle(a1) * r, -tip(a1, 1.0));
                terminator(&mut out, style.terminator, size, v + DVec2::from_angle(a2) * r, tip(a2, 1.0));
                let mid = DVec2::from_angle(a1 + sweep / 2.0);
                text_beside(&mut out, &texts[0], style, ctx, v + mid * r, mid);
            }
        }
        DimMode::Radial => {
            let (c, e) = (p[0], p[1]);
            let dir = (e - c).normalize_or_zero();
            thin_line(&mut out, c, e);
            terminator(&mut out, style.terminator, size, e, dir);
            let h = style.text_size_mm * ctx.paper;
            let w = text_width(&texts[0], h);
            let mid = e + dir * (style.text_gap_mm * ctx.paper + (dir.x.abs() * w + dir.y.abs() * h) / 2.0);
            out.push(Prim::text(mid - DVec2::new(0.0, h * 0.35), &texts[0], h, true, style.color));
        }
        DimMode::Diameter => {
            let (c, e) = (p[0], p[1]);
            let dir = (e - c).normalize_or_zero();
            let f = c - (e - c);
            thin_line(&mut out, f, e);
            terminator(&mut out, style.terminator, size, e, dir);
            terminator(&mut out, style.terminator, size, f, -dir);
            let h = style.text_size_mm * ctx.paper;
            let w = text_width(&texts[0], h);
            let mid = e + dir * (style.text_gap_mm * ctx.paper + (dir.x.abs() * w + dir.y.abs() * h) / 2.0);
            out.push(Prim::text(mid - DVec2::new(0.0, h * 0.35), &texts[0], h, true, style.color));
        }
        DimMode::ArcLength => {
            let c = p[0];
            let r0 = c.distance(p[1]);
            let a1 = (p[1] - c).to_angle();
            let sweep = ((p[2] - c).to_angle() - a1).rem_euclid(TAU);
            let r = r0 + if d.offset.abs() > 0.0 { d.offset * ctx.paper } else { 6.0 * ctx.paper };
            let over = style.ext_overshoot_mm * ctx.paper;
            let gap = style.ext_gap_mm * ctx.paper;
            if r > 0.0 && sweep > 1e-9 {
                for a in [a1, a1 + sweep] {
                    let dir = DVec2::from_angle(a);
                    if (r - r0).abs() > gap {
                        let s = (r - r0).signum();
                        thin_line(&mut out, c + dir * (r0 + gap * s), c + dir * (r + over * s));
                    }
                }
                out.push(Prim::poly(arc_points(c, r, a1, sweep), false, Fill::None));
                let tangent = |a: f64| DVec2::from_angle(a).perp();
                terminator(&mut out, style.terminator, size, c + DVec2::from_angle(a1) * r, -tangent(a1));
                terminator(&mut out, style.terminator, size, c + DVec2::from_angle(a1 + sweep) * r, tangent(a1 + sweep));
                let mid = DVec2::from_angle(a1 + sweep / 2.0);
                text_beside(&mut out, &texts[0], style, ctx, c + mid * r, mid);
            }
        }
    }
    out
}
