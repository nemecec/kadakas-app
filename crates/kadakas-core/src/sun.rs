//! Sun position from place, date and time. The NOAA solar position algorithm (the one of the
//! NOAA solar calculator, based on Meeus, "Astronomical Algorithms"). The accuracy is better
//! than 0.01 degrees for the years 1800 to 2200, which is far more than a shadow study needs.
//!
//! Everything here is a pure function. Angles in the public API are in degrees, azimuth runs
//! clockwise from true north, and elevation is the angle above the horizon.

use serde::{Deserialize, Serialize};

/// A place, a calendar date and a clock time in a time zone.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SunTime {
    /// Degrees north, -90 to 90.
    pub latitude: f64,
    /// Degrees east, -180 to 180.
    pub longitude: f64,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Local clock time, decimal hours (0 to 24).
    pub hour: f64,
    /// Offset of the local clock from UTC, in hours (for example 2 for Helsinki in winter).
    pub tz_offset: f64,
}

/// Where the sun is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SunPosition {
    /// Degrees clockwise from true north, 0 to 360.
    pub azimuth: f64,
    /// Degrees above the horizon. Includes the bending of light in the air.
    pub elevation: f64,
    /// Declination of the sun in degrees.
    pub declination: f64,
    /// Equation of time in minutes.
    pub equation_of_time: f64,
}

/// Sunrise, solar noon and sunset as local decimal hours. `sunrise` and `sunset` are `None` on
/// days when the sun stays above or below the horizon.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SunDay {
    pub sunrise: Option<f64>,
    pub solar_noon: f64,
    pub sunset: Option<f64>,
    /// True when the sun does not set (polar day).
    pub always_up: bool,
}

