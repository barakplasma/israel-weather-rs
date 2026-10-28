//! Open-Meteo `/v1/forecast` request variables and response envelope.
//!
//! Response series are keyed by the variable name the client asked for (legacy aliases like
//! `weathercode` are echoed back as-is), so values live in maps rather than one field per
//! variable. Numbers are [`serde_json::Number`] to keep integers (`weather_code`, `%`)
//! serialized as integers, like Open-Meteo does.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Number;

/// Hourly / current variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HourlyVar {
    Temperature,
    RelativeHumidity,
    DewPoint,
    ApparentTemperature,
    Precipitation,
    Rain,
    Showers,
    Snowfall,
    WeatherCode,
    WindSpeed,
    WindDirection,
    UvIndex,
    IsDay,
    PrecipitationProbability,
    WindGusts,
    /// Extension, not in Open-Meteo: the IMS weather code, finer than WMO ("hot", "dust", ...).
    ImsWeatherCode,
}

impl HourlyVar {
    /// The current Open-Meteo name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Temperature => "temperature_2m",
            Self::RelativeHumidity => "relative_humidity_2m",
            Self::DewPoint => "dew_point_2m",
            Self::ApparentTemperature => "apparent_temperature",
            Self::Precipitation => "precipitation",
            Self::Rain => "rain",
            Self::Showers => "showers",
            Self::Snowfall => "snowfall",
            Self::WeatherCode => "weather_code",
            Self::WindSpeed => "wind_speed_10m",
            Self::WindDirection => "wind_direction_10m",
            Self::UvIndex => "uv_index",
            Self::IsDay => "is_day",
            Self::PrecipitationProbability => "precipitation_probability",
            Self::WindGusts => "wind_gusts_10m",
            Self::ImsWeatherCode => "ims_weather_code",
        }
    }

    /// Current names plus the pre-2023 spellings Open-Meteo still accepts.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "temperature_2m" => Self::Temperature,
            "relative_humidity_2m" | "relativehumidity_2m" => Self::RelativeHumidity,
            "dew_point_2m" | "dewpoint_2m" => Self::DewPoint,
            "apparent_temperature" => Self::ApparentTemperature,
            "precipitation" => Self::Precipitation,
            "rain" => Self::Rain,
            "showers" => Self::Showers,
            "snowfall" => Self::Snowfall,
            "weather_code" | "weathercode" => Self::WeatherCode,
            "wind_speed_10m" | "windspeed_10m" => Self::WindSpeed,
            "wind_direction_10m" | "winddirection_10m" => Self::WindDirection,
            "uv_index" => Self::UvIndex,
            "is_day" => Self::IsDay,
            "precipitation_probability" => Self::PrecipitationProbability,
            "wind_gusts_10m" | "windgusts_10m" => Self::WindGusts,
            "ims_weather_code" => Self::ImsWeatherCode,
            _ => return None,
        })
    }
}

/// Daily aggregates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DailyVar {
    TemperatureMax,
    TemperatureMin,
    ApparentMax,
    ApparentMin,
    PrecipitationSum,
    PrecipitationHours,
    WeatherCode,
    WindSpeedMax,
    WindDirectionDominant,
    UvIndexMax,
    PrecipitationProbabilityMax,
    WindGustsMax,
    /// Extension, not in Open-Meteo: see [`HourlyVar::ImsWeatherCode`].
    ImsWeatherCode,
}

impl DailyVar {
    pub fn name(self) -> &'static str {
        match self {
            Self::TemperatureMax => "temperature_2m_max",
            Self::TemperatureMin => "temperature_2m_min",
            Self::ApparentMax => "apparent_temperature_max",
            Self::ApparentMin => "apparent_temperature_min",
            Self::PrecipitationSum => "precipitation_sum",
            Self::PrecipitationHours => "precipitation_hours",
            Self::WeatherCode => "weather_code",
            Self::WindSpeedMax => "wind_speed_10m_max",
            Self::WindDirectionDominant => "wind_direction_10m_dominant",
            Self::UvIndexMax => "uv_index_max",
            Self::PrecipitationProbabilityMax => "precipitation_probability_max",
            Self::WindGustsMax => "wind_gusts_10m_max",
            Self::ImsWeatherCode => "ims_weather_code",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "temperature_2m_max" => Self::TemperatureMax,
            "temperature_2m_min" => Self::TemperatureMin,
            "apparent_temperature_max" => Self::ApparentMax,
            "apparent_temperature_min" => Self::ApparentMin,
            "precipitation_sum" | "rain_sum" => Self::PrecipitationSum,
            "precipitation_hours" => Self::PrecipitationHours,
            "weather_code" | "weathercode" => Self::WeatherCode,
            "wind_speed_10m_max" | "windspeed_10m_max" => Self::WindSpeedMax,
            "wind_direction_10m_dominant" | "winddirection_10m_dominant" => {
                Self::WindDirectionDominant
            }
            "uv_index_max" => Self::UvIndexMax,
            "precipitation_probability_max" => Self::PrecipitationProbabilityMax,
            "wind_gusts_10m_max" | "windgusts_10m_max" => Self::WindGustsMax,
            "ims_weather_code" => Self::ImsWeatherCode,
            _ => return None,
        })
    }
}

