use std::path::PathBuf;
use log::{trace, warn};

use cached_path::Cache;
use chrono::{DateTime, LocalResult, NaiveDateTime, Utc};
use serde_xml_rs::from_str;

mod ims_structs;

static WEATHER_URL: &str =
    "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";

fn make_cache(offline: bool) -> PathBuf {
    trace!("build cache {}", offline);
    let cache = Cache::builder()
        .dir(std::env::temp_dir().join("weather/"))
        .connect_timeout(std::time::Duration::from_secs(60))
        .timeout(std::time::Duration::from_secs(60))
        .offline(offline)
        .build()
        .expect("unable to start download cache");

    let result = cache.cached_path(WEATHER_URL);

    if result.is_err() {
        trace!("cache error");
        warn!("{}", result.unwrap_err());
        return make_cache(true);
    }

    return result.expect("cache creation failed");
}

pub fn get_israeli_weather_forecast(
    offline: bool,
) -> Result<ims_structs::LocationForecasts, serde_xml_rs::Error> {
    let xml_path = make_cache(offline);

    trace!("{}", xml_path.display());

    let forecast_xml = std::fs::read_to_string(xml_path).expect("failed to read forecast xml");

    let forecasts: Result<ims_structs::LocationForecasts, serde_xml_rs::Error> =
        from_str(&forecast_xml);

    if let Ok(mut forecasts) = forecasts {
        transform_forecast_times_to_datetimes(&mut forecasts);
        transform_weather_code_to_english(&mut forecasts);
        return Ok(forecasts);
    } else if let Err(forecasts) = forecasts {
        notify_error(&forecasts, &forecast_xml);
        return Err(forecasts);
    } else {
        panic!()
    }
}

fn notify_error(e: &serde_xml_rs::Error, xml: &String) {
    eprintln!("failed to parse xml because: {:?}", e);
    let head_of_xml = xml.lines().take(50);
    eprintln!("head of xml is:");
    for line in head_of_xml {
        eprintln!("{}", line);
    }
    eprintln!("...");
}

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
        assert_eq!(english, "Fog");
    }

    #[test]
    fn transform_forecast_times_to_datetimes() {
        let xml = std::fs::read_to_string("./isr_cities_1week_6hr_forecast.xml").unwrap();
        let mut forecasts: ims_structs::LocationForecasts = serde_xml_rs::from_str(&xml).unwrap();
        super::transform_forecast_times_to_datetimes(&mut forecasts);
        let time = &forecasts.location[0].location_data.forecast[0].forecast_time;
        assert_eq!(time, "2024-12-28T02:00:00+00:00");
    }
}
