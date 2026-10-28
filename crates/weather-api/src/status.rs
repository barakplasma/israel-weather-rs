//! `/api/status`: what the server has loaded and cached, and how it has been running.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub server: Server,
    pub data: Data,
    pub cache: Cache,
    /// Most recent first.
    pub refreshes: Vec<Refresh>,
    /// Sorted by route.
    pub requests: Vec<RouteStats>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub version: String,
    pub git_commit: Option<String>,
    /// Rust target triple the binary was built for.
    pub target: String,
    pub rustc: String,
    pub started_at: DateTime<Utc>,
    pub uptime_seconds: u64,
    /// Resident memory, where the OS exposes it (Linux `/proc`).
    pub rss_bytes: Option<u64>,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Data {
    pub loaded: bool,
    /// When the XML forecast was last downloaded.
    pub fetched_at: Option<DateTime<Utc>>,
    /// When the IMS hourly JSON was last downloaded.
    pub ims_hourly_fetched_at: Option<DateTime<Utc>>,
    pub locations: usize,
    /// 6-hourly samples in the XML, all locations.
    pub xml_samples: usize,
    /// Hourly rows with IMS's own values (hourly JSON, or an XML sample hour), all locations.
    pub hourly_ims: usize,
    /// Hourly rows interpolated from the XML, all locations.
    pub hourly_interpolated: usize,
    pub first_hour: Option<DateTime<Utc>>,
    pub last_hour: Option<DateTime<Utc>>,
    pub daily_summaries: usize,
    pub country_forecasts: usize,
    /// `None` when the warnings feed has never loaded.
    pub warnings: Option<usize>,
    pub schedule: String,
    pub next_refresh: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    pub dir: String,
    pub files: Vec<CacheFile>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheFile {
    pub name: String,
    /// The URL it was downloaded from, when known.
    pub source: Option<String>,
    pub bytes: u64,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refresh {
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub sources: Vec<SourceResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceResult {
    /// e.g. "forecast xml", "IMS hourly forecast".
    pub name: String,
    /// `None` on success.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteStats {
    pub route: String,
    pub count: u64,
    /// Responses with status >= 400.
    pub errors: u64,
    pub mean_ms: f64,
    pub max_ms: f64,
}
