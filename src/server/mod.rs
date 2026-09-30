//! `weather serve`: an HTTP server that refreshes the IMS forecast on a cron schedule and serves
//! - a small native JSON API (`/api/...`),
//! - an Open-Meteo compatible `/v1/forecast`,
//! - a MET Norway (api.met.no) compatible `/weatherapi/locationforecast/2.0/{compact,complete}`,
//! - an embedded web UI at `/`.

mod api;
pub mod codes;
mod metno;
mod openmeteo;

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use chrono_tz::Asia::Jerusalem;
use cron::Schedule;
use serde_json::json;
use tracing::{error, info, warn};

use crate::hourly::{expand_hourly, HourlyPoint};
use crate::ims_structs::{Location, LocationForecasts};

/// Default refresh schedule: every hour at minute 7, Israel time.
pub const DEFAULT_SCHEDULE: &str = "0 7 * * * *";

/// Requests further than this from any IMS location are rejected rather than silently
/// answered with a far-away forecast.
const MAX_DISTANCE_KM: f64 = 100.0;

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// One successfully parsed download of the IMS forecast.
pub struct Snapshot {
    pub data: LocationForecasts,
    /// Hourly expansion of each location, parallel to `data.location`.
    pub hourly: Vec<Vec<HourlyPoint>>,
    pub fetched_at: DateTime<Utc>,
}

impl Snapshot {
    pub fn new(data: LocationForecasts, fetched_at: DateTime<Utc>) -> Self {
        let hourly = data
            .location
            .iter()
            .map(|l| expand_hourly(&l.location_data.forecast))
            .collect();
        Self {
            data,
            hourly,
            fetched_at,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    snapshot: Arc<RwLock<Option<Arc<Snapshot>>>>,
    next_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,
    schedule: Arc<str>,
    clock: Clock,
}

impl AppState {
    pub fn new(schedule: &str, clock: Clock) -> Self {
        Self {
            snapshot: Arc::default(),
            next_refresh: Arc::default(),
            schedule: schedule.into(),
            clock,
        }
    }

    pub fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.snapshot
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_snapshot(&self, snapshot: Snapshot) {
        *self.snapshot.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(snapshot));
    }

    fn next_refresh(&self) -> Option<DateTime<Utc>> {
        *self.next_refresh.read().unwrap_or_else(|e| e.into_inner())
    }

    fn set_next_refresh(&self, t: Option<DateTime<Utc>>) {
        *self.next_refresh.write().unwrap_or_else(|e| e.into_inner()) = t;
    }

    fn require_snapshot(&self) -> Result<Arc<Snapshot>, ApiError> {
        self.snapshot().ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "forecast not loaded yet, try again shortly",
            )
        })
    }
}

/// JSON error body in Open-Meteo's style: `{"error": true, "reason": "..."}`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    reason: String,
}

impl ApiError {
    pub fn new(status: StatusCode, reason: impl Into<String>) -> Self {
        Self {
            status,
            reason: reason.into(),
        }
    }

    pub fn bad_request(reason: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, reason)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": true, "reason": self.reason})),
        )
            .into_response()
    }
}

/// How a request identifies a location.
pub enum LocationQuery<'a> {
    Name(&'a str),
    Coords(f64, f64),
}

/// A location resolved against a snapshot.
pub struct Resolved<'a> {
    pub location: &'a Location,
    pub hourly: &'a [HourlyPoint],
    /// Distance from the requested coordinates, if coordinates were given.
    pub distance_km: Option<f64>,
}

pub fn resolve<'a>(snap: &'a Snapshot, query: LocationQuery) -> Result<Resolved<'a>, ApiError> {
    let (location, distance_km) = match query {
        LocationQuery::Name(name) => {
            let loc = crate::find_location(name, &snap.data)
                .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
            (loc, None)
        }
        LocationQuery::Coords(lat, lon) => {
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return Err(ApiError::bad_request(
                    "latitude must be in [-90, 90] and longitude in [-180, 180]",
                ));
            }
            let (loc, d) = crate::nearest_location(lat, lon, &snap.data)
                .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no locations loaded"))?;
            if d > MAX_DISTANCE_KM {
                return Err(ApiError::bad_request(format!(
                    "no IMS forecast location within {MAX_DISTANCE_KM} km of ({lat}, {lon}); \
                     nearest is {} at {d:.0} km",
                    loc.location_meta_data.location_name_eng.trim()
                )));
            }
            (loc, Some(d))
        }
    };
    let index = snap
        .data
        .location
        .iter()
        .position(|l| std::ptr::eq(l, location))
        .expect("location comes from this snapshot");
    Ok(Resolved {
        location,
        hourly: &snap.hourly[index],
        distance_km,
    })
}

/// Round to `decimals` places, for tidy JSON numbers from `f32` sources.
pub fn round(v: f64, decimals: i32) -> f64 {
    let p = 10f64.powi(decimals);
    (v * p).round() / p
}

/// Parse a cron expression. Accepts the `cron` crate's 6/7 field form (with seconds)
/// and standard 5 field cron (seconds default to 0).
pub fn parse_schedule(expr: &str) -> Result<Schedule, String> {
    let expr = expr.trim();
    let full = if expr.split_whitespace().count() == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    };
    Schedule::from_str(&full).map_err(|e| format!("invalid cron expression {expr:?}: {e}"))
}

async fn index() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("index.html"),
    )
}

async fn healthz(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    match state.snapshot() {
        Some(_) => "ok".into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE, "forecast not loaded").into_response(),
    }
}

