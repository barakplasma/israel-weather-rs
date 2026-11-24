//! Partial typed IMS website contracts. Validate without reserializing native responses.
//! Unknown fields, numeric strings, nulls and sentinels survive in the original JSON.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum Scalar {
    Number(f64),
    Text(String),
}

impl Scalar {
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(value) => value.trim().parse().ok(),
        }
        .filter(|value| value.is_finite())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CurrentAnalysis {
    pub lid: Option<Scalar>,
    pub temperature: Option<Scalar>,
    pub relative_humidity: Option<Scalar>,
    pub rain: Option<Scalar>,
    pub rain_chance: Option<Scalar>,
    pub wind_speed: Option<Scalar>,
    pub gust_speed: Option<Scalar>,
    pub wind_direction_id: Option<Scalar>,
    pub wind_chill: Option<Scalar>,
    pub feels_like: Option<Scalar>,
    pub heat_stress_level: Option<Scalar>,
    pub u_v_index: Option<Scalar>,
    pub u_v_i_max: Option<Scalar>,
    pub u_v_i_factor: Option<Scalar>,
    pub wave_height: Option<Scalar>,
    pub min_temp: Option<Scalar>,
    pub max_temp: Option<Scalar>,
    pub pm10: Option<Scalar>,
    pub weather_code: Option<Scalar>,
    pub forecast_time: Option<String>,
    pub modified: Option<String>,
    pub u_v_level: Option<String>,
    #[serde(rename = "due_point_Temp")]
    pub dew_point_temp: Option<Scalar>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HourlyForecast {
    pub lid: Option<Scalar>,
    pub temperature: Option<Scalar>,
    pub precise_temperature: Option<Scalar>,
    pub weather_code: Option<Scalar>,
    pub heat_stress: Option<Scalar>,
    pub heat_stress_level: Option<Scalar>,
    pub pm10: Option<Scalar>,
    pub relative_humidity: Option<Scalar>,
    pub rain: Option<Scalar>,
    pub rain_chance: Option<Scalar>,
    pub wind_speed: Option<Scalar>,
    pub gust_speed: Option<Scalar>,
    pub wind_direction_id: Option<Scalar>,
    pub wave_height: Option<Scalar>,
    pub wind_chill: Option<Scalar>,
    pub u_v_index: Option<Scalar>,
    pub u_v_i_max: Option<Scalar>,
    pub hour: Option<Scalar>,
    pub forecast_time: Option<String>,
    pub created: Option<String>,
    pub day: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DailyForecast {
    pub lid: Option<Scalar>,
    pub weather_code: Option<Scalar>,
    pub minimum_temperature: Option<Scalar>,
    pub maximum_temperature: Option<Scalar>,
    pub maximum_uvi: Option<Scalar>,
    pub u_v_i_factor: Option<Scalar>,
    pub forecast_date: Option<String>,
    pub created: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CountryDescription {
    pub forecast_day: Option<String>,
    pub created: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LocationInfo {
    pub lid: Option<Scalar>,
    pub lat: Option<Scalar>,
    pub lon: Option<Scalar>,
    pub rid: Option<Scalar>,
    pub sea_rid: Option<Scalar>,
    pub height: Option<Scalar>,
    pub zoom: Option<Scalar>,
    pub is_park: Option<Scalar>,
    pub near_coast: Option<Scalar>,
    pub is_main_coast_city: Option<Scalar>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WeatherCode {
    pub weather_code: Option<Scalar>,
    pub desc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WindDirection {
    pub direction: Option<Scalar>,
    pub desc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegionInfo {
    pub rid: Option<Scalar>,
    pub is_sea: Option<Scalar>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningRecord {
    pub wid: Option<Scalar>,
    pub alert_id: Option<Scalar>,
    pub severity_id: Option<Scalar>,
    pub warning_type_id: Option<Scalar>,
    pub valid_from_unix: Option<Scalar>,
    pub sent: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub full_en: Option<String>,
    pub full_he: Option<String>,
    pub text: Option<String>,
    pub text_full: Option<String>,
    pub groups: Option<Vec<Scalar>>,
    pub regions: Option<Vec<Scalar>>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningType {
    pub warning_type_id: Option<Scalar>,
    pub for_app: Option<Scalar>,
    pub name: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningGroup {
    pub id: Option<Scalar>,
    pub gid: Option<Scalar>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningSeverity {
    pub severity_id: Option<Scalar>,
    pub color: Option<String>,
    pub severity_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ForecastDay {
    pub daily: Option<DailyForecast>,
    pub hourly: Option<BTreeMap<String, HourlyForecast>>,
    pub country: Option<CountryDescription>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RadarFrame {
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SatelliteCategories {
    #[serde(rename = "NATURAL")]
    pub natural: Option<Vec<RadarFrame>>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RadarCategories {
    #[serde(rename = "IMSRadar")]
    pub ims_radar: Option<Vec<RadarFrame>>,
    pub radar: Option<Vec<RadarFrame>>,
    pub satellite: Option<SatelliteCategories>,
    #[serde(rename = "EUROPE")]
    pub europe: Option<Vec<RadarFrame>>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RadarData {
    pub types: RadarCategories,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningData {
    pub full_warnings_data: Option<WarningDays>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningMetadataData {
    pub ims_warning_type: BTreeMap<String, WarningType>,
    pub warning_groups: BTreeMap<String, WarningGroup>,
    pub warning_severity: BTreeMap<String, WarningSeverity>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum WarningDays {
    ByDate(BTreeMap<String, BTreeMap<String, BTreeMap<String, WarningRecord>>>),
    EmptyArray(Vec<WarningRecord>),
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CurrentResponse {
    pub data: BTreeMap<String, CurrentAnalysis>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ForecastResponse {
    pub data: BTreeMap<String, ForecastDay>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LocationsResponse {
    pub data: BTreeMap<String, LocationInfo>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningsResponse {
    pub data: WarningData,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WarningMetadataResponse {
    pub data: WarningMetadataData,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WeatherCodesResponse {
    pub data: BTreeMap<String, WeatherCode>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WindDirectionsResponse {
    pub data: BTreeMap<String, WindDirection>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegionsResponse {
    pub data: Vec<RegionInfo>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SeaRegionsResponse {
    pub data: BTreeMap<String, RegionInfo>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RadarResponse {
    pub data: RadarData,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_handles_wire_variants() {
        let s: Scalar = serde_json::from_str("\"22.5\"").unwrap();
        assert_eq!(s.number(), Some(22.5));
        let s: Scalar = serde_json::from_str("\"\"").unwrap();
        assert_eq!(s.number(), None);
        let s: Scalar = serde_json::from_str("-999").unwrap();
        assert_eq!(s.number(), Some(-999.0));
        assert!(serde_json::from_str::<Scalar>("{}").is_err());
    }
}
