//! The matching itself. See the module text of `revision` for the rules.

use std::collections::HashMap;

use super::rec::Rec;
use super::{Change, Counts, Diff, Entry, Options, Status, Via};

/// Most candidate pairs of the nearest match that are kept. A pass that makes more is cut and the
/// result is flagged `truncated`.
const MAX_CANDIDATES: usize = 4_000_000;
/// The part of the size of the drawing that the automatic tolerance uses.
const AUTO_TOLERANCE: f64 = 0.005;

type Key<'a> = (&'a str, u64, &'a str);

struct Matcher<'a> {
    base: &'a [Rec],
    new: &'a [Rec],
    /// Indices into `base` and `new` that take part, in canonical order.
    bi: Vec<usize>,
    ni: Vec<usize>,
    b_used: Vec<bool>,
    n_used: Vec<bool>,
    pairs: Vec<(usize, usize, Via)>,
    work: u64,
    budget: u64,
    truncated: bool,
}

/// The indices of `recs` in canonical order, cut to `max`. The second value is true if it cut.
fn canonical(recs: &[Rec], max: usize) -> (Vec<usize>, bool) {
    let mut idx: Vec<usize> = (0..recs.len()).collect();
    idx.sort_by(|&a, &b| recs[a].key().cmp(&recs[b].key()).then(a.cmp(&b)));
    let cut = idx.len() > max;
    idx.truncate(max);
    (idx, cut)
}

/// Matches two lists of records. See the module text.
pub fn diff(base: &[Rec], new: &[Rec], opts: &Options) -> Diff {
    let (bi, cut_b) = canonical(base, opts.max_entities);
    let (ni, cut_n) = canonical(new, opts.max_entities);
    let mut m = Matcher {
        base,
        new,
        bi,
        ni,
        b_used: vec![false; base.len()],
        n_used: vec![false; new.len()],
        pairs: Vec::new(),
        work: 0,
        budget: opts.work_budget,
        truncated: cut_b || cut_n,
    };
    let renumbered = m.by_handle();
    m.by_hash(|r| (&r.scope, r.hash, ""), Via::Hash);
    m.by_hash(|r| (&r.scope, r.geo_hash, &r.layer), Via::Hash);
    m.by_hash(|r| (&r.scope, r.geo_hash, ""), Via::Hash);
    let tolerance = if opts.proximity {
        m.tolerance(opts.tolerance)
    } else {
        0.0
    };
    if tolerance > 0.0 {
        m.nearest(tolerance, true);
        m.nearest(tolerance, false);
    }
    m.finish(renumbered, tolerance)
}

