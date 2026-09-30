//! The JSON endpoints the ims.gov.il website itself uses.
//!
//! These are undocumented, so parsing is deliberately lenient: numbers may arrive as JSON
//! numbers or strings, empty strings mean "missing", and container nesting is walked rather
//! than assumed. Every fetch keeps an on-disk copy so offline mode and outages still work.
//!
//! - `ims_full_forecast_data`: hourly forecast (7 days) for every location in one request.
//!   Its values are identical to the XML at the XML's hours, the XML is a 6-hourly sample of it.
//! - `warnings` + `warnings_metadata`: active and upcoming IMS warnings by region.
//! - `locations_info`: maps each location id to its warning region (`rid`) and sea region.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::Serialize;
use serde_json::{Map, Value};
use tracing::{trace, warn};

use crate::Error;

const DEFAULT_BASE_URL: &str = "https://ims.gov.il/en";
const USER_AGENT: &str = concat!(
    "israel-weather-rs/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/barakplasma/israel-weather-rs)"
);

pub const FULL_FORECAST: &str = "ims_full_forecast_data";
pub const WARNINGS: &str = "warnings";
pub const WARNINGS_METADATA: &str = "warnings_metadata";
pub const LOCATIONS_INFO: &str = "locations_info";

/// IMS warning group for the general public (others are aviation, seamanship, ...).
const GENERAL_PUBLIC_GROUP: &str = "18";

/// IMS `wind_direction_id` (16-point compass, 1 = N) -> degrees. From `/en/wind_directions`.
const WIND_DIRECTIONS: [f32; 18] = [
    0.0, 0.0, 23.0, 45.0, 68.0, 90.0, 113.0, 135.0, 158.0, 180.0, 203.0, 225.0, 248.0, 270.0,
    293.0, 315.0, 338.0, 0.0,
];

/// A number that may be encoded as a JSON number or a numeric string.
fn num(v: Option<&Value>) -> Option<f32> {
    match v? {
        Value::Number(n) => n.as_f64().map(|f| f as f32),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| text(Some(x))).collect(),
        Some(Value::Object(o)) => o.values().filter_map(|x| text(Some(x))).collect(),
        _ => vec![],
    }
}

fn data(root: &Value) -> Result<&Map<String, Value>, Error> {
    root.get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Json("missing `data` object".into()))
}

/// One hour of the IMS hourly forecast.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyRow {
    /// Israel local time.
    pub time: DateTime<FixedOffset>,
    /// °C
    pub temperature: f32,
    /// %
    pub relative_humidity: Option<f32>,
    /// km/h
    pub wind_speed: Option<f32>,
    /// Degrees the wind comes from.
    pub wind_direction: Option<f32>,
    /// km/h
    pub wind_gust: Option<f32>,
    /// mm during this hour.
    pub rain: f32,
    /// Probability of precipitation, %.
    pub rain_chance: Option<f32>,
    pub weather_code: i32,
    pub uv_index: Option<f32>,
    pub uv_index_max: Option<f32>,
    pub heat_stress: Option<f32>,
    /// m, for coastal locations.
    pub wave_height: Option<f32>,
    /// µg/m³
    pub pm10: Option<f32>,
}

impl HourlyRow {
    fn parse(v: &Value) -> Option<Self> {
        let time = crate::parse_time(v.get("forecast_time")?.as_str()?).ok()?;
        let temperature = num(v.get("precise_temperature")).or(num(v.get("temperature")))?;
        let wind_direction = num(v.get("wind_direction_id"))
            .and_then(|id| WIND_DIRECTIONS.get(id as usize).copied());
        Some(Self {
            time,
            temperature,
            relative_humidity: num(v.get("relative_humidity")),
            wind_speed: num(v.get("wind_speed")),
            wind_direction,
            wind_gust: num(v.get("gust_speed")),
            rain: num(v.get("rain")).unwrap_or(0.0),
            rain_chance: num(v.get("rain_chance")),
            weather_code: num(v.get("weather_code")).map_or(0, |c| c as i32),
            uv_index: num(v.get("u_v_index")),
            uv_index_max: num(v.get("u_v_i_max")),
            heat_stress: num(v.get("heat_stress")),
            wave_height: num(v.get("wave_height")),
            pm10: num(v.get("pm10")),
        })
    }
}

/// IMS's own daily summary for a location.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DailySummary {
    pub date: NaiveDate,
    pub min_temp: Option<f32>,
    pub max_temp: Option<f32>,
    pub weather_code: Option<i32>,
    pub uv_index_max: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocationForecast {
    /// Sorted by time.
    pub hourly: Vec<HourlyRow>,
    /// Sorted by date.
    pub daily: Vec<DailySummary>,
}

