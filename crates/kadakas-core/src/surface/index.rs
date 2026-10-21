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
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            if n.hi.cmplt(lo).any() || n.lo.cmpgt(hi).any() {
                continue;
            }
            if n.leaf {
                for (a, b, id) in &self.items[n.start..n.end] {
                    if !(b.cmplt(lo).any() || a.cmpgt(hi).any()) {
                        f(*id);
                    }
                }
            } else {
                stack.push(n.end);
                stack.push(n.start);
            }
        }
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
}
