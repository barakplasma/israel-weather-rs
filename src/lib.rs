use chrono::{DateTime, Utc, NaiveDateTime, LocalResult};
use serde::{Deserialize, Serialize};
use serde_xml_rs::{from_str};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationForecasts {
    pub location: Vec<Location>
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Location {
    pub location_meta_data: LocationMetaData,
    pub location_data: LocationData
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationMetaData {
    pub location_id: i16,
    pub location_name_eng: String,
    pub display_lat: f32,
    pub display_lon: f32,
    pub display_height: i16,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationData {
    pub forecast: Vec<Forecast>
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Forecast {
    pub forecast_time: String,
    pub temperature: i16,
    pub relative_humidity: i16,
    pub wind_speed: i16,
    pub rain: f32,
    pub wind_direction: i16,
    pub dew_point_temp: i16,
    pub heat_stress: i16,
    pub heat_stress_level: i16,
    pub feels_like: i16,
    pub wind_chill: i16,
    pub weather_code: i32,
    pub weather_code_english: Option<String>,
    pub min_temp: i16,
    pub max_temp: i16,
    pub uv_index: Option<i16>,
    pub uv_index_max: Option<i16>,
}

static WEATHER_URL: &str = "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";

pub fn get_israeli_weather_forecast() -> Result<LocationForecasts, i8> {
    use cached_path::cached_path;

    let path = cached_path(
        WEATHER_URL
    ).unwrap();

    let forecast_xml = std::fs::read_to_string(path).expect("failed to read forecast xml");

    let forecasts: Result<LocationForecasts, serde_xml_rs::Error> = from_str(&forecast_xml);

    if let Ok(mut forecasts) = forecasts {
        transform_forecast_times_to_datetimes(&mut forecasts);
        transform_weather_code_to_english(&mut forecasts);
        return Ok(forecasts);
    } else {
        return Err(0);
    }
}

fn parse_time(time: &str) -> Result<DateTime<Utc>, LocalResult<i8>> {
    let possible_time = NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S").expect("failed to parse forecast time").and_local_timezone(Utc).latest();
    if let Some(time) = possible_time {
        return Ok(time);
    } else {
        return Err(LocalResult::None);
    }
}

fn transform_forecast_times_to_datetimes(forecast: &mut LocationForecasts) {
    forecast.location.iter_mut().for_each(|location| {
        location.location_data.forecast.iter_mut().for_each(|forecast| {
            forecast.forecast_time = parse_time(&forecast.forecast_time).expect("failed to parse forecast time").to_rfc3339();
        });
    });
}

fn transform_weather_code_to_english(forecast: &mut LocationForecasts) {
    forecast.location.iter_mut().for_each(|location| {
        location.location_data.forecast.iter_mut().for_each(|forecast| {
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
                _ => "Unknown"
            }).to_owned().map(|s| s.to_string());
        });
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_time() {
        let time = "2023-01-31 02:00:00";
        assert_eq!(super::parse_time(time).expect("failed to parse").to_rfc3339(), "2023-01-31T02:00:00+00:00");
    }
}
