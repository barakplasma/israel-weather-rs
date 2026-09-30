//! Expand IMS 6-hour forecast blocks into hourly rows.
//!
//! IMS publishes one row per ~6 hour block. Many weather clients expect hourly data, so:
//! - instantaneous values (temperature, humidity, wind, ...) are linearly interpolated
//!   between consecutive blocks (wind direction along the shortest arc),
//! - the block's rain total is spread evenly over the hours of that block,
//! - the weather code, block min/max temperature and max UV are carried over unchanged.
//!
//! Each block is assumed to start at its `ForecastTime` and last until the next block starts.

use chrono::{DateTime, Duration, FixedOffset, Utc};
use chrono_tz::Asia::Jerusalem;

use crate::ims_structs::Forecast;

/// Duration assumed for the last block, which has no successor to measure against.
const DEFAULT_BLOCK_HOURS: i64 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct HourlyPoint {
    /// Start of the hour, in Israel local time.
    pub time: DateTime<FixedOffset>,
    /// °C
    pub temperature: f32,
    /// %
    pub relative_humidity: f32,
    /// °C
    pub dew_point: f32,
    /// °C
    pub feels_like: f32,
    /// km/h
    pub wind_speed: f32,
    /// Degrees the wind is coming from.
    pub wind_direction: f32,
    pub uv_index: Option<f32>,
    /// mm of rain expected during this hour.
    pub precipitation: f32,
    /// IMS weather code of the enclosing block.
    pub weather_code: i32,
    /// Min/max temperature (°C) and max UV of the enclosing block.
    pub block_min_temp: f32,
    pub block_max_temp: f32,
    pub block_uv_index_max: Option<f32>,
}

impl HourlyPoint {
    pub fn time_utc(&self) -> DateTime<Utc> {
        self.time.with_timezone(&Utc)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_opt(a: Option<f32>, b: Option<f32>, t: f32) -> Option<f32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(lerp(a, b, t)),
        (a, _) => a,
    }
}

/// Interpolate compass degrees along the shortest arc, result in `[0, 360)`.
fn lerp_degrees(a: f32, b: f32, t: f32) -> f32 {
    let delta = ((b - a) % 360.0 + 540.0) % 360.0 - 180.0;
    (a + delta * t).rem_euclid(360.0)
}

