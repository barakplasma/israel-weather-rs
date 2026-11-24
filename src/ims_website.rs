//! Synchronous IMS website cache and typed validation, shared by the server and native clients.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use serde_json::Value;
use weather_api::ims_website::{
    CurrentResponse, ForecastResponse, LocationsResponse, RadarResponse, RegionsResponse,
    SeaRegionsResponse, WarningDays, WarningMetadataResponse, WarningsResponse, WeatherCodesResponse,
    WindDirectionsResponse,
};

const MAX_BODY: u64 = 16 * 1024 * 1024;
const RETRY_DELAY: Duration = Duration::from_secs(60);

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
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
    pub fn name(self) -> &'static str {
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

    pub fn ttl(self) -> Duration {
        Duration::from_secs(match self {
            Self::Current | Self::Warnings | Self::RadarSatellite => 300,
            Self::Forecast => 3600,
            _ => 86400,
        })
    }

    pub fn validate(self, root: &Value, lid: Option<i16>) -> bool {
        match self {
            Self::Current => CurrentResponse::deserialize(root).is_ok_and(|response| {
                lid.is_some_and(|id| response.data.contains_key(&id.to_string()))
                    && lid
                        .and_then(|id| root.get("data")?.get(id.to_string()))
                        .and_then(Value::as_object)
                        .is_some_and(|record| !record.is_empty())
            }),
            Self::Forecast => ForecastResponse::deserialize(root).is_ok_and(|response| {
                response.data.iter().any(|(date, day)| {
                    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
                        && (day.hourly.is_some() || day.daily.is_some())
                })
            }),
            Self::Locations => {
                LocationsResponse::deserialize(root).is_ok_and(|response| !response.data.is_empty())
            }
            Self::Warnings => WarningsResponse::deserialize(root).is_ok_and(|response| {
                match response.data.full_warnings_data {
                    Some(WarningDays::EmptyArray(rows)) => rows.is_empty(),
                    _ => true,
                }
            }),
            Self::WarningsMetadata => WarningMetadataResponse::deserialize(root).is_ok(),
            Self::WeatherCodes => WeatherCodesResponse::deserialize(root)
                .is_ok_and(|response| !response.data.is_empty()),
            Self::WindDirections => WindDirectionsResponse::deserialize(root)
                .is_ok_and(|response| !response.data.is_empty()),
            Self::Regions => RegionsResponse::deserialize(root).is_ok(),
            Self::SeaRegions => SeaRegionsResponse::deserialize(root)
                .is_ok_and(|response| !response.data.is_empty()),
            Self::RadarSatellite => RadarResponse::deserialize(root).is_ok(),
        }
    }
}

pub struct Entry {
    pub value: Value,
    pub fetched: SystemTime,
}

impl Entry {
    pub fn fresh(&self, ttl: Duration) -> bool {
        self.fetched.elapsed().is_ok_and(|age| age < ttl)
    }

}

#[derive(Clone)]
pub(crate) struct WebsiteStore {
    pub(crate) dir: PathBuf,
    pub(crate) base: String,
    pub(crate) offline: bool,
}

impl WebsiteStore {
    #[cfg(feature = "server")]
    pub(crate) fn new(offline: bool) -> Self {
        Self {
            dir: crate::cache_dir(),
            base: crate::non_empty_env("WEATHER_IMS_BASE_URL")
                .unwrap_or_else(|| "https://ims.gov.il/en".into()),
            offline,
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
        let path = self
            .dir
            .join("israel-weather-rs-ims_full_forecast_data.json");
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
        let value =
            serde_json::json!({"data": bulk.get("data")?.get("info")?.get(lid.to_string())?});
        Endpoint::Forecast
            .validate(&value, Some(lid))
            .then_some(Entry {
                value,
                fetched: metadata.modified().ok()?,
            })
    }

    pub(crate) fn fetch(
        &self,
        language: &str,
        endpoint: Endpoint,
        lid: Option<i16>,
        failures: &mut HashMap<String, Instant>,
    ) -> Option<Entry> {
        let suffix = lid.map_or_else(String::new, |id| format!("/{id}"));
        let key = format!("{language}-{}{}", endpoint.name(), suffix.replace('/', "-"));
        let path = self
            .dir
            .join(format!("israel-weather-rs-website-{key}.json"));
        let ttl = endpoint.ttl();
        let mut cached = Self::read(&path, endpoint, lid);
        if endpoint == Endpoint::Forecast
            && language == "en"
            && let Some(bulk) = lid.and_then(|id| self.bulk_forecast(id))
            && cached
                .as_ref()
                .is_none_or(|entry| bulk.fetched > entry.fetched)
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
            return Err(crate::Error::Json(
                "invalid IMS website response shape".into(),
            ));
        }
        Ok(value)
    }

    pub(crate) fn save(path: &Path, value: &Value) -> Result<(), crate::Error> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
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