impl Matcher<'_> {
    fn pair(&mut self, b: usize, n: usize, via: Via) {
        self.b_used[b] = true;
        self.n_used[n] = true;
        self.pairs.push((b, n, via));
    }

    /// Spends `units` of the budget. False when they do not fit (and the result is cut).
    fn spend(&mut self, units: u64) -> bool {
        match self.work.checked_add(units) {
            Some(w) if w <= self.budget => {
                self.work = w;
                true
            }
            _ => {
                self.truncated = true;
                false
            }
        }
    }

    /// Pairs entities that have the same handle. Returns true if the handles look renumbered, and
    /// then no handle pair is kept.
    fn by_handle(&mut self) -> bool {
        let (base, new) = (self.base, self.new);
        let mut nh: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
        for &j in &self.ni {
            if let Some(h) = &new[j].handle {
                nh.entry((&new[j].scope, h)).or_default().push(j);
            }
        }
        let mut bh: HashMap<(&str, &str), usize> = HashMap::new();
        for &i in &self.bi {
            if let Some(h) = &base[i].handle {
                *bh.entry((&base[i].scope, h)).or_default() += 1;
            }
        }
        let mut found = Vec::new();
        for &i in &self.bi {
            let r = &base[i];
            let Some(h) = &r.handle else { continue };
            // A handle that two entities share (a damaged file) identifies neither of them.
            if bh[&(r.scope.as_str(), h.as_str())] != 1 {
                continue;
            }
            if let Some([j]) = nh.get(&(r.scope.as_str(), h.as_str())).map(Vec::as_slice)
                && new[*j].kind == r.kind
            {
                found.push((i, *j));
            }
        }
        // Renumbered handles: most handle pairs differ in content, and the content of those
        // entities is somewhere else in the new file.
        let mut new_hashes: HashMap<(&str, u64), usize> = HashMap::new();
        for &j in &self.ni {
            *new_hashes.entry((&new[j].scope, new[j].hash)).or_default() += 1;
        }
        let differ: Vec<usize> = found
            .iter()
            .filter(|(i, j)| base[*i].hash != new[*j].hash)
            .map(|(i, _)| *i)
            .collect();
        let elsewhere = differ
            .iter()
            .filter(|&&i| new_hashes.contains_key(&(base[i].scope.as_str(), base[i].hash)))
            .count();
        if 2 * differ.len() > found.len() && 2 * elsewhere > differ.len() {
            return true;
        }
        for (i, j) in found {
            self.pair(i, j, Via::Handle);
        }
        false
    }

    /// Pairs the entities that are left and have the same key. Equal entities are exchangeable,
    /// so they are paired in canonical order.
    fn by_hash<F: Fn(&Rec) -> Key<'_>>(&mut self, key: F, via: Via) {
        let (base, new) = (self.base, self.new);
        let mut nk: HashMap<Key<'_>, Vec<usize>> = HashMap::new();
        for &j in self.ni.iter().filter(|&&j| !self.n_used[j]) {
            nk.entry(key(&new[j])).or_default().push(j);
        }
        let mut fresh = Vec::new();
        let mut at: HashMap<Key<'_>, usize> = HashMap::new();
        for &i in self.bi.iter().filter(|&&i| !self.b_used[i]) {
            let k = key(&base[i]);
            let Some(list) = nk.get(&k) else { continue };
            let n = at.entry(k).or_default();
            if let Some(&j) = list.get(*n) {
                *n += 1;
                fresh.push((i, j));
            }
        }
        for (i, j) in fresh {
            self.pair(i, j, via);
        }
    }

    /// The tolerance of the nearest match: the option, or a part of the size of the drawing.
    fn tolerance(&self, wanted: f64) -> f64 {
        if wanted.is_finite() && wanted > 0.0 {
            return wanted;
        }
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for r in self
            .bi
            .iter()
            .map(|&i| &self.base[i])
            .chain(self.ni.iter().map(|&j| &self.new[j]))
        {
            if let Some(c) = r.center {
                for k in 0..2 {
                    lo[k] = lo[k].min(c[k]);
                    hi[k] = hi[k].max(c[k]);
                }
            }
        }
        let d = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2)).sqrt();
        if d.is_finite() && d > 0.0 {
            d * AUTO_TOLERANCE
        } else {
            0.0
        }
    }

    /// Pairs the entities that are left by distance, nearest first. Entities have to be of the
    /// same type and scope, and within `tol`. `same_layer` first, then any layer.
    fn nearest(&mut self, tol: f64, same_layer: bool) {
        let (base, new) = (self.base, self.new);
        let cell = |v: f64| (v / tol).floor() as i64;
        let mut groups: HashMap<(&str, &str), u32> = HashMap::new();
        let mut grid: HashMap<(u32, i64, i64), Vec<usize>> = HashMap::new();
        for &j in self.ni.iter().filter(|&&j| !self.n_used[j]) {
            let r = &new[j];
            let n = groups.len() as u32;
            let g = *groups.entry((&r.scope, &r.kind)).or_insert(n);
            if let Some(c) = r.center {
                grid.entry((g, cell(c[0]), cell(c[1]))).or_default().push(j);
            }
        }
        let mut cands: Vec<(f64, usize, usize)> = Vec::new();
        let todo: Vec<usize> = (self.bi.iter().copied())
            .filter(|&i| !self.b_used[i])
            .collect();
        'outer: for i in todo {
            let r = &base[i];
            let (Some(c), Some(&g)) = (r.center, groups.get(&(r.scope.as_str(), r.kind.as_str())))
            else {
                continue;
            };
            let (cx, cy) = (cell(c[0]), cell(c[1]));
            for dx in -1..=1i64 {
                for dy in -1..=1i64 {
                    let at = (g, cx.saturating_add(dx), cy.saturating_add(dy));
                    for &j in grid.get(&at).into_iter().flatten() {
                        let s = &new[j];
                        if !self.spend(1) {
                            break 'outer;
                        }
                        if same_layer && s.layer != r.layer {
                            continue;
                        }
                        let Some(d) = distance(r, s, tol) else {
                            continue;
                        };
                        if !self.spend(r.points.len().min(s.points.len()) as u64) {
                            break 'outer;
                        }
                        if cands.len() >= MAX_CANDIDATES {
                            self.truncated = true;
                            break 'outer;
                        }
                        cands.push((d, i, j));
                    }
                }
            }
        }
        cands.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| base[a.1].key().cmp(&base[b.1].key()))
                .then_with(|| new[a.2].key().cmp(&new[b.2].key()))
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        for (_, i, j) in cands {
            if !self.b_used[i] && !self.n_used[j] {
                self.pair(i, j, Via::Proximity);
            }
        }
    }

    fn finish(self, renumbered: bool, tolerance: f64) -> Diff {
        let (base, new) = (self.base, self.new);
        let mut entries = Vec::new();
        for &(i, j, via) in &self.pairs {
            let (a, b) = (&base[i], &new[j]);
            let changes = changes(a, b);
            let status = if a.hash == b.hash {
                Status::Unchanged
            } else {
                Status::Changed
            };
            let distance = (via == Via::Proximity)
                .then(|| distance(a, b, f64::INFINITY))
                .flatten();
            entries.push(Entry {
                base: Some(i),
                new: Some(j),
                status,
                via: Some(via),
                changes: if status == Status::Unchanged {
                    vec![]
                } else {
                    changes
                },
                distance,
            });
        }
        for &i in self.bi.iter().filter(|&&i| !self.b_used[i]) {
            entries.push(Entry::lone(Some(i), None, Status::Removed));
        }
        for &j in self.ni.iter().filter(|&&j| !self.n_used[j]) {
            entries.push(Entry::lone(None, Some(j), Status::Added));
        }
        let key = |e: &Entry| (e.base.map(|i| base[i].key()), e.new.map(|j| new[j].key()));
        entries.sort_by(|a, b| {
            key(a)
                .cmp(&key(b))
                .then(a.base.cmp(&b.base))
                .then(a.new.cmp(&b.new))
        });
        let mut counts = Counts::default();
        for e in &entries {
            match e.status {
                Status::Unchanged => counts.unchanged += 1,
                Status::Changed => counts.changed += 1,
                Status::Added => counts.added += 1,
                Status::Removed => counts.removed += 1,
            }
            match e.via {
                Some(Via::Handle) => counts.by_handle += 1,
                Some(Via::Hash) => counts.by_hash += 1,
                Some(Via::Proximity) => counts.by_proximity += 1,
                None => {}
            }
        }
        Diff {
            entries,
            counts,
            truncated: self.truncated,
            renumbered,
            tolerance,
            work: self.work,
        }
    }
}

