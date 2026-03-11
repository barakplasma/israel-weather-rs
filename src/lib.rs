use std::path::PathBuf;
use tracing::{error, instrument, trace, warn, Level};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

use cached_path::Cache;
use chrono::{DateTime, LocalResult, NaiveDateTime, Utc};
use serde_xml_rs::from_str;

pub mod ims_structs;

static DEFAULT_WEATHER_URL: &str =
    "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";

fn weather_url() -> String {
    std::env::var("WEATHER_URL").unwrap_or_else(|_| DEFAULT_WEATHER_URL.to_string())
}

fn cache_dir() -> std::path::PathBuf {
    std::env::var("WEATHER_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

fn init_logging() {
    let _ = SubscriberBuilder::default()
        .with_writer(std::io::stderr)
        .with_span_events(FmtSpan::CLOSE)
        .json()
        .with_max_level(Level::WARN)
        .try_init();
}

fn make_cache(offline: bool) -> PathBuf {
    trace!("build cache offline={}", offline);
    let url = weather_url();
    let dir = cache_dir();

    let cache = Cache::builder()
        .dir(dir)
        .connect_timeout(std::time::Duration::from_secs(60))
        .timeout(std::time::Duration::from_secs(60))
        .offline(offline)
        .build()
        .expect("unable to start download cache");

    match cache.cached_path(&url) {
        Ok(path) => path,
        Err(e) if !offline => {
            warn!("Download failed, falling back to cached data: {}", e);
            Cache::builder()
                .dir(cache_dir())
                .offline(true)
                .build()
                .expect("unable to start offline cache")
                .cached_path(&url)
                .expect("cache creation failed - no previously cached data available")
        }
        Err(e) => panic!("cache creation failed: {}", e),
    }
}

#[instrument]
pub fn get_israeli_weather_forecast(
    offline: bool,
) -> Result<ims_structs::LocationForecasts, serde_xml_rs::Error> {
    init_logging();
    let xml_path = make_cache(offline);

    trace!("{}", xml_path.display());

    let forecast_xml = std::fs::read_to_string(xml_path).expect("failed to read forecast xml");

    match from_str(&forecast_xml) {
        Ok(mut forecasts) => {
            trace!("forecast parsed successfully");
            transform_forecasts(&mut forecasts);
            Ok(forecasts)
        }
        Err(e) => {
            trace!("forecast not parsed successfully: {}", e);
            notify_error(&e, &forecast_xml);
            Err(e)
        }
    }
}

fn notify_error(e: &serde_xml_rs::Error, xml: &str) {
    let head = xml.lines().take(50).collect::<Vec<_>>().join("\n");
    error!("failed to parse xml because: {:?}\nhead of xml:\n{}\n...", e, head);
}

#[instrument]
fn parse_time(time: &str) -> Result<DateTime<Utc>, LocalResult<i8>> {
    let possible_time = NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S")
        .expect("failed to parse forecast time")
        .and_local_timezone(Utc)
        .latest();
    match possible_time {
        Some(time) => Ok(time),
        None => Err(LocalResult::None),
    }
}

fn weather_code_to_str(code: i32) -> &'static str {
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

#[instrument]
fn transform_forecasts(forecasts: &mut ims_structs::LocationForecasts) {
    for location in forecasts.location.iter_mut() {
        for forecast in location.location_data.forecast.iter_mut() {
            forecast.forecast_time = parse_time(&forecast.forecast_time)
                .expect("failed to parse forecast time")
                .to_rfc3339();
            forecast.weather_code_english =
                Some(weather_code_to_str(forecast.weather_code).to_string());
        }
    }
}

pub fn find_location<'a>(
    search_for: &str,
    weather_data: &'a ims_structs::LocationForecasts,
) -> &'a ims_structs::Location {
    weather_data
        .location
        .iter()
        .find(|location| location.location_meta_data.location_name_eng == search_for)
        .expect("failed to find location specified")
}

pub fn forecasts_for_location_for_next_n_hours(
    next: u8,
    desired_location: &ims_structs::Location,
    now: chrono::DateTime<Utc>,
) -> Vec<&ims_structs::Forecast> {
    desired_location
        .location_data
        .forecast
        .iter()
        .filter(|forecast| {
            chrono::DateTime::parse_from_rfc3339(&forecast.forecast_time)
                .map(|time| time > now)
                .unwrap_or(false)
        })
        .take(next.div_ceil(6) as usize)
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::ims_structs;

    #[test]
    fn parse_time() {
        let time = "2023-01-31 02:00:00";
        assert_eq!(
            super::parse_time(time)
                .expect("failed to parse")
                .to_rfc3339(),
            "2023-01-31T02:00:00+00:00"
        );
    }

    #[test]
    fn transform_weather_code() {
        let xml = std::fs::read_to_string("./isr_cities_1week_6hr_forecast.xml").unwrap();
        let mut forecasts: ims_structs::LocationForecasts = serde_xml_rs::from_str(&xml).unwrap();
        super::transform_forecasts(&mut forecasts);
        let english = forecasts.location[0].location_data.forecast[0]
            .weather_code_english
            .as_ref()
            .unwrap();
        assert_eq!(english, "Cloudy, possible rain");
    }

    #[test]
    fn transform_forecast_times_to_datetimes() {
        let xml = std::fs::read_to_string("./isr_cities_1week_6hr_forecast.xml").unwrap();
        let mut forecasts: ims_structs::LocationForecasts = serde_xml_rs::from_str(&xml).unwrap();
        super::transform_forecasts(&mut forecasts);
        let time = &forecasts.location[0].location_data.forecast[0].forecast_time;
        assert_eq!(time, "2025-03-07T02:00:00+00:00");
    }

    #[test]
    #[ignore = "requires network"]
    fn make_cache_online() {
        let xml = super::make_cache(false);
        assert_eq!(xml.is_file(), true);
    }

    #[test]
    #[ignore = "requires network for initial download"]
    fn make_cache_offline() {
        super::make_cache(false);
        let xml = super::make_cache(true);
        assert_eq!(xml.is_file(), true);
    }

    #[test]
    fn test_init_logging() {
        super::init_logging()
    }

    #[test]
    #[ignore = "requires network"]
    fn test_get_israeli_weather_forecast() {
        let forecast = super::get_israeli_weather_forecast(false);
        assert_eq!(forecast.is_ok(), true);
    }

    #[test]
    fn test_find_location() {
        let xml = std::fs::read_to_string("./isr_cities_1week_6hr_forecast.xml").unwrap();
        let forecasts: ims_structs::LocationForecasts = serde_xml_rs::from_str(&xml).unwrap();
        let location = super::find_location("Tel Aviv Coast", &forecasts);
        assert_eq!(
            location.location_meta_data.location_name_eng,
            "Tel Aviv Coast"
        );
    }

    #[test]
    fn test_forecasts_for_location_for_next_n_hours() {
        let xml = std::fs::read_to_string("./isr_cities_1week_6hr_forecast.xml").unwrap();
        let mut forecasts: ims_structs::LocationForecasts = serde_xml_rs::from_str(&xml).unwrap();
        super::transform_forecasts(&mut forecasts);

        let location = super::find_location("Tel Aviv Coast", &forecasts);
        let next_forecasts = super::forecasts_for_location_for_next_n_hours(
            24,
            location,
            chrono::DateTime::parse_from_rfc3339("2025-03-08T23:22:00+00:00")
                .unwrap()
                .into(),
        );
        assert_eq!(next_forecasts.len(), 4);
        assert_eq!(next_forecasts[0].forecast_time, "2025-03-09T02:00:00+00:00");
    }
}
