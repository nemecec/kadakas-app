//! Trim, extend, split, fillet and chamfer on chains of lines and arcs.
//!
//! All functions are pure: they take chains and clicked points and give new chains. The commands
//! and the API wrap them. A clicked point (a "pick") tells which part of an object the user means.

use std::f64::consts::PI;

use glam::DVec2;

use super::piece::{Chain, Piece, T_EPS, curve_intersections, piece_intersections};

const POS_EPS: f64 = 1e-9;

fn join_chains(mut a: Vec<Piece>, b: Vec<Piece>) -> Vec<Piece> {
    a.extend(b);
    a
}

/// The cut positions of `target` against `cutters`: piece index plus parameter, sorted, without
/// repeats. A closed chain has positions in 0..n.
fn cut_positions(target: &Chain, cutters: &[&Chain]) -> Vec<f64> {
    let n = target.pieces.len() as f64;
    let mut out = Vec::new();
    for (i, p) in target.pieces.iter().enumerate() {
        for c in cutters {
            for q in &c.pieces {
                for (tp, _, _) in piece_intersections(p, q) {
                    out.push(i as f64 + tp);
                }
            }
        }
    }
    if target.closed {
        for s in &mut out {
            if *s >= n - POS_EPS {
                *s = 0.0;
            }
        }
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < POS_EPS);
    out
}

fn position_of(chain: &Chain, pick: DVec2) -> f64 {
    let (i, t, _, _) = chain.closest(pick);
    i as f64 + t
}

/// Removes the part of `target` that the click is on, between the nearest crossings with the
/// cutters. Returns the chains that stay: none, one or two. An open chain gets two when the click
/// is between two crossings. A closed chain becomes one open chain.
pub fn trim(target: &Chain, pick: DVec2, cutters: &[&Chain]) -> Result<Vec<Chain>, String> {
    let cuts = cut_positions(target, cutters);
    if cuts.is_empty() {
        return Err("nothing crosses the object near the click".into());
    }
    let n = target.pieces.len() as f64;
    let sp = position_of(target, pick);
    if !target.closed {
        let lower = cuts.iter().rev().find(|&&s| s < sp - POS_EPS).copied();
        let upper = cuts.iter().find(|&&s| s > sp + POS_EPS).copied();
        let mut out = Vec::new();
        if let Some(l) = lower {
            let p = target.between(0.0, l);
            if !p.is_empty() {
                out.push(Chain::open(p));
            }
        }
        if let Some(u) = upper {
            let p = target.between(u, n);
            if !p.is_empty() {
                out.push(Chain::open(p));
            }
        }
        return Ok(out);
    }
    if cuts.len() < 2 {
        return Err("a closed shape needs two crossings to trim a part of it".into());
    }
    // The cut before the click and the cut after it, going around.
    let lower = cuts
        .iter()
        .rev()
        .find(|&&s| s <= sp + POS_EPS)
        .copied()
        .unwrap_or(*cuts.last().expect("not empty"));
    let upper = cuts
        .iter()
        .find(|&&s| s > sp + POS_EPS)
        .copied()
        .unwrap_or(cuts[0]);
    // Keep the way from `upper` forward to `lower`.
    let pieces = if upper < lower {
        target.between(upper, lower)
    } else {
        join_chains(target.between(upper, n), target.between(0.0, lower))
    };
    Ok(if pieces.is_empty() {
        Vec::new()
    } else {
        vec![Chain::open(pieces)]
    })
}