/// Why a [`SunTime`] is not valid.
pub fn check(t: &SunTime) -> Result<(), String> {
    if !(t.latitude.is_finite() && (-90.0..=90.0).contains(&t.latitude)) {
        return Err("latitude must be from -90 to 90".into());
    }
    if !(t.longitude.is_finite() && (-180.0..=180.0).contains(&t.longitude)) {
        return Err("longitude must be from -180 to 180".into());
    }
    if !(1800..=2200).contains(&t.year) {
        return Err("year must be from 1800 to 2200".into());
    }
    if !(1..=12).contains(&t.month) || t.day < 1 || t.day > days_in_month(t.year, t.month) {
        return Err("the date does not exist".into());
    }
    if !(t.hour.is_finite() && (0.0..=24.0).contains(&t.hour)) {
        return Err("hour must be from 0 to 24".into());
    }
    if !(t.tz_offset.is_finite() && (-14.0..=14.0).contains(&t.tz_offset)) {
        return Err("time zone offset must be from -14 to 14 hours".into());
    }
    Ok(())
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Julian day number at 0h UT of a Gregorian date (a half-integer).
pub fn julian_day(year: i32, month: u32, day: u32) -> f64 {
    let (mut y, mut m) = (year as f64, month as f64);
    if month <= 2 {
        y -= 1.0;
        m += 12.0;
    }
    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day as f64 + b - 1524.5
}

/// The sun terms that depend on the instant only.
struct Terms {
    declination: f64,
    eq_time: f64,
}

fn terms(jc: f64) -> Terms {
    let rad = f64::to_radians;
    let l0 = (280.46646 + jc * (36000.76983 + jc * 0.0003032)).rem_euclid(360.0);
    let m = 357.52911 + jc * (35999.05029 - 0.0001537 * jc);
    let e = 0.016708634 - jc * (0.000042037 + 0.0000001267 * jc);
    let c = rad(m).sin() * (1.914602 - jc * (0.004817 + 0.000014 * jc))
        + rad(2.0 * m).sin() * (0.019993 - 0.000101 * jc)
        + rad(3.0 * m).sin() * 0.000289;
    let true_long = l0 + c;
    let omega = 125.04 - 1934.136 * jc;
    let app_long = true_long - 0.00569 - 0.00478 * rad(omega).sin();
    let mean_obliq =
        23.0 + (26.0 + (21.448 - jc * (46.815 + jc * (0.00059 - jc * 0.001813))) / 60.0) / 60.0;
    let obliq = mean_obliq + 0.00256 * rad(omega).cos();
    let declination = (rad(obliq).sin() * rad(app_long).sin()).asin().to_degrees();
    let y = (rad(obliq) / 2.0).tan().powi(2);
    let eq_time = 4.0
        * (y * rad(2.0 * l0).sin() - 2.0 * e * rad(m).sin()
            + 4.0 * e * y * rad(m).sin() * rad(2.0 * l0).cos()
            - 0.5 * y * y * rad(4.0 * l0).sin()
            - 1.25 * e * e * rad(2.0 * m).sin())
        .to_degrees();
    Terms {
        declination,
        eq_time,
    }
}

/// Bending of light in the air, in degrees, for a true elevation (NOAA formula).
fn refraction(elevation: f64) -> f64 {
    if elevation > 85.0 {
        return 0.0;
    }
    let te = elevation.to_radians().tan();
    let seconds = if elevation > 5.0 {
        58.1 / te - 0.07 / te.powi(3) + 0.000086 / te.powi(5)
    } else if elevation > -0.575 {
        1735.0
            + elevation * (-518.2 + elevation * (103.4 + elevation * (-12.79 + elevation * 0.711)))
    } else {
        -20.772 / te
    };
    seconds / 3600.0
}

/// Position of the sun. Inputs are not checked here; use [`check`] first for user input.
pub fn position(t: &SunTime) -> SunPosition {
    let jd = julian_day(t.year, t.month, t.day) + (t.hour - t.tz_offset) / 24.0;
    let jc = (jd - 2_451_545.0) / 36525.0;
    let Terms {
        declination,
        eq_time,
    } = terms(jc);
    let tst = (t.hour * 60.0 + eq_time + 4.0 * t.longitude - 60.0 * t.tz_offset).rem_euclid(1440.0);
    let ha = tst / 4.0 - 180.0;
    let (lat, dec, ha_r) = (
        t.latitude.to_radians(),
        declination.to_radians(),
        ha.to_radians(),
    );
    let cos_zen = (lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha_r.cos()).clamp(-1.0, 1.0);
    let zen = cos_zen.acos();
    let true_elev = 90.0 - zen.to_degrees();
    let denom = lat.cos() * zen.sin();
    let azimuth = if denom.abs() < 1e-9 {
        // The sun is at the zenith, or the observer is at a pole: the azimuth has no meaning.
        if t.latitude >= 0.0 { 180.0 } else { 0.0 }
    } else {
        let a = ((lat.sin() * cos_zen - dec.sin()) / denom)
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();
        if ha > 0.0 {
            (a + 180.0).rem_euclid(360.0)
        } else {
            (540.0 - a).rem_euclid(360.0)
        }
    };
    SunPosition {
        azimuth,
        elevation: true_elev + refraction(true_elev),
        declination,
        equation_of_time: eq_time,
    }
}

/// Sunrise, solar noon and sunset for the date of `t` (its `hour` is ignored).
pub fn day(t: &SunTime) -> SunDay {
    // The sun of the local noon is close enough for the whole day.
    let jd = julian_day(t.year, t.month, t.day) + (12.0 - t.tz_offset) / 24.0;
    let Terms {
        declination,
        eq_time,
    } = terms((jd - 2_451_545.0) / 36525.0);
    let noon = (720.0 - 4.0 * t.longitude - eq_time + t.tz_offset * 60.0) / 60.0;
    let (lat, dec) = (t.latitude.to_radians(), declination.to_radians());
    let cos_ha = 90.833f64.to_radians().cos() / (lat.cos() * dec.cos()) - lat.tan() * dec.tan();
    if !cos_ha.is_finite() || cos_ha.abs() > 1.0 {
        return SunDay {
            sunrise: None,
            solar_noon: noon,
            sunset: None,
            always_up: cos_ha.is_finite() && cos_ha < -1.0,
        };
    }
    let ha_hours = cos_ha.acos().to_degrees() / 15.0;
    SunDay {
        sunrise: Some(noon - ha_hours),
        solar_noon: noon,
        sunset: Some(noon + ha_hours),
        always_up: false,
    }
}

/// Unit vector from the ground toward the sun in model axes. X is east, Y is north and Z is up,
/// where true north lies `north_angle` degrees clockwise from model +Y.
pub fn direction(p: &SunPosition, north_angle: f64) -> [f64; 3] {
    let az = (p.azimuth + north_angle).to_radians();
    let el = p.elevation.to_radians();
    [az.sin() * el.cos(), az.cos() * el.cos(), el.sin()]
}

/// Formats decimal hours as `HH:MM`.
pub fn hhmm(hours: f64) -> String {
    let total = (hours.rem_euclid(24.0) * 60.0).round() as u32 % 1440;
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(lat: f64, lon: f64, ymd: (i32, u32, u32), hour: f64, tz: f64) -> SunTime {
        SunTime {
            latitude: lat,
            longitude: lon,
            year: ymd.0,
            month: ymd.1,
            day: ymd.2,
            hour,
            tz_offset: tz,
        }
    }

    #[test]
    fn julian_day_of_known_dates() {
        // Meeus: 2000 Jan 1.5 is JD 2451545.0, and 1957 Oct 4.81 is JD 2436116.31.
        assert_eq!(julian_day(2000, 1, 1), 2_451_544.5);
        assert!((julian_day(1957, 10, 4) + 0.81 - 2_436_116.31).abs() < 1e-9);
        assert_eq!(julian_day(1992, 10, 13), 2_448_908.5);
        assert_eq!(julian_day(2024, 3, 1), julian_day(2024, 2, 29) + 1.0);
    }

    #[test]
    fn meeus_example_25a_declination_and_equation_of_time() {
        // 1992 October 13, 0h: declination -7.78507 degrees (Meeus, example 25.a), and the
        // equation of time is +13 min 42.7 s (example 28.a).
        let p = position(&t(0.0, 0.0, (1992, 10, 13), 0.0, 0.0));
        assert!((p.declination + 7.78507).abs() < 0.01, "{}", p.declination);
        assert!(
            (p.equation_of_time - 13.7117).abs() < 0.05,
            "{}",
            p.equation_of_time
        );
    }

    #[test]
    fn equinox_noon_at_the_equator_is_overhead() {
        // The March equinox 2024 was at 03:06 UTC. At the equator on the Greenwich meridian the
        // sun is nearly at the zenith around 12:07 UTC (the equation of time is -7 minutes).
        let p = position(&t(0.0, 0.0, (2024, 3, 20), 12.1, 0.0));
        assert!(p.elevation > 88.5, "{}", p.elevation);
        assert!(p.declination.abs() < 0.3);
    }

    #[test]
    fn solstice_noon_elevation_is_90_minus_latitude_plus_tilt() {
        // Helsinki, June solstice: the highest sun is 90 - 60.17 + 23.44 = 53.27 degrees.
        let loc = t(60.17, 24.94, (2024, 6, 21), 12.0, 3.0);
        let d = day(&loc);
        let p = position(&SunTime {
            hour: d.solar_noon,
            ..loc
        });
        assert!((p.elevation - 53.27).abs() < 0.1, "{}", p.elevation);
        // At solar noon in the northern hemisphere the sun is due south.
        assert!((p.azimuth - 180.0).abs() < 1.0, "{}", p.azimuth);
        // Solar noon in Helsinki in summer time is about 13:22.
        assert!((d.solar_noon - 13.37).abs() < 0.05, "{}", d.solar_noon);
        // The day is about 18 hours 54 minutes long.
        let len = d.sunset.unwrap() - d.sunrise.unwrap();
        assert!((len - 18.9).abs() < 0.15, "{len}");
        // December solstice: the highest sun is 90 - 60.17 - 23.44 = 6.39 degrees.
        let w = t(60.17, 24.94, (2024, 12, 21), 12.0, 2.0);
        let wn = position(&SunTime {
            hour: day(&w).solar_noon,
            ..w
        });
        assert!((wn.elevation - 6.4).abs() < 0.3, "{}", wn.elevation);
    }

    #[test]
    fn morning_is_east_and_afternoon_is_west() {
        let am = position(&t(52.0, 5.0, (2024, 9, 22), 8.0, 2.0));
        let pm = position(&t(52.0, 5.0, (2024, 9, 22), 17.0, 2.0));
        assert!((45.0..135.0).contains(&am.azimuth), "{}", am.azimuth);
        assert!((225.0..315.0).contains(&pm.azimuth), "{}", pm.azimuth);
        assert!(am.elevation > 0.0 && pm.elevation > 0.0);
        // Equinox: the day is about 12 hours long.
        let d = day(&t(0.0, 0.0, (2024, 9, 22), 0.0, 0.0));
        assert!((d.sunset.unwrap() - d.sunrise.unwrap() - 12.1).abs() < 0.1);
    }

    #[test]
    fn southern_hemisphere_noon_sun_is_north() {
        let loc = t(-33.87, 151.21, (2024, 1, 15), 12.0, 11.0);
        let p = position(&SunTime {
            hour: day(&loc).solar_noon,
            ..loc
        });
        assert!(p.azimuth < 5.0 || p.azimuth > 355.0, "{}", p.azimuth);
    }

    #[test]
    fn polar_day_and_night() {
        let night = day(&t(80.0, 0.0, (2024, 12, 21), 12.0, 0.0));
        assert!(night.sunrise.is_none() && !night.always_up);
        let up = day(&t(80.0, 0.0, (2024, 6, 21), 12.0, 0.0));
        assert!(up.sunset.is_none() && up.always_up);
        let low = position(&t(80.0, 0.0, (2024, 12, 21), 12.0, 0.0));
        assert!(low.elevation < 0.0);
        // A pole has no azimuth: the function still gives a number.
        let pole = position(&t(90.0, 0.0, (2024, 6, 21), 12.0, 0.0));
        assert!(pole.azimuth.is_finite() && pole.elevation > 23.0);
        let south = position(&t(-90.0, 0.0, (2024, 12, 21), 12.0, 0.0));
        assert!(south.azimuth.is_finite());
    }

    #[test]
    fn refraction_pieces_are_sane() {
        assert_eq!(refraction(89.0), 0.0);
        for e in [60.0, 10.0, 3.0, 0.0, -0.3, -2.0, -10.0] {
            assert!(refraction(e).is_finite(), "{e}");
        }
        // About half a degree at the horizon.
        assert!((refraction(0.0) - 0.48).abs() < 0.08);
    }

    #[test]
    fn direction_vector_matches_the_angles() {
        let up = SunPosition {
            azimuth: 0.0,
            elevation: 90.0,
            declination: 0.0,
            equation_of_time: 0.0,
        };
        let d = direction(&up, 0.0);
        assert!(d[2] > 0.999999);
        let east = SunPosition {
            azimuth: 90.0,
            elevation: 0.0,
            ..up
        };
        let d = direction(&east, 0.0);
        assert!((d[0] - 1.0).abs() < 1e-9 && d[1].abs() < 1e-9);
        // True north is 90 degrees clockwise from model +Y, which is model +X.
        let north = SunPosition {
            azimuth: 0.0,
            elevation: 0.0,
            ..up
        };
        let d = direction(&north, 90.0);
        assert!((d[0] - 1.0).abs() < 1e-9, "{d:?}");
        let any = direction(&position(&t(40.0, -3.0, (2024, 5, 5), 15.0, 2.0)), 33.0);
        let len = (any[0] * any[0] + any[1] * any[1] + any[2] * any[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-9);
    }

    #[test]
    fn input_checks() {
        let ok = t(60.0, 25.0, (2024, 2, 29), 12.0, 2.0);
        assert!(check(&ok).is_ok());
        assert!(check(&SunTime { day: 30, ..ok }).is_err());
        assert!(check(&SunTime { month: 13, ..ok }).is_err());
        assert!(check(&SunTime { month: 0, ..ok }).is_err());
        assert!(
            check(&SunTime {
                year: 2023,
                month: 2,
                day: 29,
                ..ok
            })
            .is_err()
        );
        assert!(check(&SunTime { year: 1700, ..ok }).is_err());
        assert!(
            check(&SunTime {
                latitude: 91.0,
                ..ok
            })
            .is_err()
        );
        assert!(
            check(&SunTime {
                latitude: f64::NAN,
                ..ok
            })
            .is_err()
        );
        assert!(
            check(&SunTime {
                longitude: 181.0,
                ..ok
            })
            .is_err()
        );
        assert!(check(&SunTime { hour: 24.5, ..ok }).is_err());
        assert!(
            check(&SunTime {
                tz_offset: 15.0,
                ..ok
            })
            .is_err()
        );
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(hhmm(13.5), "13:30");
        assert_eq!(hhmm(23.999), "00:00");
    }
}
