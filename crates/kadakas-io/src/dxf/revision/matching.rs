//! The matching itself. See the module text of `revision` for the rules.

use std::collections::{HashMap, HashSet};

use super::rec::Rec;
use super::{Change, Counts, Diff, Entry, Options, Status, Via};

/// Most candidate pairs of the nearest match that are kept. A pass that makes more is cut and the
/// result is flagged `truncated`.
const MAX_CANDIDATES: usize = 4_000_000;
/// The part of the size of the drawing that the automatic tolerance uses.
const AUTO_TOLERANCE: f64 = 0.005;
/// How far an entity may move, as a multiple of its own size. A small entity does not jump as far
/// as a large one (but never less than a twentieth of the tolerance).
const SIZE_FACTOR: f64 = 2.0;
const MIN_PART: f64 = 0.05;

type Key<'a> = (&'a str, u64, &'a str, &'a str);

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

/// The scopes of block definitions that have another key in the new file. A file that is saved
/// again can number the BLOCK_RECORD of an unchanged block differently, and the entities of a
/// block are never matched across scopes: all of them would be removed and added. A definition that
/// is only in the base file and one with the same name that is only in the new file are one
/// definition. Gives the new records with the scope of the base file where that is so, or `None`
/// when no definition needs it.
fn align_blocks(base: &[Rec], new: &[Rec]) -> Option<Vec<Rec>> {
    let scopes = |recs: &[Rec]| -> HashMap<String, String> {
        recs.iter()
            .filter(|r| r.scope.starts_with("block:") && !r.block.is_empty())
            .map(|r| (r.scope.clone(), r.block.clone()))
            .collect()
    };
    let (b, n) = (scopes(base), scopes(new));
    let only = |a: &HashMap<String, String>, other: &HashMap<String, String>| {
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for (scope, name) in a.iter().filter(|(s, _)| !other.contains_key(*s)) {
            by_name.entry(name.clone()).or_default().push(scope.clone());
        }
        by_name
    };
    let (b_only, n_only) = (only(&b, &n), only(&n, &b));
    let alias: HashMap<&str, &str> = n_only
        .iter()
        .filter_map(
            |(name, news)| match (news.as_slice(), b_only.get(name)?.as_slice()) {
                ([new_scope], [base_scope]) => Some((new_scope.as_str(), base_scope.as_str())),
                _ => None,
            },
        )
        .collect();
    if alias.is_empty() {
        return None;
    }
    Some(
        new.iter()
            .map(|r| {
                let mut r = r.clone();
                if let Some(s) = alias.get(r.scope.as_str()) {
                    r.scope = (*s).to_string();
                }
                r
            })
            .collect(),
    )
}

/// Matches two lists of records. See the module text.
pub fn diff(base: &[Rec], new: &[Rec], opts: &Options) -> Diff {
    let aligned = align_blocks(base, new);
    let new = aligned.as_deref().unwrap_or(new);
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
    let tolerance = if opts.proximity {
        m.tolerance(opts.tolerance, opts.max_tolerance)
    } else {
        0.0
    };
    let renumbered = m.by_handle(tolerance);
    m.by_hash(|r| (&r.scope, r.hash, "", ""), Via::Hash);
    m.by_hash(|r| (&r.scope, r.geo_hash, &r.layer, &r.color), Via::Hash);
    m.by_hash(|r| (&r.scope, r.geo_hash, &r.layer, ""), Via::Hash);
    m.by_hash(|r| (&r.scope, r.geo_hash, "", ""), Via::Hash);
    if tolerance > 0.0 {
        m.nearest(tolerance, true);
        m.nearest(tolerance, false);
    }
    m.finish(renumbered, tolerance)
}

