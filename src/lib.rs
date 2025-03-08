use std::path::PathBuf;
use tracing::{error, instrument, trace, warn, Level};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

use cached_path::Cache;
use chrono::{DateTime, LocalResult, NaiveDateTime, Utc};
use serde_xml_rs::from_str;

pub mod ims_structs;

static WEATHER_URL: &str =
    "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";

fn init_logging() {
    SubscriberBuilder::default()
        .with_writer(std::io::stderr)
        .with_span_events(FmtSpan::CLOSE)
        .json()
        .with_max_level(Level::WARN) // Set the default log level to WARN
        .finish();
}

fn make_cache(offline: bool) -> PathBuf {
    init_logging();
    trace!("build cache {}", offline);

    let cache = Cache::builder()
        .dir(std::env::temp_dir().join("weather/"))
        .connect_timeout(std::time::Duration::from_secs(60))
        .timeout(std::time::Duration::from_secs(60))
        .offline(offline)
        .build()
        .expect("unable to start download cache");

    let result = cache.cached_path(WEATHER_URL);

    if result.is_err() && !offline {
        trace!("cache error");
        warn!("{}", result.unwrap_err());
        return make_cache(true);
    }

    return result.expect("cache creation failed");
}

#[instrument]
pub fn get_israeli_weather_forecast(
    offline: bool,
) -> Result<ims_structs::LocationForecasts, serde_xml_rs::Error> {
    let xml_path = make_cache(offline);

    trace!("{}", xml_path.display());

    let forecast_xml = std::fs::read_to_string(xml_path).expect("failed to read forecast xml");

    let forecasts: Result<ims_structs::LocationForecasts, serde_xml_rs::Error> =
        from_str(&forecast_xml);

    if let Ok(mut forecasts) = forecasts {
        trace!("forecast parsed successfully");
        transform_forecast_times_to_datetimes(&mut forecasts);
        transform_weather_code_to_english(&mut forecasts);
        return Ok(forecasts);
    } else if let Err(forecasts) = forecasts {
        trace!(
            "forecast not parsed successfully: {}",
            forecasts.to_string()
        );
        notify_error(&forecasts, &forecast_xml);
        return Err(forecasts);
    } else {
        panic!()
    }
}

fn notify_error(e: &serde_xml_rs::Error, xml: &String) {
    error!("failed to parse xml because: {:?}", e);
    let head_of_xml = xml.lines().take(50);
    error!("head of xml is:");
    for line in head_of_xml {
        error!("{}", line);
    }
    error!("...");
}

#[instrument]
fn parse_time(time: &str) -> Result<DateTime<Utc>, LocalResult<i8>> {
    let possible_time = NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S")
        .expect("failed to parse forecast time")
        .and_local_timezone(Utc)
        .latest();
    if let Some(time) = possible_time {
        return Ok(time);
    } else {
        return Err(LocalResult::None);
    }
}

#[instrument]
fn transform_forecast_times_to_datetimes(forecast: &mut ims_structs::LocationForecasts) {
    forecast.location.iter_mut().for_each(|location| {
        location
            .location_data
            .forecast
            .iter_mut()
            .for_each(|forecast| {
                forecast.forecast_time = parse_time(&forecast.forecast_time)
                    .expect("failed to parse forecast time")
                    .to_rfc3339();
            });
    });
}

#[instrument]
fn transform_weather_code_to_english(forecast: &mut ims_structs::LocationForecasts) {
    forecast.location.iter_mut().for_each(|location| {
        location
            .location_data
            .forecast
            .iter_mut()
            .for_each(|forecast| {
                forecast.weather_code_english = Some(match forecast.weather_code {
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
                })
                .to_owned()
                .map(|s| s.to_string());
            });
    });
}

pub fn find_location<'a>(
    search_for: &String,
    weather_data: &'a ims_structs::LocationForecasts,
) -> &'a ims_structs::Location {
    weather_data
        .location
        .iter()
        .find(|location| &location.location_meta_data.location_name_eng == search_for)
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
            match chrono::DateTime::parse_from_rfc3339(&forecast.forecast_time) {
                Ok(time) => time > now,
                Err(e) => {
                    error!("failed to parse forecast datetime with rfc3339: {} because {}", forecast.forecast_time, e);
                    match chrono::NaiveDateTime::parse_from_str(&forecast.forecast_time, "%Y-%m-%d %H:%M:%S") {
                        Ok(time) => time.and_utc() > now,
                        Err(e) => {
                            error!("failed to parse naive utc forecast datetime: {} because {}", forecast.forecast_time, e);
                            return false;
                        }
                    }
                }
            }
        })
        .take((next / 6) as usize)
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
        super::transform_weather_code_to_english(&mut forecasts);
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
        super::transform_forecast_times_to_datetimes(&mut forecasts);
        let time = &forecasts.location[0].location_data.forecast[0].forecast_time;
        assert_eq!(time, "2025-03-07T02:00:00+00:00");
    }

    #[test]
    fn make_cache_online() {
        let xml = super::make_cache(false);
        assert_eq!(xml.is_file(), true);
    }

    #[test]
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
    fn test_get_israeli_weather_forecast() {
        let forecast = super::get_israeli_weather_forecast(true);
        assert_eq!(forecast.is_ok(), true);
    }

    #[test]
    fn test_find_location() {
        let xml = std::fs::read_to_string("./isr_cities_1week_6hr_forecast.xml").unwrap();
        let forecasts: ims_structs::LocationForecasts = serde_xml_rs::from_str(&xml).unwrap();
        let location = super::find_location(&"Tel Aviv Coast".to_string(), &forecasts);
        assert_eq!(
            location.location_meta_data.location_name_eng,
            "Tel Aviv Coast"
        );
    }

    #[test]
    fn test_forecasts_for_location_for_next_n_hours() {
        let xml = std::fs::read_to_string("./isr_cities_1week_6hr_forecast.xml").unwrap();
        let forecasts: ims_structs::LocationForecasts = serde_xml_rs::from_str(&xml).unwrap();

        let location = super::find_location(&"Tel Aviv Coast".to_string(), &forecasts);
        let next_forecasts = super::forecasts_for_location_for_next_n_hours(
            24,
            location,
            chrono::DateTime::parse_from_rfc3339("2025-03-08T23:22:00+00:00")
                .unwrap()
                .into(),
        );
        assert_eq!(next_forecasts.len(), 4);
        assert_eq!(next_forecasts[0].forecast_time, "2025-03-09 02:00:00");
    }
}
