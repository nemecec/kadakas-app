//! A box tree: the boxes of edges or faces, split in halves along their longest side, so that the
//! boxes that meet a query box are found in `O(log n + k)` time. It is built for one check or one
//! edit and never saved.

use glam::DVec3;

/// Boxes with an id each. The same boxes in the same order give the same tree.
pub(super) struct BoxTree {
    items: Vec<(DVec3, DVec3, u32)>,
    nodes: Vec<Node>,
}

struct Node {
    lo: DVec3,
    hi: DVec3,
    /// The items of a leaf, or the two children (`start` and `end` are node indices then).
    start: usize,
    end: usize,
    leaf: bool,
}

const LEAF: usize = 4;

impl BoxTree {
    pub fn new(items: Vec<(DVec3, DVec3, u32)>) -> BoxTree {
        let mut t = BoxTree {
            items,
            nodes: Vec::new(),
        };
        if !t.items.is_empty() {
            let n = t.items.len();
            t.build(0, n);
        }
        t
    }

    fn build(&mut self, start: usize, end: usize) -> usize {
        let (lo, hi) = self.items[start..end].iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), (a, b, _)| (lo.min(*a), hi.max(*b)),
        );
        let id = self.nodes.len();
        self.nodes.push(Node {
            lo,
            hi,
            start,
            end,
            leaf: true,
        });
        if end - start <= LEAF {
            return id;
        }
        let size = hi - lo;
        let axis = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        let mid = start + (end - start) / 2;
        let centre = |b: &(DVec3, DVec3, u32)| (b.0[axis] + b.1[axis], b.2);
        self.items[start..end].select_nth_unstable_by(mid - start, |p, q| {
            let (a, b) = (centre(p), centre(q));
            a.0.total_cmp(&b.0).then(a.1.cmp(&b.1))
        });
        let left = self.build(start, mid);
        let right = self.build(mid, end);
        let node = &mut self.nodes[id];
        node.leaf = false;
        node.start = left;
        node.end = right;
        id
    }

    /// Calls `f` with the id of each box that meets the box `lo`–`hi` (touching counts).
    pub fn query(&self, lo: DVec3, hi: DVec3, f: &mut dyn FnMut(u32)) {
        self.query_by(&|a, b| !(b.cmplt(lo).any() || a.cmpgt(hi).any()), f);
    }

    /// Calls `f` with the id of each box for which `keep` is true. `keep` must be true for a box
    /// that holds any box for which it is true (a test of "meets a region" is), because it decides
    /// for the boxes of whole branches too.
    pub fn query_by(&self, keep: &dyn Fn(DVec3, DVec3) -> bool, f: &mut dyn FnMut(u32)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            if !keep(n.lo, n.hi) {
                continue;
            }
            if n.leaf {
                for (a, b, id) in &self.items[n.start..n.end] {
                    if keep(*a, *b) {
                        f(*id);
                    }
                }
            } else {
                stack.push(n.end);
                stack.push(n.start);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// A box tree that grows: boxes are added one at a time (for the index of one edit, which adds
/// vertices, edges and faces as it goes). The boxes are kept in a few box trees of falling size;
/// when the newest boxes are many, they and the smaller trees are built into one tree again, so a
/// box is rebuilt `O(log n)` times. Boxes are never removed; the caller tests what a query finds.
pub(super) struct Boxes {
    trees: Vec<BoxTree>,
    recent: Vec<(DVec3, DVec3, u32)>,
}

/// The number of newest boxes that are looked at one by one.
const RECENT: usize = 64;

impl Boxes {
    pub fn new(items: Vec<(DVec3, DVec3, u32)>) -> Boxes {
        Boxes {
            trees: vec![BoxTree::new(items)],
            recent: Vec::new(),
        }
    }

    pub fn insert(&mut self, lo: DVec3, hi: DVec3, id: u32) {
        self.recent.push((lo, hi, id));
        if self.recent.len() < RECENT {
            return;
        }
        let mut items = std::mem::take(&mut self.recent);
        while let Some(t) = self.trees.pop_if(|t| t.len() <= items.len()) {
            items.extend(t.items);
        }
        self.trees.push(BoxTree::new(items));
    }

    /// The ids of the boxes that meet the box `lo`–`hi` (touching counts), sorted, each once.
    pub fn ids_meeting(&self, lo: DVec3, hi: DVec3) -> Vec<u32> {
        let mut out = Vec::new();
        for t in &self.trees {
            t.query(lo, hi, &mut |id| out.push(id));
        }
        for (a, b, id) in &self.recent {
            if !(b.cmplt(lo).any() || a.cmpgt(hi).any()) {
                out.push(*id);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_finds_exactly_the_boxes_that_meet_it() {
        let items: Vec<(DVec3, DVec3, u32)> = (0..500u32)
            .map(|i| {
                let p = DVec3::new(
                    f64::from(i % 10),
                    f64::from((i / 10) % 10),
                    f64::from(i / 100),
                );
                (p, p + DVec3::splat(0.5), i)
            })
            .collect();
        let t = BoxTree::new(items.clone());
        let (lo, hi) = (DVec3::new(2.2, 3.0, 0.0), DVec3::new(4.0, 3.4, 2.5));
        let mut got = Vec::new();
        t.query(lo, hi, &mut |i| got.push(i));
        got.sort_unstable();
        let want: Vec<u32> = items
            .iter()
            .filter(|(a, b, _)| !(b.cmplt(lo).any() || a.cmpgt(hi).any()))
            .map(|x| x.2)
            .collect();
        assert_eq!(got, want);
        assert!(!want.is_empty());
        BoxTree::new(Vec::new()).query(lo, hi, &mut |_| panic!("empty"));
    }

    #[test]
    fn a_growing_tree_finds_what_one_tree_finds() {
        let item = |i: u32| {
            let p = DVec3::new(
                f64::from(i % 17),
                f64::from((i / 17) % 13),
                f64::from(i % 5),
            );
            (p, p + DVec3::splat(f64::from(i % 3) * 0.4), i)
        };
        let mut g = Boxes::new((0..300).map(item).collect());
        for i in 300..1500 {
            let (a, b, id) = item(i);
            g.insert(a, b, id);
            // A box added twice is found once.
            if i % 7 == 0 {
                g.insert(a, b, id);
            }
        }
        assert!(g.trees.len() < 12, "{} trees", g.trees.len());
        let all: Vec<_> = (0..1500).map(item).collect();
        for (lo, hi) in [
            (DVec3::new(2.2, 3.0, 0.0), DVec3::new(4.0, 3.4, 2.5)),
            (DVec3::splat(-1.0), DVec3::splat(100.0)),
            (DVec3::splat(50.0), DVec3::splat(60.0)),
        ] {
            let want: Vec<u32> = all
                .iter()
                .filter(|(a, b, _)| !(b.cmplt(lo).any() || a.cmpgt(hi).any()))
                .map(|x| x.2)
                .collect();
            assert_eq!(g.ids_meeting(lo, hi), want);
        }
    }
}
