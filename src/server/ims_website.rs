//! Website-compatible IMS JSON routes. Shapes are based on the MIT-licensed py-weatheril
//! client; see docs/ims-website-api.md and the third-party notice.
//! Raw envelopes deliberately preserve upstream values rather than inventing observations.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use axum::extract::{Path as RoutePath, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::Mutex;

use super::{ApiError, AppState};

const MAX_BODY: u64 = 16 * 1024 * 1024;
const RETRY_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endpoint {
    Current,
    Forecast,
    Locations,
    Warnings,
    WarningsMetadata,
    WeatherCodes,
    WindDirections,
    Regions,
    SeaRegions,
    RadarSatellite,
}

impl Endpoint {
    fn name(self) -> &'static str {
        match self {
            Self::Current => "now_analysis",
            Self::Forecast => "full_forecast_data",
            Self::Locations => "locations_info",
            Self::Warnings => "warnings",
            Self::WarningsMetadata => "warnings_metadata",
            Self::WeatherCodes => "weather_codes",
            Self::WindDirections => "wind_directions",
            Self::Regions => "regions",
            Self::SeaRegions => "sea_regions",
            Self::RadarSatellite => "radar_satellite",
        }
    }

    fn ttl(self) -> Duration {
        Duration::from_secs(match self {
            Self::Current | Self::Warnings | Self::RadarSatellite => 300,
            Self::Forecast => 3600,
            _ => 86400,
        })
    }

    fn validate(self, root: &Value, lid: Option<i16>) -> bool {
        let Some(data) = root.get("data") else {
            return false;
        };
        match self {
            Self::Current => lid
                .and_then(|id| data.get(id.to_string()))
                .and_then(Value::as_object)
                .is_some_and(|record| !record.is_empty()),
            Self::Forecast => data.as_object().is_some_and(|days| {
                days.iter().any(|(date, day)| {
                    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
                        && (day.get("hourly").is_some() || day.get("daily").is_some())
                })
            }),
            Self::Regions => data.is_array(),
            Self::RadarSatellite => data.get("types").is_some_and(Value::is_object),
            Self::Warnings => data.is_object(),
            Self::WarningsMetadata => data.get("ims_warning_type").is_some()
                && data.get("warning_groups").is_some()
                && data.get("warning_severity").is_some(),
            _ => data.as_object().is_some_and(|map| !map.is_empty()),
        }
    }
}

struct Entry {
    value: Value,
    fetched: SystemTime,
}

impl Entry {
    fn fresh(&self, ttl: Duration) -> bool {
        self.fetched.elapsed().is_ok_and(|age| age < ttl)
    }

    fn response(self, ttl: Duration) -> Response {
        let stale = !self.fresh(ttl);
        let timestamp = DateTime::<Utc>::from(self.fetched).to_rfc3339();
        let mut response = Json(self.value).into_response();
        if let Ok(value) = HeaderValue::from_str(&timestamp) {
            response.headers_mut().insert("x-ims-fetched-at", value);
        }
        response.headers_mut().insert(
            "x-ims-stale",
            HeaderValue::from_static(if stale { "true" } else { "false" }),
        );
        response
    }
}

#[derive(Clone)]
pub(super) struct WebsiteCache {
    dir: PathBuf,
    base: String,
    offline: bool,
    enabled: bool,
    // Serialize cache misses and remember failures briefly. All keys are bounded by the
    // fixed endpoint list, two languages and IDs in the forecast snapshot.
    gate: Arc<Mutex<HashMap<String, Instant>>>,
}

impl WebsiteCache {
    pub(super) fn new(offline: bool, enabled: bool) -> Self {
        Self {
            dir: crate::cache_dir(),
            base: crate::non_empty_env("WEATHER_IMS_BASE_URL")
                .unwrap_or_else(|| "https://ims.gov.il/en".into()),
            offline,
            enabled,
            gate: Arc::default(),
        }
    }

    fn base_for(&self, language: &str) -> String {
        let base = self.base.trim_end_matches('/');
        if let Some(prefix) = base
            .strip_suffix("/en")
            .or_else(|| base.strip_suffix("/he"))
        {
            format!("{prefix}/{language}")
        } else if language == "en" {
            base.to_owned()
        } else {
            format!("{base}/{language}")
        }
    }

