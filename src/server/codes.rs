//! Translate IMS weather codes into the vocabularies other weather APIs use.

use chrono::{DateTime, Utc};

pub use weather_api::ims::wmo_code;

/// IMS weather code -> MET Norway `symbol_code` (see api.met.no weathericons legend).
pub fn metno_symbol(ims: i32, is_day: bool) -> String {
    let suffix = if is_day { "_day" } else { "_night" };
    let (base, has_variant) = match ims {
        1250 | 1300 | 1310 | 1580 => ("clearsky", true),
        1260 | 1270 | 1320 | 1590 => ("fair", true),
        1220 => ("partlycloudy", true),
        1230 | 1010 | 1570 => ("cloudy", false),
        1160 => ("fog", false),
        1560 => ("lightrain", false),
        1140 => ("rain", false),
        1510 => ("heavyrain", false),
        1080 => ("sleet", false),
        1070 => ("lightsnow", false),
        1060 => ("snow", false),
        1520 => ("heavysnow", false),
        1530 | 1540 => ("lightrainshowers", true),
        1020 => ("rainandthunder", false),
        _ => ("cloudy", false),
    };
    if has_variant {
        format!("{base}{suffix}")
    } else {
        base.to_string()
    }
}

/// Whether the sun is above the horizon (accounting for refraction) at `t` for the given point.
/// Low-precision solar position, accurate to a few minutes around sunrise/sunset.
pub fn is_day(lat: f64, lon: f64, t: DateTime<Utc>) -> bool {
    // Days since J2000.0
    let d = t.timestamp() as f64 / 86_400.0 + 2_440_587.5 - 2_451_545.0;
    let g = (357.529 + 0.985_600_28 * d).to_radians();
    let q = 280.459 + 0.985_647_36 * d;
    let l = (q + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians();
    let e = (23.439 - 0.000_000_36 * d).to_radians();
    let declination = (e.sin() * l.sin()).asin();
    let right_ascension = (e.cos() * l.sin()).atan2(l.cos());
    let gmst_hours = (18.697_374_558 + 24.065_709_824_419_08 * d).rem_euclid(24.0);
    let hour_angle = (gmst_hours * 15.0 + lon).to_radians() - right_ascension;
    let lat = lat.to_radians();
    let elevation = (lat.sin() * declination.sin()
        + lat.cos() * declination.cos() * hour_angle.cos())
    .asin()
    .to_degrees();
    elevation > -0.833
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().into()
    }

    #[test]
    fn wmo_mapping() {
        assert_eq!(wmo_code(1250), 0);
        assert_eq!(wmo_code(1140), 63);
        assert_eq!(wmo_code(1020), 95);
        assert_eq!(wmo_code(9999), 3);
    }

    #[test]
    fn metno_mapping_uses_day_night_variants() {
        assert_eq!(metno_symbol(1250, true), "clearsky_day");
        assert_eq!(metno_symbol(1250, false), "clearsky_night");
        assert_eq!(metno_symbol(1230, false), "cloudy");
        assert_eq!(metno_symbol(1530, true), "lightrainshowers_day");
    }

    #[test]
    fn day_and_night_in_tel_aviv() {
        let (lat, lon) = (32.08, 34.78);
        assert!(is_day(lat, lon, utc("2025-06-21T09:00:00Z")));
        assert!(!is_day(lat, lon, utc("2025-06-21T23:00:00Z")));
        // Winter sunrise ~06:40 local (04:40Z), sunset ~16:40 local (14:40Z).
        assert!(!is_day(lat, lon, utc("2025-01-15T04:15:00Z")));
        assert!(is_day(lat, lon, utc("2025-01-15T05:15:00Z")));
        assert!(is_day(lat, lon, utc("2025-01-15T14:30:00Z")));
        assert!(!is_day(lat, lon, utc("2025-01-15T15:15:00Z")));
    }
}