impl LocationForecast {
    pub fn daily_for(&self, date: NaiveDate) -> Option<&DailySummary> {
        self.daily.iter().find(|d| d.date == date)
    }
}

/// A written forecast for the whole country, one per day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CountryForecast {
    pub date: NaiveDate,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FullForecast {
    /// Keyed by IMS location id (`LocationId` in the XML).
    pub locations: HashMap<i16, LocationForecast>,
    /// Sorted by date.
    pub country: Vec<CountryForecast>,
}

/// Parse `/en/ims_full_forecast_data` (`data.info[lid][date].{hourly,daily,country}`).
pub fn parse_full_forecast(json: &str) -> Result<FullForecast, Error> {
    let root: Value = serde_json::from_str(json)?;
    let info = data(&root)?
        .get("info")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Json("missing `data.info` object".into()))?;

    let mut out = FullForecast::default();
    let mut country: HashMap<NaiveDate, String> = HashMap::new();
    let mut skipped = 0usize;
    for (lid, days) in info {
        let Ok(lid) = lid.parse::<i16>() else {
            continue;
        };
        let loc = out.locations.entry(lid).or_default();
        for (date, day) in days.as_object().into_iter().flatten() {
            let Ok(date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
                continue;
            };
            for row in day
                .get("hourly")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                match HourlyRow::parse(row.1) {
                    Some(r) => loc.hourly.push(r),
                    None => skipped += 1,
                }
            }
            if let Some(d) = day.get("daily") {
                loc.daily.push(DailySummary {
                    date,
                    min_temp: num(d.get("minimum_temperature")),
                    max_temp: num(d.get("maximum_temperature")),
                    weather_code: num(d.get("weather_code")).map(|c| c as i32),
                    uv_index_max: num(d.get("maximum_uvi")),
                });
            }
            if let Some(desc) = text(day.get("country").and_then(|c| c.get("description"))) {
                country.entry(date).or_insert(desc);
            }
        }
        loc.hourly.sort_by_key(|r| r.time);
        loc.hourly.dedup_by_key(|r| r.time);
        loc.daily.sort_by_key(|d| d.date);
    }
    if skipped > 0 {
        warn!(skipped, "skipped unparseable hourly rows in IMS json");
    }
    if out.locations.values().all(|l| l.hourly.is_empty()) {
        return Err(Error::Json("no hourly rows in forecast".into()));
    }
    out.country = country
        .into_iter()
        .map(|(date, description)| CountryForecast { date, description })
        .collect();
    out.country.sort_by_key(|c| c.date);
    Ok(out)
}

/// A location's warning regions, from `/en/locations_info`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocationRegion {
    pub rid: Option<String>,
    pub sea_rid: Option<String>,
}

