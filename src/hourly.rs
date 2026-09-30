//! Build hourly forecast rows.
//!
//! Preferred source: the hourly IMS JSON forecast ([`crate::ims_json`]), used as-is.
//!
//! Fallback: the XML, which is a *6-hourly sample of that same hourly forecast* (every XML
//! value, rain included, equals the hourly value at its `ForecastTime`). Between samples:
//! - instantaneous values and the rain rate (mm/h) are linearly interpolated
//!   (wind direction along the shortest arc),
//! - the weather code is taken from the nearest sample.
//!
//! Rows at sample times, and rows from the JSON, are marked [`Source::Ims`]; the rest
//! [`Source::Interpolated`].

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, FixedOffset, Utc};
use chrono_tz::Asia::Jerusalem;
use serde::Serialize;

use crate::ims_json::HourlyRow;
use crate::ims_structs::Forecast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Value published by IMS for this hour.
    Ims,
    /// Interpolated between 6-hourly XML samples.
    Interpolated,
}

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
    /// km/h, only from the JSON source.
    pub wind_gust: Option<f32>,
    /// Degrees the wind is coming from.
    pub wind_direction: f32,
    pub uv_index: Option<f32>,
    pub uv_index_max: Option<f32>,
    /// mm of rain expected during this hour.
    pub precipitation: f32,
    /// %, only from the JSON source.
    pub precipitation_probability: Option<f32>,
    /// IMS weather code.
    pub weather_code: i32,
    pub source: Source,
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

/// Dew point (°C) from temperature (°C) and relative humidity (%), Magnus formula.
pub fn dew_point(temperature: f32, relative_humidity: f32) -> f32 {
    const B: f32 = 17.62;
    const C: f32 = 243.12;
    let rh = relative_humidity.clamp(1.0, 100.0) / 100.0;
    let gamma = rh.ln() + B * temperature / (C + temperature);
    C * gamma / (B - gamma)
}

fn parse(f: &Forecast) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&f.forecast_time)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn local(t: DateTime<Utc>) -> DateTime<FixedOffset> {
    t.with_timezone(&Jerusalem).fixed_offset()
}

/// Expand 6-hourly XML samples (with RFC 3339 `forecast_time`, as produced by
/// [`crate::parse_forecast`]) into hourly points, sorted by time.
pub fn expand_hourly(forecasts: &[Forecast]) -> Vec<HourlyPoint> {
    let mut samples: Vec<(DateTime<Utc>, &Forecast)> = forecasts
        .iter()
        .filter_map(|f| Some((parse(f)?, f)))
        .collect();
    samples.sort_by_key(|(t, _)| *t);
    samples.dedup_by_key(|(t, _)| *t);

    let mut out = Vec::with_capacity(samples.len() * 6);
    for (i, (start, cur)) in samples.iter().enumerate() {
        // The last sample is a single instant; don't extrapolate past it.
        let (hours, next) = match samples.get(i + 1) {
            Some((t, n)) => ((*t - *start).num_hours().max(1), *n),
            None => (1, *cur),
        };
        for h in 0..hours {
            let t = h as f32 / hours as f32;
            let nearest = if t < 0.5 { cur } else { next };
            out.push(HourlyPoint {
                time: local(*start + Duration::hours(h)),
                temperature: lerp(cur.temperature, next.temperature, t),
                relative_humidity: lerp(cur.relative_humidity, next.relative_humidity, t),
                dew_point: lerp(cur.dew_point_temp, next.dew_point_temp, t),
                feels_like: lerp(cur.feels_like, next.feels_like, t),
                wind_speed: lerp(cur.wind_speed, next.wind_speed, t),
                wind_gust: None,
                wind_direction: lerp_degrees(cur.wind_direction, next.wind_direction, t),
                uv_index: lerp_opt(cur.uv_index, next.uv_index, t),
                uv_index_max: nearest.uv_index_max,
                precipitation: lerp(cur.rain, next.rain, t),
                precipitation_probability: None,
                weather_code: nearest.weather_code,
                source: if h == 0 {
                    Source::Ims
                } else {
                    Source::Interpolated
                },
            });
        }
    }
    out
}

