//! B-spline curves and surfaces (rational or not): evaluation by the Cox-de Boor recurrence.
//! Public algorithm, written here from the definition.

use glam::DVec3;
use serde::{Deserialize, Serialize};

/// The highest degree that the reader accepts.
pub const MAX_DEGREE: usize = 25;

/// The knot vector, expanded: one entry for each multiplicity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Knots {
    pub degree: usize,
    pub values: Vec<f64>,
    /// Number of control points along this direction.
    pub count: usize,
}

impl Knots {
    /// Checks the vector: `count + degree + 1` entries that never decrease and are finite.
    pub fn new(degree: usize, count: usize, values: Vec<f64>) -> Result<Knots, String> {
        if degree == 0 || degree > MAX_DEGREE {
            return Err(format!(
                "degree {degree} is not supported (1 to {MAX_DEGREE})"
            ));
        }
        if count < degree + 1 {
            return Err(format!(
                "{count} control points are too few for degree {degree}"
            ));
        }
        if values.len() != count + degree + 1 {
            return Err(format!(
                "{} knots, but {count} control points of degree {degree} need {}",
                values.len(),
                count + degree + 1
            ));
        }
        if values.iter().any(|k| !k.is_finite()) || values.windows(2).any(|w| w[1] < w[0]) {
            return Err("the knots are not finite or not in order".into());
        }
        let k = Knots {
            degree,
            values,
            count,
        };
        let (a, b) = k.domain();
        if b <= a {
            return Err("the parameter range of the knots is empty".into());
        }
        Ok(k)
    }

    /// The checks of [`Knots::new`] for a vector that comes from a file.
    pub fn check(&self) -> Result<(), String> {
        Knots::new(self.degree, self.count, self.values.clone()).map(|_| ())
    }

    /// A clamped uniform vector (also the vector of a Bezier curve when `count = degree + 1`).
    pub fn clamped(degree: usize, count: usize) -> Result<Knots, String> {
        let spans = count.saturating_sub(degree).max(1);
        let mut v = vec![0.0; degree + 1];
        for i in 1..spans {
            v.push(i as f64);
        }
        v.extend(std::iter::repeat_n(spans as f64, degree + 1));
        Knots::new(degree, count, v)
    }

    /// An unclamped uniform vector.
    pub fn uniform(degree: usize, count: usize) -> Result<Knots, String> {
        Knots::new(
            degree,
            count,
            (0..count + degree + 1).map(|i| i as f64).collect(),
        )
    }

    /// Expands knots with multiplicities.
    pub fn expand(
        degree: usize,
        count: usize,
        mult: &[i64],
        knots: &[f64],
    ) -> Result<Knots, String> {
        if mult.len() != knots.len() || mult.is_empty() {
            return Err("knots and multiplicities differ in length".into());
        }
        let mut v = Vec::new();
        for (m, k) in mult.iter().zip(knots) {
            if *m < 1 || *m as usize > degree + 1 {
                return Err(format!("multiplicity {m} is not valid for degree {degree}"));
            }
            if v.len() + *m as usize > count + degree + 1 {
                return Err("the multiplicities add up to more than the knots needed".into());
            }
            v.extend(std::iter::repeat_n(*k, *m as usize));
        }
        Knots::new(degree, count, v)
    }

    pub fn domain(&self) -> (f64, f64) {
        (self.values[self.degree], self.values[self.count])
    }

    /// The span index `i` with `k[i] <= t < k[i+1]`, clamped to the valid spans.
    fn span(&self, t: f64) -> usize {
        let (a, b) = self.domain();
        let t = t.clamp(a, b);
        let (lo, hi) = (self.degree, self.count - 1);
        if t >= self.values[hi + 1].min(b) && self.values[hi] < b {
            return hi;
        }
        // Binary search for the last span with k[i] <= t.
        let (mut l, mut h) = (lo, hi);
        while l < h {
            let m = (l + h).div_ceil(2);
            if self.values[m] <= t {
                l = m;
            } else {
                h = m - 1;
            }
        }
        // Skip empty spans at the top of a clamped vector.
        let mut i = l;
        while i > lo && self.values[i + 1] <= self.values[i] {
            i -= 1;
        }
        i
    }

