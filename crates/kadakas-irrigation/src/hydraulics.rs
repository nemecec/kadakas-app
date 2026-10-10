//! Hydraulic formulas. Pure functions with units in the names.
//!
//! Units: flow in l/min, lengths in m, diameters in mm, pressure in kPa, velocity in m/s,
//! precipitation in mm/h.
//!
//! # Pressure loss: Hazen-Williams
//!
//! In SI units the head loss in a pipe is
//!
//! ```text
//! hf = 10.67 * L * Q^1.852 / (C^1.852 * d^4.87)
//! ```
//!
//! with `hf` in m of water, `L` the length in m, `Q` the flow in m3/s, `d` the inner diameter in m
//! and `C` the roughness coefficient of the pipe material (PVC 150, PE 140, copper 140, ...).
//! The pressure loss is `hf` times 9.80665 kPa per m (water at 4 C).
//! The input flow is in l/min, so `Q = flow / 60 000`.
//!
//! # Valves
//!
//! A valve has a flow factor `kv` (m3/h at a loss of 1 bar). The loss is
//! `100 * (Q / kv)^2` kPa with `Q` in m3/h.
//!
//! # Precipitation rate
//!
//! `PR = 60 * Q / A` mm/h for a flow `Q` in l/min spread over an area `A` in m2
//! (1 l/m2 is 1 mm of water). For heads in a layout, `A` is the square `S * S`
//! (square pattern) or `S * S * sin(60 deg)` (triangular pattern) for the head-to-head spacing `S`.

/// Pressure of 1 m of water column, in kPa.
pub const KPA_PER_M_HEAD: f64 = 9.80665;

/// Velocity limit in pipes, in m/s. Faster flow gives a warning (water hammer, noise, wear).
pub const MAX_VELOCITY_MS: f64 = 1.5;

/// Head loss of a pipe in m of water (Hazen-Williams). Zero for no flow or no length.
/// A pipe without a positive diameter or roughness gives infinity when it carries flow.
pub fn head_loss_m(flow_lpm: f64, inner_diameter_mm: f64, c: f64, length_m: f64) -> f64 {
    if flow_lpm <= 0.0 || length_m <= 0.0 {
        return 0.0;
    }
    if inner_diameter_mm <= 0.0 || c <= 0.0 {
        return f64::INFINITY;
    }
    let q = flow_lpm / 60_000.0;
    let d = inner_diameter_mm / 1000.0;
    10.67 * length_m * q.powf(1.852) / (c.powf(1.852) * d.powf(4.87))
}

/// Pressure loss of a pipe in kPa (Hazen-Williams, see the module text).
pub fn pipe_loss_kpa(flow_lpm: f64, inner_diameter_mm: f64, c: f64, length_m: f64) -> f64 {
    head_loss_m(flow_lpm, inner_diameter_mm, c, length_m) * KPA_PER_M_HEAD
}

/// Mean velocity in m/s: flow divided by the cross-section area.
pub fn velocity_ms(flow_lpm: f64, inner_diameter_mm: f64) -> f64 {
    if flow_lpm <= 0.0 {
        return 0.0;
    }
    if inner_diameter_mm <= 0.0 {
        return f64::INFINITY;
    }
    let d = inner_diameter_mm / 1000.0;
    (flow_lpm / 60_000.0) / (std::f64::consts::PI * d * d / 4.0)
}

/// Pressure loss of a valve in kPa from its flow factor `kv` (m3/h at 1 bar).
pub fn valve_loss_kpa(flow_lpm: f64, kv: f64) -> f64 {
    if flow_lpm <= 0.0 {
        return 0.0;
    }
    if kv <= 0.0 {
        return f64::INFINITY;
    }
    let q = flow_lpm * 60.0 / 1000.0;
    100.0 * (q / kv).powi(2)
}

/// Precipitation rate in mm/h of a flow (l/min) over an area (m2). Zero for no area.
pub fn precipitation_rate(flow_lpm: f64, area_m2: f64) -> f64 {
    if area_m2 <= 0.0 {
        0.0
    } else {
        60.0 * flow_lpm / area_m2
    }
}

/// Precipitation rate of a head in a square layout with head-to-head spacing `spacing_m`.
pub fn precipitation_rate_square(flow_lpm: f64, spacing_m: f64) -> f64 {
    precipitation_rate(flow_lpm, spacing_m * spacing_m)
}

/// Precipitation rate of a head in a triangular layout (rows `spacing * sin 60` apart).
pub fn precipitation_rate_triangular(flow_lpm: f64, spacing_m: f64) -> f64 {
    precipitation_rate(
        flow_lpm,
        spacing_m * spacing_m * (std::f64::consts::PI / 3.0).sin(),
    )
}

/// Minutes of running time that give `depth_mm` of water at a precipitation rate in mm/h.
/// `None` when the rate is not positive.
pub fn run_time_minutes(depth_mm: f64, rate_mm_h: f64) -> Option<f64> {
    if rate_mm_h > 0.0 && depth_mm >= 0.0 {
        Some(depth_mm / rate_mm_h * 60.0)
    } else {
        None
    }
}