/// Extends the end of an open chain that is nearer to the click, along its last piece, to the
/// nearest crossing with a boundary.
pub fn extend(target: &Chain, pick: DVec2, boundaries: &[&Chain]) -> Result<Chain, String> {
    if target.closed {
        return Err("a closed shape has no end to extend".into());
    }
    let at_start = pick.distance(target.start()) < pick.distance(target.end());
    let mut pieces = target.pieces.clone();
    if at_start {
        pieces = pieces.iter().rev().map(Piece::reversed).collect();
    }
    let last = *pieces.last().expect("not empty");
    let mut best: Option<(f64, DVec2)> = None;
    for b in boundaries {
        for q in &b.pieces {
            for x in curve_intersections(&last, q) {
                if !Piece::has_param(q.param_of(x)) {
                    continue;
                }
                // How far beyond the end of the last piece, as a parameter step.
                let t = match last {
                    Piece::Line { .. } => last.param_of(x),
                    Piece::Arc { start, sweep, c, .. } => {
                        let end = start + sweep;
                        let u = (((x - c).to_angle() - end) * sweep.signum()).rem_euclid(PI * 2.0);
                        1.0 + u / sweep.abs()
                    }
                };
                if t > 1.0 + T_EPS && best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, x));
                }
            }
        }
    }
    let Some((t, x)) = best else {
        return Err("no boundary lies beyond the end of the object".into());
    };
    let n = pieces.len();
    pieces[n - 1] = match last {
        Piece::Line { a, .. } => Piece::Line { a, b: x },
        Piece::Arc { c, r, start, sweep } => Piece::Arc {
            c,
            r,
            start,
            sweep: sweep * t,
        },
    };
    if at_start {
        pieces = pieces.iter().rev().map(Piece::reversed).collect();
    }
    Ok(Chain::open(pieces))
}

/// Cuts a chain at the points of it that are nearest to `at`. Returns the pieces in order.
/// A closed chain cut at one point becomes one open chain.
pub fn split(chain: &Chain, at: &[DVec2]) -> Result<Vec<Chain>, String> {
    let n = chain.pieces.len() as f64;
    let mut cuts: Vec<f64> = at.iter().map(|p| position_of(chain, *p)).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < POS_EPS);
    if chain.closed {
        for s in &mut cuts {
            if *s >= n - POS_EPS {
                *s = 0.0;
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < POS_EPS);
        if cuts.is_empty() {
            return Err("give at least one point on the shape".into());
        }
        let k = cuts.len();
        let mut out = Vec::new();
        for i in 0..k {
            let (a, b) = (cuts[i], cuts[(i + 1) % k]);
            let pieces = if b > a {
                chain.between(a, b)
            } else {
                join_chains(chain.between(a, n), chain.between(0.0, b))
            };
            if !pieces.is_empty() {
                out.push(Chain::open(pieces));
            }
        }
        return Ok(out);
    }
    cuts.retain(|&s| s > POS_EPS && s < n - POS_EPS);
    if cuts.is_empty() {
        return Err("the point is at an end of the object, so there is nothing to split".into());
    }
    let mut bounds = vec![0.0];
    bounds.extend(cuts);
    bounds.push(n);
    Ok(bounds
        .windows(2)
        .map(|w| Chain::open(chain.between(w[0], w[1])))
        .filter(|c| !c.pieces.is_empty())
        .collect())
}

/// What the fillet or chamfer found for two pieces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joint {
    /// The new arc (fillet) or line (chamfer). It runs from the first piece to the second.
    pub piece: Piece,
    /// Where it meets the first and the second piece, as parameters on those pieces.
    pub t1: f64,
    pub t2: f64,
    /// True if the part of the piece that stays after a cut holds its end (false: its start).
    pub keep_end1: bool,
    pub keep_end2: bool,
}

/// Which side of the infinite curve of `p` a point is on: -1, 0 or 1.
fn side_of(p: &Piece, q: DVec2) -> f64 {
    match *p {
        Piece::Line { a, b } => (b - a).perp_dot(q - a).signum(),
        Piece::Arc { c, r, .. } => (q.distance(c) - r).signum(),
    }
}

/// The point of the infinite curve nearest to `q`.
fn nearest_on_curve(p: &Piece, q: DVec2) -> Option<DVec2> {
    match *p {
        Piece::Line { a, b } => {
            let d = b - a;
            let l2 = d.length_squared();
            (l2 > 0.0).then(|| a + d * ((q - a).dot(d) / l2))
        }
        Piece::Arc { c, r, .. } => (q - c).try_normalize().map(|u| c + u * r),
    }
}

