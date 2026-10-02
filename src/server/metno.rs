//! MET Norway Locationforecast 2.0 compatible endpoint
//! (<https://api.met.no/weatherapi/locationforecast/2.0/documentation>).
//!
//! Serves hourly `timeseries` from the current hour onward. Each step has `instant` values and
//! `next_1_hours` / `next_6_hours` / `next_12_hours` summaries whenever the data covers the
//! whole period. Fields IMS does not provide (air pressure, cloud cover, ...) are omitted.
//! `/complete` returns the same document as `/compact`.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::IntoResponse;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::codes::{is_day, metno_symbol, wmo_code};
use super::{ApiError, AppState, LocationQuery, resolve, round};
use crate::hourly::HourlyPoint;

#[derive(Deserialize)]
pub struct Params {
    lat: Option<f64>,
    lon: Option<f64>,
    /// Accepted for compatibility; the IMS location's elevation is always used.
    #[allow(dead_code)]
    altitude: Option<f64>,
}

fn kmh_to_ms(kmh: f32) -> f64 {
    round(kmh as f64 / 3.6, 1)
}

fn http_date(t: DateTime<Utc>) -> Option<HeaderValue> {
    HeaderValue::from_str(&t.format("%a, %d %b %Y %H:%M:%S GMT").to_string()).ok()
}

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Summary for the `hours` hours starting at `window[0]`, or `None` if data doesn't cover it.
fn period(
    window: &[HourlyPoint],
    hours: usize,
    lat: f64,
    lon: f64,
    details: bool,
) -> Option<Value> {
    let covered = window.get(..hours)?;
    // Only summarise a gap-free run of hours.
    let span = covered[hours - 1].time - covered[0].time;
    if span.num_hours() != hours as i64 - 1 {
        return None;
    }
    // Most significant weather in the period, using the WMO code as a severity ranking.
    let code = covered
        .iter()
        .max_by_key(|p| wmo_code(p.weather_code))?
        .weather_code;
    let symbol = metno_symbol(code, is_day(lat, lon, covered[0].time_utc()));
    let mut d = Map::new();
    if details {
        let precip: f32 = covered.iter().map(|p| p.precipitation).sum();
        d.insert(
            "precipitation_amount".into(),
            json!(round(precip as f64, 1)),
        );
        if hours > 1 {
            let max = covered
                .iter()
                .map(|p| p.temperature)
                .fold(f32::MIN, f32::max);
            let min = covered
                .iter()
                .map(|p| p.temperature)
                .fold(f32::MAX, f32::min);
            d.insert("air_temperature_max".into(), json!(round(max as f64, 1)));
            d.insert("air_temperature_min".into(), json!(round(min as f64, 1)));
        }
    }
    // Only when IMS published a chance for every hour in the period.
    if let Some(chances) = covered
        .iter()
        .map(|p| p.precipitation_probability)
        .collect::<Option<Vec<f32>>>()
    {
        let max = chances.into_iter().fold(0.0, f32::max);
        d.insert(
            "probability_of_precipitation".into(),
            json!(round(max as f64, 1)),
        );
    }
    Some(json!({"summary": {"symbol_code": symbol}, "details": d}))
}

