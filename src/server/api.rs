//! Native JSON API used by the embedded web UI.

use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Duration};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{resolve, round, ApiError, AppState, LocationQuery};
use crate::hourly::HourlyPoint;
use crate::ims_structs::{Forecast, Location};

#[derive(Serialize)]
pub struct LocationInfo<'a> {
    id: i16,
    name: &'a str,
    latitude: f32,
    longitude: f32,
    elevation: f32,
}

impl<'a> From<&'a Location> for LocationInfo<'a> {
    fn from(l: &'a Location) -> Self {
        let m = &l.location_meta_data;
        Self {
            id: m.location_id,
            name: m.location_name_eng.trim(),
            latitude: m.display_lat,
            longitude: m.display_lon,
            elevation: m.display_height,
        }
    }
}

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let snap = state.snapshot();
    let extras = snap.as_ref().map(|s| &s.extras);
    Json(json!({
        "loaded": snap.is_some(),
        "fetched_at": snap.as_ref().map(|s| s.fetched_at.to_rfc3339()),
        "locations": snap.as_ref().map(|s| s.data.location.len()).unwrap_or(0),
        "next_refresh": state.next_refresh().map(|t| t.to_rfc3339()),
        "schedule": &*state.schedule,
        "sources": {
            "xml": snap.is_some(),
            "ims_hourly": extras.is_some_and(|e| e.full.is_some()),
            "ims_hourly_fetched_at": extras.and_then(|e| e.full_fetched_at).map(|t| t.to_rfc3339()),
            "warnings": extras.and_then(|e| e.warnings.as_ref()).map(|w| w.len()),
        },
    }))
}

pub async fn locations(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let snap = state.require_snapshot()?;
    let list: Vec<LocationInfo> = snap.data.location.iter().map(Into::into).collect();
    Ok(Json(json!(list)))
}

#[derive(Deserialize)]
pub struct ForecastParams {
    location: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    /// Only return samples within the next `hours` hours.
    hours: Option<u32>,
}

fn hourly_json(p: &HourlyPoint) -> Value {
    let r1 = |v: f32| round(v as f64, 1);
    json!({
        "time": p.time.to_rfc3339(),
        "temperature": r1(p.temperature),
        "feels_like": r1(p.feels_like),
        "relative_humidity": p.relative_humidity.round(),
        "precipitation": round(p.precipitation as f64, 2),
        "precipitation_probability": p.precipitation_probability.map(|v| v.round()),
        "wind_speed": r1(p.wind_speed),
        "wind_gust": p.wind_gust.map(r1),
        "wind_direction": p.wind_direction.round(),
        "uv_index": p.uv_index.map(r1),
        "weather_code": p.weather_code,
        "weather": crate::weather_code_to_str(p.weather_code),
        "source": p.source,
    })
}

/// The 6-hourly XML samples still relevant: the most recent one (within 6 h) and all later ones.
fn upcoming(
    forecasts: &[Forecast],
    now: DateTime<chrono::Utc>,
    hours: Option<u32>,
) -> Vec<&Forecast> {
    let block = Duration::hours(6);
    let horizon = hours.map(|h| now + Duration::hours(h as i64));
    forecasts
        .iter()
        .filter(|f| {
            DateTime::parse_from_rfc3339(&f.forecast_time)
                .is_ok_and(|t| t + block > now && horizon.is_none_or(|h| t < h))
        })
        .collect()
}

pub async fn forecast(
    State(state): State<AppState>,
    Query(params): Query<ForecastParams>,
) -> Result<Json<Value>, ApiError> {
    let snap = state.require_snapshot()?;
    let query = match (&params.location, params.lat, params.lon) {
        (Some(name), _, _) => LocationQuery::Name(name),
        (None, Some(lat), Some(lon)) => LocationQuery::Coords(lat, lon),
        _ => {
            return Err(ApiError::bad_request(
                "pass either `location=<name>` or both `lat` and `lon`",
            ))
        }
    };
    let resolved = resolve(&snap, query)?;
    let now = state.now();
    let forecasts = upcoming(&resolved.location.location_data.forecast, now, params.hours);
    let today = now.with_timezone(&chrono_tz::Asia::Jerusalem).date_naive();
    let daily: Vec<_> = resolved.daily.iter().filter(|d| d.date >= today).collect();
    let hour_start =
        DateTime::from_timestamp(now.timestamp() - now.timestamp().rem_euclid(3600), 0)
            .expect("valid timestamp");
    let horizon = params.hours.map(|h| now + Duration::hours(h as i64));
    let hourly: Vec<Value> = resolved
        .hourly
        .iter()
        .filter(|p| p.time_utc() >= hour_start && horizon.is_none_or(|h| p.time_utc() < h))
        .map(hourly_json)
        .collect();
    Ok(Json(json!({
        "location": LocationInfo::from(resolved.location),
        "distance_km": resolved.distance_km.map(|d| round(d, 1)),
        "fetched_at": snap.fetched_at.to_rfc3339(),
        "warnings": snap.warnings_for(resolved.location.location_meta_data.location_id, now),
        "country_forecast": snap.country_forecast(today),
        "daily": daily,
        "hourly": hourly,
        "forecasts": forecasts,
    })))
}

