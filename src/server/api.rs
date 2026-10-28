//! Endpoints the forecast APIs don't cover: Open-Meteo style geocoding over the IMS locations,
//! IMS-only extras (warnings, country forecast) and server status.

use std::path::Path;
use std::time::Instant;

use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use weather_api::geocoding::{Place, SearchResponse};
use weather_api::{ims, status};

use super::{ApiError, AppState, LocationQuery, resolve, round};
use crate::hourly::Source;
use crate::ims_json::{CountryForecast, Warning};
use crate::ims_structs::Location;

const MAX_SEARCH_RESULTS: usize = 100;

fn place(l: &Location) -> Place {
    let m = &l.location_meta_data;
    Place {
        id: m.location_id.into(),
        name: m.location_name_eng.trim().to_string(),
        latitude: round(m.display_lat as f64, 4),
        longitude: round(m.display_lon as f64, 4),
        elevation: m.display_height as f64,
        feature_code: "PPL".into(),
        country_code: "IL".into(),
        country: "Israel".into(),
        timezone: "Asia/Jerusalem".into(),
    }
}

#[derive(Deserialize)]
pub struct SearchParams {
    name: Option<String>,
    count: Option<usize>,
}

/// Open-Meteo geocoding `/v1/search?name=…&count=…`: case-insensitive, prefix matches first,
/// then other substring matches, each alphabetical.
pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, ApiError> {
    let started = Instant::now();
    let Some(name) = params.name else {
        return Err(ApiError::bad_request("Parameter 'name' is required"));
    };
    let count = params.count.unwrap_or(10);
    if !(1..=MAX_SEARCH_RESULTS).contains(&count) {
        return Err(ApiError::bad_request(format!(
            "count must be 1-{MAX_SEARCH_RESULTS}"
        )));
    }
    let snap = state.require_snapshot()?;
    let wanted = name.trim().to_lowercase();
    let mut hits: Vec<(bool, String, &Location)> = snap
        .data
        .location
        .iter()
        .filter_map(|l| {
            let n = l.location_meta_data.location_name_eng.trim().to_lowercase();
            // Like Open-Meteo, a single character only matches name starts.
            let pos = n.find(&wanted)?;
            (pos == 0 || wanted.chars().count() > 1).then_some((pos != 0, n, l))
        })
        .collect();
    hits.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    Ok(Json(SearchResponse {
        results: if wanted.is_empty() {
            vec![]
        } else {
            hits.into_iter()
                .take(count)
                .map(|(_, _, l)| place(l))
                .collect()
        },
        generationtime_ms: started.elapsed().as_secs_f64() * 1000.0,
    }))
}

#[derive(Deserialize)]
pub struct CoordParams {
    latitude: Option<f64>,
    longitude: Option<f64>,
}

fn warning(w: &Warning) -> ims::Warning {
    ims::Warning {
        alert_id: w.alert_id.clone(),
        severity_id: w.severity_id,
        severity: w.severity.clone(),
        color: w.color.clone(),
        warning_type: w.warning_type.clone(),
        valid_from: w.valid_from,
        valid_to: w.valid_to,
        text: w.text.clone(),
        text_he: w.text_he.clone(),
    }
}

/// `/api/ims?latitude=…&longitude=…`: the nearest IMS location, its warnings and today's
/// country forecast.
pub async fn extras(
    State(state): State<AppState>,
    Query(params): Query<CoordParams>,
) -> Result<Json<ims::Extras>, ApiError> {
    let (Some(lat), Some(lon)) = (params.latitude, params.longitude) else {
        return Err(ApiError::bad_request(
            "Parameter 'latitude' and 'longitude' are required",
        ));
    };
    let snap = state.require_snapshot()?;
    let resolved = resolve(&snap, LocationQuery::Coords(lat, lon))?;
    let now = state.now();
    let today = now.with_timezone(&chrono_tz::Asia::Jerusalem).date_naive();
    Ok(Json(ims::Extras {
        location: place(resolved.location),
        distance_km: round(resolved.distance_km.unwrap_or(0.0), 1),
        warnings: snap
            .warnings_for(resolved.location.location_meta_data.location_id, now)
            .into_iter()
            .map(warning)
            .collect(),
        country_forecast: snap.country_forecast(today).map(|c: &CountryForecast| {
            ims::CountryForecast {
                date: c.date,
                description: c.description.clone(),
            }
        }),
    }))
}