fn step(window: &[HourlyPoint], lat: f64, lon: f64) -> Value {
    let p = &window[0];
    let mut instant = Map::new();
    instant.insert(
        "air_temperature".into(),
        json!(round(p.temperature as f64, 1)),
    );
    instant.insert(
        "dew_point_temperature".into(),
        json!(round(p.dew_point as f64, 1)),
    );
    instant.insert(
        "relative_humidity".into(),
        json!(round(p.relative_humidity as f64, 1)),
    );
    instant.insert(
        "wind_from_direction".into(),
        json!(round(p.wind_direction as f64, 1)),
    );
    instant.insert("wind_speed".into(), json!(kmh_to_ms(p.wind_speed)));
    if let Some(gust) = p.wind_gust {
        instant.insert("wind_speed_of_gust".into(), json!(kmh_to_ms(gust)));
    }
    if let Some(uv) = p.uv_index {
        instant.insert(
            "ultraviolet_index_clear_sky".into(),
            json!(round(uv as f64, 1)),
        );
    }

    let mut data = Map::new();
    data.insert("instant".into(), json!({"details": instant}));
    for (key, hours, details) in [
        ("next_1_hours", 1, true),
        ("next_6_hours", 6, true),
        ("next_12_hours", 12, false),
    ] {
        if let Some(v) = period(window, hours, lat, lon, details) {
            data.insert(key.into(), v);
        }
    }
    json!({"time": iso(p.time_utc()), "data": data})
}

pub async fn compact(
    State(state): State<AppState>,
    Query(params): Query<Params>,
) -> Result<impl IntoResponse, ApiError> {
    let (Some(lat), Some(lon)) = (params.lat, params.lon) else {
        return Err(ApiError::bad_request(
            "lat and lon query parameters are required",
        ));
    };
    let snap = state.require_snapshot()?;
    let resolved = resolve(&snap, LocationQuery::Coords(lat, lon))?;
    let meta = &resolved.location.location_meta_data;
    let (loc_lat, loc_lon) = (meta.display_lat as f64, meta.display_lon as f64);

    let now = state.now();
    let hour_start = now.timestamp() - now.timestamp().rem_euclid(3600);
    let hourly = resolved.hourly;
    let first = hourly.partition_point(|p| p.time_utc().timestamp() < hour_start);
    let timeseries: Vec<Value> = (first..hourly.len())
        .map(|i| step(&hourly[i..], loc_lat, loc_lon))
        .collect();

    let body = json!({
        "type": "Feature",
        "geometry": {
            "type": "Point",
            "coordinates": [round(loc_lon, 4), round(loc_lat, 4), meta.display_height],
        },
        "properties": {
            "meta": {
                "updated_at": iso(snap.fetched_at),
                "units": {
                    "air_temperature": "celsius",
                    "air_temperature_max": "celsius",
                    "air_temperature_min": "celsius",
                    "dew_point_temperature": "celsius",
                    "precipitation_amount": "mm",
                    "probability_of_precipitation": "%",
                    "relative_humidity": "%",
                    "ultraviolet_index_clear_sky": "1",
                    "wind_from_direction": "degrees",
                    "wind_speed": "m/s",
                    "wind_speed_of_gust": "m/s",
                },
            },
            "timeseries": timeseries,
        },
    });

    let mut headers = HeaderMap::new();
    if let Some(v) = http_date(snap.fetched_at) {
        headers.insert(header::LAST_MODIFIED, v);
    }
    if let Some(v) = state
        .next_refresh()
        .filter(|t| *t > now)
        .and_then(http_date)
    {
        headers.insert(header::EXPIRES, v);
    }
    Ok((headers, Json(body)))
}

#[cfg(test)]
mod tests {
    use crate::server::test_support::*;
    use axum::http::{StatusCode, header};
    use serde_json::Value;

    const URL: &str = "/weatherapi/locationforecast/2.0/compact?lat=32.08&lon=34.78";

