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
