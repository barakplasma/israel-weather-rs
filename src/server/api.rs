//! Native JSON API used by the embedded web UI.

use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Duration};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{resolve, round, ApiError, AppState, LocationQuery};
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
    Json(json!({
        "loaded": snap.is_some(),
        "fetched_at": snap.as_ref().map(|s| s.fetched_at.to_rfc3339()),
        "locations": snap.as_ref().map(|s| s.data.location.len()).unwrap_or(0),
        "next_refresh": state.next_refresh().map(|t| t.to_rfc3339()),
        "schedule": &*state.schedule,
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
    /// Only return blocks covering the next `hours` hours.
    hours: Option<u32>,
}

/// Blocks that have not ended yet (the current block plus everything after it).
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
    let forecasts = upcoming(
        &resolved.location.location_data.forecast,
        state.now(),
        params.hours,
    );
    Ok(Json(json!({
        "location": LocationInfo::from(resolved.location),
        "distance_km": resolved.distance_km.map(|d| round(d, 1)),
        "fetched_at": snap.fetched_at.to_rfc3339(),
        "forecasts": forecasts,
    })))
}

#[cfg(test)]
mod tests {
    use crate::server::test_support::*;
    use axum::http::StatusCode;

    const NOW: &str = "2025-03-08T23:22:00Z";

    #[tokio::test]
    async fn locations_are_trimmed_and_complete() {
        let (status, json) = get_json(state_at(NOW), "/api/locations").await;
        assert_eq!(status, StatusCode::OK);
        let list = json.as_array().unwrap();
        assert_eq!(list.len(), 163);
        assert_eq!(list[0]["name"], "Jerusalem");
        assert!(list.iter().all(|l| {
            let n = l["name"].as_str().unwrap();
            n == n.trim()
        }));
    }

    #[tokio::test]
    async fn forecast_by_name_includes_current_block() {
        let (status, json) =
            get_json(state_at(NOW), "/api/forecast?location=tel%20aviv%20coast").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["location"]["name"], "Tel Aviv Coast");
        assert!(json["distance_km"].is_null());
        // 01:22 local: the 20:00 block (until 02:00) is still current.
        let f = json["forecasts"].as_array().unwrap();
        assert_eq!(f[0]["ForecastTime"], "2025-03-08T20:00:00+02:00");
        assert_eq!(f[1]["ForecastTime"], "2025-03-09T02:00:00+02:00");
    }

    #[tokio::test]
    async fn forecast_by_coordinates_with_hours_limit() {
        let (status, json) =
            get_json(state_at(NOW), "/api/forecast?lat=31.78&lon=35.2&hours=12").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["location"]["name"], "Jerusalem");
        assert!(json["distance_km"].as_f64().unwrap() < 1.0);
        // Current block (20:00) plus blocks starting before 13:22 local: 02:00, 08:00.
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
    async fn status_reports_loaded() {
        let (_, json) = get_json(state_at(NOW), "/api/status").await;
        assert_eq!(json["loaded"], true);
        assert_eq!(json["locations"], 163);
    }
}
