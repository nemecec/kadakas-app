//! Picking an edge of an outline with the pointer (align the pattern to an edge). The index is the
//! one that `align_to_edge` takes: edge n runs from vertex n of the outline to vertex n + 1.

use glam::DVec2;
use kadakas_core::curve::{self, Vertex};
use kadakas_core::geom::dist_to_segment;

/// The edge of the closed ring `outline` that is nearest to `p` and within `tol`. The edge may be
/// an arc: it is cut into short straight pieces for the distance.
pub fn nearest_edge(outline: &[Vertex], p: DVec2, tol: f64) -> Option<usize> {
    let n = outline.len();
    let mut best: Option<(usize, f64)> = None;
    for i in 0..n {
        let piece = [outline[i], outline[(i + 1) % n]];
        // The kind of the first vertex says how the edge to the next one runs.
        let pts = curve::flatten(&piece, false, (tol / 4.0).max(0.01));
        let d = pts
            .windows(2)
            .map(|w| dist_to_segment(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min);
        if d <= tol && best.is_none_or(|(_, b)| d < b) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// The edge of a ring of plain points (a zone outline).
pub fn nearest_ring_edge(ring: &[DVec2], p: DVec2, tol: f64) -> Option<usize> {
    nearest_edge(&curve::corners(ring), p, tol)
}

/// The two ends of edge `n` of a ring of vertices, for highlighting it.
pub fn edge_points(outline: &[Vertex], n: usize, tol: f64) -> Vec<DVec2> {
    if outline.is_empty() {
        return Vec::new();
    }
    let n = n % outline.len();
    let piece = [outline[n], outline[(n + 1) % outline.len()]];
    curve::flatten(&piece, false, (tol / 4.0).max(0.01))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_edge_of_a_square_is_found_and_far_points_find_none() {
        let sq = curve::corners(&[
            DVec2::new(0.0, 0.0),
            DVec2::new(1000.0, 0.0),
            DVec2::new(1000.0, 1000.0),
            DVec2::new(0.0, 1000.0),
        ]);
        assert_eq!(nearest_edge(&sq, DVec2::new(500.0, 3.0), 10.0), Some(0));
        assert_eq!(nearest_edge(&sq, DVec2::new(997.0, 400.0), 10.0), Some(1));
        assert_eq!(nearest_edge(&sq, DVec2::new(400.0, 1004.0), 10.0), Some(2));
        // The closing edge runs from the last vertex to the first.
        assert_eq!(nearest_edge(&sq, DVec2::new(2.0, 400.0), 10.0), Some(3));
        assert_eq!(nearest_edge(&sq, DVec2::new(500.0, 500.0), 10.0), None);
        // The ring of plain points gives the same answer.
        let ring: Vec<DVec2> = sq.iter().map(|v| v.p).collect();
        assert_eq!(nearest_ring_edge(&ring, DVec2::new(500.0, 3.0), 10.0), Some(0));
        assert_eq!(edge_points(&sq, 3, 10.0).len(), 2);
        assert!(edge_points(&[], 0, 10.0).is_empty());
    }
}
