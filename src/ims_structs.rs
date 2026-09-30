use serde::{Deserialize, Deserializer, Serialize};

/// IMS sometimes sends empty elements (e.g. `<UVIndexMax/>`); treat those as `None`.
fn optional_f32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f32>, D::Error> {
    let raw: Option<String> = Option::deserialize(deserializer)?;
    match raw.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => v.parse().map(Some).map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationForecasts {
    pub location: Vec<Location>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Location {
    pub location_meta_data: LocationMetaData,
    pub location_data: LocationData,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationMetaData {
    pub location_id: i16,
    pub location_name_eng: String,
    pub display_lat: f32,
    pub display_lon: f32,
    pub display_height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LocationData {
    pub forecast: Vec<Forecast>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// IMS spells this `UVIndex`; keep serializing as `UvIndex` for backwards-compatible JSON.
    #[serde(
        rename(deserialize = "UVIndex"),
        default,
        deserialize_with = "optional_f32"
    )]
    pub uv_index: Option<f32>,
    #[serde(
        rename(deserialize = "UVIndexMax"),
        default,
        deserialize_with = "optional_f32"
    )]
    pub uv_index_max: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Uv {
        #[serde(rename = "UVIndex", default, deserialize_with = "optional_f32")]
        uv: Option<f32>,
    }

    fn parse(xml: &str) -> Option<f32> {
        serde_xml_rs::from_str::<Uv>(xml).unwrap().uv
    }

    #[test]
    fn optional_f32_handles_value_empty_and_missing() {
        assert_eq!(parse("<Uv><UVIndex>4</UVIndex></Uv>"), Some(4.0));
        assert_eq!(parse("<Uv><UVIndex/></Uv>"), None);
        assert_eq!(parse("<Uv></Uv>"), None);
    }

    #[test]
    fn optional_f32_rejects_garbage() {
        assert!(serde_xml_rs::from_str::<Uv>("<Uv><UVIndex>high</UVIndex></Uv>").is_err());
    }
}
