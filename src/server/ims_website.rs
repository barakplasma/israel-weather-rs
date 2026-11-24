//! Website-compatible IMS JSON routes. Shapes are based on the MIT-licensed py-weatheril
//! client; see docs/ims-website-api.md and the third-party notice.
//! Raw envelopes deliberately preserve upstream values rather than inventing observations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path as RoutePath, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::sync::Mutex;
use weather_api::ims_website::{
    CurrentAnalysis, CurrentResponse, ForecastResponse, LocationsResponse, RadarResponse,
    RegionsResponse, SeaRegionsResponse, WarningMetadataResponse, WarningsResponse,
    WeatherCodesResponse, WindDirectionsResponse,
};

use super::{ApiError, AppState};
use crate::ims_website::{Endpoint, Entry, WebsiteStore};

const ENRICHMENT_WAIT: Duration = Duration::from_secs(1);

fn response(entry: Entry, ttl: Duration) -> Response {
    let stale = !entry.fresh(ttl);
    let timestamp = DateTime::<Utc>::from(entry.fetched).to_rfc3339();
    let mut response = Json(entry.value).into_response();
    if let Ok(value) = HeaderValue::from_str(&timestamp) {
        response.headers_mut().insert("x-ims-fetched-at", value);
    }
    response.headers_mut().insert(
        "x-ims-stale",
        HeaderValue::from_static(if stale { "true" } else { "false" }),
    );
    response
}

#[derive(Clone)]
pub(super) struct WebsiteCache {
    store: WebsiteStore,
    enabled: bool,
    gate: Arc<Mutex<HashMap<String, Instant>>>,
}

impl WebsiteCache {
    pub(super) fn new(offline: bool, enabled: bool) -> Self {
        Self {
            store: WebsiteStore::new(offline),
            enabled,
            gate: Arc::default(),
        }
    }

    // Optional enrichment: never wait behind a fetch, never use the network in offline mode, and
    // give the original endpoint a short latency budget. A timed-out blocking refresh can
    // finish and populate the cache, but the original response uses its forecast fallback.
    pub(super) async fn current_for(
        &self,
        lid: i16,
        now: DateTime<Utc>,
    ) -> Option<CurrentAnalysis> {
        if !self.enabled {
            return None;
        }
        let gate = self.gate.clone().try_lock_owned().ok()?;
        let cache = self.clone();
        let job = tokio::task::spawn_blocking(move || {
            let mut gate = gate;
            let entry = cache
                .store
                .fetch("en", Endpoint::Current, Some(lid), &mut gate)?;
            // Offline mode can use a cached analysis past its refresh TTL, but never
            // substitute analysis from a different hour or the future.
            if !cache.store.offline && !entry.fresh(Endpoint::Current.ttl()) {
                return None;
            }
            let response = CurrentResponse::deserialize(&entry.value).ok()?;
            let record = response.data.get(&lid.to_string())?;
            let time = crate::parse_time(record.forecast_time.as_deref()?).ok()?;
            if time.timestamp() / 3600 != now.timestamp() / 3600
                || time.timestamp() > now.timestamp()
            {
                return None;
            }
            Some(record.clone())
        });
        tokio::time::timeout(ENRICHMENT_WAIT, job)
            .await
            .ok()?
            .ok()?
    }

}

async fn serve(
    state: AppState,
    language: String,
    endpoint: Endpoint,
    lid: Option<i16>,
) -> Result<Response, ApiError> {
    if language != "he" && language != "en" {
        return Err(ApiError::bad_request("language must be he or en"));
    }
    let cache = state.website.clone();
    if !cache.enabled {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "IMS website JSON disabled",
        ));
    }
    if let Some(id) = lid {
        let snapshot = state.require_snapshot()?;
        let known = snapshot
            .data
            .location
            .iter()
            .any(|location| location.location_meta_data.location_id == id)
            || snapshot
                .extras
                .full
                .as_ref()
                .is_some_and(|full| full.locations.contains_key(&id));
        if !known {
            return Err(ApiError::new(StatusCode::NOT_FOUND, "unknown IMS location"));
        }
    }
    let gate = tokio::time::timeout(Duration::from_secs(20), cache.gate.clone().lock_owned())
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "IMS website cache busy"))?;
    let entry = tokio::task::spawn_blocking(move || {
        let mut gate = gate;
        cache.store.fetch(&language, endpoint, lid, &mut gate)
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "IMS website task failed"))?
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "IMS website data unavailable",
        )
    })?;
    Ok(response(entry, endpoint.ttl()))
}

