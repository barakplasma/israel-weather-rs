use serde::{Deserialize, Serialize};
use serde_xml_rs::{from_str};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct LocationForecasts {
    location: Vec<Location>
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct Location {
    location_meta_data: LocationMetaData,
    location_data: LocationData
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct LocationMetaData {
    location_id: i16,
    location_name_eng: String,
    display_lat: f32,
    display_lon: f32,
    display_height: i16,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct LocationData {
    forecast: Vec<Forecast>
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct Forecast {
    forecast_time: String,
    temperature: i16,
    relative_humidity: i16,
    wind_speed: i16,
    rain: f32,
    wind_direction: i16,
    dew_point_temp: i16,
    heat_stress: i16,
    heat_stress_level: i16,
    feels_like: i16,
    wind_chill: i16,
    weather_code: i32,
    min_temp: i16,
    max_temp: i16,
    uv_index: Option<i16>,
    uv_index_max: Option<i16>,
}

fn main() {
    let weather_url = "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";
    let forecast_xml = ureq::get(weather_url)
        .call()
        .expect("failed to fetch forecast")
        .into_string()
        .expect("invalid xml");

    let forecasts: LocationForecasts = from_str(&forecast_xml).expect("failed to parse xml");

    forecasts.location.iter().for_each(|location| {
        print_location(location);
    });
}

fn print_location(location: &Location) {
    println!("");
    println!("Location: {}", location.location_meta_data.location_name_eng);
    println!("Average Temperature: {}", location.location_data.forecast.iter().map(|f| f.temperature).sum::<i16>() / location.location_data.forecast.len() as i16);
}