/// Resident set size from `/proc/self/status` (Linux only).
fn rss_bytes() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = s.lines().find_map(|l| l.strip_prefix("VmRSS:"))?;
    Some(
        kb.trim()
            .trim_end_matches("kB")
            .trim()
            .parse::<u64>()
            .ok()?
            * 1024,
    )
}

fn modified(meta: &std::fs::Metadata) -> Option<DateTime<Utc>> {
    meta.modified().ok().map(DateTime::<Utc>::from)
}

/// Files this program keeps in the cache directory: its own `israel-weather-rs-*` JSON
/// copies, and `cached-path` downloads (found through their `.meta` sidecars) from IMS.
fn cache_files(dir: &Path) -> Vec<status::CacheFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut files: Vec<status::CacheFile> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("israel-weather-rs-") {
                let meta = e.metadata().ok()?;
                return Some(status::CacheFile {
                    name,
                    source: None,
                    bytes: meta.len(),
                    modified: modified(&meta),
                });
            }
            if !name.ends_with(".meta") {
                return None;
            }
            let sidecar: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            let resource = sidecar["resource"].as_str()?;
            if !resource.contains("ims.gov.il") {
                return None;
            }
            let data = name.trim_end_matches(".meta");
            let meta = std::fs::metadata(dir.join(data)).ok()?;
            Some(status::CacheFile {
                name: data.to_string(),
                source: Some(resource.to_string()),
                bytes: meta.len(),
                modified: modified(&meta),
            })
        })
        .collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

pub async fn status(State(state): State<AppState>) -> Json<status::Status> {
    let snap = state.snapshot();
    let now = state.now();
    let hourly = snap.iter().flat_map(|s| s.hourly.iter().flatten());
    let count = |src: Source| hourly.clone().filter(|p| p.source == src).count();
    let extras = snap.as_ref().map(|s| &s.extras);
    let dir = crate::cache_dir();
    let files = tokio::task::spawn_blocking({
        let dir = dir.clone();
        move || cache_files(&dir)
    })
    .await
    .unwrap_or_default();
    Json(status::Status {
        server: status::Server {
            version: env!("CARGO_PKG_VERSION").into(),
            git_commit: option_env!("WEATHER_GIT_COMMIT").map(Into::into),
            target: env!("WEATHER_TARGET").into(),
            rustc: env!("WEATHER_RUSTC").into(),
            started_at: state.stats.started_at,
            uptime_seconds: (now - state.stats.started_at).num_seconds().max(0) as u64,
            rss_bytes: rss_bytes(),
            pid: std::process::id(),
        },
        data: status::Data {
            loaded: snap.is_some(),
            fetched_at: snap.as_ref().map(|s| s.fetched_at),
            ims_hourly_fetched_at: extras.and_then(|e| e.full_fetched_at),
            locations: snap.as_ref().map_or(0, |s| s.data.location.len()),
            xml_samples: snap.as_ref().map_or(0, |s| {
                s.data
                    .location
                    .iter()
                    .map(|l| l.location_data.forecast.len())
                    .sum()
            }),
            hourly_ims: count(Source::Ims),
            hourly_interpolated: count(Source::Interpolated),
            first_hour: hourly.clone().map(|p| p.time_utc()).min(),
            last_hour: hourly.clone().map(|p| p.time_utc()).max(),
            daily_summaries: extras
                .and_then(|e| e.full.as_ref())
                .map_or(0, |f| f.locations.values().map(|l| l.daily.len()).sum()),
            country_forecasts: extras
                .and_then(|e| e.full.as_ref())
                .map_or(0, |f| f.country.len()),
            warnings: extras.and_then(|e| e.warnings.as_ref()).map(|w| w.len()),
            schedule: state.schedule.to_string(),
            next_refresh: state.next_refresh(),
        },
        cache: status::Cache {
            dir: dir.display().to_string(),
            total_bytes: files.iter().map(|f| f.bytes).sum(),
            files,
        },
        refreshes: state.stats.refreshes(),
        requests: state.stats.routes(),
    })
}