#[utoipa::path(
    get,
    path = "/{language}/now_analysis/{lid}",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en"), ("lid" = i16, Path, description = "IMS location ID")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = CurrentResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn current(
    State(state): State<AppState>,
    RoutePath((language, lid)): RoutePath<(String, i16)>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::Current, Some(lid)).await
}

#[utoipa::path(
    get,
    path = "/{language}/full_forecast_data/{lid}",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en"), ("lid" = i16, Path, description = "IMS location ID")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = ForecastResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn forecast(
    State(state): State<AppState>,
    RoutePath((language, lid)): RoutePath<(String, i16)>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::Forecast, Some(lid)).await
}

#[utoipa::path(
    get,
    path = "/{language}/locations_info",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = LocationsResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn locations(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::Locations, None).await
}

#[utoipa::path(
    get,
    path = "/{language}/warnings",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = WarningsResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn warnings(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::Warnings, None).await
}

#[utoipa::path(
    get,
    path = "/{language}/warnings_metadata",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = WarningMetadataResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn warnings_metadata(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::WarningsMetadata, None).await
}

#[utoipa::path(
    get,
    path = "/{language}/weather_codes",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = WeatherCodesResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn weather_codes(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::WeatherCodes, None).await
}

#[utoipa::path(
    get,
    path = "/{language}/wind_directions",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = WindDirectionsResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn wind_directions(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::WindDirections, None).await
}

#[utoipa::path(
    get,
    path = "/{language}/regions",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = RegionsResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn regions(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::Regions, None).await
}

#[utoipa::path(
    get,
    path = "/{language}/sea_regions",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = SeaRegionsResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn sea_regions(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::SeaRegions, None).await
}

