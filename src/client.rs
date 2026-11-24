//! Instance-scoped synchronous client for embedded applications (including Python).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::hourly::{expand_hourly, merge_hourly};
use crate::ims_structs::LocationForecasts;
use crate::ims_website::{Endpoint, Entry, WebsiteStore};
use crate::{Error, ims_json};

/// Configuration is local to one client; no environment mutation is required.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub language: String,
    pub cache_dir: PathBuf,
    pub offline: bool,
    pub ims_base_url: String,
    pub xml_url: String,
    pub cache_expiration_in_sec: Option<u64>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            language: "en".into(),
            cache_dir: crate::cache_dir(),
            offline: false,
            ims_base_url: crate::non_empty_env("WEATHER_IMS_BASE_URL")
                .unwrap_or_else(|| "https://ims.gov.il/en".into()),
            xml_url: crate::weather_url(),
            cache_expiration_in_sec: None,
        }
    }
}

pub struct Client {
    config: ClientConfig,
    store: WebsiteStore,
    xml: Mutex<Option<Arc<LocationForecasts>>>,
    failures: Mutex<HashMap<String, Instant>>,
}

impl Client {
    pub fn new(config: ClientConfig) -> Result<Self, Error> {
        if !matches!(config.language.as_str(), "en" | "he") {
            return Err(Error::Json("language must be en or he".into()));
        }
        Ok(Self {
            store: WebsiteStore {
                dir: config.cache_dir.clone(),
                base: config.ims_base_url.clone(),
                offline: config.offline,
                ttl_override: config.cache_expiration_in_sec.map(Duration::from_secs),
            },
            config,
            xml: Mutex::default(),
            failures: Mutex::default(),
        })
    }

    /// Explicitly reload the XML forecast. Keep an earlier in-memory forecast on failure.
    pub fn refresh(&self) -> Result<usize, Error> {
        let mut xml = self.xml.lock().unwrap_or_else(|e| e.into_inner());
        match crate::forecast_with_config(
            self.config.offline,
            &self.config.cache_dir,
            &self.config.xml_url,
        ) {
            Ok(data) => {
                let count = data.location.len();
                *xml = Some(Arc::new(data));
                Ok(count)
            }
            Err(error) => xml.as_ref().map(|data| data.location.len()).ok_or(error),
        }
    }

    fn xml(&self) -> Result<Arc<LocationForecasts>, Error> {
        if let Some(data) = self.xml.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(data);
        }
        self.refresh()?;
        self.xml
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| Error::Json("XML forecast unavailable".into()))
    }

    pub fn locations(&self) -> Result<Value, Error> {
        let data = self.xml()?;
        Ok(Value::Array(
            data.location
                .iter()
                .map(|location| {
                    let m = &location.location_meta_data;
                    json!({
                        "location_id": m.location_id,
                        "name": m.location_name_eng,
                        "latitude": m.display_lat,
                        "longitude": m.display_lon,
                        "elevation": m.display_height,
                    })
                })
                .collect(),
        ))
    }

    fn website_entry(&self, endpoint: Endpoint, lid: Option<i16>) -> Result<Entry, Error> {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        self.store
            .fetch(&self.config.language, endpoint, lid, &mut failures)
            .ok_or_else(|| Error::Json(format!("{} unavailable", endpoint.name())))
    }

    /// Original IMS envelope, retrieval time and cache freshness.
    pub fn website(&self, endpoint: Endpoint, lid: Option<i16>) -> Result<Value, Error> {
        if matches!(endpoint, Endpoint::Current | Endpoint::Forecast) != lid.is_some()
            || lid.is_some_and(|id| id <= 0)
        {
            return Err(Error::Json("invalid location ID for endpoint".into()));
        }
        let entry = self.website_entry(endpoint, lid)?;
        let stale = !entry.fresh(self.store.ttl(endpoint));
        Ok(json!({
            "payload": entry.value,
            "fetched_at": DateTime::<Utc>::from(entry.fetched),
            "stale": stale,
        }))
    }

    /// Same XML expansion and native hourly overlay as the server, with XML-only fallback.
    pub fn forecast(&self, lid: i16) -> Result<Value, Error> {
        let xml = self.xml()?;
        let location = xml
            .location
            .iter()
            .find(|l| l.location_meta_data.location_id == lid)
            .ok_or_else(|| Error::LocationNotFound {
                query: lid.to_string(),
                available: Vec::new(),
            })?;
        let base = expand_hourly(&location.location_data.forecast);
        let native = self
            .website_entry(Endpoint::Forecast, Some(lid))
            .ok()
            .and_then(|entry| {
                let body = json!({"data":{"info":{(lid.to_string()): entry.value.get("data")?}}});
                let parsed = ims_json::parse_full_forecast(&body.to_string()).ok()?;
                Some((
                    parsed.locations.get(&lid)?.clone(),
                    DateTime::<Utc>::from(entry.fetched),
                    !entry.fresh(self.store.ttl(Endpoint::Forecast)),
                ))
            });
        let (hourly, daily, fetched_at, stale) = match native {
            Some((forecast, fetched_at, stale)) => (
                merge_hourly(base, &forecast.hourly),
                forecast.daily,
                Some(fetched_at),
                Some(stale),
            ),
            None => (base, Vec::new(), None, None),
        };
        Ok(json!({
            "location_id": lid,
            "hourly": hourly,
            "daily": daily,
            "native_fetched_at": fetched_at,
            "native_stale": stale,
        }))
    }
}
