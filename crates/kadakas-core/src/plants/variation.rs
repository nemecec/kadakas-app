//! Random variation of placed plants, and the limit of one placement request. Both are part of
//! commands, so they live in core. The placement math is in the `kadakas-plants` crate.

use serde::{Deserialize, Serialize};

/// The most plants that one request may make.
pub const MAX_POINTS: usize = 20_000;

/// Random variation of placed plants. The same seed always gives the same plants.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Variation {
    pub seed: u64,
    /// Size changes by up to this fraction up or down: 0.2 is 80% to 120%. From 0 to 0.9.
    pub size: f64,
    /// Turn each plant by a random angle.
    pub rotation: bool,
}

impl Default for Variation {
    fn default() -> Self {
        Variation {
            seed: 1,
            size: 0.0,
            rotation: false,
        }
    }
}

impl Variation {
    pub fn validate(&self) -> Result<(), String> {
        if self.size.is_finite() && (0.0..=0.9).contains(&self.size) {
            Ok(())
        } else {
            Err("size variation must be from 0 to 0.9".into())
        }
    }

    /// True if the variation changes anything.
    pub fn is_active(&self) -> bool {
        self.size > 0.0 || self.rotation
    }

    /// For each of `n` plants: the size factor and the rotation in radians. Plant `i` always gets the
    /// same values for the same seed, whatever `n` is.
    pub fn factors(&self, n: usize) -> Vec<(f64, f64)> {
        (0..n as u64)
            .map(|i| {
                let a = unit(self.seed, 2 * i);
                let b = unit(self.seed, 2 * i + 1);
                let scale = 1.0 + self.size * (2.0 * a - 1.0);
                let angle = if self.rotation {
                    b * std::f64::consts::TAU
                } else {
                    0.0
                };
                (scale, angle)
            })
            .collect()
    }
}

/// A number from 0 (included) to 1 (excluded), the same for the same seed and index (splitmix64).
fn unit(seed: u64, index: u64) -> f64 {
    let mut z = seed.wrapping_add(index.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}