#[utoipa::path(
    get,
    path = "/{language}/radar_satellite",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = RadarResponse),
        (status = 400, description = "Invalid request", body = weather_api::ApiError),
        (status = 404, description = "Unknown location", body = weather_api::ApiError),
        (status = 503, description = "Unavailable", body = weather_api::ApiError)
    )
)]
pub(super) async fn radar_satellite(
    State(state): State<AppState>,
    RoutePath(language): RoutePath<String>,
) -> Result<Response, ApiError> {
    serve(state, language, Endpoint::RadarSatellite, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use serde_json::Value;
    use crate::server::test_support::{NOW, request, state_at};

    #[test]
    fn validates_native_shapes() {
        let current = serde_json::json!({"data":{"2":{"temperature":"22.5","rain":"-999"}}});
        assert!(Endpoint::Current.validate(&current, Some(2)));
        assert!(!Endpoint::Current.validate(&current, Some(3)));
        let forecast = serde_json::json!({"data":{"2026-11-24":{"hourly":{}}}});
        assert!(Endpoint::Forecast.validate(&forecast, Some(2)));
        assert!(!Endpoint::Forecast.validate(&current, Some(2)));
        assert!(Endpoint::RadarSatellite.validate(
            &serde_json::json!({"data":{"types":{"IMSRadar":[],"satellite":{"NATURAL":[]}}}}),
            None,
        ));
        assert!(!Endpoint::RadarSatellite.validate(&current, None));
        assert!(Endpoint::Regions.validate(&serde_json::json!({"data":[]}), None));
    }

    #[test]
    fn typed_validation_uses_existing_ims_fixtures() {
        let locations: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/ims_locations_info.json"))
                .unwrap();
        assert!(Endpoint::Locations.validate(&locations, None));
        let warnings: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/ims_warnings.json")).unwrap();
        assert!(Endpoint::Warnings.validate(&warnings, None));
        let metadata: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/ims_warnings_metadata.json"
        ))
        .unwrap();
        assert!(Endpoint::WarningsMetadata.validate(&metadata, None));
        let full: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/ims_full_forecast_data.json"
        ))
        .unwrap();
        for (lid, days) in full["data"]["info"].as_object().unwrap() {
            let envelope = serde_json::json!({"data": days});
            assert!(Endpoint::Forecast.validate(&envelope, lid.parse().ok()));
        }
        assert!(!Endpoint::Current.validate(
            &serde_json::json!({"data":{"2":{"temperature":{}}}}),
            Some(2),
        ));
    }

    #[tokio::test]
    async fn existing_routes_enrich_only_with_fresh_matching_analysis() {
        use crate::server::test_support::get_json;

        let mut state = state_at(NOW);
        let dir = std::env::temp_dir().join(format!(
            "weather-enrichment-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        state.website.store.dir = dir.clone();
        let query = "/v1/forecast?latitude=32.0821&longitude=34.7802&current=temperature_2m,relative_humidity_2m,wind_gusts_10m&current_weather=true";
        let extras = "/api/ims?latitude=32.0821&longitude=34.7802";
        let (status, baseline) = get_json(state.clone(), query).await;
        assert_eq!(status, StatusCode::OK);
        let (_, base_extras) = get_json(state.clone(), extras).await;
        assert!(base_extras.get("current_analysis").is_none());
        let path = dir.join("israel-weather-rs-website-en-now_analysis-2.json");
        let mut record = serde_json::json!({
            "data": {"2": {"forecast_time":"2026-09-30 21:00:00",
                "temperature":"37.5","relative_humidity":-999,"gust_speed":-999,
                "pm10":"25","unmodeled_field":true}}
        });
        WebsiteStore::save(&path, &record).unwrap();
        state.website.store.offline = false;
        let (status, enriched) = get_json(state.clone(), query).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(enriched["current"]["temperature_2m"], 37.5);
        assert_eq!(enriched["current_weather"]["temperature"], 37.5);
        assert_eq!(
            enriched["current"]["relative_humidity_2m"],
            baseline["current"]["relative_humidity_2m"]
        );
        assert_eq!(
            enriched["current"]["wind_gusts_10m"],
            baseline["current"]["wind_gusts_10m"]
        );
        let (_, enriched_extras) = get_json(state.clone(), extras).await;
        assert_eq!(enriched_extras["current_analysis"]["pm10"], "25");
        let (_, native) = get_json(state.clone(), "/en/now_analysis/2").await;
        assert_eq!(native, record);
        record["data"]["2"]["forecast_time"] = serde_json::json!("2026-09-30 20:00:00");
        WebsiteStore::save(&path, &record).unwrap();
        let (_, mismatch) = get_json(state.clone(), query).await;
        assert_eq!(mismatch["current"], baseline["current"]);
        let (_, mismatch_extras) = get_json(state.clone(), extras).await;
        assert!(mismatch_extras.get("current_analysis").is_none());
        state.website.store.offline = true;
        let (_, offline) = get_json(state.clone(), query).await;
        assert_eq!(offline["current"], baseline["current"]);
        // Even a TTL-expired cache can enrich offline if the analysis is for this hour.
        record["data"]["2"]["forecast_time"] = serde_json::json!("2026-09-30 21:00:00");
        WebsiteStore::save(&path, &record).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(600))
            .unwrap();
        let (_, cached) = get_json(state.clone(), query).await;
        assert_eq!(cached["current"]["temperature_2m"], 37.5);
        let (_, cached_extras) = get_json(state, extras).await;
        assert_eq!(cached_extras["current_analysis"]["pm10"], "25");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn offline_route_uses_language_specific_cache() {
        let mut state = state_at(NOW);
        let dir = std::env::temp_dir().join(format!(
            "weather-website-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        state.website.store.dir = dir.clone();
        let current = serde_json::json!({"data":{"2":{"temperature":"22.5","rain":"-999"}}});
        WebsiteStore::save(
            &dir.join("israel-weather-rs-website-en-now_analysis-2.json"),
            &current,
        )
        .unwrap();
        let (status, headers, body) = request(state.clone(), "/en/now_analysis/2").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), current);
        assert_eq!(headers["x-ims-stale"], "false");
        assert!(headers.contains_key("x-ims-fetched-at"));
        let (status, _, _) = request(state.clone(), "/he/now_analysis/2").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let (status, _, _) = request(state.clone(), "/fr/now_analysis/2").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, _) = request(state.clone(), "/en/now_analysis/32767").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        state.website.enabled = false;
        let (status, _, _) = request(state, "/en/now_analysis/2").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