/// The fillet of radius `r` between two pieces. The picks choose the corner and the side. The
/// pieces are taken as infinite lines and full circles, so a short piece is extended.
pub fn fillet_joint(p1: &Piece, pick1: DVec2, p2: &Piece, pick2: DVec2, r: f64) -> Result<Joint, String> {
    if !r.is_finite() || r <= 0.0 {
        return Err("the radius must be above zero".into());
    }
    let offsets = |p: &Piece| -> Vec<Piece> {
        match *p {
            Piece::Line { .. } => [r, -r].iter().filter_map(|d| p.offset(*d)).collect(),
            Piece::Arc { c, r: rr, .. } => [rr + r, rr - r]
                .into_iter()
                .filter(|v| *v > 1e-12)
                .map(|v| Piece::Arc {
                    c,
                    r: v,
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                })
                .collect(),
        }
    };
    let mut best: Option<(f64, f64, DVec2, DVec2, DVec2)> = None;
    for o1 in offsets(p1) {
        for o2 in offsets(p2) {
            for cc in curve_intersections(&o1, &o2) {
                let (Some(t1), Some(t2)) = (nearest_on_curve(p1, cc), nearest_on_curve(p2, cc))
                else {
                    continue;
                };
                // The center must be on the side of each piece where the click on the other is.
                let ok = side_of(p1, cc) == side_of(p1, pick2) && side_of(p2, cc) == side_of(p2, pick1);
                let score = t1.distance(pick1) + t2.distance(pick2);
                let rank = if ok { 0.0 } else { 1.0 };
                if best.is_none_or(|(br, bs, ..)| (rank, score) < (br, bs)) {
                    best = Some((rank, score, cc, t1, t2));
                }
            }
        }
    }
    let Some((_, _, c, a, b)) = best else {
        return Err("a fillet cannot join parallel or concentric objects, or the radius is too large".into());
    };
    let (a1, a2) = ((a - c).to_angle(), (b - c).to_angle());
    let mut d = (a2 - a1).rem_euclid(PI * 2.0);
    if d > PI {
        d -= PI * 2.0;
    }
    if d.abs() < 1e-9 {
        return Err("the two objects are tangent, so there is no corner to round".into());
    }
    Ok(Joint {
        piece: Piece::Arc {
            c,
            r,
            start: a1,
            sweep: d,
        },
        t1: p1.param_of(a),
        t2: p2.param_of(b),
        keep_end1: p1.param_of(pick1) >= p1.param_of(a),
        keep_end2: p2.param_of(pick2) >= p2.param_of(b),
    })
}

/// How the size of a chamfer is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSize {
    /// Distances from the corner along the first and the second piece.
    Distances(f64, f64),
    /// Distance along the first piece, and the angle (radians) between that piece and the chamfer.
    DistanceAngle(f64, f64),
    /// The length of the chamfer line. The two distances are equal.
    Length(f64),
}

/// The chamfer between two pieces. Like the fillet it takes the pieces as infinite curves.
pub fn chamfer_joint(
    p1: &Piece,
    pick1: DVec2,
    p2: &Piece,
    pick2: DVec2,
    size: ChamferSize,
) -> Result<Joint, String> {
    let corners = curve_intersections(p1, p2);
    let Some(x) = corners.into_iter().min_by(|a, b| {
        (a.distance(pick1) + a.distance(pick2)).total_cmp(&(b.distance(pick1) + b.distance(pick2)))
    }) else {
        return Err("a chamfer cannot join parallel objects".into());
    };
    let (tx1, tx2) = (p1.param_of(x), p2.param_of(x));
    let dir = |p: &Piece, tx: f64, pick: DVec2| {
        let tp = p.param_of(pick);
        if tp >= tx { 1.0 } else { -1.0 }
    };
    let (s1, s2) = (dir(p1, tx1, pick1), dir(p2, tx2, pick2));
    let (u1, u2) = (p1.tangent(tx1) * s1, p2.tangent(tx2) * s2);
    let gamma = u1.dot(u2).clamp(-1.0, 1.0).acos();
    if !(1e-6..PI - 1e-6).contains(&gamma) {
        return Err("a chamfer cannot join parallel objects".into());
    }
    let (d1, d2) = match size {
        ChamferSize::Distances(a, b) => (a, b),
        ChamferSize::DistanceAngle(d, alpha) => {
            let den = (gamma + alpha).sin();
            if !(alpha > 0.0 && alpha < PI) || den <= 1e-9 {
                return Err("this angle does not reach the second object".into());
            }
            (d, d * alpha.sin() / den)
        }
        ChamferSize::Length(l) => {
            let d = l / (2.0 * (gamma / 2.0).sin());
            (d, d)
        }
    };
    if !(d1.is_finite() && d2.is_finite() && d1 > 0.0 && d2 > 0.0) {
        return Err("the chamfer distances must be above zero".into());
    }
    let (t1, t2) = (tx1 + s1 * d1 / p1.length(), tx2 + s2 * d2 / p2.length());
    Ok(Joint {
        piece: Piece::Line {
            a: p1.point(t1),
            b: p2.point(t2),
        },
        t1,
        t2,
        keep_end1: s1 > 0.0,
        keep_end2: s2 > 0.0,
    })
}