#[cfg(test)]
mod tests {
    use crate::server::test_support::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn locations_are_trimmed_and_complete() {
        let (status, json) = get_json(state_at(NOW), "/api/locations").await;
        assert_eq!(status, StatusCode::OK);
        let list = json.as_array().unwrap();
        assert_eq!(list.len(), 175);
        assert_eq!(list[0]["name"], "Jerusalem");
        assert!(list.iter().all(|l| {
            let n = l["name"].as_str().unwrap();
            n == n.trim()
        }));
    }

    #[tokio::test]
    async fn forecast_by_name_includes_current_sample() {
        let (status, json) =
            get_json(state_at(NOW), "/api/forecast?location=tel%20aviv%20coast").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["location"]["name"], "Tel Aviv Coast");
        assert!(json["distance_km"].is_null());
        // 21:45 local: the 21:00 sample is the current one.
        let f = json["forecasts"].as_array().unwrap();
        assert_eq!(f[0]["ForecastTime"], "2026-09-30T21:00:00+03:00");
        assert_eq!(f[1]["ForecastTime"], "2026-10-01T03:00:00+03:00");
    }

    #[tokio::test]
    async fn forecast_includes_ims_extras() {
        let (_, json) = get_json(
            state_at(NOW),
            "/api/forecast?location=Tel%20Aviv%20-%20Yafo",
        )
        .await;
        let warnings = json["warnings"].as_array().unwrap();
        assert!(!warnings.is_empty());
        let w = &warnings[0];
        for key in [
            "severity",
            "color",
            "warning_type",
            "valid_from",
            "valid_to",
            "text",
        ] {
            assert!(w[key].is_string(), "{key}: {w}");
        }
        assert!(json["country_forecast"]["description"].is_string());
        let daily = json["daily"].as_array().unwrap();
        assert_eq!(daily[0]["date"], "2026-09-30");
        assert!(daily[0]["max_temp"].is_number());
    }

    #[tokio::test]
    async fn forecast_hourly_rows() {
        let (_, json) = get_json(
            state_at(NOW),
            "/api/forecast?location=Tel%20Aviv%20Coast&hours=48",
        )
        .await;
        let hourly = json["hourly"].as_array().unwrap();
        // From the current hour (21:00 local) for 48 hours.
        assert_eq!(hourly[0]["time"], "2026-09-30T21:00:00+03:00");
        assert_eq!(hourly.len(), 49);
        assert_eq!(hourly[0]["source"], "ims");
        assert_eq!(hourly[1]["source"], "interpolated");
        let rainy = hourly
            .iter()
            .find(|h| h["time"] == "2026-10-02T11:00:00+03:00")
            .unwrap();
        assert_eq!(rainy["precipitation"], 0.77);
        assert_eq!(rainy["precipitation_probability"], 10.0);
        assert!(rainy["weather"].is_string());
    }

    #[tokio::test]
    async fn forecast_hourly_includes_current_hour_with_subsecond_clock() {
        let (_, json) = get_json(
            state_at("2026-09-30T18:45:02.071Z"),
            "/api/forecast?location=Tel%20Aviv%20Coast&hours=2",
        )
        .await;
        assert_eq!(json["hourly"][0]["time"], "2026-09-30T21:00:00+03:00");
    }

    #[tokio::test]
    async fn forecast_without_ims_json_has_empty_extras() {
        let (_, json) = get_json(xml_state_at(NOW), "/api/forecast?location=Haifa").await;
        assert_eq!(json["warnings"], serde_json::json!([]));
        assert!(json["country_forecast"].is_null());
        assert_eq!(json["daily"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn forecast_by_coordinates_with_hours_limit() {
        let (status, json) =
            get_json(state_at(NOW), "/api/forecast?lat=31.78&lon=35.2&hours=12").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["location"]["name"], "Jerusalem");
        assert!(json["distance_km"].as_f64().unwrap() < 1.0);
        // Current sample (21:00) plus samples before 09:45 local: 03:00, 09:00.
        assert_eq!(json["forecasts"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn forecast_errors() {
        let (status, json) = get_json(state_at(NOW), "/api/forecast?location=Atlantis").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(json["reason"].as_str().unwrap().contains("Jerusalem"));
        let (status, _) = get_json(state_at(NOW), "/api/forecast?lat=32").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn status_reports_sources() {
        let (_, json) = get_json(state_at(NOW), "/api/status").await;
        assert_eq!(json["loaded"], true);
        assert_eq!(json["locations"], 175);
        assert_eq!(json["sources"]["ims_hourly"], true);
        assert!(json["sources"]["warnings"].as_u64().unwrap() > 0);
        let (_, json) = get_json(xml_state_at(NOW), "/api/status").await;
        assert_eq!(json["sources"]["ims_hourly"], false);
    }
}
