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
    pub min_temp: i16,
    pub max_temp: i16,
    pub uv_index: Option<i16>,
    pub uv_index_max: Option<i16>,
}

static WEATHER_URL: &str = "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";

pub fn get_israeli_weather_forecast() -> Result<LocationForecasts, serde_xml_rs::Error> {
    let forecast_xml = ureq::get(WEATHER_URL)
        .call()
        .timeout_read(180_0000)
        .expect("failed to fetch forecast")
        .into_string()
        .expect("invalid xml");

    let forecasts: Result<LocationForecasts, serde_xml_rs::Error> = from_str(&forecast_xml);

    return forecasts;
}
