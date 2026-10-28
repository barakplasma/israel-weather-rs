//! Open-Meteo geocoding `/v1/search`, answered from the IMS forecast locations.

use serde::{Deserialize, Serialize};

/// One place. Field names and meanings follow Open-Meteo's geocoding API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// The IMS location id.
    pub id: i64,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: f64,
    /// GeoNames feature class; always `PPL` (populated place).
    pub feature_code: String,
    pub country_code: String,
    pub country: String,
    pub timezone: String,
}

/// Like Open-Meteo, `results` is left out entirely when nothing matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<Place>,
    pub generationtime_ms: f64,
}