    /// The `degree + 1` basis functions that are not zero at `t`, and the span.
    pub fn basis(&self, t: f64) -> (usize, [f64; MAX_DEGREE + 1]) {
        let p = self.degree;
        let (a, b) = self.domain();
        let t = t.clamp(a, b);
        let i = self.span(t);
        let k = &self.values;
        let mut n = [0.0; MAX_DEGREE + 1];
        let mut left = [0.0; MAX_DEGREE + 1];
        let mut right = [0.0; MAX_DEGREE + 1];
        n[0] = 1.0;
        for j in 1..=p {
            left[j] = t - k[i + 1 - j];
            right[j] = k[i + j] - t;
            let mut saved = 0.0;
            for r in 0..j {
                let d = right[r + 1] + left[j - r];
                let temp = if d != 0.0 { n[r] / d } else { 0.0 };
                n[r] = saved + right[r + 1] * temp;
                saved = left[j - r] * temp;
            }
            n[j] = saved;
        }
        (i, n)
    }
}

/// A B-spline curve in space, rational when it has weights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BSplineCurve {
    pub knots: Knots,
    pub points: Vec<DVec3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
}

impl BSplineCurve {
    /// The invariants that [`BSplineCurve::eval`] relies on, for data from a file.
    pub fn check(&self) -> Result<(), String> {
        self.knots.check()?;
        if self.points.len() != self.knots.count || self.points.iter().any(|p| !p.is_finite()) {
            return Err(
                "a spline curve has the wrong number of points or a point that is not finite"
                    .into(),
            );
        }
        if let Some(w) = &self.weights
            && (w.len() != self.points.len() || w.iter().any(|x| !x.is_finite() || *x <= 0.0))
        {
            return Err("the weights of a spline curve are not usable".into());
        }
        Ok(())
    }

    pub fn domain(&self) -> (f64, f64) {
        self.knots.domain()
    }

    pub fn eval(&self, t: f64) -> DVec3 {
        let (i, n) = self.knots.basis(t);
        let p = self.knots.degree;
        let mut acc = DVec3::ZERO;
        let mut wsum = 0.0;
        for (j, nj) in n.iter().enumerate().take(p + 1) {
            let idx = i - p + j;
            let w = self.weights.as_ref().map_or(1.0, |w| w[idx]);
            acc += self.points[idx] * (nj * w);
            wsum += nj * w;
        }
        if wsum != 0.0 && wsum.is_finite() {
            acc / wsum
        } else {
            acc
        }
    }
}

/// A B-spline surface in space, rational when it has weights. Points are in rows of `u`
/// (`points[iu * nv + iv]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BSplineSurface {
    pub u: Knots,
    pub v: Knots,
    pub points: Vec<DVec3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
}

