use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, instrument, trace, warn};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

use cached_path::Cache;
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Asia::Jerusalem;
use serde_xml_rs::from_str;

pub mod ims_structs;

static DEFAULT_WEATHER_URL: &str =
    "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";

/// Format IMS uses for `ForecastTime` (Israel local wall-clock time).
const IMS_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Each IMS forecast entry covers a 6 hour block.
const FORECAST_INTERVAL_HOURS: u8 = 6;

#[derive(Debug)]
pub enum Error {
    /// Downloading or reading the cached forecast failed.
    Cache(cached_path::Error),
    /// Reading the cached forecast file failed.
    Io(std::io::Error),
    /// The forecast XML could not be parsed.
    Xml(serde_xml_rs::Error),
    /// A `ForecastTime` value was not in the expected format.
    InvalidTime(String),
    /// No location matched the requested name.
    LocationNotFound {
        query: String,
        available: Vec<String>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cache(e) => write!(f, "failed to fetch forecast: {e}"),
            Error::Io(e) => write!(f, "failed to read cached forecast: {e}"),
            Error::Xml(e) => write!(f, "failed to parse forecast xml: {e}"),
            Error::InvalidTime(t) => write!(f, "invalid forecast time: {t:?}"),
            Error::LocationNotFound { query, available } => write!(
                f,
                "location {query:?} not found. Available locations: {}",
                available.join(", ")
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Cache(e) => Some(e),
            Error::Io(e) => Some(e),
            Error::Xml(e) => Some(e),
            Error::InvalidTime(_) | Error::LocationNotFound { .. } => None,
        }
    }
}

