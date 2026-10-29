//! `/api/ims`: the IMS-only data neither Open-Meteo nor MET Norway has a place for.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::geocoding::Place;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

/// The written forecast for the whole country.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CountryForecast {
    pub date: NaiveDate,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extras {
    /// The IMS location the coordinates resolved to.
    pub location: Place,
    pub distance_km: f64,
    /// Warnings in force now or later for this location's region.
    pub warnings: Vec<Warning>,
    /// Today's, when IMS has published it.
    pub country_forecast: Option<CountryForecast>,
}

/// IMS weather code -> WMO 4677 code, restricted to the subset Open-Meteo emits
/// (0-3, 45, 48, 51-67, 71-77, 80-86, 95-99) so existing clients recognise every value.
/// Non-sky conditions (hot, windy, frost, ...) map to the closest sky state.
pub fn wmo_code(ims: i32) -> u8 {
    match ims {
        1250 | 1300 | 1310 | 1580 => 0, // clear, frost, hot, extremely hot
        1260 | 1270 | 1320 | 1590 => 1, // windy, muggy, cold, extremely cold
        1220 => 2,                      // partly cloudy
        1230 | 1010 | 1570 => 3,        // cloudy, sandstorm, dust
        1160 => 45,                     // fog
        1560 => 61,                     // cloudy, light rain
        1140 => 63,                     // rain
        1510 => 65,                     // stormy
        1080 => 66,                     // sleet
        1070 => 71,                     // light snow
        1060 => 73,                     // snow
        1520 => 75,                     // heavy snow
        1530 | 1540 => 80,              // possible rain -> slight showers
        1020 => 95,                     // thunderstorms
        _ => 3,
    }
}

/// English name of an IMS weather code (see `CodesForIMSWebSite_1.pdf`).
pub fn label(code: i32) -> &'static str {
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
