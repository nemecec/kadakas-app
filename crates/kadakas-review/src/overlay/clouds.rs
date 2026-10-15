//! Clusters of changes for revision clouds (IOP-24, ANN-10).
//!
//! A comparison of two drawings can have thousands of changes. A revision cloud is drawn around a
//! *cluster*: changes whose boxes lie close together. [`clusters`] groups boxes by a grid, in time
//! linear in their number, and gives the box around each group, the biggest groups first. The
//! count of clouds is limited, so a drawing in which everything changed gets a few clouds and not
//! thousands.

use std::collections::HashMap;

use glam::DVec2;

/// The most clouds that [`clusters`] returns.
pub const MAX_CLOUDS: usize = 64;
/// The most boxes that [`clusters`] reads. More are counted in `skipped`.
pub const MAX_BOXES: usize = 500_000;
/// The most grid cells that one box may cover. A longer box is tested against every box instead.
const MAX_CELLS_PER_BOX: i64 = 4096;
/// The most boxes that are tested that way. Further long boxes join only through the grid, which
/// they do not enter: they stay alone.
const MAX_BIG_BOXES: usize = 64;

/// A group of changes that lie close together.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    /// The box around the group.
    pub lo: DVec2,
    pub hi: DVec2,
    /// How many changes are in the group.
    pub count: usize,
}

/// The clusters, and what the bounds left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clusters {
    /// At most [`MAX_CLOUDS`], the biggest first (then by place).
    pub clusters: Vec<Cluster>,
    /// Groups beyond [`MAX_CLOUDS`] that have no cloud.
    pub dropped: usize,
    /// Boxes beyond [`MAX_BOXES`] that were not read.
    pub skipped: usize,
}

/// A good distance for `gap` for boxes in a drawing: 4 percent of the size of the drawing, at least
/// 1 (a unit of the drawing).
pub fn default_gap(boxes: &[(DVec2, DVec2)]) -> f64 {
    let (lo, hi) = boxes
        .iter()
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), (a, b)| (lo.min(*a), hi.max(*b)),
        );
    if !lo.is_finite() || !hi.is_finite() {
        return 1.0;
    }
    ((hi - lo).max_element() * 0.04).max(1.0)
}

fn find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        let p = parent[i as usize];
        parent[i as usize] = parent[p as usize];
        i = p;
    }
    i
}

/// Groups boxes (`lo`, `hi`) whose neighborhoods of `gap` touch. Boxes that are not finite are
/// ignored. `gap` must be a positive number.
pub fn clusters(boxes: &[(DVec2, DVec2)], gap: f64) -> Clusters {
    let gap = if gap.is_finite() && gap > 0.0 {
        gap
    } else {
        1.0
    };
    let read = boxes.len().min(MAX_BOXES);
    let mut out = Clusters {
        skipped: boxes.len() - read,
        ..Clusters::default()
    };
    let boxes: Vec<(DVec2, DVec2)> = boxes[..read]
        .iter()
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .map(|(a, b)| (a.min(*b), a.max(*b)))
        .collect();
    let mut parent: Vec<u32> = (0..boxes.len() as u32).collect();
    let mut cells: HashMap<(i64, i64), u32> = HashMap::new();
    let mut big: Vec<u32> = Vec::new();
    let cell = |v: f64| (v / gap).floor().clamp(-1.0e15, 1.0e15) as i64;
    for (i, (lo, hi)) in boxes.iter().enumerate() {
        // The box grown by half a gap on every side: two boxes less than a gap apart share a cell
        // of the grid that is a gap wide, or neighbor cells that this loop covers as well.
        let (x0, x1) = (cell(lo.x - gap / 2.0), cell(hi.x + gap / 2.0));
        let (y0, y1) = (cell(lo.y - gap / 2.0), cell(hi.y + gap / 2.0));
        let span = (x1 - x0 + 1).saturating_mul(y1 - y0 + 1);
        if span > MAX_CELLS_PER_BOX {
            // Too long for the grid: it is tested against every box below.
            if big.len() < MAX_BIG_BOXES {
                big.push(i as u32);
            }
            continue;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                match cells.get(&(x, y)) {
                    Some(&j) => {
                        let (a, b) = (find(&mut parent, i as u32), find(&mut parent, j));
                        if a != b {
                            parent[a.max(b) as usize] = a.min(b);
                        }
                    }
                    None => {
                        cells.insert((x, y), i as u32);
                    }
                }
            }
        }
    }
    let half = gap / 2.0;
    for &k in &big {
        let (klo, khi) = boxes[k as usize];
        for (j, (lo, hi)) in boxes.iter().enumerate() {
            let apart = (lo.x - half > khi.x + half)
                || (klo.x - half > hi.x + half)
                || (lo.y - half > khi.y + half)
                || (klo.y - half > hi.y + half);
            if !apart {
                let (a, b) = (find(&mut parent, k), find(&mut parent, j as u32));
                if a != b {
                    parent[a.max(b) as usize] = a.min(b);
                }
            }
        }
    }
    let mut groups: HashMap<u32, Cluster> = HashMap::new();
    for (i, (lo, hi)) in boxes.iter().enumerate() {
        let root = find(&mut parent, i as u32);
        groups
            .entry(root)
            .and_modify(|c| {
                c.lo = c.lo.min(*lo);
                c.hi = c.hi.max(*hi);
                c.count += 1;
            })
            .or_insert(Cluster {
                lo: *lo,
                hi: *hi,
                count: 1,
            });
    }
    let mut list: Vec<Cluster> = groups.into_values().collect();
    // The biggest first; the place breaks ties, so the result does not depend on the hash map.
    list.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.lo.x.total_cmp(&b.lo.x))
            .then(a.lo.y.total_cmp(&b.lo.y))
    });
    out.dropped = list.len().saturating_sub(MAX_CLOUDS);
    list.truncate(MAX_CLOUDS);
    out.clusters = list;
    out
}