    fn read(path: &Path, endpoint: Endpoint, lid: Option<i16>) -> Option<Entry> {
        let file = std::fs::File::open(path).ok()?;
        let metadata = file.metadata().ok()?;
        if metadata.len() > MAX_BODY {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_BODY + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_BODY {
            return None;
        }
        let value = serde_json::from_slice(&bytes).ok()?;
        endpoint.validate(&value, lid).then_some(Entry {
            value,
            fetched: metadata.modified().ok()?,
        })
    }

    fn bulk_forecast(&self, lid: i16) -> Option<Entry> {
        // The scheduled bulk cache is English by default. Do not use it for Hebrew.
        if !self.base.trim_end_matches('/').ends_with("/en") {
            return None;
        }
        let path = self.dir.join("israel-weather-rs-ims_full_forecast_data.json");
        let file = std::fs::File::open(path).ok()?;
        let metadata = file.metadata().ok()?;
        // Bulk data can be larger than a single location.
        const MAX_BULK: u64 = 64 * 1024 * 1024;
        if metadata.len() > MAX_BULK {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_BULK + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_BULK {
            return None;
        }
        let bulk: Value = serde_json::from_slice(&bytes).ok()?;
        let value = serde_json::json!({"data": bulk.get("data")?.get("info")?.get(lid.to_string())?});
        Endpoint::Forecast.validate(&value, Some(lid)).then_some(Entry {
            value,
            fetched: metadata.modified().ok()?,
        })
    }

    fn fetch(
        &self,
        language: &str,
        endpoint: Endpoint,
        lid: Option<i16>,
        failures: &mut HashMap<String, Instant>,
    ) -> Option<Entry> {
        let suffix = lid.map_or_else(String::new, |id| format!("/{id}"));
        let key = format!("{language}-{}{}", endpoint.name(), suffix.replace('/', "-"));
        let path = self.dir.join(format!("israel-weather-rs-website-{key}.json"));
        let ttl = endpoint.ttl();
        let mut cached = Self::read(&path, endpoint, lid);
        if endpoint == Endpoint::Forecast
            && language == "en"
            && let Some(bulk) = lid.and_then(|id| self.bulk_forecast(id))
            && cached.as_ref().is_none_or(|entry| bulk.fetched > entry.fetched)
        {
            cached = Some(bulk);
        }
        if self.offline || cached.as_ref().is_some_and(|entry| entry.fresh(ttl)) {
            return cached;
        }
        failures.retain(|_, when| when.elapsed() < RETRY_DELAY);
        if failures.contains_key(&key) {
            return cached;
        }
        let url = format!("{}/{}{}", self.base_for(language), endpoint.name(), suffix);
        match Self::download(&url, endpoint, lid) {
            Ok(value) => {
                let fetched = SystemTime::now();
                if let Err(error) = Self::save(&path, &value) {
                    tracing::warn!(%error, %key, "could not persist IMS website response");
                }
                Some(Entry { value, fetched })
            }
            Err(error) => {
                failures.insert(key.clone(), Instant::now());
                tracing::warn!(%error, %key, "IMS website request failed; using cache if available");
                cached
            }
        }
    }

    fn download(url: &str, endpoint: Endpoint, lid: Option<i16>) -> Result<Value, crate::Error> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!(
                "israel-weather-rs/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/barakplasma/israel-weather-rs)"
            ))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let response = client.get(url).send()?.error_for_status()?;
        let mut bytes = Vec::new();
        response.take(MAX_BODY + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BODY {
            return Err(crate::Error::Json("IMS website response too large".into()));
        }
        let value = serde_json::from_slice(&bytes)?;
        if !endpoint.validate(&value, lid) {
            return Err(crate::Error::Json("invalid IMS website response shape".into()));
        }
        Ok(value)
    }

    fn save(path: &Path, value: &Value) -> Result<(), crate::Error> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| {
            std::fs::write(&tmp, serde_json::to_vec(value)?)?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
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
        cache.fetch(&language, endpoint, lid, &mut gate)
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "IMS website task failed"))?
    .ok_or_else(|| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "IMS website data unavailable"))?;
    Ok(entry.response(endpoint.ttl()))
}

#[utoipa::path(
    get,
    path = "/{language}/now_analysis/{lid}",
    tag = "ims-website",
    params(("language" = String, Path, description = "he or en"), ("lid" = i16, Path, description = "IMS location ID")),
    responses(
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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
        (status = 200, description = "Upstream IMS JSON envelope", body = Object),
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

    #[tokio::test]
    async fn offline_route_uses_language_specific_cache() {
        let mut state = state_at(NOW);
        let dir = std::env::temp_dir().join(format!(
            "weather-website-test-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
        ));
        state.website.dir = dir.clone();
        let current = serde_json::json!({"data":{"2":{"temperature":"22.5","rain":"-999"}}});
        WebsiteCache::save(
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