impl BSplineSurface {
    /// The invariants that [`BSplineSurface::eval`] relies on, for data from a file.
    pub fn check(&self) -> Result<(), String> {
        self.u.check()?;
        self.v.check()?;
        if self.points.len() != self.u.count.saturating_mul(self.v.count)
            || self.points.iter().any(|p| !p.is_finite())
        {
            return Err(
                "a spline surface has the wrong number of points or a point that is not finite"
                    .into(),
            );
        }
        if let Some(w) = &self.weights
            && (w.len() != self.points.len() || w.iter().any(|x| !x.is_finite() || *x <= 0.0))
        {
            return Err("the weights of a spline surface are not usable".into());
        }
        Ok(())
    }

    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (self.u.domain(), self.v.domain())
    }

    pub fn eval(&self, u: f64, v: f64) -> DVec3 {
        let (iu, nu) = self.u.basis(u);
        let (iv, nv) = self.v.basis(v);
        let (p, q) = (self.u.degree, self.v.degree);
        let cols = self.v.count;
        let mut acc = DVec3::ZERO;
        let mut wsum = 0.0;
        for (a, na) in nu.iter().enumerate().take(p + 1) {
            for (b, nb) in nv.iter().enumerate().take(q + 1) {
                let idx = (iu - p + a) * cols + (iv - q + b);
                let w = self.weights.as_ref().map_or(1.0, |w| w[idx]);
                let c = na * nb * w;
                acc += self.points[idx] * c;
                wsum += c;
            }
        }
        if wsum != 0.0 && wsum.is_finite() {
            acc / wsum
        } else {
            acc
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_of_unity_and_line() {
        let k = Knots::clamped(3, 6).unwrap();
        for t in [0.0, 0.3, 1.0, 1.5, 2.999, 3.0] {
            let (_, n) = k.basis(t);
            let s: f64 = n.iter().take(4).sum();
            assert!((s - 1.0).abs() < 1e-12, "{t}");
        }
        // Control points on a line, uniform spacing: the curve is that line.
        let c = BSplineCurve {
            knots: Knots::clamped(2, 5).unwrap(),
            points: (0..5).map(|i| DVec3::new(i as f64, 0.0, 0.0)).collect(),
            weights: None,
        };
        let (a, b) = c.domain();
        assert!((c.eval(a) - DVec3::ZERO).length() < 1e-12);
        assert!((c.eval(b) - DVec3::new(4.0, 0.0, 0.0)).length() < 1e-12);
        assert!((c.eval((a + b) / 2.0).x - 2.0).abs() < 1e-9);
    }

    #[test]
    fn rational_quarter_circle() {
        let w = 0.5f64.sqrt();
        let c = BSplineCurve {
            knots: Knots::clamped(2, 3).unwrap(),
            points: vec![
                DVec3::new(1.0, 0.0, 0.0),
                DVec3::new(1.0, 1.0, 0.0),
                DVec3::new(0.0, 1.0, 0.0),
            ],
            weights: Some(vec![1.0, w, 1.0]),
        };
        for k in 0..=10 {
            let p = c.eval(k as f64 / 10.0);
            assert!((p.length() - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn surface_plane_and_bad_knots() {
        let mut pts = Vec::new();
        for i in 0..2 {
            for j in 0..2 {
                pts.push(DVec3::new(i as f64 * 2.0, j as f64 * 3.0, 1.0));
            }
        }
        let s = BSplineSurface {
            u: Knots::clamped(1, 2).unwrap(),
            v: Knots::clamped(1, 2).unwrap(),
            points: pts,
            weights: None,
        };
        let p = s.eval(0.5, 0.25);
        assert!((p - DVec3::new(1.0, 0.75, 1.0)).length() < 1e-12);
        assert_eq!(s.domain(), ((0.0, 1.0), (0.0, 1.0)));
        assert!(Knots::new(0, 3, vec![0.0; 4]).is_err());
        assert!(Knots::new(2, 2, vec![0.0; 5]).is_err());
        assert!(Knots::new(1, 2, vec![0.0, 0.0, 1.0]).is_err());
        assert!(Knots::new(1, 2, vec![0.0, 1.0, 0.5, 1.0]).is_err());
        assert!(Knots::new(1, 2, vec![1.0, 1.0, 1.0, 1.0]).is_err());
        assert!(Knots::new(1, 2, vec![0.0, 0.0, f64::NAN, 1.0]).is_err());
        assert!(Knots::expand(1, 2, &[2, 2], &[0.0, 1.0]).is_ok());
        assert!(Knots::expand(1, 2, &[3, 1], &[0.0, 1.0]).is_err());
        assert!(Knots::expand(1, 2, &[2, 2], &[0.0]).is_err());
        assert!(Knots::expand(1, 2, &[2, 2, 2], &[0.0, 1.0, 2.0]).is_err());
        assert!(Knots::uniform(2, 4).is_ok());
    }
}