#[cfg(test)]
mod tests {
    use crate::server::AppState;
    use crate::server::test_support::*;
    use axum::http::StatusCode;
    use weather_api::geocoding::SearchResponse;
    use weather_api::ims::Extras;
    use weather_api::status::Status;

    async fn typed<T: serde::de::DeserializeOwned>(state: AppState, uri: &str) -> T {
        let (status, json) = get_json(state, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {json}");
        serde_json::from_value(json).unwrap()
    }

    #[tokio::test]
    async fn search_prefix_matches_first() {
        let r: SearchResponse = typed(state_at(NOW), "/v1/search?name=tel%20aviv").await;
        assert!(r.results.len() >= 2);
        assert!(
            r.results
                .iter()
                .all(|p| p.name.to_lowercase().contains("tel aviv"))
        );
        assert!(r.results[0].name.to_lowercase().starts_with("tel aviv"));
        assert_eq!(r.results[0].country_code, "IL");

        // Substring hits rank after prefix hits.
        let r: SearchResponse = typed(state_at(NOW), "/v1/search?name=aviv&count=100").await;
        assert!(r.results.iter().any(|p| p.name.starts_with("Tel Aviv")));
    }

    #[tokio::test]
    async fn search_no_match_omits_results_and_validates() {
        let (status, json) = get_json(state_at(NOW), "/v1/search?name=Atlantis").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.get("results").is_none(), "{json}");
        let r: SearchResponse = typed(state_at(NOW), "/v1/search?name=j&count=100").await;
        assert!(r.results.iter().all(|p| p.name.starts_with('J')));
        for uri in [
            "/v1/search",
            "/v1/search?name=a&count=0",
            "/v1/search?name=a&count=101",
        ] {
            let (status, _) = get_json(state_at(NOW), uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        }
    }

    #[tokio::test]
    async fn ims_extras_for_searched_location() {
        let r: SearchResponse = typed(
            state_at(NOW),
            "/v1/search?name=Tel%20Aviv%20-%20Yafo&count=1",
        )
        .await;
        let p = &r.results[0];
        let e: Extras = typed(
            state_at(NOW),
            &format!("/api/ims?latitude={}&longitude={}", p.latitude, p.longitude),
        )
        .await;
        assert_eq!(e.location.name, "Tel Aviv - Yafo");
        assert!(e.distance_km < 0.1);
        assert!(!e.warnings.is_empty());
        assert!(!e.warnings[0].warning_type.is_empty());
        assert!(e.country_forecast.is_some());

        let e: Extras = typed(xml_state_at(NOW), "/api/ims?latitude=32.79&longitude=34.99").await;
        assert!(e.warnings.is_empty() && e.country_forecast.is_none());
        let (status, _) = get_json(state_at(NOW), "/api/ims?latitude=32").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn status_counts_data_and_requests() {
        let state = state_at(NOW);
        let _ = get_json(state.clone(), "/v1/search?name=haifa").await;
        let _ = get_json(state.clone(), "/v1/search").await;
        let s: Status = typed(state, "/api/status").await;
        assert!(s.data.loaded);
        assert_eq!(s.data.locations, 175);
        assert!(s.data.hourly_ims > 0 && s.data.hourly_interpolated > 0);
        assert!(s.data.first_hour < s.data.last_hour);
        assert!(s.data.warnings.unwrap() > 0);
        assert!(!s.server.version.is_empty() && !s.server.target.is_empty());
        let search = s.requests.iter().find(|r| r.route == "/v1/search").unwrap();
        assert_eq!((search.count, search.errors), (2, 1));

        let s: Status = typed(xml_state_at(NOW), "/api/status").await;
        // Only the 6-hourly sample hours are IMS values without the hourly JSON.
        assert!(s.data.hourly_ims * 4 < s.data.hourly_interpolated);
        assert_eq!(s.data.warnings, None);
    }
}
