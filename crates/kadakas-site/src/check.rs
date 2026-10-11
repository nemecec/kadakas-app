//! Checks of the source data of a site model, and a command to simplify it.
//!
//! Survey data often has errors that make a bad surface. [`check`] finds three kinds:
//!
//! - **Duplicate XY with different Z**: two source points at the same place with different
//!   heights. The triangulation keeps only the first one.
//! - **Spikes**: a point whose height is far from the height that its neighbors predict
//!   (a plane through the neighbors in the triangulation).
//! - **Crossing breaklines**: two line segments that cross in plan view at different heights.
//!   The triangulation keeps the first segment and drops the other.
//!
//! [`simplify`] removes points and line vertices that the surface does not need: copies, loose
//! points that their neighbors predict within a tolerance, and line vertices that lie within a
//! tolerance of the line through their neighbors.

use std::collections::{HashMap, HashSet};

use glam::{DVec2, DVec3};

use kadakas_core::site::SiteLine;

use crate::tin::{SourceData, SourceLine, Tin};

/// Where a source vertex comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// A loose point: its index in the list of points.
    Point(usize),
    /// A vertex of a line: the index of the line and of the vertex.
    Vertex { line: usize, vertex: usize },
}

/// What a check found.
#[derive(Clone, Debug, PartialEq)]
pub enum Issue {
    /// Source points at one place with heights that differ by more than the tolerance.
    DuplicateXy {
        xy: DVec2,
        sources: Vec<Source>,
        z_min: f64,
        z_max: f64,
    },
    /// A point far off the surface of its neighbors.
    Spike {
        source: Source,
        xy: DVec2,
        z: f64,
        expected: f64,
        deviation: f64,
    },
    /// Two segments that cross in plan view at different heights. A segment is a line index
    /// and the index of its first vertex.
    CrossingBreaklines {
        a: (usize, usize),
        b: (usize, usize),
        xy: DVec2,
        z_a: f64,
        z_b: f64,
    },
}

impl Issue {
    /// A short name of the kind: `duplicate_xy`, `spike` or `crossing_breaklines`.
    pub fn kind(&self) -> &'static str {
        match self {
            Issue::DuplicateXy { .. } => "duplicate_xy",
            Issue::Spike { .. } => "spike",
            Issue::CrossingBreaklines { .. } => "crossing_breaklines",
        }
    }
}

/// The limits of the checks, in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckOptions {
    /// Two points closer than this in plan view are at one place. Default 1.
    pub xy_tolerance: f64,
    /// Heights that differ by more than this are different. Default 10.
    pub z_tolerance: f64,
    /// A point is a spike when it is farther than this from the height that its neighbors
    /// predict. Default 1000.
    pub spike_threshold: f64,
    /// The most issues to list. The counts stay exact. Default 1000.
    pub max_issues: usize,
}

impl Default for CheckOptions {
    fn default() -> Self {
        CheckOptions {
            xy_tolerance: 1.0,
            z_tolerance: 10.0,
            spike_threshold: 1000.0,
            max_issues: 1000,
        }
    }
}

impl CheckOptions {
    /// True if all limits are finite and positive (the issue count may be any number).
    pub fn is_valid(&self) -> bool {
        [self.xy_tolerance, self.z_tolerance, self.spike_threshold]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
    }
}

/// The result of [`check`].
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CheckReport {
    pub issues: Vec<Issue>,
    pub vertices_checked: usize,
    pub duplicate_xy: usize,
    /// Copies with the same height. They are harmless, and `simplify` removes them.
    pub exact_duplicates: usize,
    pub spikes: usize,
    pub crossings: usize,
    /// True if more issues were found than `max_issues`.
    pub truncated: bool,
}

impl CheckReport {
    pub fn is_clean(&self) -> bool {
        self.duplicate_xy + self.spikes + self.crossings == 0
    }
}

struct Item {
    p: DVec3,
    src: Source,
}

fn items(points: &[DVec3], lines: &[SiteLine]) -> Vec<Item> {
    let mut v: Vec<Item> = points
        .iter()
        .enumerate()
        .map(|(i, p)| Item {
            p: *p,
            src: Source::Point(i),
        })
        .collect();
    for (l, line) in lines.iter().enumerate() {
        for (i, p) in line.points.iter().enumerate() {
            v.push(Item {
                p: *p,
                src: Source::Vertex { line: l, vertex: i },
            });
        }
    }
    v
}