/// Area of a circular sector in m2 from a radius in mm and an arc in degrees.
pub fn sector_area_m2(radius_mm: f64, arc_deg: f64) -> f64 {
    let r = radius_mm / 1000.0;
    std::f64::consts::PI * r * r * (arc_deg.clamp(0.0, 360.0) / 360.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1e-12)
    }

    /// Hand calculation. Q = 60 l/min = 0.001 m3/s, d = 50 mm, C = 150, L = 100 m.
    /// Q^1.852 = 2.7867e-6, C^1.852 = 10 693, d^4.87 = 4.5395e-7 (all rounded).
    /// hf = 10.67 * 100 * 2.7867e-6 / (10 693 * 4.5395e-7) = 0.5999 m. Loss = 0.5999 * 9.80665 = 5.883 kPa.
    #[test]
    fn hazen_williams_hand_example() {
        assert!(close(head_loss_m(60.0, 50.0, 150.0, 100.0), 0.59987, 1e-3));
        assert!(close(pipe_loss_kpa(60.0, 50.0, 150.0, 100.0), 5.8827, 1e-3));
    }

    /// Q = 30 l/min, d = 25.4 mm (1 inch), C = 150, L = 100 m: hf = 4.498 m = 44.11 kPa.
    #[test]
    fn hazen_williams_second_example() {
        assert!(close(pipe_loss_kpa(30.0, 25.4, 150.0, 100.0), 44.108, 1e-3));
    }

    #[test]
    fn loss_edge_cases() {
        assert_eq!(head_loss_m(0.0, 50.0, 150.0, 10.0), 0.0);
        assert_eq!(head_loss_m(10.0, 50.0, 150.0, 0.0), 0.0);
        assert!(head_loss_m(10.0, 0.0, 150.0, 10.0).is_infinite());
        assert!(head_loss_m(10.0, 50.0, 0.0, 10.0).is_infinite());
    }

    /// 60 l/min = 0.001 m3/s through 50 mm: area = pi * 0.025^2 = 0.0019635 m2. v = 0.5093 m/s.
    /// The limit of 1.5 m/s in 25 mm pipe is 44.18 l/min: 7.363e-4 m3/s / 4.9087e-4 m2.
    #[test]
    fn velocity_hand_example() {
        assert!(close(velocity_ms(60.0, 50.0), 0.50930, 1e-4));
        assert_eq!(velocity_ms(0.0, 50.0), 0.0);
        assert!(velocity_ms(1.0, 0.0).is_infinite());
        assert!(close(velocity_ms(44.18, 25.0), MAX_VELOCITY_MS, 1e-3));
    }

    /// Q = 30 l/min = 1.8 m3/h, kv = 3.0: loss = 100 * (1.8 / 3.0)^2 = 36 kPa.
    #[test]
    fn valve_hand_example() {
        assert!(close(valve_loss_kpa(30.0, 3.0), 36.0, 1e-12));
        assert_eq!(valve_loss_kpa(0.0, 3.0), 0.0);
        assert!(valve_loss_kpa(1.0, 0.0).is_infinite());
    }

    /// 8 l/min per head at 4 m spacing: 60 * 8 / 16 = 30 mm/h (square).
    /// Triangular: 60 * 8 / (16 * 0.8660) = 34.64 mm/h.
    #[test]
    fn precipitation_hand_examples() {
        assert!(close(precipitation_rate_square(8.0, 4.0), 30.0, 1e-12));
        assert!(close(precipitation_rate_triangular(8.0, 4.0), 34.641, 1e-4));
        assert_eq!(precipitation_rate(10.0, 0.0), 0.0);
        // 30 mm/h and a target of 12 mm: 24 minutes.
        assert!(close(run_time_minutes(12.0, 30.0).unwrap(), 24.0, 1e-12));
        assert_eq!(run_time_minutes(12.0, 0.0), None);
        assert_eq!(run_time_minutes(-1.0, 10.0), None);
    }

    #[test]
    fn sector_areas() {
        assert!(close(
            sector_area_m2(1000.0, 360.0),
            std::f64::consts::PI,
            1e-12
        ));
        assert!(close(
            sector_area_m2(1000.0, 90.0),
            std::f64::consts::PI / 4.0,
            1e-12
        ));
        assert_eq!(sector_area_m2(1000.0, -5.0), 0.0);
    }

    proptest! {
        #[test]
        fn loss_grows_with_flow_and_length(q in 1.0f64..200.0, d in 10.0f64..100.0, l in 1.0f64..500.0) {
            let a = pipe_loss_kpa(q, d, 140.0, l);
            prop_assert!(a > 0.0 && a.is_finite());
            prop_assert!(pipe_loss_kpa(q * 1.5, d, 140.0, l) > a);
            prop_assert!(pipe_loss_kpa(q, d, 140.0, l * 2.0) > a);
            prop_assert!(pipe_loss_kpa(q, d * 1.2, 140.0, l) < a);
            // Loss is linear in length.
            prop_assert!(close(pipe_loss_kpa(q, d, 140.0, l * 2.0), 2.0 * a, 1e-9));
        }

        #[test]
        fn run_time_inverts_rate(depth in 0.1f64..50.0, rate in 1.0f64..100.0) {
            let t = run_time_minutes(depth, rate).unwrap();
            prop_assert!(close(t / 60.0 * rate, depth, 1e-9));
        }
    }
}