    fn step<'a>(ts: &'a [Value], time: &str) -> &'a Value {
        ts.iter()
            .find(|s| s["time"] == time)
            .unwrap_or_else(|| panic!("{time} missing"))
    }

    #[tokio::test]
    async fn geojson_shape() {
        let (status, headers, body) = request(state_at(NOW), URL).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            headers[header::LAST_MODIFIED],
            "Wed, 30 Sep 2026 18:45:00 GMT"
        );
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["type"], "Feature");
        assert_eq!(json["geometry"]["coordinates"][0], 34.7802);
        assert_eq!(json["properties"]["meta"]["units"]["wind_speed"], "m/s");
        let ts = json["properties"]["timeseries"].as_array().unwrap();
        // Starts at the current hour, in UTC.
        assert_eq!(ts[0]["time"], "2026-09-30T18:00:00Z");
        assert_eq!(ts[1]["time"], "2026-09-30T19:00:00Z");
        let d = &ts[0]["data"];
        assert!(d["instant"]["details"]["air_temperature"].is_number());
        assert!(d["next_1_hours"]["summary"]["symbol_code"].is_string());
        assert!(d["next_6_hours"]["details"]["air_temperature_max"].is_number());
        assert!(d["next_12_hours"]["summary"]["symbol_code"].is_string());
    }

    #[tokio::test]
    async fn ims_hourly_fields() {
        let (_, json) = get_json(state_at(NOW), URL).await;
        let ts = json["properties"]["timeseries"].as_array().unwrap();
        // 2026-10-02 11:00 Israel time = 08:00Z: 0.77 mm, 10% chance, gusts 37 km/h.
        let s = step(ts, "2026-10-02T08:00:00Z");
        assert_eq!(
            s["data"]["next_1_hours"]["details"]["precipitation_amount"],
            0.8
        );
        assert_eq!(
            s["data"]["next_1_hours"]["details"]["probability_of_precipitation"],
            10.0
        );
        assert_eq!(s["data"]["instant"]["details"]["wind_speed_of_gust"], 10.3);
        // The 6 hours starting 06:00Z include that rain.
        let six = &step(ts, "2026-10-02T06:00:00Z")["data"]["next_6_hours"]["details"];
        assert!(six["precipitation_amount"].as_f64().unwrap() >= 0.8);
        assert!(six["probability_of_precipitation"].as_f64().unwrap() >= 10.0);
        assert!(six["air_temperature_max"].as_f64() >= six["air_temperature_min"].as_f64());
    }

    #[tokio::test]
    async fn xml_fallback_has_no_probability_or_gusts() {
        let (_, json) = get_json(xml_state_at(NOW), URL).await;
        let ts = json["properties"]["timeseries"].as_array().unwrap();
        let s = &ts[3]["data"];
        assert!(
            s["next_1_hours"]["details"]
                .get("probability_of_precipitation")
                .is_none()
        );
        assert!(s["instant"]["details"].get("wind_speed_of_gust").is_none());
    }

    #[tokio::test]
    async fn periods_are_omitted_at_the_end() {
        let (_, json) = get_json(state_at(NOW), URL).await;
        let ts = json["properties"]["timeseries"].as_array().unwrap();
        let last = &ts[ts.len() - 1]["data"];
        assert!(last["next_1_hours"].is_object());
        assert!(last.get("next_6_hours").is_none());
        assert!(ts[ts.len() - 6]["data"]["next_6_hours"].is_object());
        assert!(ts[ts.len() - 6]["data"].get("next_12_hours").is_none());
    }

    #[tokio::test]
    async fn six_hour_precipitation_is_sum_of_hours() {
        let (_, json) = get_json(state_at(NOW), URL).await;
        let ts = json["properties"]["timeseries"].as_array().unwrap();
        for i in 0..ts.len().saturating_sub(6) {
            let six = ts[i]["data"]["next_6_hours"]["details"]["precipitation_amount"]
                .as_f64()
                .unwrap();
            let sum: f64 = (i..i + 6)
                .map(|j| {
                    ts[j]["data"]["next_1_hours"]["details"]["precipitation_amount"]
                        .as_f64()
                        .unwrap()
                })
                .sum();
            assert!((six - sum).abs() < 0.35, "step {i}: {six} vs {sum}");
        }
    }

    #[tokio::test]
    async fn complete_alias_and_errors() {
        let (status, _) = get_json(
            state_at(NOW),
            "/weatherapi/locationforecast/2.0/complete?lat=32.08&lon=34.78",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = get_json(
            state_at(NOW),
            "/weatherapi/locationforecast/2.0/compact?lat=32",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