fn cell_of(p: DVec2, size: f64) -> (i64, i64) {
    ((p.x / size).floor() as i64, (p.y / size).floor() as i64)
}

/// Runs all checks on the source data.
pub fn check(points: &[DVec3], lines: &[SiteLine], opts: &CheckOptions) -> CheckReport {
    let mut r = CheckReport::default();
    let all = items(points, lines);
    r.vertices_checked = all.len();
    let push = |r: &mut CheckReport, issue: Issue| {
        if r.issues.len() < opts.max_issues {
            r.issues.push(issue);
        } else {
            r.truncated = true;
        }
    };

    // Duplicate XY. Points are grouped in cells of the tolerance and compared with the cells around.
    let size = opts.xy_tolerance.max(1e-9);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, it) in all.iter().enumerate() {
        grid.entry(cell_of(it.p.truncate(), size))
            .or_default()
            .push(i);
    }
    let mut group_of: Vec<Option<usize>> = vec![None; all.len()];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..all.len() {
        if group_of[i].is_some() {
            continue;
        }
        let (cx, cy) = cell_of(all[i].p.truncate(), size);
        let mut members = vec![i];
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    if j > i
                        && group_of[j].is_none()
                        && all[j].p.truncate().distance(all[i].p.truncate()) <= opts.xy_tolerance
                    {
                        members.push(j);
                    }
                }
            }
        }
        if members.len() > 1 {
            for m in &members {
                group_of[*m] = Some(groups.len());
            }
            groups.push(members);
        }
    }
    for g in &groups {
        let zs: Vec<f64> = g.iter().map(|i| all[*i].p.z).collect();
        let (lo, hi) = zs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(*z), b.max(*z)));
        if hi - lo > opts.z_tolerance {
            r.duplicate_xy += 1;
            let first = all[g[0]].p.truncate();
            push(
                &mut r,
                Issue::DuplicateXy {
                    xy: first,
                    sources: g.iter().map(|i| all[*i].src).collect(),
                    z_min: lo,
                    z_max: hi,
                },
            );
        } else {
            r.exact_duplicates += 1;
        }
    }

    // Spikes.
    for s in spikes(&all, opts) {
        r.spikes += 1;
        push(&mut r, s);
    }

    // Crossing breaklines.
    for c in crossings(lines, opts) {
        r.crossings += 1;
        push(&mut r, c);
    }
    r
}

/// Solves a 3 by 3 system. `None` if it has no unique solution.
fn solve3(mut m: [[f64; 4]; 3]) -> Option<[f64; 3]> {
    for c in 0..3 {
        let piv = (c..3).max_by(|a, b| m[*a][c].abs().total_cmp(&m[*b][c].abs()))?;
        if m[piv][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, piv);
        for r in 0..3 {
            if r != c {
                let f = m[r][c] / m[c][c];
                let pivot_row = m[c];
                for (x, p) in m[r].iter_mut().zip(pivot_row).skip(c) {
                    *x -= f * p;
                }
            }
        }
    }
    Some([m[0][3] / m[0][0], m[1][3] / m[1][1], m[2][3] / m[2][2]])
}

/// The height at `at` of the plane that fits the neighbors best (least squares). With fewer than
/// three neighbors, or neighbors on a line, the mean height.
pub fn plane_prediction(at: DVec2, neighbors: &[DVec3]) -> Option<f64> {
    if neighbors.is_empty() {
        return None;
    }
    let n = neighbors.len() as f64;
    let mean = neighbors.iter().map(|p| p.z).sum::<f64>() / n;
    if neighbors.len() < 3 {
        return Some(mean);
    }
    // z = a*dx + b*dy + c with (dx, dy) measured from `at`.
    let (mut sxx, mut sxy, mut syy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut sxz, mut syz, mut sz) = (0.0, 0.0, 0.0);
    for p in neighbors {
        let (dx, dy) = (p.x - at.x, p.y - at.y);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
        sx += dx;
        sy += dy;
        sxz += dx * p.z;
        syz += dy * p.z;
        sz += p.z;
    }
    match solve3([[sxx, sxy, sx, sxz], [sxy, syy, sy, syz], [sx, sy, n, sz]]) {
        Some([_, _, c]) if c.is_finite() => Some(c),
        _ => Some(mean),
    }
}