/// The four corners of the box of a cluster grown by `pad` on every side, counter-clockwise: the
/// points of a revision cloud.
pub fn corners(c: &Cluster, pad: f64) -> [DVec2; 4] {
    let (lo, hi) = (c.lo - DVec2::splat(pad), c.hi + DVec2::splat(pad));
    [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f64, y: f64, w: f64) -> (DVec2, DVec2) {
        (DVec2::new(x, y), DVec2::new(x + w, y + w))
    }

    #[test]
    fn close_boxes_make_one_cluster_and_far_ones_another() {
        let boxes = vec![
            b(0.0, 0.0, 10.0),
            b(15.0, 0.0, 10.0),
            b(30.0, 5.0, 10.0),
            b(1000.0, 1000.0, 10.0),
        ];
        let c = clusters(&boxes, 10.0);
        assert_eq!(c.clusters.len(), 2);
        assert_eq!(c.clusters[0].count, 3);
        assert_eq!(c.clusters[0].lo, DVec2::ZERO);
        assert_eq!(c.clusters[0].hi, DVec2::new(40.0, 15.0));
        assert_eq!(c.clusters[1].count, 1);
        assert_eq!(c.dropped + c.skipped, 0);
    }

    #[test]
    fn the_order_of_the_input_does_not_matter() {
        let mut boxes: Vec<_> = (0..50)
            .map(|i| b(i as f64 * 12.0, (i % 3) as f64, 5.0))
            .collect();
        boxes.extend((0..20).map(|i| b(5000.0 + i as f64 * 6.0, 800.0, 4.0)));
        let a = clusters(&boxes, 10.0);
        boxes.reverse();
        assert_eq!(a, clusters(&boxes, 10.0));
        assert_eq!(a.clusters.len(), 2);
    }

    #[test]
    fn the_number_of_clouds_is_limited() {
        let boxes: Vec<_> = (0..(MAX_CLOUDS * 3))
            .map(|i| b(i as f64 * 1000.0, 0.0, 1.0))
            .collect();
        let c = clusters(&boxes, 5.0);
        assert_eq!(c.clusters.len(), MAX_CLOUDS);
        assert_eq!(c.dropped, MAX_CLOUDS * 2);
    }

    #[test]
    fn bad_input_is_ignored() {
        let boxes = vec![
            (DVec2::new(f64::NAN, 0.0), DVec2::ZERO),
            (DVec2::new(f64::INFINITY, 0.0), DVec2::ONE),
            b(0.0, 0.0, 1.0),
        ];
        assert_eq!(clusters(&boxes, f64::NAN).clusters.len(), 1);
        assert_eq!(clusters(&boxes, -3.0).clusters.len(), 1);
        assert!(clusters(&[], 1.0).clusters.is_empty());
        assert_eq!(default_gap(&[]), 1.0);
        assert_eq!(default_gap(&boxes), 1.0);
        // A very long box covers a bounded number of cells and still joins what it touches.
        let long = vec![
            (DVec2::new(-1.0e12, 0.0), DVec2::new(1.0e12, 1.0)),
            b(0.0, 0.0, 1.0),
        ];
        assert_eq!(clusters(&long, 1.0).clusters.len(), 1);
    }

    #[test]
    fn the_gap_follows_the_size_of_the_drawing_and_the_corners_are_counter_clockwise() {
        let boxes = vec![b(0.0, 0.0, 1.0), b(1000.0, 500.0, 1.0)];
        assert!((default_gap(&boxes) - 40.04).abs() < 1e-9);
        let c = Cluster {
            lo: DVec2::ZERO,
            hi: DVec2::new(10.0, 4.0),
            count: 2,
        };
        let k = corners(&c, 1.0);
        assert_eq!(k[0], DVec2::new(-1.0, -1.0));
        assert_eq!(k[2], DVec2::new(11.0, 5.0));
        let area: f64 = (0..4).map(|i| k[i].perp_dot(k[(i + 1) % 4])).sum::<f64>() / 2.0;
        assert!(area > 0.0);
    }

    #[test]
    fn many_boxes_are_read_in_bounded_time() {
        let boxes: Vec<_> = (0..20_000)
            .map(|i| b((i % 200) as f64 * 3.0, (i / 200) as f64 * 3.0, 1.0))
            .collect();
        let c = clusters(&boxes, 3.0);
        assert_eq!(c.clusters.len(), 1);
        assert_eq!(c.clusters[0].count, 20_000);
    }
}