/// A typed `/v1/forecast` request, for clients. Builds the query string.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForecastQuery {
    pub latitude: f64,
    pub longitude: f64,
    pub hourly: Vec<HourlyVar>,
    pub daily: Vec<DailyVar>,
    pub current: Vec<HourlyVar>,
    /// IANA name, `auto` or `GMT`.
    pub timezone: Option<String>,
    pub forecast_days: Option<u32>,
}

impl ForecastQuery {
    pub fn to_query(&self) -> String {
        let list = |names: Vec<&str>| names.join(",");
        let mut q = format!("latitude={}&longitude={}", self.latitude, self.longitude);
        for (key, names) in [
            (
                "hourly",
                self.hourly.iter().map(|v| v.name()).collect::<Vec<_>>(),
            ),
            ("daily", self.daily.iter().map(|v| v.name()).collect()),
            ("current", self.current.iter().map(|v| v.name()).collect()),
        ] {
            if !names.is_empty() {
                q += &format!("&{key}={}", list(names));
            }
        }
        if let Some(tz) = &self.timezone {
            q += &format!("&timezone={tz}");
        }
        if let Some(d) = self.forecast_days {
            q += &format!("&forecast_days={d}");
        }
        q
    }
}

/// A timestamp as `timeformat` asked for: `2026-09-30T21:00` / `2026-09-30`, or unix seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Time {
    Unix(i64),
    Iso(String),
}

/// `variable name -> unit`, including `time`.
pub type Units = BTreeMap<String, String>;

/// Hourly or daily series: one `time` array plus one value array per requested variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub time: Vec<Time>,
    #[serde(flatten)]
    pub values: BTreeMap<String, Vec<Option<Number>>>,
}

impl Series {
    /// Values of one variable as `f64`, `None` where missing; empty if it was not requested.
    pub fn f64s(&self, name: &str) -> Vec<Option<f64>> {
        self.values
            .get(name)
            .map(|v| {
                v.iter()
                    .map(|n| n.as_ref().and_then(Number::as_f64))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Current {
    pub time: Time,
    pub interval: u32,
    #[serde(flatten)]
    pub values: BTreeMap<String, Option<Number>>,
}

/// The legacy `current_weather=true` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurrentWeather {
    pub time: Time,
    pub interval: u32,
    pub temperature: Option<Number>,
    pub windspeed: Option<Number>,
    pub winddirection: Option<Number>,
    pub is_day: Option<Number>,
    pub weathercode: Option<Number>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forecast {
    pub latitude: f64,
    pub longitude: f64,
    pub generationtime_ms: f64,
    pub utc_offset_seconds: i32,
    pub timezone: String,
    pub timezone_abbreviation: String,
    pub elevation: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_units: Option<Units>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Current>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_weather_units: Option<Units>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_weather: Option<CurrentWeather>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hourly_units: Option<Units>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hourly: Option<Series>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_units: Option<Units>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily: Option<Series>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_through_parse() {
        use HourlyVar::*;
        for v in [
            Temperature,
            RelativeHumidity,
            DewPoint,
            ApparentTemperature,
            Precipitation,
            Rain,
            Showers,
            Snowfall,
            HourlyVar::WeatherCode,
            WindSpeed,
            WindDirection,
            UvIndex,
            IsDay,
            PrecipitationProbability,
            WindGusts,
            HourlyVar::ImsWeatherCode,
        ] {
            assert_eq!(HourlyVar::parse(v.name()), Some(v));
        }
        use DailyVar::*;
        for v in [
            TemperatureMax,
            TemperatureMin,
            ApparentMax,
            ApparentMin,
            PrecipitationSum,
            PrecipitationHours,
            DailyVar::WeatherCode,
            WindSpeedMax,
            WindDirectionDominant,
            UvIndexMax,
            PrecipitationProbabilityMax,
            WindGustsMax,
            DailyVar::ImsWeatherCode,
        ] {
            assert_eq!(DailyVar::parse(v.name()), Some(v));
        }
    }

    #[test]
    fn query_string() {
        let q = ForecastQuery {
            latitude: 32.07,
            longitude: 34.81,
            hourly: vec![HourlyVar::Temperature, HourlyVar::Precipitation],
            timezone: Some("auto".into()),
            ..Default::default()
        };
        assert_eq!(
            q.to_query(),
            "latitude=32.07&longitude=34.81&hourly=temperature_2m,precipitation&timezone=auto"
        );
    }

    #[test]
    fn series_flattens_variables() {
        let s: Series = serde_json::from_str(
            r#"{"time":["2026-09-30T00:00"],"weather_code":[3],"rain":[null]}"#,
        )
        .unwrap();
        assert_eq!(s.f64s("weather_code"), vec![Some(3.0)]);
        assert_eq!(s.f64s("rain"), vec![None]);
        assert!(s.f64s("missing").is_empty());
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"time":["2026-09-30T00:00"],"rain":[null],"weather_code":[3]}"#
        );
    }
}
