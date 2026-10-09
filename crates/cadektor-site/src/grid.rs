//! A uniform grid that finds the triangles near a point or box.

use glam::DVec2;

/// Triangles sorted into square cells. Each triangle is in every cell that its box touches.
#[derive(Clone, Debug)]
pub struct GridIndex {
    origin: DVec2,
    cell: f64,
    nx: usize,
    ny: usize,
    /// `starts[c]..starts[c + 1]` is the range of cell `c` in `items`.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl GridIndex {
    /// Builds the index. `boxes` holds the (min, max) box of each item, and the item number is
    /// its position in the slice.
    pub fn new(boxes: &[(DVec2, DVec2)]) -> Self {
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for (a, b) in boxes {
            lo = lo.min(*a);
            hi = hi.max(*b);
        }
        if boxes.is_empty() {
            lo = DVec2::ZERO;
            hi = DVec2::ZERO;
        }
        let size = (hi - lo).max(DVec2::splat(1e-9));
        // About two items per cell, and never more than 4 cells per item.
        let target_cells = (boxes.len() as f64 / 2.0).max(1.0);
        let cell = (size.x * size.y / target_cells).sqrt().max(size.max_element() / 4096.0);
        let nx = ((size.x / cell).ceil() as usize).max(1);
        let ny = ((size.y / cell).ceil() as usize).max(1);
        let mut grid = GridIndex { origin: lo, cell, nx, ny, starts: vec![0; nx * ny + 1], items: Vec::new() };
        for (a, b) in boxes {
            let ((x0, y0), (x1, y1)) = grid.cell_range(*a, *b);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid.starts[y * nx + x + 1] += 1;
                }
            }
        }
        for c in 0..nx * ny {
            grid.starts[c + 1] += grid.starts[c];
        }
        let mut fill = grid.starts.clone();
        grid.items = vec![0; *grid.starts.last().unwrap() as usize];
        for (i, (a, b)) in boxes.iter().enumerate() {
            let ((x0, y0), (x1, y1)) = grid.cell_range(*a, *b);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = y * nx + x;
                    grid.items[fill[c] as usize] = i as u32;
                    fill[c] += 1;
                }
            }
        }
        grid
    }

    fn cell_of(&self, p: DVec2) -> (usize, usize) {
        let f = |v: f64, n: usize| ((v / self.cell).floor().max(0.0) as usize).min(n - 1);
        (f(p.x - self.origin.x, self.nx), f(p.y - self.origin.y, self.ny))
    }

    fn cell_range(&self, lo: DVec2, hi: DVec2) -> ((usize, usize), (usize, usize)) {
        (self.cell_of(lo), self.cell_of(hi))
    }

    /// Items in the cell of `p`.
    pub fn near_point(&self, p: DVec2) -> &[u32] {
        let (x, y) = self.cell_of(p);
        let c = y * self.nx + x;
        &self.items[self.starts[c] as usize..self.starts[c + 1] as usize]
    }

    /// Calls `f` once for every item whose cells touch the box. An item can be reported
    /// more than once, so the caller removes repeats.
    pub fn for_each_in_box(&self, lo: DVec2, hi: DVec2, mut f: impl FnMut(u32)) {
        let ((x0, y0), (x1, y1)) = self.cell_range(lo, hi);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = y * self.nx + x;
                self.items[self.starts[c] as usize..self.starts[c + 1] as usize].iter().for_each(|i| f(*i));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> (DVec2, DVec2) {
        (DVec2::new(x0, y0), DVec2::new(x1, y1))
    }

    #[test]
    fn finds_items_near_a_point_and_in_a_box() {
        let boxes = vec![bx(0.0, 0.0, 1.0, 1.0), bx(9.0, 9.0, 10.0, 10.0), bx(0.0, 0.0, 10.0, 0.5)];
        let g = GridIndex::new(&boxes);
        assert!(g.near_point(DVec2::new(0.5, 0.2)).contains(&0));
        assert!(g.near_point(DVec2::new(0.5, 0.2)).contains(&2));
        assert!(g.near_point(DVec2::new(9.5, 9.5)).contains(&1));
        let mut seen = Vec::new();
        g.for_each_in_box(DVec2::new(8.0, 8.0), DVec2::new(11.0, 11.0), |i| seen.push(i));
        assert!(seen.contains(&1));
        // A point outside the grid is clamped to the nearest cell instead of failing.
        let _ = g.near_point(DVec2::new(-100.0, 500.0));
    }

    #[test]
    fn empty_index_has_no_items() {
        let g = GridIndex::new(&[]);
        assert!(g.near_point(DVec2::ZERO).is_empty());
    }
}