async fn allow_any_origin(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/api/status", get(api::status))
        .route("/api/locations", get(api::locations))
        .route("/api/forecast", get(api::forecast))
        .route("/v1/forecast", get(openmeteo::forecast))
        .route(
            "/weatherapi/locationforecast/2.0/compact",
            get(metno::compact),
        )
        .route(
            "/weatherapi/locationforecast/2.0/complete",
            get(metno::compact),
        )
        .layer(axum::middleware::map_response(allow_any_origin))
        .with_state(state)
}

/// Download (or fall back to the cached copy of) the forecast and publish it to `state`.
pub async fn refresh(state: &AppState, offline: bool) {
    let result =
        tokio::task::spawn_blocking(move || crate::get_israeli_weather_forecast(offline)).await;
    match result {
        Ok(Ok(data)) => {
            let locations = data.location.len();
            state.set_snapshot(Snapshot::new(data, state.now()));
            info!(locations, "forecast refreshed");
        }
        Ok(Err(e)) => warn!("forecast refresh failed, keeping previous data: {e}"),
        Err(e) => error!("forecast refresh task panicked: {e}"),
    }
}

async fn run_scheduler(state: AppState, schedule: Schedule, offline: bool) {
    loop {
        let Some(next) = schedule.upcoming(Jerusalem).next() else {
            warn!("cron schedule has no upcoming runs; stopping refreshes");
            state.set_next_refresh(None);
            return;
        };
        let next = next.with_timezone(&Utc);
        state.set_next_refresh(Some(next));
        let wait = (next - Utc::now()).to_std().unwrap_or_default();
        tokio::time::sleep(wait).await;
        refresh(&state, offline).await;
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("shutting down");
}

pub struct ServeOptions {
    pub listen: SocketAddr,
    pub schedule: String,
    pub offline: bool,
}

/// Run the server until SIGINT/SIGTERM.
pub async fn serve(opts: ServeOptions) -> Result<(), Box<dyn std::error::Error>> {
    let schedule = parse_schedule(&opts.schedule)?;
    let state = AppState::new(&opts.schedule, Arc::new(Utc::now));

    refresh(&state, opts.offline).await;
    if state.snapshot().is_none() {
        warn!("no forecast available yet; serving 503 until the next successful refresh");
    }
    tokio::spawn(run_scheduler(state.clone(), schedule, opts.offline));

    let listener = tokio::net::TcpListener::bind(opts.listen).await?;
    eprintln!(
        "weather server listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub const FIXTURE: &str = include_str!("../../isr_cities_1week_6hr_forecast.xml");

    /// State loaded with the bundled fixture and a clock frozen at `now`.
    pub fn state_at(now: &str) -> AppState {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now).unwrap().into();
        let state = AppState::new(DEFAULT_SCHEDULE, Arc::new(move || now));
        let data = crate::parse_forecast(FIXTURE).unwrap();
        state.set_snapshot(Snapshot::new(data, now));
        state
    }

    pub async fn request(
        state: AppState,
        uri: &str,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let response = router(state)
            .oneshot(
                axum::http::Request::get(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8(body.to_vec()).unwrap())
    }

    pub async fn get_json(state: AppState, uri: &str) -> (StatusCode, serde_json::Value) {
        let (status, _, body) = request(state, uri).await;
        let json = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
        (status, json)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn parse_schedule_accepts_5_and_6_fields() {
        assert!(parse_schedule("7 * * * *").is_ok());
        assert!(parse_schedule(DEFAULT_SCHEDULE).is_ok());
        assert!(parse_schedule("every hour").is_err());
    }

    #[test]
    fn five_field_schedule_runs_at_second_zero() {
        use chrono::Timelike;
        let next = parse_schedule("*/15 * * * *")
            .unwrap()
            .upcoming(Utc)
            .next()
            .unwrap();
        assert_eq!(next.second(), 0);
        assert_eq!(next.minute() % 15, 0);
    }

    #[test]
    fn round_decimals() {
        assert_eq!(round(22.600000381, 1), 22.6);
        assert_eq!(round(0.126, 2), 0.13);
    }

    #[tokio::test]
    async fn serves_index_html() {
        let (status, headers, body) = request(state_at("2025-03-08T23:22:00Z"), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert!(body.contains("<title>"));
    }

    #[tokio::test]
    async fn healthz_reflects_data_availability() {
        let empty = AppState::new(DEFAULT_SCHEDULE, Arc::new(Utc::now));
        assert_eq!(
            request(empty.clone(), "/healthz").await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        let (status, json) = get_json(empty, "/api/locations").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json["error"], true);
        assert_eq!(
            request(state_at("2025-03-08T23:22:00Z"), "/healthz")
                .await
                .0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn cors_header_on_api_responses() {
        let (_, headers, _) = request(state_at("2025-03-08T23:22:00Z"), "/api/locations").await;
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    }

    #[test]
    fn resolve_rejects_far_away_and_invalid_coordinates() {
        let state = state_at("2025-03-08T23:22:00Z");
        let snap = state.snapshot().unwrap();
        // London
        let err = resolve(&snap, LocationQuery::Coords(51.5, -0.12))
            .err()
            .unwrap();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.reason.contains("nearest is"));
        let err = resolve(&snap, LocationQuery::Coords(95.0, 0.0))
            .err()
            .unwrap();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        let ok = resolve(&snap, LocationQuery::Coords(32.08, 34.78)).unwrap();
        assert_eq!(
            ok.location.location_meta_data.location_name_eng,
            "Tel Aviv Coast"
        );
        assert!(!ok.hourly.is_empty());
    }
}