impl Entry {
    fn lone(base: Option<usize>, new: Option<usize>, status: Status) -> Entry {
        Entry {
            base,
            new,
            status,
            via: None,
            changes: vec![],
            distance: None,
        }
    }
}

/// Which properties differ between two matched records. A difference in the content that none of
/// the named properties explains is `Other`.
pub fn changes(a: &Rec, b: &Rec) -> Vec<Change> {
    let mut out = Vec::new();
    let mut add = |differs: bool, c: Change| {
        if differs {
            out.push(c);
        }
    };
    add(a.geo_hash != b.geo_hash, Change::Geometry);
    add(a.layer != b.layer, Change::Layer);
    add(a.color != b.color, Change::Color);
    add(a.linetype != b.linetype, Change::Linetype);
    add(a.lineweight != b.lineweight, Change::Lineweight);
    add(a.text != b.text, Change::Text);
    if out.is_empty() && a.hash != b.hash {
        out.push(Change::Other);
    }
    out
}

/// How far two records of the same type are apart, in drawing units, or None if they are farther
/// apart than `tol`. The distance is the largest of: the mean distance of the points (the distance
/// of the centers when the point counts differ), the change of the size of the box, and the change
/// of the sizes (radius, height). So a moved entity has its move as distance, and an edited one a
/// small number.
fn distance(a: &Rec, b: &Rec, tol: f64) -> Option<f64> {
    let (ca, cb) = (a.center?, b.center?);
    let center = (ca[0] - cb[0]).hypot(ca[1] - cb[1]);
    // The mean point distance is never smaller than the distance of the centers, so this test
    // does not lose a pair.
    if center > tol {
        return None;
    }
    let mean = if a.points.len() == b.points.len() && !a.points.is_empty() {
        a.points
            .iter()
            .zip(&b.points)
            .map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]))
            .sum::<f64>()
            / a.points.len() as f64
    } else {
        center
    };
    let size = (a.diag - b.diag).abs();
    let scalar = if a.sizes.len() == b.sizes.len() {
        a.sizes
            .iter()
            .zip(&b.sizes)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max)
    } else {
        0.0
    };
    let d = mean.max(size).max(scalar);
    (d.is_finite() && d <= tol).then_some(d)
}