impl From<cached_path::Error> for Error {
    fn from(e: cached_path::Error) -> Self {
        Error::Cache(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_xml_rs::Error> for Error {
    fn from(e: serde_xml_rs::Error) -> Self {
        Error::Xml(e)
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn weather_url() -> String {
    non_empty_env("WEATHER_URL").unwrap_or_else(|| DEFAULT_WEATHER_URL.to_string())
}

fn cache_dir() -> PathBuf {
    non_empty_env("WEATHER_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Log JSON to stderr. Level defaults to `warn` and can be overridden with `RUST_LOG`.
pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(filter)
        .json()
        .try_init();
}

fn build_cache(offline: bool) -> Result<Cache, Error> {
    Ok(Cache::builder()
        .dir(cache_dir())
        .connect_timeout(Duration::from_secs(60))
        .timeout(Duration::from_secs(60))
        .offline(offline)
        .build()?)
}

fn make_cache(offline: bool) -> Result<PathBuf, Error> {
    trace!("build cache offline={}", offline);
    let url = weather_url();

    match build_cache(offline)?.cached_path(&url) {
        Ok(path) => Ok(path),
        Err(e) if !offline => {
            warn!("Download failed, falling back to cached data: {}", e);
            Ok(build_cache(true)?.cached_path(&url)?)
        }
        Err(e) => Err(e.into()),
    }
}

/// Download (or reuse the cached copy of) the IMS forecast and parse it.
#[instrument]
pub fn get_israeli_weather_forecast(
    offline: bool,
) -> Result<ims_structs::LocationForecasts, Error> {
    let xml_path = make_cache(offline)?;
    trace!("{}", xml_path.display());
    let forecast_xml = std::fs::read_to_string(xml_path)?;
    parse_forecast(&forecast_xml)
}

/// Parse IMS forecast XML, normalising times to RFC 3339 and adding English weather descriptions.
pub fn parse_forecast(xml: &str) -> Result<ims_structs::LocationForecasts, Error> {
    let mut forecasts = from_str(xml).inspect_err(|e| notify_error(e, xml))?;
    transform_forecasts(&mut forecasts)?;
    trace!("forecast parsed successfully");
    Ok(forecasts)
}

fn notify_error(e: &serde_xml_rs::Error, xml: &str) {
    let head = xml.lines().take(50).collect::<Vec<_>>().join("\n");
    error!(
        "failed to parse xml because: {:?}\nhead of xml:\n{}\n...",
        e, head
    );
}

/// IMS publishes forecast times as Israel local wall-clock time without an offset.
/// Interpret them in `Asia/Jerusalem` so they compare correctly against the current time.
fn parse_time(time: &str) -> Result<DateTime<FixedOffset>, Error> {
    let naive = NaiveDateTime::parse_from_str(time.trim(), IMS_TIME_FORMAT)
        .map_err(|_| Error::InvalidTime(time.to_string()))?;
    let local = Jerusalem
        .from_local_datetime(&naive)
        // Ambiguous (clocks going back): pick the first occurrence.
        .earliest()
        // Non-existent (clocks jumping forward, e.g. 02:00 on DST start):
        // interpret with the pre-transition offset, which lands just after the gap.
        .or_else(|| {
            Jerusalem
                .from_local_datetime(&(naive - chrono::Duration::hours(1)))
                .earliest()
                .map(|t| t + chrono::Duration::hours(1))
        })
        .ok_or_else(|| Error::InvalidTime(time.to_string()))?;
    Ok(local.fixed_offset())
}

pub fn weather_code_to_str(code: i32) -> &'static str {
    match code {
        1010 => "Sandstorms",
        1020 => "Thunderstorms",
        1060 => "Snow",
        1070 => "Light snow",
        1080 => "Sleet",
        1140 => "Rainy",
        1160 => "Fog",
        1220 => "Partly cloudy",
        1230 => "Cloudy",
        1250 => "Clear",
        1260 => "Windy",
        1270 => "Muggy",
        1300 => "Frost",
        1310 => "Hot",
        1320 => "Cold",
        1510 => "Stormy",
        1520 => "Heavy snow",
        1530 => "Partly cloudy possible rain",
        1540 => "Cloudy, possible rain",
        1560 => "Cloudy, light rain",
        1570 => "Dust",
        1580 => "Extremely hot",
        1590 => "Extremely cold",
        _ => "Unknown",
    }
}

fn transform_forecasts(forecasts: &mut ims_structs::LocationForecasts) -> Result<(), Error> {
    for location in forecasts.location.iter_mut() {
        for forecast in location.location_data.forecast.iter_mut() {
            forecast.forecast_time = parse_time(&forecast.forecast_time)?.to_rfc3339();
            forecast.weather_code_english =
                Some(weather_code_to_str(forecast.weather_code).to_string());
        }
    }
    Ok(())
}

/// All location names in the forecast, in the order IMS lists them.
pub fn location_names(weather_data: &ims_structs::LocationForecasts) -> Vec<&str> {
    weather_data
        .location
        .iter()
        .map(|l| l.location_meta_data.location_name_eng.as_str())
        .collect()
}

/// Find a location by English name (case-insensitive, surrounding whitespace ignored).
pub fn find_location<'a>(
    search_for: &str,
    weather_data: &'a ims_structs::LocationForecasts,
) -> Result<&'a ims_structs::Location, Error> {
    let wanted = search_for.trim();
    weather_data
        .location
        .iter()
        .find(|location| {
            location
                .location_meta_data
                .location_name_eng
                .trim()
                .eq_ignore_ascii_case(wanted)
        })
        .ok_or_else(|| Error::LocationNotFound {
            query: search_for.to_string(),
            available: location_names(weather_data)
                .into_iter()
                .map(String::from)
                .collect(),
        })
}

/// Forecasts starting after `now`, enough 6 hour blocks to cover the next `next` hours.
pub fn forecasts_for_location_for_next_n_hours(
    next: u8,
    desired_location: &ims_structs::Location,
    now: DateTime<Utc>,
) -> Vec<&ims_structs::Forecast> {
    desired_location
        .location_data
        .forecast
        .iter()
        .filter(|forecast| {
            DateTime::parse_from_rfc3339(&forecast.forecast_time)
                .map(|time| time > now)
                .unwrap_or(false)
        })
        .take(next.div_ceil(FORECAST_INTERVAL_HOURS) as usize)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../isr_cities_1week_6hr_forecast.xml");

    fn fixture() -> ims_structs::LocationForecasts {
        parse_forecast(FIXTURE).expect("fixture should parse")
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().into()
    }

    #[test]
    fn parse_time_winter_is_utc_plus_2() {
        assert_eq!(
            parse_time("2023-01-31 02:00:00").unwrap().to_rfc3339(),
            "2023-01-31T02:00:00+02:00"
        );
    }

    #[test]
    fn parse_time_summer_is_utc_plus_3() {
        assert_eq!(
            parse_time("2025-07-01 14:00:00").unwrap().to_rfc3339(),
            "2025-07-01T14:00:00+03:00"
        );
    }

    #[test]
    fn parse_time_during_spring_forward_gap() {
        // Israel DST 2025 starts Friday 2025-03-28 at 02:00 -> 03:00, so 02:00 does not exist.
        let t = parse_time("2025-03-28 02:00:00").unwrap();
        assert_eq!(t.with_timezone(&Utc), utc("2025-03-28T00:00:00Z"));
    }

    #[test]
    fn parse_time_during_fall_back_overlap() {
        // Israel DST 2025 ends Sunday 2025-10-26 at 02:00 -> 01:00, so 01:30 happens twice.
        let t = parse_time("2025-10-26 01:30:00").unwrap();
        assert_eq!(t.to_rfc3339(), "2025-10-26T01:30:00+03:00");
    }

    #[test]
    fn parse_time_rejects_garbage() {
        assert!(matches!(
            parse_time("not a time"),
            Err(Error::InvalidTime(_))
        ));
    }

    #[test]
    fn weather_codes() {
        assert_eq!(weather_code_to_str(1250), "Clear");
        assert_eq!(weather_code_to_str(1140), "Rainy");
        assert_eq!(weather_code_to_str(0), "Unknown");
    }

    #[test]
    fn transform_weather_code() {
        let forecasts = fixture();
        let english = forecasts.location[0].location_data.forecast[0]
            .weather_code_english
            .as_deref();
        assert_eq!(english, Some("Cloudy, possible rain"));
    }

    #[test]
    fn transform_forecast_times_to_israel_local_rfc3339() {
        let forecasts = fixture();
        let time = &forecasts.location[0].location_data.forecast[0].forecast_time;
        assert_eq!(time, "2025-03-07T02:00:00+02:00");
    }

    #[test]
    fn uv_index_is_parsed() {
        let forecasts = fixture();
        let uv_max = forecasts.location[0]
            .location_data
            .forecast
            .iter()
            .filter_map(|f| f.uv_index_max)
            .fold(0.0_f32, f32::max);
        assert!(uv_max > 0.0, "UVIndexMax should be read from the xml");
    }

    #[test]
    fn parse_forecast_rejects_invalid_xml() {
        assert!(matches!(parse_forecast("<nope>"), Err(Error::Xml(_))));
    }

    #[test]
    fn parse_forecast_rejects_invalid_time() {
        let xml = FIXTURE.replacen("2025-03-07 02:00:00", "yesterday", 1);
        assert!(matches!(parse_forecast(&xml), Err(Error::InvalidTime(_))));
    }

    #[test]
    fn test_init_logging_is_idempotent() {
        init_logging();
        init_logging();
    }

    #[test]
    fn test_find_location() {
        let forecasts = fixture();
        let location = find_location("Tel Aviv Coast", &forecasts).unwrap();
        assert_eq!(
            location.location_meta_data.location_name_eng,
            "Tel Aviv Coast"
        );
    }

    #[test]
    fn find_location_is_case_and_whitespace_insensitive() {
        let forecasts = fixture();
        let location = find_location("  tel aviv COAST ", &forecasts).unwrap();
        assert_eq!(
            location.location_meta_data.location_name_eng,
            "Tel Aviv Coast"
        );
    }

    #[test]
    fn find_location_not_found_lists_available() {
        let forecasts = fixture();
        let err = find_location("Atlantis", &forecasts).unwrap_err();
        match &err {
            Error::LocationNotFound { query, available } => {
                assert_eq!(query, "Atlantis");
                assert!(available.iter().any(|l| l == "Jerusalem"));
                assert_eq!(available.len(), forecasts.location.len());
            }
            other => panic!("unexpected error {other:?}"),
        }
        assert!(err.to_string().contains("Jerusalem"));
    }

    #[test]
    fn location_names_in_order() {
        let forecasts = fixture();
        let names = location_names(&forecasts);
        assert_eq!(names[0], "Jerusalem");
        assert!(names.contains(&"Tel Aviv Coast"));
    }

    #[test]
    fn test_forecasts_for_location_for_next_n_hours() {
        let forecasts = fixture();
        let location = find_location("Tel Aviv Coast", &forecasts).unwrap();
        // 2025-03-08 23:22 UTC is 2025-03-09 01:22 in Israel.
        let next =
            forecasts_for_location_for_next_n_hours(24, location, utc("2025-03-08T23:22:00+00:00"));
        assert_eq!(next.len(), 4);
        assert_eq!(next[0].forecast_time, "2025-03-09T02:00:00+02:00");
    }

    #[test]
    fn next_n_hours_respects_israel_offset() {
        let forecasts = fixture();
        let location = find_location("Tel Aviv Coast", &forecasts).unwrap();
        // 01:30 UTC is 03:30 in Israel, so the 02:00 local forecast is already in the past.
        let next =
            forecasts_for_location_for_next_n_hours(6, location, utc("2025-03-09T01:30:00Z"));
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].forecast_time, "2025-03-09T08:00:00+02:00");
    }

    #[test]
    fn next_n_hours_rounds_up_to_whole_blocks() {
        let forecasts = fixture();
        let location = find_location("Tel Aviv Coast", &forecasts).unwrap();
        let now = utc("2025-03-08T23:22:00Z");
        assert_eq!(
            forecasts_for_location_for_next_n_hours(0, location, now).len(),
            0
        );
        assert_eq!(
            forecasts_for_location_for_next_n_hours(1, location, now).len(),
            1
        );
        assert_eq!(
            forecasts_for_location_for_next_n_hours(7, location, now).len(),
            2
        );
    }

    #[test]
    fn next_n_hours_after_forecast_ends_is_empty() {
        let forecasts = fixture();
        let location = find_location("Tel Aviv Coast", &forecasts).unwrap();
        let next =
            forecasts_for_location_for_next_n_hours(24, location, utc("2030-01-01T00:00:00Z"));
        assert!(next.is_empty());
    }

    #[test]
    #[ignore = "requires network"]
    fn make_cache_online() {
        assert!(make_cache(false).unwrap().is_file());
    }

    #[test]
    #[ignore = "requires network for initial download"]
    fn make_cache_offline() {
        make_cache(false).unwrap();
        assert!(make_cache(true).unwrap().is_file());
    }

    #[test]
    #[ignore = "requires network"]
    fn test_get_israeli_weather_forecast() {
        assert!(get_israeli_weather_forecast(false).is_ok());
    }
}