/// The neighbors of each TIN vertex.
fn neighbors_of(tin: &Tin) -> Vec<Vec<u32>> {
    let mut nb: Vec<Vec<u32>> = vec![Vec::new(); tin.vertices.len()];
    for t in &tin.triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if !nb[a as usize].contains(&b) {
                nb[a as usize].push(b);
            }
            if !nb[b as usize].contains(&a) {
                nb[b as usize].push(a);
            }
        }
    }
    nb
}

fn xy_key(p: DVec3) -> (u64, u64) {
    ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits())
}

/// The deviation of each interior vertex of the TIN from the plane through its neighbors.
fn deviations(tin: &Tin) -> Vec<(f64, f64)> {
    let nb = neighbors_of(tin);
    let edge = tin.boundary_vertices();
    (0..tin.vertices.len())
        .map(|i| {
            let v = tin.vertices[i];
            if edge.contains(&(i as u32)) {
                return (0.0, v.z);
            }
            let ns: Vec<DVec3> = nb[i].iter().map(|j| tin.vertices[*j as usize]).collect();
            match plane_prediction(v.truncate(), &ns) {
                Some(e) => ((v.z - e).abs(), e),
                None => (0.0, v.z),
            }
        })
        .collect()
}

fn spikes(all: &[Item], opts: &CheckOptions) -> Vec<Issue> {
    let src = SourceData {
        points: all.iter().map(|i| i.p).collect(),
        lines: Vec::new(),
    };
    let Ok(tin) = Tin::build(&src) else {
        return Vec::new();
    };
    let mut first: HashMap<(u64, u64), Source> = HashMap::new();
    for it in all {
        first.entry(xy_key(it.p)).or_insert(it.src);
    }
    let dev = deviations(&tin);
    let nb = neighbors_of(&tin);
    let mut out = Vec::new();
    for (i, (d, e)) in dev.iter().enumerate() {
        if *d <= opts.spike_threshold {
            continue;
        }
        // Report the worst point of a group: a spike also bends the plane of its neighbors.
        if nb[i].iter().any(|j| dev[*j as usize].0 > *d) {
            continue;
        }
        let v = tin.vertices[i];
        if let Some(s) = first.get(&xy_key(v)) {
            out.push(Issue::Spike {
                source: *s,
                xy: v.truncate(),
                z: v.z,
                expected: *e,
                deviation: *d,
            });
        }
    }
    out.sort_by(|a, b| match (a, b) {
        (Issue::Spike { source: a, .. }, Issue::Spike { source: b, .. }) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    });
    out
}

/// Where segments `p..p2` and `q..q2` cross properly: the parameters along both.
fn cross_point(p: DVec2, p2: DVec2, q: DVec2, q2: DVec2) -> Option<(f64, f64)> {
    let (r, s) = (p2 - p, q2 - q);
    let d = r.perp_dot(s);
    if d.abs() < 1e-12 {
        return None;
    }
    let t = (q - p).perp_dot(s) / d;
    let u = (q - p).perp_dot(r) / d;
    // Meeting at an end is not a crossing.
    let eps = 1e-9;
    (t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps).then_some((t, u))
}