fn parse(f: &Forecast) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&f.forecast_time)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Expand forecasts (with RFC 3339 `forecast_time`, as produced by
/// [`crate::parse_forecast`]) into hourly points, sorted by time.
pub fn expand_hourly(forecasts: &[Forecast]) -> Vec<HourlyPoint> {
    let mut blocks: Vec<(DateTime<Utc>, &Forecast)> = forecasts
        .iter()
        .filter_map(|f| Some((parse(f)?, f)))
        .collect();
    blocks.sort_by_key(|(t, _)| *t);

    let mut out = Vec::with_capacity(blocks.len() * DEFAULT_BLOCK_HOURS as usize);
    for (i, (start, cur)) in blocks.iter().enumerate() {
        let next = blocks.get(i + 1);
        let hours = next
            .map(|(t, _)| (*t - *start).num_hours())
            .unwrap_or(DEFAULT_BLOCK_HOURS)
            .max(1);
        let rain_per_hour = cur.rain / hours as f32;

        for h in 0..hours {
            // Hold the last block's values rather than extrapolating.
            let (t, nxt) = match next {
                Some((_, n)) => (h as f32 / hours as f32, *n),
                None => (0.0, *cur),
            };
            let time = (*start + Duration::hours(h))
                .with_timezone(&Jerusalem)
                .fixed_offset();
            out.push(HourlyPoint {
                time,
                temperature: lerp(cur.temperature, nxt.temperature, t),
                relative_humidity: lerp(cur.relative_humidity, nxt.relative_humidity, t),
                dew_point: lerp(cur.dew_point_temp, nxt.dew_point_temp, t),
                feels_like: lerp(cur.feels_like, nxt.feels_like, t),
                wind_speed: lerp(cur.wind_speed, nxt.wind_speed, t),
                wind_direction: lerp_degrees(cur.wind_direction, nxt.wind_direction, t),
                uv_index: lerp_opt(cur.uv_index, nxt.uv_index, t),
                precipitation: rain_per_hour,
                weather_code: cur.weather_code,
                block_min_temp: cur.min_temp,
                block_max_temp: cur.max_temp,
                block_uv_index_max: cur.uv_index_max,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forecast(time: &str, temp: f32, rain: f32, wind_dir: f32) -> Forecast {
        Forecast {
            forecast_time: time.to_string(),
            temperature: temp,
            relative_humidity: 50.0,
            wind_speed: 10.0,
            rain,
            wind_direction: wind_dir,
            dew_point_temp: 10.0,
            heat_stress: 0.0,
            heat_stress_level: 0.0,
            feels_like: temp,
            wind_chill: temp,
            weather_code: 1250,
            weather_code_english: None,
            min_temp: temp - 1.0,
            max_temp: temp + 1.0,
            uv_index: Some(0.0),
            uv_index_max: Some(3.0),
        }
    }

    #[test]
    fn expands_each_block_into_hours() {
        let f = vec![
            forecast("2025-03-09T02:00:00+02:00", 10.0, 6.0, 0.0),
            forecast("2025-03-09T08:00:00+02:00", 16.0, 0.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        assert_eq!(hourly.len(), 12);
        assert_eq!(hourly[0].time.to_rfc3339(), "2025-03-09T02:00:00+02:00");
        assert_eq!(hourly[11].time.to_rfc3339(), "2025-03-09T13:00:00+02:00");
    }

    #[test]
    fn interpolates_temperature_and_holds_last_block() {
        let f = vec![
            forecast("2025-03-09T02:00:00+02:00", 10.0, 0.0, 0.0),
            forecast("2025-03-09T08:00:00+02:00", 16.0, 0.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        assert_eq!(hourly[3].temperature, 13.0);
        assert!(hourly[6..].iter().all(|p| p.temperature == 16.0));
    }

    #[test]
    fn spreads_rain_evenly_and_preserves_total() {
        let f = vec![
            forecast("2025-03-09T02:00:00+02:00", 10.0, 6.0, 0.0),
            forecast("2025-03-09T08:00:00+02:00", 10.0, 3.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        assert!(hourly[..6].iter().all(|p| p.precipitation == 1.0));
        assert!(hourly[6..].iter().all(|p| p.precipitation == 0.5));
        let total: f32 = hourly.iter().map(|p| p.precipitation).sum();
        assert_eq!(total, 9.0);
    }

    #[test]
    fn wind_direction_takes_shortest_arc() {
        assert_eq!(lerp_degrees(350.0, 10.0, 0.5), 0.0);
        assert_eq!(lerp_degrees(10.0, 350.0, 0.5), 0.0);
        assert_eq!(lerp_degrees(90.0, 180.0, 0.5), 135.0);
        assert_eq!(lerp_degrees(360.0, 360.0, 0.3), 0.0);
    }

    #[test]
    fn uneven_block_lengths_and_unsorted_input() {
        let f = vec![
            forecast("2025-03-09T05:00:00+02:00", 13.0, 0.0, 0.0),
            forecast("2025-03-09T02:00:00+02:00", 10.0, 3.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        assert_eq!(hourly.len(), 3 + 6);
        assert_eq!(hourly[0].precipitation, 1.0);
        assert_eq!(hourly[1].temperature, 11.0);
    }

    #[test]
    fn crossing_dst_start_keeps_hours_contiguous() {
        // Israel DST 2025 starts 2025-03-28 02:00 local -> 03:00.
        let f = vec![
            forecast("2025-03-27T20:00:00+02:00", 10.0, 0.0, 0.0),
            forecast("2025-03-28T03:00:00+03:00", 10.0, 0.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        // 18:00Z .. 00:00Z is 6 real hours.
        assert_eq!(hourly.len(), 6 + 6);
        assert_eq!(hourly[5].time.to_rfc3339(), "2025-03-28T01:00:00+02:00");
        assert_eq!(hourly[6].time.to_rfc3339(), "2025-03-28T03:00:00+03:00");
    }

    #[test]
    fn empty_input() {
        assert!(expand_hourly(&[]).is_empty());
    }
}