/// How often a hash is in a count map.
fn count<'a>(m: &HashMap<(&'a str, u64), usize>, scope: &'a str, h: u64) -> usize {
    m.get(&(scope, h)).copied().unwrap_or(0)
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

    /// Pairs entities that have the same handle, and judges each pair on its own. Returns true if
    /// the handles of some scope look renumbered; no handle pair of that scope is kept then.
    ///
    /// A pair whose content differs is dropped when the content or the geometry of the base entity
    /// is still unpaired in the new file (a better partner exists). A scope in which most pairs
    /// differ in content that is found elsewhere, or in which most pairs have a better partner, is
    /// renumbered. The distance counts only in a scope where some pair has a better partner (so
    /// renumbering is suspected): there a pair that is farther apart than the tolerance, with other
    /// geometry, is dropped too. In any other scope a pair with a handle is the same entity,
    /// whatever the distance, so a drawing that moved as a whole (a new survey base) has every
    /// entity moved, none removed and added. A far pair never counts for the vote.
    fn by_handle(&mut self, tol: f64) -> bool {
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
        let mut new_hash: HashMap<(&str, u64), usize> = HashMap::new();
        let mut new_geo: HashMap<(&str, u64), usize> = HashMap::new();
        for &j in &self.ni {
            *new_hash.entry((&new[j].scope, new[j].hash)).or_default() += 1;
            *new_geo.entry((&new[j].scope, new[j].geo_hash)).or_default() += 1;
        }
        // What the pairs themselves explain: equal content, equal geometry.
        let mut used_hash: HashMap<(&str, u64), usize> = HashMap::new();
        let mut used_geo: HashMap<(&str, u64), usize> = HashMap::new();
        for &(i, j) in &found {
            if base[i].hash == new[j].hash {
                *used_hash.entry((&base[i].scope, base[i].hash)).or_default() += 1;
            }
            if base[i].geo_hash == new[j].geo_hash {
                *used_geo
                    .entry((&base[i].scope, base[i].geo_hash))
                    .or_default() += 1;
            }
        }
        // What each pair says. A pair of equal content is fine. For the others: is a better partner
        // still free (`suspect`), is the content found elsewhere (`elsewhere`), and are the two far
        // apart with other geometry (`far`)?
        struct Judged {
            i: usize,
            j: usize,
            same: bool,
            suspect: bool,
            elsewhere: bool,
            far: bool,
        }
        let judged: Vec<Judged> = found
            .iter()
            .map(|&(i, j)| {
                let (a, b) = (&base[i], &new[j]);
                let same = a.hash == b.hash;
                let free_hash =
                    count(&new_hash, &a.scope, a.hash) > count(&used_hash, &a.scope, a.hash);
                let free_geo =
                    count(&new_geo, &a.scope, a.geo_hash) > count(&used_geo, &a.scope, a.geo_hash);
                let far = tol > 0.0
                    && a.geo_hash != b.geo_hash
                    && matches!((a.center, b.center), (Some(p), Some(q))
                        if (p[0] - q[0]).hypot(p[1] - q[1]) > tol);
                Judged {
                    i,
                    j,
                    same,
                    suspect: !same && (free_hash || free_geo),
                    elsewhere: !same && count(&new_hash, &a.scope, a.hash) > 0,
                    far,
                }
            })
            .collect();
        #[derive(Default)]
        struct Tally {
            found: usize,
            differ: usize,
            elsewhere: usize,
            suspect: usize,
        }
        let mut tally: HashMap<&str, Tally> = HashMap::new();
        for p in &judged {
            let t = tally.entry(&base[p.i].scope).or_default();
            t.found += 1;
            t.differ += usize::from(!p.same);
            t.elsewhere += usize::from(p.elsewhere);
            t.suspect += usize::from(p.suspect);
        }
        // A scope is renumbered when most pairs differ with their content found elsewhere, or when
        // most pairs have a better partner. A "far" pair never counts for this.
        let renumbered: HashSet<&str> = tally
            .iter()
            .filter(|(_, t)| {
                (2 * t.differ > t.found && 2 * t.elsewhere > t.differ) || 2 * t.suspect > t.found
            })
            .map(|(s, _)| *s)
            .collect();
        for p in &judged {
            let scope = base[p.i].scope.as_str();
            // The distance counts only in a scope where some pair is suspect. Elsewhere a pair with
            // its handle is the same entity, whatever the distance: a survey that moved as a whole
            // moves every entity.
            let suspected = tally[scope].suspect > 0;
            if !renumbered.contains(scope) && !p.suspect && !(suspected && p.far) {
                self.pair(p.i, p.j, Via::Handle);
            }
        }
        !renumbered.is_empty()
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

    /// The tolerance of the nearest match: the option, or a part of the size of the drawing. The
    /// size is the box around the middle 90 percent of the centers, so a stray entity far from the
    /// drawing does not widen it. It is at most `cap`. A drawing whose centers all coincide uses the
    /// size of its entities.
    fn tolerance(&self, wanted: f64, cap: f64) -> f64 {
        if wanted.is_finite() && wanted > 0.0 {
            return wanted;
        }
        let cap = if cap.is_finite() && cap > 0.0 {
            cap
        } else {
            f64::INFINITY
        };
        let recs: Vec<&Rec> = self
            .bi
            .iter()
            .map(|&i| &self.base[i])
            .chain(self.ni.iter().map(|&j| &self.new[j]))
            .collect();
        let mut axes: [Vec<f64>; 2] = [vec![], vec![]];
        for c in recs.iter().filter_map(|r| r.center) {
            axes[0].push(c[0]);
            axes[1].push(c[1]);
        }
        let n = axes[0].len();
        if n == 0 {
            return 0.0;
        }
        let mut span = [0.0; 2];
        for (k, a) in axes.iter_mut().enumerate() {
            a.sort_by(f64::total_cmp);
            let lo = ((n - 1) as f64 * 0.05).floor() as usize;
            let hi = ((n - 1) as f64 * 0.95).ceil() as usize;
            span[k] = a[hi] - a[lo];
        }
        let diag = span[0].hypot(span[1]);
        let mut tol = if diag.is_finite() {
            diag * AUTO_TOLERANCE
        } else {
            0.0
        };
        if tol <= 0.0 {
            let mut sizes: Vec<f64> = recs.iter().map(|r| r.size()).filter(|s| *s > 0.0).collect();
            sizes.sort_by(f64::total_cmp);
            tol = sizes.get(sizes.len() / 2).map_or(0.0, |s| s * 0.5);
        }
        if tol.is_finite() { tol.min(cap) } else { 0.0 }
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
                        // The cost is charged before the point loop of `distance`.
                        let cost = 1 + r.points.len().min(s.points.len()) as u64;
                        if !self.spend(cost) {
                            break 'outer;
                        }
                        if same_layer && s.layer != r.layer {
                            continue;
                        }
                        let Some(d) = distance(r, s, tol) else {
                            continue;
                        };
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
                    changes(a, b)
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
/// apart than they may be. They may be at most `tol` apart, and at most a few times the size of
/// the entity (`SIZE_FACTOR`, but never less than a twentieth of `tol`).
///
/// The distance is the largest of: the mean distance of the points (the distance of the centers
/// when the point counts differ), the change of the size of the box, and the change of the sizes
/// (radius, height). So a moved entity has its move as distance, and an edited one a small number.
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
    let allowed = tol.min((SIZE_FACTOR * a.size().max(b.size())).max(tol * MIN_PART));
    (d.is_finite() && d <= allowed).then_some(d)
}