fn crossings(lines: &[SiteLine], opts: &CheckOptions) -> Vec<Issue> {
    // All segments, with a grid over their boxes.
    struct Seg {
        line: usize,
        i: usize,
        a: DVec3,
        b: DVec3,
    }
    let mut segs: Vec<Seg> = Vec::new();
    for (l, line) in lines.iter().enumerate() {
        let n = line.points.len();
        let count = if line.closed && n > 2 {
            n
        } else {
            n.saturating_sub(1)
        };
        for i in 0..count {
            segs.push(Seg {
                line: l,
                i,
                a: line.points[i],
                b: line.points[(i + 1) % n],
            });
        }
    }
    if segs.len() < 2 {
        return Vec::new();
    }
    let total: f64 = segs
        .iter()
        .map(|s| s.a.truncate().distance(s.b.truncate()))
        .sum();
    let size = (total / segs.len() as f64).max(1.0) * 2.0;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, s) in segs.iter().enumerate() {
        let (lo, hi) = (
            s.a.truncate().min(s.b.truncate()),
            s.a.truncate().max(s.b.truncate()),
        );
        let (c0, c1) = (cell_of(lo, size), cell_of(hi, size));
        // A very long segment would fill many cells. Limit the walk.
        if (c1.0 - c0.0 + 1).saturating_mul(c1.1 - c0.1 + 1) > 10_000 {
            grid.entry((i64::MIN, i64::MIN)).or_default().push(k);
            continue;
        }
        for x in c0.0..=c1.0 {
            for y in c0.1..=c1.1 {
                grid.entry((x, y)).or_default().push(k);
            }
        }
    }
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut out = Vec::new();
    let long = grid.get(&(i64::MIN, i64::MIN)).cloned().unwrap_or_default();
    for (cell, ks) in &grid {
        let mut ks = ks.clone();
        if *cell != (i64::MIN, i64::MIN) {
            ks.extend(long.iter().copied());
        }
        for x in 0..ks.len() {
            for y in x + 1..ks.len() {
                let (a, b) = (ks[x].min(ks[y]), ks[x].max(ks[y]));
                if a == b || !seen.insert((a, b)) {
                    continue;
                }
                let (s, t) = (&segs[a], &segs[b]);
                if let Some((u, v)) = cross_point(
                    s.a.truncate(),
                    s.b.truncate(),
                    t.a.truncate(),
                    t.b.truncate(),
                ) {
                    let (za, zb) = (s.a.z + (s.b.z - s.a.z) * u, t.a.z + (t.b.z - t.a.z) * v);
                    if (za - zb).abs() > opts.z_tolerance {
                        out.push(Issue::CrossingBreaklines {
                            a: (s.line, s.i),
                            b: (t.line, t.i),
                            xy: s.a.truncate() + (s.b.truncate() - s.a.truncate()) * u,
                            z_a: za,
                            z_b: zb,
                        });
                    }
                }
            }
        }
    }
    out.sort_by(|p, q| match (p, q) {
        (
            Issue::CrossingBreaklines { a: a1, b: b1, .. },
            Issue::CrossingBreaklines { a: a2, b: b2, .. },
        ) => (a1, b1).cmp(&(a2, b2)),
        _ => std::cmp::Ordering::Equal,
    });
    out
}

// ---------------------------------------------------------------------------
// Simplify

/// What [`simplify`] removed.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Simplified {
    pub points: Vec<DVec3>,
    pub lines: Vec<SiteLine>,
    pub removed_duplicates: usize,
    pub removed_points: usize,
    pub removed_vertices: usize,
}

/// Distance of `p` from the segment `a..b` in 3D.
fn seg_distance(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 == 0.0 {
        0.0
    } else {
        ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

/// Douglas-Peucker on a list of 3D vertices. Returns which ones stay.
fn dp_keep(pts: &[DVec3], tol: f64) -> Vec<bool> {
    let n = pts.len();
    let mut keep = vec![false; n];
    if n == 0 {
        return keep;
    }
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut worst, mut wi) = (0.0, a);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = seg_distance(*p, pts[a], pts[b]);
            if d > worst {
                worst = d;
                wi = i;
            }
        }
        if worst > tol {
            keep[wi] = true;
            stack.push((a, wi));
            stack.push((wi, b));
        }
    }
    keep
}