/// The part of a piece that stays after a cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kept {
    pub piece: Piece,
    /// The part that is cut off, if the cut is inside the piece.
    pub rest: Option<Piece>,
    /// True if the kept part holds the end of the original piece, false if it holds the start.
    pub keeps_end: bool,
}

/// What stays of `p` when it is cut at parameter `t`, with the click `pick` on the part to keep.
/// A cut beyond the piece extends it. It is an error if nothing would stay.
pub fn keep_side(p: &Piece, t: f64, pick: DVec2) -> Result<Kept, String> {
    keep_part(p, t, p.param_of(pick) >= t)
}

/// Like `keep_side`, with the side given: the end of the piece or its start.
pub fn keep_part(p: &Piece, t: f64, keeps_end: bool) -> Result<Kept, String> {
    let (piece, rest) = if keeps_end {
        (p.sub(t, 1.0f64.max(t + 1e-12)), (t > 0.0).then(|| p.sub(0.0, t)))
    } else {
        (p.sub(0.0f64.min(t - 1e-12), t), (t < 1.0).then(|| p.sub(t, 1.0)))
    };
    let too_short = !(piece.length() > 1e-9 * p.length() && piece.length().is_finite())
        || matches!(piece, Piece::Arc { sweep, .. } if sweep.abs() > PI * 2.0 + 1e-9);
    if too_short {
        return Err("the object is too short for this size".into());
    }
    Ok(Kept {
        piece,
        rest: rest.filter(|r| r.length() > 1e-9 * p.length()),
        keeps_end,
    })
}

/// What to do at the corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JoinKind {
    Fillet(f64),
    Chamfer(ChamferSize),
}

fn joint(kind: JoinKind, p1: &Piece, k1: DVec2, p2: &Piece, k2: DVec2) -> Result<Joint, String> {
    match kind {
        JoinKind::Fillet(r) => fillet_joint(p1, k1, p2, k2, r),
        JoinKind::Chamfer(s) => chamfer_joint(p1, k1, p2, k2, s),
    }
}

/// Rounds or bevels the corners `joints` of one chain. Corner `j` lies between piece `j` and the
/// next piece (for a closed chain the next of the last piece is the first). The joint replaces the
/// corner and stays a part of the chain. Fails if a piece is too short for its two corners.
pub fn round_corners(chain: &Chain, joints: &[usize], kind: JoinKind) -> Result<Chain, String> {
    let n = chain.pieces.len();
    let last_joint = if chain.closed { n } else { n.saturating_sub(1) };
    let mut js: Vec<usize> = joints.to_vec();
    js.sort_unstable();
    js.dedup();
    if js.is_empty() {
        return Err("give at least one corner".into());
    }
    if js.iter().any(|&j| j >= last_joint) {
        return Err("this corner does not exist".into());
    }
    // For each piece: the parameter range that stays, and the joint to put after it.
    let mut lo = vec![0.0f64; n];
    let mut hi = vec![1.0f64; n];
    let mut after: Vec<Option<Piece>> = vec![None; n];
    for &j in &js {
        let (a, b) = (j, (j + 1) % n);
        let (p1, p2) = (chain.pieces[a], chain.pieces[b]);
        let d = joint(kind, &p1, p1.point(0.5), &p2, p2.point(0.5))?;
        hi[a] = d.t1;
        lo[b] = d.t2;
        after[a] = Some(d.piece);
    }
    let mut pieces = Vec::new();
    for i in 0..n {
        if lo[i] > hi[i] - 1e-9 {
            return Err("a segment is too short for the size at its corners".into());
        }
        if lo[i] < -T_EPS || hi[i] > 1.0 + T_EPS {
            // The joint lies beyond the piece: the piece is extended. Only a line or an arc that
            // stays inside one turn can do that.
            if let Piece::Arc { sweep, .. } = chain.pieces[i]
                && sweep.abs() * (hi[i] - lo[i]) > PI * 2.0
            {
                return Err("a segment is too short for the size at its corners".into());
            }
        }
        pieces.push(chain.pieces[i].sub(lo[i], hi[i]));
        if let Some(j) = after[i] {
            pieces.push(j);
        }
    }
    Ok(Chain {
        pieces,
        closed: chain.closed,
    })
}

