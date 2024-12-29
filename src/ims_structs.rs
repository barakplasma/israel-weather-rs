use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationForecasts {
    pub location: Vec<Location>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Location {
    pub location_meta_data: LocationMetaData,
    pub location_data: LocationData,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationMetaData {
    pub location_id: i16,
    pub location_name_eng: String,
    pub display_lat: f32,
    pub display_lon: f32,
    pub display_height: f32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationData {
    pub forecast: Vec<Forecast>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Forecast {
    pub forecast_time: String,
    pub temperature: f32,
    pub relative_humidity: f32,
    pub wind_speed: f32,
    pub rain: f32,
    pub wind_direction: f32,
    pub dew_point_temp: f32,
    pub heat_stress: f32,
    pub heat_stress_level: f32,
    pub feels_like: f32,
    pub wind_chill: f32,
    pub weather_code: i32,
    pub weather_code_english: Option<String>,
    pub min_temp: f32,
    pub max_temp: f32,
    pub uv_index: Option<f32>,
    pub uv_index_max: Option<f32>,
}