/// Removes what the surface does not need. `tolerance` (mm, positive) is the largest change of
/// height or line shape that is allowed. Lines keep their ends; a closed line keeps at least
/// three vertices. Loose points that sit on a line vertex are kept, as the triangulation uses
/// the first point at a place.
pub fn simplify(points: &[DVec3], lines: &[SiteLine], tolerance: f64) -> Simplified {
    let mut out = Simplified::default();
    // Lines first: they are constraints.
    for l in lines {
        let n = l.points.len();
        let keep = if l.closed && n > 3 {
            // Rotate so that the farthest vertex from the first one is a fixed second end.
            let far = (1..n)
                .max_by(|a, b| {
                    l.points[*a]
                        .distance_squared(l.points[0])
                        .total_cmp(&l.points[*b].distance_squared(l.points[0]))
                })
                .unwrap_or(1);
            let first: Vec<DVec3> = l.points[..=far].to_vec();
            let mut second: Vec<DVec3> = l.points[far..].to_vec();
            second.push(l.points[0]);
            let (k1, k2) = (dp_keep(&first, tolerance), dp_keep(&second, tolerance));
            let mut keep = vec![false; n];
            keep[..=far].copy_from_slice(&k1);
            for (i, k) in k2.iter().take(n - far).enumerate() {
                keep[far + i] |= *k;
            }
            keep
        } else {
            dp_keep(&l.points, tolerance)
        };
        let pts: Vec<DVec3> = l
            .points
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(p, _)| *p)
            .collect();
        out.removed_vertices += n - pts.len();
        out.lines.push(SiteLine {
            points: pts,
            closed: l.closed,
        });
    }

    // Copies: later points at the same place with (nearly) the same height.
    let line_places: HashSet<(u64, u64)> = lines
        .iter()
        .flat_map(|l| l.points.iter().map(|p| xy_key(*p)))
        .collect();
    let mut seen: HashMap<(u64, u64), f64> = HashMap::new();
    let mut loose: Vec<DVec3> = Vec::new();
    for p in points {
        if line_places.contains(&xy_key(*p)) {
            out.removed_duplicates += 1;
            continue;
        }
        match seen.get(&xy_key(*p)) {
            Some(z) if (z - p.z).abs() <= tolerance => out.removed_duplicates += 1,
            Some(_) => loose.push(*p), // different height: that is an error to look at, not to hide
            None => {
                seen.insert(xy_key(*p), p.z);
                loose.push(*p);
            }
        }
    }

    // Loose points that the neighbors predict.
    let mut removed_all: Vec<DVec3> = Vec::new();
    for _ in 0..8 {
        let mut all: Vec<DVec3> = loose.clone();
        let loose_count = all.len();
        for l in &out.lines {
            all.extend(l.points.iter().copied());
        }
        let Ok(tin) = Tin::build(&SourceData {
            points: all.clone(),
            lines: out
                .lines
                .iter()
                .map(|l| SourceLine {
                    points: l.points.clone(),
                    closed: l.closed,
                })
                .collect(),
        }) else {
            break;
        };
        let dev = deviations(&tin);
        let nb = neighbors_of(&tin);
        let index: HashMap<(u64, u64), usize> = tin
            .vertices
            .iter()
            .enumerate()
            .map(|(i, v)| (xy_key(*v), i))
            .collect();
        let mut remove: HashSet<usize> = HashSet::new();
        let mut blocked: HashSet<usize> = HashSet::new();
        // Smallest deviation first, and no two neighbors in one pass.
        let mut order: Vec<usize> = (0..loose_count).collect();
        order.sort_by(|a, b| {
            let da = index
                .get(&xy_key(loose[*a]))
                .map_or(f64::MAX, |i| dev[*i].0);
            let db = index
                .get(&xy_key(loose[*b]))
                .map_or(f64::MAX, |i| dev[*i].0);
            da.total_cmp(&db)
        });
        for k in order {
            let Some(&i) = index.get(&xy_key(loose[k])) else {
                continue;
            };
            if dev[i].0 <= tolerance
                && !blocked.contains(&i)
                && !tin.boundary_vertices().contains(&(i as u32))
            {
                remove.insert(k);
                blocked.insert(i);
                blocked.extend(nb[i].iter().map(|j| *j as usize));
            }
        }
        if remove.is_empty() {
            break;
        }
        // Check against the first data: the pass may not move the surface by more than the
        // tolerance at any point that was removed so far. Points that it moves too far stay.
        let mut gone = removed_all.clone();
        gone.extend(remove.iter().map(|k| loose[*k]));
        let kept: Vec<DVec3> = loose
            .iter()
            .enumerate()
            .filter(|(k, _)| !remove.contains(k))
            .map(|(_, p)| *p)
            .collect();
        let mut src: Vec<DVec3> = kept.clone();
        src.extend(out.lines.iter().flat_map(|l| l.points.iter().copied()));
        let Ok(candidate) = Tin::build(&SourceData {
            points: src,
            lines: out
                .lines
                .iter()
                .map(|l| SourceLine {
                    points: l.points.clone(),
                    closed: l.closed,
                })
                .collect(),
        }) else {
            break;
        };
        let too_far = |p: &DVec3| {
            candidate
                .elevation_at(p.truncate())
                .is_some_and(|z| (z - p.z).abs() > tolerance)
        };
        let restore: HashSet<usize> = remove
            .iter()
            .copied()
            .filter(|k| too_far(&loose[*k]))
            .collect();
        let remove: HashSet<usize> = remove.difference(&restore).copied().collect();
        if remove.is_empty() || gone.iter().any(too_far) && restore.is_empty() {
            break;
        }
        removed_all.extend(remove.iter().map(|k| loose[*k]));
        out.removed_points += remove.len();
        loose = loose
            .into_iter()
            .enumerate()
            .filter(|(k, _)| !remove.contains(k))
            .map(|(_, p)| p)
            .collect();
    }
    // A last look at the whole result: a removed point that the final surface misses by more
    // than the tolerance comes back. Bringing points back changes the surface, so the look
    // repeats until no point comes back. Each round restores at least one point, so it ends.
    while !removed_all.is_empty() {
        let mut src = loose.clone();
        src.extend(out.lines.iter().flat_map(|l| l.points.iter().copied()));
        let Ok(tin) = Tin::build(&SourceData {
            points: src,
            lines: out
                .lines
                .iter()
                .map(|l| SourceLine {
                    points: l.points.clone(),
                    closed: l.closed,
                })
                .collect(),
        }) else {
            break;
        };
        let (bad, good): (Vec<DVec3>, Vec<DVec3>) = removed_all.iter().partition(|p| {
            tin.elevation_at(p.truncate())
                .is_some_and(|z| (z - p.z).abs() > tolerance)
        });
        if bad.is_empty() {
            break;
        }
        out.removed_points -= bad.len();
        loose.extend(bad);
        removed_all = good;
    }
    out.points = loose;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn grid(n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> Vec<DVec3> {
        let mut v = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 * step, j as f64 * step);
                v.push(DVec3::new(x, y, f(x, y)));
            }
        }
        v
    }

    #[test]
    fn a_clean_plane_has_no_issues() {
        let pts = grid(6, 1000.0, |x, y| 100.0 + x * 0.01 + y * 0.02);
        let r = check(&pts, &[], &CheckOptions::default());
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(r.vertices_checked, pts.len());
        assert!(!r.truncated);
    }

    #[test]
    fn a_duplicate_with_another_height_is_found_and_a_copy_is_counted() {
        let mut pts = grid(4, 1000.0, |_, _| 0.0);
        pts.push(DVec3::new(1000.0, 1000.0, 500.0));
        pts.push(DVec3::new(2000.0, 2000.0, 0.0));
        pts.push(DVec3::new(2000.4, 2000.0, 5.0));
        let r = check(&pts, &[], &CheckOptions::default());
        assert_eq!(r.duplicate_xy, 1);
        assert_eq!(r.exact_duplicates, 1);
        let Issue::DuplicateXy {
            z_min,
            z_max,
            sources,
            ..
        } = &r.issues[0]
        else {
            panic!("{:?}", r.issues);
        };
        assert_eq!((*z_min, *z_max), (0.0, 500.0));
        assert_eq!(sources.len(), 2);
        assert_eq!(r.issues[0].kind(), "duplicate_xy");
    }

    #[test]
    fn a_spike_is_found_once_at_the_worst_point() {
        let mut pts = grid(7, 1000.0, |x, _| x * 0.05);
        let i = pts
            .iter()
            .position(|p| p.x == 3000.0 && p.y == 3000.0)
            .unwrap();
        pts[i].z += 5000.0;
        let r = check(&pts, &[], &CheckOptions::default());
        assert_eq!(r.spikes, 1, "{r:?}");
        let Issue::Spike {
            source,
            deviation,
            expected,
            ..
        } = r.issues[0].clone()
        else {
            panic!()
        };
        assert_eq!(source, Source::Point(i));
        assert!((deviation - 5000.0).abs() < 1.0);
        assert!((expected - pts[i].z + 5000.0).abs() < 1.0);
        // A looser threshold ignores it.
        let loose = CheckOptions {
            spike_threshold: 6000.0,
            ..CheckOptions::default()
        };
        assert!(check(&pts, &[], &loose).is_clean());
    }

    #[test]
    fn crossing_breaklines_at_different_heights_are_found() {
        let lines = vec![
            SiteLine {
                points: vec![
                    DVec3::new(0.0, 0.0, 0.0),
                    DVec3::new(10_000.0, 10_000.0, 0.0),
                ],
                closed: false,
            },
            SiteLine {
                points: vec![
                    DVec3::new(0.0, 10_000.0, 1000.0),
                    DVec3::new(10_000.0, 0.0, 1000.0),
                ],
                closed: false,
            },
        ];
        let r = check(&[], &lines, &CheckOptions::default());
        assert_eq!(r.crossings, 1);
        let Issue::CrossingBreaklines { a, b, xy, z_a, z_b } = r.issues[0].clone() else {
            panic!("{:?}", r.issues)
        };
        assert_eq!((a, b), ((0, 0), (1, 0)));
        assert!(xy.distance(DVec2::new(5000.0, 5000.0)) < 1e-6);
        assert_eq!((z_a, z_b), (0.0, 1000.0));
        // Equal heights at the crossing are fine. Lines that meet at an end are fine.
        let mut same = lines.clone();
        same[1].points.iter_mut().for_each(|p| p.z = 0.0);
        assert_eq!(check(&[], &same, &CheckOptions::default()).crossings, 0);
        let touch = vec![
            lines[0].clone(),
            SiteLine {
                points: vec![DVec3::new(0.0, 0.0, 800.0), DVec3::new(0.0, 5000.0, 0.0)],
                closed: false,
            },
        ];
        assert_eq!(check(&[], &touch, &CheckOptions::default()).crossings, 0);
    }

    #[test]
    fn a_closed_line_checks_its_closing_segment_and_the_list_is_capped() {
        let sq = SiteLine {
            points: vec![
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(10_000.0, 0.0, 0.0),
                DVec3::new(10_000.0, 10_000.0, 0.0),
                DVec3::new(0.0, 10_000.0, 0.0),
            ],
            closed: true,
        };
        let cut = SiteLine {
            points: vec![
                DVec3::new(-1000.0, 5000.0, 500.0),
                DVec3::new(500.0, 5000.0, 500.0),
            ],
            closed: false,
        };
        let opts = CheckOptions {
            max_issues: 0,
            ..CheckOptions::default()
        };
        let r = check(&[], &[sq, cut], &opts);
        assert_eq!(r.crossings, 1);
        assert!(r.issues.is_empty() && r.truncated);
    }

    #[test]
    fn options_are_validated() {
        assert!(CheckOptions::default().is_valid());
        assert!(
            !CheckOptions {
                z_tolerance: 0.0,
                ..CheckOptions::default()
            }
            .is_valid()
        );
        assert!(
            !CheckOptions {
                spike_threshold: f64::NAN,
                ..CheckOptions::default()
            }
            .is_valid()
        );
    }

    #[test]
    fn plane_prediction_handles_small_and_flat_inputs() {
        assert_eq!(plane_prediction(DVec2::ZERO, &[]), None);
        assert_eq!(
            plane_prediction(
                DVec2::ZERO,
                &[DVec3::new(1.0, 0.0, 4.0), DVec3::new(0.0, 1.0, 6.0)]
            ),
            Some(5.0)
        );
        // Neighbors on one line: no unique plane, so the mean.
        let on_line = [
            DVec3::new(1.0, 0.0, 1.0),
            DVec3::new(2.0, 0.0, 2.0),
            DVec3::new(3.0, 0.0, 3.0),
        ];
        assert_eq!(plane_prediction(DVec2::ZERO, &on_line), Some(2.0));
        let plane: Vec<DVec3> = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]
            .iter()
            .map(|(x, y)| DVec3::new(*x, *y, 10.0 + 2.0 * x + 3.0 * y))
            .collect();
        assert!((plane_prediction(DVec2::ZERO, &plane).unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn simplify_removes_copies_flat_points_and_straight_vertices() {
        let mut pts = grid(7, 1000.0, |_, _| 0.0);
        let before = pts.len();
        pts.push(pts[10]);
        let line = SiteLine {
            points: (0..=10)
                .map(|i| DVec3::new(i as f64 * 1000.0, -2000.0, 5.0))
                .collect(),
            closed: false,
        };
        let s = simplify(&pts, &[line], 5.0);
        assert_eq!(s.removed_duplicates, 1);
        assert_eq!(s.lines[0].points.len(), 2);
        assert_eq!(s.removed_vertices, 9);
        assert!(s.removed_points > 0 && s.points.len() < before);
        // The surface stays within the tolerance.
        let tin = Tin::build(&SourceData {
            points: s.points.clone(),
            lines: s
                .lines
                .iter()
                .map(|l| SourceLine {
                    points: l.points.clone(),
                    closed: l.closed,
                })
                .collect(),
        })
        .unwrap();
        for p in &grid(7, 1000.0, |_, _| 0.0) {
            if let Some(z) = tin.elevation_at(p.truncate()) {
                assert!(z.abs() <= 5.0 + 1e-9);
            }
        }
    }

    #[test]
    fn simplify_keeps_features_and_distinct_heights() {
        let mut pts = grid(7, 1000.0, |x, y| {
            if x == 3000.0 && y == 3000.0 {
                800.0
            } else {
                0.0
            }
        });
        pts.push(DVec3::new(0.0, 0.0, 300.0)); // same place, other height: stays
        let s = simplify(&pts, &[], 5.0);
        assert!(s.points.iter().any(|p| p.z == 800.0));
        assert!(s.points.iter().any(|p| p.z == 300.0));
        // A closed line keeps its shape.
        let ring = SiteLine {
            points: (0..12)
                .map(|i| {
                    let a = i as f64 / 12.0 * std::f64::consts::TAU;
                    DVec3::new(a.cos() * 5000.0, a.sin() * 5000.0, 0.0)
                })
                .collect(),
            closed: true,
        };
        let s = simplify(&[], std::slice::from_ref(&ring), 1.0);
        assert_eq!(s.lines[0].points.len(), 12);
        let s = simplify(&[], &[ring], 2000.0);
        assert!(s.lines[0].points.len() >= 3 && s.lines[0].points.len() < 12);
        // A line vertex place is not kept as a loose point too.
        let l = SiteLine {
            points: vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 0.0, 0.0)],
            closed: false,
        };
        let s = simplify(&[DVec3::new(0.0, 0.0, 0.0)], &[l], 1.0);
        assert!(s.points.is_empty() && s.removed_duplicates == 1);
        // Nothing to triangulate: the points stay.
        let s = simplify(&[DVec3::ZERO, DVec3::X], &[], 1.0);
        assert_eq!(s.points.len(), 2);
    }

    /// Shrunk case of the property below, found once by proptest (a 6 by 6 grid of heights).
    #[test]
    fn simplify_regression_sparse_grid_stays_within_the_tolerance() {
        let zs = [
            0.0,
            0.0,
            0.0,
            46.80807916565487,
            0.0,
            0.0,
            0.0,
            0.0,
            42.819876373516244,
            13.984815533302179,
            0.0,
            0.0,
            0.0,
            0.0,
            28.28665398785454,
            14.835260929129856,
            0.0,
            0.0,
            43.259550958162734,
            26.828903960422878,
            22.861120602652665,
            48.06414068593241,
            0.0,
            0.0,
            19.588512058879722,
            32.44771158964106,
            31.71666303103338,
            0.0,
            0.0,
            0.0,
            42.31342689034366,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let tol = 9.606617896334644;
        let pts: Vec<DVec3> = (0..36)
            .map(|i| DVec3::new((i % 6) as f64 * 1000.0, (i / 6) as f64 * 1000.0, zs[i]))
            .collect();
        let s = simplify(&pts, &[], tol);
        assert_eq!(s.points.len() + s.removed_points + s.removed_duplicates, 36);
        let full = Tin::build(&SourceData {
            points: pts.clone(),
            lines: vec![],
        })
        .unwrap();
        let small = Tin::build(&SourceData {
            points: s.points.clone(),
            lines: vec![],
        })
        .unwrap();
        for p in &pts {
            if let (Some(a), Some(b)) = (
                full.elevation_at(p.truncate()),
                small.elevation_at(p.truncate()),
            ) {
                assert!((a - b).abs() <= tol * 2.0 + 1e-6, "{p:?}: {a} vs {b}");
            }
        }
    }

    proptest! {
        #[test]
        fn simplify_never_adds_and_keeps_the_surface_within_the_tolerance(
            zs in prop::collection::vec(0.0f64..50.0, 36),
            tol in 1.0f64..100.0,
        ) {
            let pts: Vec<DVec3> = (0..36)
                .map(|i| DVec3::new((i % 6) as f64 * 1000.0, (i / 6) as f64 * 1000.0, zs[i]))
                .collect();
            let s = simplify(&pts, &[], tol);
            prop_assert!(s.points.len() <= pts.len());
            prop_assert_eq!(s.points.len() + s.removed_points + s.removed_duplicates, pts.len());
            let full = Tin::build(&SourceData { points: pts.clone(), lines: vec![] }).unwrap();
            let small = Tin::build(&SourceData { points: s.points.clone(), lines: vec![] }).unwrap();
            // Every removed point is predicted within a few tolerances by the reduced surface.
            for p in &pts {
                if let (Some(a), Some(b)) = (full.elevation_at(p.truncate()), small.elevation_at(p.truncate())) {
                    prop_assert!((a - b).abs() <= tol * 2.0 + 1e-6);
                }
            }
        }

        #[test]
        fn check_never_panics(
            pts in prop::collection::vec((-1e5f64..1e5, -1e5f64..1e5, -1e4f64..1e4), 0..40)
        ) {
            let pts: Vec<DVec3> = pts.into_iter().map(|(x, y, z)| DVec3::new(x, y, z)).collect();
            let line = SiteLine { points: pts.clone(), closed: true };
            let _ = check(&pts, &[line], &CheckOptions::default());
        }
    }
}
