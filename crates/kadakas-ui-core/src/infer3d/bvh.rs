//! A bounding volume hierarchy over boxes: the spatial index of the scene. Built once (median
//! split, ties broken by index, so the same input gives the same tree), queried with a test on
//! boxes.

use glam::DVec3;

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: DVec3::splat(f64::INFINITY),
        max: DVec3::splat(f64::NEG_INFINITY),
    };

    pub fn of_points(points: &[DVec3]) -> Aabb {
        points.iter().fold(Aabb::EMPTY, |b, p| b.grow(*p))
    }

    pub fn grow(self, p: DVec3) -> Aabb {
        Aabb {
            min: self.min.min(p),
            max: self.max.max(p),
        }
    }

    pub fn union(self, o: Aabb) -> Aabb {
        Aabb {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    pub fn center(&self) -> DVec3 {
        (self.min + self.max) * 0.5
    }

    pub fn corners(&self) -> [DVec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            DVec3::new(a.x, a.y, a.z),
            DVec3::new(b.x, a.y, a.z),
            DVec3::new(a.x, b.y, a.z),
            DVec3::new(b.x, b.y, a.z),
            DVec3::new(a.x, a.y, b.z),
            DVec3::new(b.x, a.y, b.z),
            DVec3::new(a.x, b.y, b.z),
            DVec3::new(b.x, b.y, b.z),
        ]
    }

    /// The distance along a ray where it enters the box (`None` when it misses or the box is
    /// behind the origin). The box is widened by `pad`.
    pub fn ray_enter(&self, origin: DVec3, dir: DVec3, pad: f64) -> Option<f64> {
        let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
        for k in 0..3 {
            let (lo, hi) = (self.min[k] - pad, self.max[k] + pad);
            if dir[k].abs() < 1e-300 {
                if origin[k] < lo || origin[k] > hi {
                    return None;
                }
            } else {
                let (a, b) = ((lo - origin[k]) / dir[k], (hi - origin[k]) / dir[k]);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(t0)
    }
}

const LEAF: usize = 8;

#[derive(Clone, Debug)]
struct Node {
    aabb: Aabb,
    /// A leaf: the start in `order`. An inner node: the index of the left child.
    first: u32,
    /// A leaf: the count. An inner node: 0.
    count: u32,
    /// An inner node: the index of the right child.
    right: u32,
}

/// The tree. Items are numbered by their position in the list given to [`Bvh::build`].
#[derive(Clone, Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
}

impl Bvh {
    pub fn build(boxes: &[Aabb]) -> Bvh {
        let mut bvh = Bvh {
            nodes: Vec::new(),
            order: Vec::new(),
        };
        if !boxes.is_empty() {
            let mut order: Vec<u32> = (0..boxes.len() as u32).collect();
            bvh.split(boxes, &mut order, 0);
            bvh.order = order;
        }
        bvh
    }

    fn split(&mut self, boxes: &[Aabb], items: &mut [u32], start: usize) -> u32 {
        let aabb = items
            .iter()
            .fold(Aabb::EMPTY, |b, i| b.union(boxes[*i as usize]));
        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            aabb,
            first: start as u32,
            count: items.len() as u32,
            right: 0,
        });
        if items.len() <= LEAF {
            return id;
        }
        let cb = items
            .iter()
            .fold(Aabb::EMPTY, |b, i| b.grow(boxes[*i as usize].center()));
        let ext = cb.max - cb.min;
        let axis = if ext.x >= ext.y && ext.x >= ext.z {
            0
        } else if ext.y >= ext.z {
            1
        } else {
            2
        };
        let mid = items.len() / 2;
        items.select_nth_unstable_by(mid, |a, b| {
            let (ca, cb) = (
                boxes[*a as usize].center()[axis],
                boxes[*b as usize].center()[axis],
            );
            ca.total_cmp(&cb).then(a.cmp(b))
        });
        let (l, r) = items.split_at_mut(mid);
        let left = self.split(boxes, l, start);
        let right = self.split(boxes, r, start + mid);
        let n = &mut self.nodes[id as usize];
        n.first = left;
        n.count = 0;
        n.right = right;
        id
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Calls `visit` for every item of a leaf whose box and all boxes above it pass `hit`. The
    /// order of the calls is not specified; a caller that needs an order sorts the result.
    pub fn query(&self, hit: &dyn Fn(&Aabb) -> bool, visit: &mut dyn FnMut(u32)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i as usize];
            if !hit(&n.aabb) {
                continue;
            }
            if n.count == 0 {
                stack.push(n.first);
                stack.push(n.right);
            } else {
                for k in 0..n.count {
                    visit(self.order[(n.first + k) as usize]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(p: DVec3) -> Aabb {
        Aabb {
            min: p,
            max: p + DVec3::ONE,
        }
    }

    #[test]
    fn an_empty_tree_visits_nothing() {
        let b = Bvh::build(&[]);
        assert!(b.is_empty());
        b.query(&|_| true, &mut |_| panic!());
    }

    #[test]
    fn the_query_finds_exactly_the_items_in_a_region() {
        let boxes: Vec<Aabb> = (0..500)
            .map(|i| {
                unit(DVec3::new(
                    (i % 10) as f64 * 3.0,
                    ((i / 10) % 10) as f64 * 3.0,
                    (i / 100) as f64 * 3.0,
                ))
            })
            .collect();
        let bvh = Bvh::build(&boxes);
        assert!(!bvh.is_empty());
        let region = Aabb {
            min: DVec3::new(2.0, 2.0, -1.0),
            max: DVec3::new(10.0, 8.0, 4.0),
        };
        let overlaps = |a: &Aabb| a.min.cmple(region.max).all() && a.max.cmpge(region.min).all();
        let mut found = Vec::new();
        bvh.query(&overlaps, &mut |i| {
            if overlaps(&boxes[i as usize]) {
                found.push(i);
            }
        });
        found.sort();
        let expect: Vec<u32> = (0..500).filter(|i| overlaps(&boxes[*i as usize])).collect();
        assert_eq!(found, expect);
        assert!(!found.is_empty());
    }

    #[test]
    fn the_same_input_gives_the_same_tree() {
        let boxes: Vec<Aabb> = (0..200)
            .map(|i| unit(DVec3::new((i % 7) as f64, (i % 5) as f64, 0.0)))
            .collect();
        let (a, b) = (Bvh::build(&boxes), Bvh::build(&boxes));
        assert_eq!(a.order, b.order);
    }

    #[test]
    fn ray_boxes() {
        let b = Aabb {
            min: DVec3::splat(-1.0),
            max: DVec3::splat(1.0),
        };
        assert_eq!(
            b.ray_enter(DVec3::new(-5.0, 0.0, 0.0), DVec3::X, 0.0),
            Some(4.0)
        );
        assert_eq!(b.ray_enter(DVec3::ZERO, DVec3::X, 0.0), Some(0.0));
        assert_eq!(b.ray_enter(DVec3::new(-5.0, 3.0, 0.0), DVec3::X, 0.0), None);
        assert!(
            b.ray_enter(DVec3::new(-5.0, 1.5, 0.0), DVec3::X, 1.0)
                .is_some()
        );
        assert_eq!(b.ray_enter(DVec3::new(5.0, 0.0, 0.0), DVec3::X, 0.0), None);
        assert_eq!(b.ray_enter(DVec3::new(0.0, 5.0, 5.0), DVec3::X, 0.0), None);
        assert_eq!(
            Aabb::of_points(&[DVec3::ZERO, DVec3::ONE]).center(),
            DVec3::splat(0.5)
        );
        assert_eq!(b.corners().len(), 8);
    }
}