pub fn parse_location_regions(json: &str) -> Result<HashMap<i16, LocationRegion>, Error> {
    let root: Value = serde_json::from_str(json)?;
    let regions: HashMap<i16, LocationRegion> = data(&root)?
        .values()
        .filter_map(|l| {
            let lid = num(l.get("lid"))? as i16;
            Some((
                lid,
                LocationRegion {
                    rid: text(l.get("rid")),
                    sea_rid: text(l.get("sea_rid")),
                },
            ))
        })
        .collect();
    if regions.is_empty() {
        return Err(Error::Json("no locations in locations_info".into()));
    }
    Ok(regions)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Warning {
    pub alert_id: String,
    pub severity_id: u8,
    /// e.g. "Yellow Warning", "Orange Early Warning".
    pub severity: String,
    /// Hex color IMS uses for this severity.
    pub color: String,
    /// e.g. "Flash Floods", "Heat Stress".
    pub warning_type: String,
    pub valid_from: DateTime<FixedOffset>,
    pub valid_to: DateTime<FixedOffset>,
    pub text: String,
    pub text_he: String,
    #[serde(skip)]
    pub regions: Vec<String>,
    #[serde(skip)]
    pub groups: Vec<String>,
}

impl Warning {
    /// Whether this general-public warning covers the location's land or sea region.
    pub fn applies_to(&self, region: &LocationRegion) -> bool {
        self.groups.iter().any(|g| g == GENERAL_PUBLIC_GROUP)
            && [&region.rid, &region.sea_rid]
                .into_iter()
                .flatten()
                .any(|r| self.regions.contains(r))
    }

    pub fn is_current_or_upcoming(&self, now: DateTime<chrono::Utc>) -> bool {
        self.valid_to > now
    }
}

struct WarningNames {
    severity: HashMap<String, (String, String)>,
    types: HashMap<String, String>,
}

impl WarningNames {
    fn parse(metadata: Option<&str>) -> Self {
        let root: Value = metadata
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or(Value::Null);
        let d = root.get("data");
        let severity = d
            .and_then(|d| d.get("warning_severity"))
            .and_then(Value::as_object)
            .map(|o| {
                o.values()
                    .filter_map(|s| {
                        Some((
                            text(s.get("severity_id"))?,
                            (
                                text(s.get("severity_name"))?,
                                text(s.get("color")).unwrap_or_default(),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let types = d
            .and_then(|d| d.get("ims_warning_type"))
            .and_then(Value::as_object)
            .map(|o| {
                o.values()
                    .filter_map(|t| Some((text(t.get("warning_type_id"))?, text(t.get("name"))?)))
                    .collect()
            })
            .unwrap_or_default();
        Self { severity, types }
    }
}

fn collect_alerts<'a>(v: &'a Value, out: &mut Vec<&'a Map<String, Value>>) {
    match v {
        Value::Object(o) => {
            if o.get("alert_id").is_some_and(|a| !a.is_null()) && o.contains_key("valid_to") {
                out.push(o);
            } else {
                o.values().for_each(|c| collect_alerts(c, out));
            }
        }
        Value::Array(a) => a.iter().for_each(|c| collect_alerts(c, out)),
        _ => {}
    }
}

/// Parse `/en/warnings`, naming severities and types from `/en/warnings_metadata` if given.
/// The nesting of this endpoint varies, so every object carrying an `alert_id` is collected.
pub fn parse_warnings(json: &str, metadata: Option<&str>) -> Result<Vec<Warning>, Error> {
    let root: Value = serde_json::from_str(json)?;
    let names = WarningNames::parse(metadata);
    let mut alerts = Vec::new();
    collect_alerts(
        data(&root)?.get("distinct_warnings").unwrap_or(&root),
        &mut alerts,
    );
    if alerts.is_empty() {
        collect_alerts(&root, &mut alerts);
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for a in alerts {
        let Some(alert_id) = text(a.get("alert_id")) else {
            continue;
        };
        if !seen.insert(alert_id.clone()) {
            continue;
        }
        let time = |k: &str| {
            a.get(k)
                .and_then(Value::as_str)
                .and_then(|t| crate::parse_time(t).ok())
        };
        let (Some(valid_from), Some(valid_to)) = (time("valid_from"), time("valid_to")) else {
            trace!(alert_id, "warning without valid times");
            continue;
        };
        let severity_key = text(a.get("severity_id")).unwrap_or_default();
        let type_key = text(a.get("warning_type_id")).unwrap_or_default();
        let (severity, color) = names
            .severity
            .get(&severity_key)
            .cloned()
            .unwrap_or_else(|| (format!("Severity {severity_key}"), String::new()));
        out.push(Warning {
            alert_id,
            severity_id: severity_key.parse().unwrap_or(0),
            severity,
            color,
            warning_type: names
                .types
                .get(&type_key)
                .cloned()
                .unwrap_or_else(|| format!("Warning type {type_key}")),
            valid_from,
            valid_to,
            text: text(a.get("full_en")).unwrap_or_default(),
            text_he: text(a.get("full_he")).unwrap_or_default(),
            regions: strings(a.get("regions")),
            groups: strings(a.get("groups")),
        });
    }
    out.sort_by_key(|w| (w.valid_from, std::cmp::Reverse(w.severity_id)));
    Ok(out)
}

fn base_url() -> String {
    crate::non_empty_env("WEATHER_IMS_BASE_URL")
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn cache_file(endpoint: &str) -> PathBuf {
    crate::cache_dir().join(format!("israel-weather-rs-{endpoint}.json"))
}

fn download(endpoint: &str) -> Result<String, Error> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .build()?;
    let url = format!("{}/{endpoint}", base_url());
    Ok(client.get(url).send()?.error_for_status()?.text()?)
}

/// Fetch an IMS JSON endpoint and parse it. A successfully parsed response is saved to the
/// cache directory. When `offline`, or when downloading or parsing fails, the saved copy is used.
pub fn fetch<T>(
    endpoint: &str,
    offline: bool,
    parse: impl Fn(&str) -> Result<T, Error>,
) -> Result<T, Error> {
    let path = cache_file(endpoint);
    if !offline {
        match download(endpoint).and_then(|body| Ok((parse(&body)?, body))) {
            Ok((parsed, body)) => {
                if let Err(e) = std::fs::write(&path, body) {
                    warn!("could not cache {endpoint} at {}: {e}", path.display());
                }
                return Ok(parsed);
            }
            Err(e) => warn!("fetching {endpoint} failed, trying cached copy: {e}"),
        }
    }
    parse(&std::fs::read_to_string(&path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    const FULL: &str = include_str!("../tests/fixtures/ims_full_forecast_data.json");
    const WARNINGS_JSON: &str = include_str!("../tests/fixtures/ims_warnings.json");
    const METADATA: &str = include_str!("../tests/fixtures/ims_warnings_metadata.json");
    const LOCATIONS: &str = include_str!("../tests/fixtures/ims_locations_info.json");

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().into()
    }

    #[test]
    fn lenient_numbers() {
        assert_eq!(num(Some(&serde_json::json!("25.3"))), Some(25.3));
        assert_eq!(num(Some(&serde_json::json!(8))), Some(8.0));
        assert_eq!(num(Some(&serde_json::json!(""))), None);
        assert_eq!(num(Some(&serde_json::json!(null))), None);
        assert_eq!(num(None), None);
    }

    #[test]
    fn full_forecast_is_hourly() {
        let f = parse_full_forecast(FULL).unwrap();
        assert_eq!(f.locations.len(), 6);
        let tlv = &f.locations[&2];
        // Hourly and contiguous.
        assert!(tlv.hourly.len() > 140);
        for w in tlv.hourly.windows(2) {
            assert_eq!((w[1].time - w[0].time).num_hours(), 1);
        }
        let first = &tlv.hourly[0];
        assert_eq!(first.time.to_rfc3339(), "2026-09-30T23:00:00+03:00");
        assert!(first.rain_chance.is_some());
        assert!(first.wind_gust.is_some());
        assert!(
            tlv.daily_for(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap())
                .is_some()
        );
        assert!(!f.country.is_empty());
    }

    #[test]
    fn full_forecast_matches_known_values() {
        let f = parse_full_forecast(FULL).unwrap();
        // Tel Aviv Coast: 0.77 mm at 11:00 on Oct 2, which the 6-hourly XML never shows.
        let row = f.locations[&2]
            .hourly
            .iter()
            .find(|r| r.time.to_rfc3339() == "2026-10-02T11:00:00+03:00")
            .unwrap();
        assert_eq!(row.rain, 0.77);
        // Wind direction ids map to compass degrees.
        assert!(
            f.locations[&2]
                .hourly
                .iter()
                .all(|r| r.wind_direction.is_some_and(|d| (0.0..360.0).contains(&d)))
        );
    }

    #[test]
    fn full_forecast_rejects_bad_shapes() {
        assert!(matches!(parse_full_forecast("{}"), Err(Error::Json(_))));
        assert!(matches!(
            parse_full_forecast(r#"{"data":{"info":{}}}"#),
            Err(Error::Json(_))
        ));
        assert!(matches!(
            parse_full_forecast("not json"),
            Err(Error::Json(_))
        ));
    }

    #[test]
    fn warnings_are_named_and_deduplicated() {
        let w = parse_warnings(WARNINGS_JSON, Some(METADATA)).unwrap();
        assert!(!w.is_empty());
        let ids: HashSet<_> = w.iter().map(|w| &w.alert_id).collect();
        assert_eq!(ids.len(), w.len());
        assert!(w.iter().all(|w| !w.severity.starts_with("Severity ")));
        assert!(w.iter().all(|w| w.valid_to > w.valid_from));
        assert!(w.iter().any(|w| w.warning_type == "Heat Stress"));
    }

    #[test]
    fn warnings_without_metadata_still_parse() {
        let w = parse_warnings(WARNINGS_JSON, None).unwrap();
        assert!(
            w.iter()
                .all(|w| w.warning_type.starts_with("Warning type "))
        );
    }

    #[test]
    fn warnings_match_regions_for_general_public() {
        let w = parse_warnings(WARNINGS_JSON, Some(METADATA)).unwrap();
        let regions = parse_location_regions(LOCATIONS).unwrap();
        // Tel Aviv - Yafo is coastal: its sea region gets the high sea swimming warning.
        let tlv: Vec<_> = w.iter().filter(|w| w.applies_to(&regions[&84])).collect();
        assert!(tlv.iter().any(|w| w.warning_type.contains("Sea")));
        // Aviation-only warnings (aircraft icing) never apply.
        assert!(tlv.iter().all(|w| w.groups.iter().any(|g| g == "18")));
        let now = utc("2026-09-30T18:45:00Z");
        assert!(w.iter().any(|w| w.is_current_or_upcoming(now)));
        assert!(
            !w.iter()
                .any(|w| w.is_current_or_upcoming(utc("2030-01-01T00:00:00Z")))
        );
    }

    #[test]
    fn location_regions() {
        let r = parse_location_regions(LOCATIONS).unwrap();
        assert_eq!(r.len(), 175);
        assert_eq!(r[&84].rid.as_deref(), Some("103"));
        assert_eq!(r[&84].sea_rid.as_deref(), Some("55"));
        assert_eq!(r[&1].sea_rid, None);
    }
}