/// Overlay real hourly IMS rows onto (typically XML-derived) points. JSON rows win where both
/// exist; values the JSON lacks (dew point, feels-like) come from the XML point for that hour,
/// or are derived when there is none. Result is sorted by time.
pub fn merge_hourly(base: Vec<HourlyPoint>, rows: &[HourlyRow]) -> Vec<HourlyPoint> {
    let mut by_time: BTreeMap<i64, HourlyPoint> = base
        .into_iter()
        .map(|p| (p.time_utc().timestamp(), p))
        .collect();
    for r in rows {
        let key = r.time.timestamp();
        let old = by_time.get(&key);
        let relative_humidity = r
            .relative_humidity
            .or(old.map(|o| o.relative_humidity))
            .unwrap_or(0.0);
        let point = HourlyPoint {
            time: local(r.time.with_timezone(&Utc)),
            temperature: r.temperature,
            relative_humidity,
            dew_point: old.map_or_else(
                || dew_point(r.temperature, relative_humidity),
                |o| o.dew_point,
            ),
            feels_like: old.map_or(r.temperature, |o| o.feels_like),
            wind_speed: r.wind_speed.or(old.map(|o| o.wind_speed)).unwrap_or(0.0),
            wind_gust: r.wind_gust,
            wind_direction: r
                .wind_direction
                .or(old.map(|o| o.wind_direction))
                .unwrap_or(0.0),
            uv_index: r.uv_index.or(old.and_then(|o| o.uv_index)),
            uv_index_max: r.uv_index_max.or(old.and_then(|o| o.uv_index_max)),
            precipitation: r.rain,
            precipitation_probability: r.rain_chance,
            weather_code: r.weather_code,
            source: Source::Ims,
        };
        by_time.insert(key, point);
    }
    by_time.into_values().collect()
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
            min_temp: temp,
            max_temp: temp,
            uv_index: Some(0.0),
            uv_index_max: Some(3.0),
        }
    }

    fn row(time: &str, temp: f32, rain: f32) -> HourlyRow {
        HourlyRow {
            time: DateTime::parse_from_rfc3339(time).unwrap(),
            temperature: temp,
            relative_humidity: Some(60.0),
            wind_speed: Some(12.0),
            wind_direction: Some(270.0),
            wind_gust: Some(20.0),
            rain,
            rain_chance: Some(40.0),
            weather_code: 1140,
            uv_index: Some(1.0),
            uv_index_max: Some(2.0),
            heat_stress: None,
            wave_height: None,
            pm10: None,
        }
    }

    #[test]
    fn expands_samples_into_hours_without_extrapolating() {
        let f = vec![
            forecast("2025-03-09T02:00:00+02:00", 10.0, 0.0, 0.0),
            forecast("2025-03-09T08:00:00+02:00", 16.0, 0.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        // 02..07 interpolated towards 08, then the final 08:00 sample alone.
        assert_eq!(hourly.len(), 7);
        assert_eq!(hourly[0].time.to_rfc3339(), "2025-03-09T02:00:00+02:00");
        assert_eq!(hourly[6].time.to_rfc3339(), "2025-03-09T08:00:00+02:00");
        assert_eq!(hourly[3].temperature, 13.0);
        assert_eq!(hourly[6].temperature, 16.0);
    }

    #[test]
    fn samples_are_ims_and_between_is_interpolated() {
        let f = vec![
            forecast("2025-03-09T02:00:00+02:00", 10.0, 0.0, 0.0),
            forecast("2025-03-09T08:00:00+02:00", 16.0, 0.0, 0.0),
        ];
        let sources: Vec<Source> = expand_hourly(&f).iter().map(|p| p.source).collect();
        assert_eq!(sources[0], Source::Ims);
        assert!(sources[1..6].iter().all(|s| *s == Source::Interpolated));
        assert_eq!(sources[6], Source::Ims);
    }

    #[test]
    fn rain_is_an_hourly_rate_not_a_block_total() {
        // XML rain is the value for the sample hour itself.
        let f = vec![
            forecast("2025-03-09T02:00:00+02:00", 10.0, 3.0, 0.0),
            forecast("2025-03-09T08:00:00+02:00", 10.0, 0.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        assert_eq!(hourly[0].precipitation, 3.0);
        assert_eq!(hourly[3].precipitation, 1.5);
        assert_eq!(hourly[6].precipitation, 0.0);
    }

    #[test]
    fn weather_code_from_nearest_sample() {
        let mut later = forecast("2025-03-09T08:00:00+02:00", 10.0, 0.0, 0.0);
        later.weather_code = 1140;
        let f = vec![forecast("2025-03-09T02:00:00+02:00", 10.0, 0.0, 0.0), later];
        let codes: Vec<i32> = expand_hourly(&f).iter().map(|p| p.weather_code).collect();
        assert_eq!(codes, vec![1250, 1250, 1250, 1140, 1140, 1140, 1140]);
    }

    #[test]
    fn wind_direction_takes_shortest_arc() {
        assert_eq!(lerp_degrees(350.0, 10.0, 0.5), 0.0);
        assert_eq!(lerp_degrees(10.0, 350.0, 0.5), 0.0);
        assert_eq!(lerp_degrees(90.0, 180.0, 0.5), 135.0);
        assert_eq!(lerp_degrees(360.0, 360.0, 0.3), 0.0);
    }

    #[test]
    fn uneven_gaps_and_unsorted_input() {
        let f = vec![
            forecast("2025-03-09T05:00:00+02:00", 13.0, 0.0, 0.0),
            forecast("2025-03-09T02:00:00+02:00", 10.0, 3.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        assert_eq!(hourly.len(), 3 + 1);
        assert_eq!(hourly[1].temperature, 11.0);
        assert_eq!(hourly[1].precipitation, 2.0);
    }

    #[test]
    fn crossing_dst_start_keeps_hours_contiguous() {
        // Israel DST 2025 starts 2025-03-28 02:00 local -> 03:00.
        let f = vec![
            forecast("2025-03-27T20:00:00+02:00", 10.0, 0.0, 0.0),
            forecast("2025-03-28T03:00:00+03:00", 10.0, 0.0, 0.0),
        ];
        let hourly = expand_hourly(&f);
        // 18:00Z .. 00:00Z is 6 real hours, plus the final sample.
        assert_eq!(hourly.len(), 6 + 1);
        assert_eq!(hourly[5].time.to_rfc3339(), "2025-03-28T01:00:00+02:00");
        assert_eq!(hourly[6].time.to_rfc3339(), "2025-03-28T03:00:00+03:00");
    }

    #[test]
    fn empty_input() {
        assert!(expand_hourly(&[]).is_empty());
        assert!(merge_hourly(vec![], &[]).is_empty());
    }

    #[test]
    fn merge_prefers_ims_rows_and_keeps_xml_only_hours() {
        let base = expand_hourly(&[
            forecast("2025-03-09T02:00:00+02:00", 10.0, 0.0, 0.0),
            forecast("2025-03-09T08:00:00+02:00", 16.0, 0.0, 0.0),
        ]);
        let rows = vec![
            row("2025-03-09T04:00:00+02:00", 11.5, 0.4),
            // An hour the XML doesn't reach.
            row("2025-03-09T09:00:00+02:00", 17.0, 0.0),
        ];
        let merged = merge_hourly(base, &rows);
        assert_eq!(merged.len(), 8);
        let four = &merged[2];
        assert_eq!(four.time.to_rfc3339(), "2025-03-09T04:00:00+02:00");
        assert_eq!(four.temperature, 11.5);
        assert_eq!(four.precipitation, 0.4);
        assert_eq!(four.precipitation_probability, Some(40.0));
        assert_eq!(four.wind_gust, Some(20.0));
        assert_eq!(four.source, Source::Ims);
        // Dew point comes from the XML-derived point.
        assert_eq!(four.dew_point, 10.0);
        // Untouched hours keep interpolated values.
        assert_eq!(merged[1].source, Source::Interpolated);
        // JSON-only hour gets a derived dew point.
        let nine = merged.last().unwrap();
        assert_eq!(nine.time.to_rfc3339(), "2025-03-09T09:00:00+02:00");
        assert!((nine.dew_point - dew_point(17.0, 60.0)).abs() < 1e-6);
        assert!(merged.windows(2).all(|w| w[0].time < w[1].time));
    }

    #[test]
    fn dew_point_magnus() {
        // 25 °C at 60% RH is about 16.7 °C.
        assert!((dew_point(25.0, 60.0) - 16.7).abs() < 0.2);
        assert!((dew_point(20.0, 100.0) - 20.0).abs() < 0.01);
    }
}