/// How the originals change when a joint joins two separate objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum JoinMode {
    /// Make the joint and leave the objects alone.
    Standard,
    /// Cut the objects at the joint. The cut-off parts go.
    #[default]
    Trim,
    /// Cut the objects at the joint and keep the cut-off parts as separate objects.
    Split,
}

/// The result of joining two separate objects.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    /// The first and second chain after the cut (unchanged in `Standard` mode).
    pub a: Chain,
    pub b: Chain,
    pub joint: Piece,
    /// Cut-off parts, in `Split` mode.
    pub rest: Vec<Chain>,
}

fn cut_end(chain: &Chain, i: usize, k: Kept) -> Result<(Chain, Option<Chain>), String> {
    let n = chain.pieces.len();
    let mut pieces = chain.pieces.clone();
    // The cut end must be an end of the chain: the first piece loses its start, the last its end.
    let ok = !chain.closed && ((i == 0 && k.keeps_end) || (i + 1 == n && !k.keeps_end));
    if !ok {
        return Err("to round a corner between two objects, click their end segments. Inside a polyline or a closed shape, click two adjacent segments of it".into());
    }
    pieces[i] = k.piece;
    Ok((Chain::open(pieces), k.rest.map(|r| Chain::open(vec![r]))))
}

/// Joins two separate chains at the segments under the clicks. The segments must be the end
/// segments of open chains.
pub fn join_objects(
    a: &Chain,
    pick_a: DVec2,
    b: &Chain,
    pick_b: DVec2,
    kind: JoinKind,
    mode: JoinMode,
) -> Result<Joined, String> {
    let (ia, ..) = a.closest(pick_a);
    let (ib, ..) = b.closest(pick_b);
    let (p1, p2) = (a.pieces[ia], b.pieces[ib]);
    let j = joint(kind, &p1, pick_a, &p2, pick_b)?;
    if mode == JoinMode::Standard {
        return Ok(Joined {
            a: a.clone(),
            b: b.clone(),
            joint: j.piece,
            rest: Vec::new(),
        });
    }
    let (na, ra) = cut_end(a, ia, keep_part(&p1, j.t1, j.keep_end1)?)?;
    let (nb, rb) = cut_end(b, ib, keep_part(&p2, j.t2, j.keep_end2)?)?;
    let mut rest = Vec::new();
    if mode == JoinMode::Split {
        rest.extend(ra);
        rest.extend(rb);
    }
    Ok(Joined {
        a: na,
        b: nb,
        joint: j.piece,
        rest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::corners;
    use crate::model::Shape;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn line(a: DVec2, b: DVec2) -> Chain {
        Chain::open(vec![Piece::line(a, b)])
    }

    fn poly(pts: &[DVec2], closed: bool) -> Chain {
        Chain::from_shape(&Shape::Polyline {
            points: corners(pts),
            closed,
        })
        .unwrap()
    }

    #[test]
    fn trim_a_line_between_two_cutters() {
        let t = line(v(0.0, 0.0), v(10.0, 0.0));
        let c1 = line(v(3.0, -1.0), v(3.0, 1.0));
        let c2 = line(v(7.0, -1.0), v(7.0, 1.0));
        let out = trim(&t, v(5.0, 0.0), &[&c1, &c2]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].end(), v(3.0, 0.0));
        assert_eq!(out[1].start(), v(7.0, 0.0));
        // The end part has one crossing only.
        let out = trim(&t, v(9.0, 0.0), &[&c1, &c2]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].end(), v(7.0, 0.0));
        assert!(trim(&t, v(5.0, 0.0), &[]).is_err());
    }

    #[test]
    fn trim_a_closed_shape_opens_it() {
        let sq = poly(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)], true);
        let cut = line(v(5.0, -5.0), v(5.0, 15.0));
        // The click is on the left half: that half goes, the right half stays (5 + 10 + 5).
        let out = trim(&sq, v(2.0, 0.0), &[&cut]).unwrap();
        assert_eq!(out.len(), 1);
        let len = out[0].length();
        assert!((len - 20.0).abs() < 1e-9, "{len}");
        // One crossing is not enough.
        let short = line(v(5.0, -5.0), v(5.0, 5.0));
        assert!(trim(&sq, v(2.0, 0.0), &[&short]).is_err());
    }

    #[test]
    fn trim_a_circle_with_a_line() {
        let c = Chain::circle(v(0.0, 0.0), 5.0).unwrap();
        let l = line(v(-10.0, 0.0), v(10.0, 0.0));
        let out = trim(&c, v(0.0, 5.0), &[&l]).unwrap();
        assert_eq!(out.len(), 1);
        // The upper half is gone, half a circle stays.
        assert!((out[0].length() - PI * 5.0).abs() < 1e-9);
        assert!(out[0].start().distance(v(-5.0, 0.0)) < 1e-9 || out[0].start().distance(v(5.0, 0.0)) < 1e-9);
    }

    #[test]
    fn extend_to_a_boundary() {
        let t = line(v(0.0, 0.0), v(4.0, 0.0));
        let b = line(v(10.0, -5.0), v(10.0, 5.0));
        let e = extend(&t, v(3.5, 0.0), &[&b]).unwrap();
        assert_eq!(e.end(), v(10.0, 0.0));
        // Clicking near the start extends the start, which points away from the boundary.
        assert!(extend(&t, v(0.2, 0.0), &[&b]).is_err());
        let b2 = line(v(-3.0, -5.0), v(-3.0, 5.0));
        assert_eq!(extend(&t, v(0.2, 0.0), &[&b2]).unwrap().start(), v(-3.0, 0.0));
        assert!(extend(&poly(&[v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)], true), v(0.0, 0.0), &[&b]).is_err());
    }

    #[test]
    fn extend_an_arc_along_its_circle() {
        let arc = Chain::open(vec![Piece::Arc {
            c: v(0.0, 0.0),
            r: 5.0,
            start: 0.0,
            sweep: 1.0,
        }]);
        let b = line(v(-3.0, -10.0), v(-3.0, 10.0));
        // The boundary crosses the circle at (-3, 4): the arc grows counterclockwise.
        let e = extend(&arc, arc.end(), &[&b]).unwrap();
        let Piece::Arc { sweep, c, .. } = e.pieces[0] else {
            panic!()
        };
        assert!(sweep > 1.0);
        assert!(b.pieces[0].closest(e.end()).2 < 1e-9);
        assert!((e.end().distance(c) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn split_open_and_closed_chains() {
        let l = line(v(0.0, 0.0), v(10.0, 0.0));
        let out = split(&l, &[v(4.0, 2.0)]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].end(), v(4.0, 0.0));
        assert!((out[0].length() + out[1].length() - 10.0).abs() < 1e-12);
        assert!(split(&l, &[v(-1.0, 0.0)]).is_err());
        assert_eq!(split(&l, &[v(2.0, 0.0), v(8.0, 0.0)]).unwrap().len(), 3);
        let sq = poly(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)], true);
        let one = split(&sq, &[v(5.0, 0.0)]).unwrap();
        assert_eq!(one.len(), 1);
        assert!((one[0].length() - 40.0).abs() < 1e-9);
        let two = split(&sq, &[v(5.0, 0.0), v(5.0, 10.0)]).unwrap();
        assert_eq!(two.len(), 2);
        assert!((two[0].length() + two[1].length() - 40.0).abs() < 1e-9);
        assert!(split(&sq, &[]).is_err());
    }

    #[test]
    fn fillet_two_lines_trims_both() {
        let a = line(v(0.0, 0.0), v(10.0, 0.0));
        let b = line(v(0.0, 0.0), v(0.0, 10.0));
        let j = join_objects(&a, v(8.0, 0.0), &b, v(0.0, 8.0), JoinKind::Fillet(2.0), JoinMode::Trim).unwrap();
        // The fillet center is (2, 2) with tangent points (2, 0) and (0, 2).
        assert!(j.joint.from().distance(v(2.0, 0.0)) < 1e-9);
        assert!(j.joint.to().distance(v(0.0, 2.0)) < 1e-9);
        assert!(j.a.start().distance(v(2.0, 0.0)) < 1e-9);
        assert!(j.b.start().distance(v(0.0, 2.0)) < 1e-9);
        assert!(j.rest.is_empty());
        let Piece::Arc { c, r, sweep, .. } = j.joint else {
            panic!()
        };
        assert!(c.distance(v(2.0, 2.0)) < 1e-9);
        assert_eq!(r, 2.0);
        assert!((sweep.abs() - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn fillet_extends_short_lines_and_splits() {
        // The lines stop before the corner. The fillet extends them.
        let a = line(v(5.0, 0.0), v(10.0, 0.0));
        let b = line(v(0.0, 5.0), v(0.0, 10.0));
        let j = join_objects(&a, v(8.0, 0.0), &b, v(0.0, 8.0), JoinKind::Fillet(2.0), JoinMode::Split).unwrap();
        assert!(j.a.start().distance(v(2.0, 0.0)) < 1e-9);
        assert!(j.rest.is_empty(), "nothing was cut off");
        // Long lines, split mode: the corner pieces stay as separate objects.
        let a = line(v(-4.0, 0.0), v(10.0, 0.0));
        let b = line(v(0.0, -4.0), v(0.0, 10.0));
        let j = join_objects(&a, v(8.0, 0.0), &b, v(0.0, 8.0), JoinKind::Fillet(2.0), JoinMode::Split).unwrap();
        assert_eq!(j.rest.len(), 2);
        assert!((j.rest[0].length() - 6.0).abs() < 1e-9);
        // Standard mode changes nothing.
        let s = join_objects(&a, v(8.0, 0.0), &b, v(0.0, 8.0), JoinKind::Fillet(2.0), JoinMode::Standard).unwrap();
        assert_eq!(s.a, a);
        // The end segments only, and no parallel lines.
        let p = line(v(0.0, 5.0), v(10.0, 5.0));
        assert!(join_objects(&a, v(8.0, 0.0), &p, v(8.0, 5.0), JoinKind::Fillet(1.0), JoinMode::Trim).is_err());
        assert!(join_objects(&a, v(8.0, 0.0), &b, v(0.0, 8.0), JoinKind::Fillet(0.0), JoinMode::Trim).is_err());
    }

    #[test]
    fn fillet_between_a_line_and_a_circle() {
        let l = line(v(-20.0, 0.0), v(20.0, 0.0));
        let c = Chain::circle(v(0.0, 8.0), 5.0).unwrap();
        // Click the line to the right and the upper right of the circle.
        let (p1, p2) = (l.pieces[0], c.pieces[0]);
        let j = fillet_joint(&p1, v(10.0, 0.0), &p2, v(5.0, 8.0), 2.0).unwrap();
        let Piece::Arc { c: fc, r, .. } = j.piece else {
            panic!()
        };
        assert_eq!(r, 2.0);
        // The fillet is tangent to the line and to the circle.
        assert!((fc.y - 2.0).abs() < 1e-9);
        assert!((fc.distance(v(0.0, 8.0)) - 7.0).abs() < 1e-9 || (fc.distance(v(0.0, 8.0)) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn round_corners_of_a_polyline_and_a_rectangle() {
        let sq = poly(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)], true);
        let all: Vec<usize> = (0..4).collect();
        let r = round_corners(&sq, &all, JoinKind::Fillet(2.0)).unwrap();
        assert!(r.closed);
        assert_eq!(r.pieces.len(), 8);
        // Area of a rounded square: s^2 - (4 - pi) r^2.
        let area = chain_area(&r);
        assert!((area - (100.0 - (4.0 - PI) * 4.0)).abs() < 1e-6, "{area}");
        // Chamfer: the area loses four triangles.
        let ch = round_corners(&sq, &all, JoinKind::Chamfer(ChamferSize::Distances(2.0, 2.0))).unwrap();
        assert!((chain_area(&ch) - (100.0 - 4.0 * 2.0)).abs() < 1e-9);
        // One corner of an open polyline.
        let pl = poly(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)], false);
        let one = round_corners(&pl, &[0], JoinKind::Fillet(3.0)).unwrap();
        assert_eq!(one.pieces.len(), 3);
        assert!(one.length() < pl.length());
        // Too big a radius, a corner that is not there.
        assert!(round_corners(&sq, &all, JoinKind::Fillet(8.0)).is_err());
        assert!(round_corners(&pl, &[1], JoinKind::Fillet(1.0)).is_err());
        assert!(round_corners(&pl, &[], JoinKind::Fillet(1.0)).is_err());
    }

    fn chain_area(c: &Chain) -> f64 {
        let mut pts = Vec::new();
        for p in &c.pieces {
            let f = p.flatten(1e-7);
            pts.extend_from_slice(&f[..f.len() - 1]);
        }
        let n = pts.len();
        (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum::<f64>().abs() / 2.0
    }

    #[test]
    fn chamfer_sizes() {
        let a = Piece::line(v(0.0, 0.0), v(10.0, 0.0));
        let b = Piece::line(v(0.0, 0.0), v(0.0, 10.0));
        let j = chamfer_joint(&a, v(8.0, 0.0), &b, v(0.0, 8.0), ChamferSize::Distances(2.0, 3.0)).unwrap();
        assert!(j.piece.from().distance(v(2.0, 0.0)) < 1e-9);
        assert!(j.piece.to().distance(v(0.0, 3.0)) < 1e-9);
        // Equal distances from a given chamfer length.
        let j = chamfer_joint(&a, v(8.0, 0.0), &b, v(0.0, 8.0), ChamferSize::Length(2f64.sqrt() * 2.0)).unwrap();
        assert!(j.piece.from().distance(v(2.0, 0.0)) < 1e-9);
        // Distance and angle: 45 degrees on a right angle gives equal distances.
        let j = chamfer_joint(&a, v(8.0, 0.0), &b, v(0.0, 8.0), ChamferSize::DistanceAngle(2.0, PI / 4.0)).unwrap();
        assert!(j.piece.to().distance(v(0.0, 2.0)) < 1e-9);
        let par = Piece::line(v(0.0, 3.0), v(10.0, 3.0));
        assert!(chamfer_joint(&a, v(8.0, 0.0), &par, v(8.0, 3.0), ChamferSize::Length(1.0)).is_err());
        assert!(chamfer_joint(&a, v(8.0, 0.0), &b, v(0.0, 8.0), ChamferSize::Distances(-1.0, 1.0)).is_err());
        assert!(chamfer_joint(&a, v(8.0, 0.0), &b, v(0.0, 8.0), ChamferSize::DistanceAngle(1.0, 4.0)).is_err());
    }

    #[test]
    fn chamfer_two_objects_trims_them() {
        let a = line(v(0.0, 0.0), v(10.0, 0.0));
        let b = line(v(0.0, 0.0), v(0.0, 10.0));
        let j = join_objects(&a, v(8.0, 0.0), &b, v(0.0, 8.0), JoinKind::Chamfer(ChamferSize::Distances(3.0, 3.0)), JoinMode::Trim).unwrap();
        assert!(j.a.start().distance(v(3.0, 0.0)) < 1e-9);
        assert!(j.b.start().distance(v(0.0, 3.0)) < 1e-9);
        // A chamfer longer than the lines.
        assert!(join_objects(&a, v(8.0, 0.0), &b, v(0.0, 8.0), JoinKind::Chamfer(ChamferSize::Distances(30.0, 3.0)), JoinMode::Trim).is_err());
    }

    #[test]
    fn keep_side_cases() {
        let p = Piece::line(v(0.0, 0.0), v(10.0, 0.0));
        let k = keep_side(&p, 0.3, v(9.0, 0.0)).unwrap();
        assert_eq!(k.piece.from(), v(3.0, 0.0));
        assert_eq!(k.rest.unwrap().to(), v(3.0, 0.0));
        assert!(k.keeps_end);
        let k = keep_side(&p, 0.3, v(1.0, 0.0)).unwrap();
        assert_eq!(k.piece.to(), v(3.0, 0.0));
        // The cut is at the far end and the start is wanted: the whole piece stays.
        assert!(keep_side(&p, 1.0, v(9.0, 0.0)).is_ok());
        // The cut is at the start and the start is wanted: nothing stays.
        assert!(keep_part(&p, 0.0, false).is_err());
        assert!(keep_part(&p, 1.0, true).is_err());
    }
}
