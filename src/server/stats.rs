//! In-memory counters for `/api/status`: refresh history and per-route request metrics.
//! Everything resets on restart.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, Utc};
use weather_api::status::{Refresh, RouteStats};

use super::AppState;

/// How many refreshes `/api/status` remembers.
const REFRESH_HISTORY: usize = 24;

#[derive(Default)]
struct Route {
    count: u64,
    errors: u64,
    total_ms: f64,
    max_ms: f64,
}

pub struct Stats {
    pub started_at: DateTime<Utc>,
    refreshes: Mutex<VecDeque<Refresh>>,
    routes: Mutex<BTreeMap<String, Route>>,
}

impl Stats {
    pub fn new(started_at: DateTime<Utc>) -> Self {
        Self {
            started_at,
            refreshes: Mutex::default(),
            routes: Mutex::default(),
        }
    }

    pub fn record_refresh(&self, r: Refresh) {
        let mut q = self.refreshes.lock().unwrap_or_else(|e| e.into_inner());
        q.push_front(r);
        q.truncate(REFRESH_HISTORY);
    }

    /// Most recent first.
    pub fn refreshes(&self) -> Vec<Refresh> {
        let q = self.refreshes.lock().unwrap_or_else(|e| e.into_inner());
        q.iter().cloned().collect()
    }

    fn record_request(&self, route: &str, error: bool, ms: f64) {
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        let r = routes.entry(route.to_string()).or_default();
        r.count += 1;
        r.errors += error as u64;
        r.total_ms += ms;
        r.max_ms = r.max_ms.max(ms);
    }

    pub fn routes(&self) -> Vec<RouteStats> {
        let routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        routes
            .iter()
            .map(|(route, r)| RouteStats {
                route: route.clone(),
                count: r.count,
                errors: r.errors,
                mean_ms: if r.count == 0 {
                    0.0
                } else {
                    r.total_ms / r.count as f64
                },
                max_ms: r.max_ms,
            })
            .collect()
    }
}

/// Route-layer middleware: keyed by the route pattern, so arbitrary URLs can't grow the map.
pub async fn track(
    State(state): State<AppState>,
    path: MatchedPath,
    req: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let response = next.run(req).await;
    state.stats.record_request(
        path.as_str(),
        response.status().as_u16() >= 400,
        started.elapsed().as_secs_f64() * 1000.0,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_history_is_bounded_and_newest_first() {
        let stats = Stats::new(Utc::now());
        for ms in 0..30 {
            stats.record_refresh(Refresh {
                started_at: Utc::now(),
                duration_ms: ms,
                sources: vec![],
            });
        }
        let r = stats.refreshes();
        assert_eq!(r.len(), REFRESH_HISTORY);
        assert_eq!(r[0].duration_ms, 29);
    }

    #[test]
    fn route_mean_and_max() {
        let stats = Stats::new(Utc::now());
        stats.record_request("/a", false, 1.0);
        stats.record_request("/a", true, 3.0);
        let r = &stats.routes()[0];
        assert_eq!((r.count, r.errors, r.mean_ms, r.max_ms), (2, 1, 2.0, 3.0));
    }
}
