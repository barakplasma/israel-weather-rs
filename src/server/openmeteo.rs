//! Open-Meteo compatible `/v1/forecast`.
//!
//! Supports the commonly used subset of <https://open-meteo.com/en/docs>:
//! `latitude`, `longitude`, `hourly`, `daily`, `current`, `current_weather`, `timezone`
//! (`GMT`/`UTC`, `auto`, or any IANA name), `timeformat` (`iso8601`/`unixtime`),
//! `forecast_days`, `past_days`, `temperature_unit`, `wind_speed_unit` and `precipitation_unit`.
//! Hours outside the IMS forecast range are returned as `null`, like Open-Meteo does for
//! missing data. Returned latitude/longitude/elevation are those of the IMS location used.

use std::collections::HashMap;
use std::str::FromStr;

use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Duration, NaiveDate, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::codes::{is_day, wmo_code};
use super::{resolve, round, ApiError, AppState, LocationQuery};
use crate::hourly::HourlyPoint;

const MAX_FORECAST_DAYS: u32 = 16;
const MAX_PAST_DAYS: u32 = 92;

#[derive(Deserialize, Default)]
pub struct Params {
    latitude: Option<f64>,
    longitude: Option<f64>,
    hourly: Option<String>,
    daily: Option<String>,
    current: Option<String>,
    current_weather: Option<bool>,
    timezone: Option<String>,
    timeformat: Option<String>,
    forecast_days: Option<u32>,
    past_days: Option<u32>,
    temperature_unit: Option<String>,
    wind_speed_unit: Option<String>,
    /// Pre-2023 spelling, still accepted by Open-Meteo.
    windspeed_unit: Option<String>,
    precipitation_unit: Option<String>,
}

#[derive(Clone, Copy)]
enum WindUnit {
    Kmh,
    Ms,
    Mph,
    Kn,
}

#[derive(Clone, Copy)]
struct Units {
    fahrenheit: bool,
    wind: WindUnit,
    inch: bool,
}

impl Units {
    fn parse(p: &Params) -> Result<Self, ApiError> {
        let fahrenheit = match p.temperature_unit.as_deref().unwrap_or("celsius") {
            "celsius" => false,
            "fahrenheit" => true,
            other => {
                return Err(ApiError::bad_request(format!(
                    "Invalid temperature_unit {other}"
                )))
            }
        };
        let wind_param = p.wind_speed_unit.as_deref().or(p.windspeed_unit.as_deref());
        let wind = match wind_param.unwrap_or("kmh") {
            "kmh" => WindUnit::Kmh,
            "ms" => WindUnit::Ms,
            "mph" => WindUnit::Mph,
            "kn" => WindUnit::Kn,
            other => {
                return Err(ApiError::bad_request(format!(
                    "Invalid wind_speed_unit {other}"
                )))
            }
        };
        let inch = match p.precipitation_unit.as_deref().unwrap_or("mm") {
            "mm" => false,
            "inch" => true,
            other => {
                return Err(ApiError::bad_request(format!(
                    "Invalid precipitation_unit {other}"
                )))
            }
        };
        Ok(Self {
            fahrenheit,
            wind,
            inch,
        })
    }

    fn temp(&self, c: f32) -> f64 {
        let c = c as f64;
        round(
            if self.fahrenheit {
                c * 9.0 / 5.0 + 32.0
            } else {
                c
            },
            1,
        )
    }

    fn temp_unit(&self) -> &'static str {
        if self.fahrenheit {
            "°F"
        } else {
            "°C"
        }
    }

    /// IMS wind speed is km/h.
    fn wind(&self, kmh: f32) -> f64 {
        let kmh = kmh as f64;
        round(
            match self.wind {
                WindUnit::Kmh => kmh,
                WindUnit::Ms => kmh / 3.6,
                WindUnit::Mph => kmh / 1.609_344,
                WindUnit::Kn => kmh / 1.852,
            },
            1,
        )
    }

    fn wind_unit(&self) -> &'static str {
        match self.wind {
            WindUnit::Kmh => "km/h",
            WindUnit::Ms => "m/s",
            WindUnit::Mph => "mp/h",
            WindUnit::Kn => "kn",
        }
    }

    fn precip(&self, mm: f32) -> f64 {
        let mm = mm as f64;
        if self.inch {
            round(mm / 25.4, 3)
        } else {
            round(mm, 2)
        }
    }

    fn precip_unit(&self) -> &'static str {
        if self.inch {
            "inch"
        } else {
            "mm"
        }
    }
}

/// Hourly / current variables, including legacy aliases.
#[derive(Clone, Copy)]
enum HourlyVar {
    Temperature,
    RelativeHumidity,
    DewPoint,
    ApparentTemperature,
    Precipitation,
    Rain,
    ZeroPrecip, // showers / snowfall: IMS does not split these out
    WeatherCode,
    WindSpeed,
    WindDirection,
    UvIndex,
    IsDay,
}

impl HourlyVar {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "temperature_2m" => Self::Temperature,
            "relative_humidity_2m" | "relativehumidity_2m" => Self::RelativeHumidity,
            "dew_point_2m" | "dewpoint_2m" => Self::DewPoint,
            "apparent_temperature" => Self::ApparentTemperature,
            "precipitation" => Self::Precipitation,
            "rain" => Self::Rain,
            "showers" | "snowfall" => Self::ZeroPrecip,
            "weather_code" | "weathercode" => Self::WeatherCode,
            "wind_speed_10m" | "windspeed_10m" => Self::WindSpeed,
            "wind_direction_10m" | "winddirection_10m" => Self::WindDirection,
            "uv_index" => Self::UvIndex,
            "is_day" => Self::IsDay,
            _ => return None,
        })
    }

    fn unit(self, u: &Units, name: &str) -> &'static str {
        match self {
            Self::Temperature | Self::DewPoint | Self::ApparentTemperature => u.temp_unit(),
            Self::RelativeHumidity => "%",
            Self::Precipitation | Self::Rain => u.precip_unit(),
            Self::ZeroPrecip if name == "snowfall" => {
                if u.inch {
                    "inch"
                } else {
                    "cm"
                }
            }
            Self::ZeroPrecip => u.precip_unit(),
            Self::WeatherCode => "wmo code",
            Self::WindSpeed => u.wind_unit(),
            Self::WindDirection => "°",
            Self::UvIndex | Self::IsDay => "",
        }
    }

    fn value(self, p: &HourlyPoint, u: &Units, lat: f64, lon: f64) -> Value {
        match self {
            Self::Temperature => json!(u.temp(p.temperature)),
            Self::RelativeHumidity => json!(p.relative_humidity.round() as i64),
            Self::DewPoint => json!(u.temp(p.dew_point)),
            Self::ApparentTemperature => json!(u.temp(p.feels_like)),
            Self::Precipitation | Self::Rain => json!(u.precip(p.precipitation)),
            Self::ZeroPrecip => json!(0.0),
            Self::WeatherCode => json!(wmo_code(p.weather_code)),
            Self::WindSpeed => json!(u.wind(p.wind_speed)),
            Self::WindDirection => json!(p.wind_direction.round() as i64),
            Self::UvIndex => p
                .uv_index
                .map_or(Value::Null, |v| json!(round(v as f64, 2))),
            Self::IsDay => json!(is_day(lat, lon, p.time_utc()) as u8),
        }
    }
}

#[derive(Clone, Copy)]
enum DailyVar {
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
}

impl DailyVar {
    fn parse(name: &str) -> Option<Self> {
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
            _ => return None,
        })
    }

    fn unit(self, u: &Units) -> &'static str {
        match self {
            Self::TemperatureMax | Self::TemperatureMin | Self::ApparentMax | Self::ApparentMin => {
                u.temp_unit()
            }
            Self::PrecipitationSum => u.precip_unit(),
            Self::PrecipitationHours => "h",
            Self::WeatherCode => "wmo code",
            Self::WindSpeedMax => u.wind_unit(),
            Self::WindDirectionDominant => "°",
            Self::UvIndexMax => "",
        }
    }

    fn value(self, day: &[&HourlyPoint], u: &Units) -> Value {
        if day.is_empty() {
            return Value::Null;
        }
        let max = |f: fn(&HourlyPoint) -> f32| day.iter().map(|p| f(p)).fold(f32::MIN, f32::max);
        let min = |f: fn(&HourlyPoint) -> f32| day.iter().map(|p| f(p)).fold(f32::MAX, f32::min);
        match self {
            Self::TemperatureMax => json!(u.temp(max(|p| p.block_max_temp.max(p.temperature)))),
            Self::TemperatureMin => json!(u.temp(min(|p| p.block_min_temp.min(p.temperature)))),
            Self::ApparentMax => json!(u.temp(max(|p| p.feels_like))),
            Self::ApparentMin => json!(u.temp(min(|p| p.feels_like))),
            Self::PrecipitationSum => json!(u.precip(day.iter().map(|p| p.precipitation).sum())),
            Self::PrecipitationHours => {
                json!(day.iter().filter(|p| p.precipitation > 0.0).count())
            }
            Self::WeatherCode => json!(day.iter().map(|p| wmo_code(p.weather_code)).max()),
            Self::WindSpeedMax => json!(u.wind(max(|p| p.wind_speed))),
            Self::WindDirectionDominant => {
                // Speed-weighted vector mean of the wind direction.
                let (x, y) = day.iter().fold((0.0f64, 0.0f64), |(x, y), p| {
                    let r = (p.wind_direction as f64).to_radians();
                    let w = p.wind_speed as f64;
                    (x + w * r.sin(), y + w * r.cos())
                });
                json!(x.atan2(y).to_degrees().rem_euclid(360.0).round() as i64)
            }
            Self::UvIndexMax => day
                .iter()
                .filter_map(|p| p.block_uv_index_max.or(p.uv_index))
                .reduce(f32::max)
                .map_or(Value::Null, |v| json!(round(v as f64, 2))),
        }
    }
}

fn split_list(s: &Option<String>) -> Vec<&str> {
    s.as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect()
}

fn parse_vars<T>(
    s: &Option<String>,
    parse: fn(&str) -> Option<T>,
) -> Result<Vec<(&str, T)>, ApiError> {
    split_list(s)
        .into_iter()
        .map(|name| {
            parse(name).map(|v| (name, v)).ok_or_else(|| {
                ApiError::bad_request(format!(
                    "Cannot initialize WeatherVariable from invalid String value {name}"
                ))
            })
        })
        .collect()
}

/// Resolve the `timezone` parameter; returns the zone and the name to echo back.
fn parse_timezone(tz: Option<&str>) -> Result<(Tz, String), ApiError> {
    match tz.unwrap_or("GMT") {
        "" | "GMT" | "UTC" => Ok((chrono_tz::UTC, "GMT".into())),
        // All IMS locations are in Israel.
        "auto" => Ok((chrono_tz::Asia::Jerusalem, "Asia/Jerusalem".into())),
        name => Tz::from_str(name)
            .map(|tz| (tz, name.to_string()))
            .map_err(|_| ApiError::bad_request(format!("Invalid timezone {name}"))),
    }
}

fn local_midnight(tz: Tz, date: NaiveDate) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight is valid");
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        // Midnight skipped by a DST jump: the day starts one hour later.
        .unwrap_or_else(|| {
            tz.from_local_datetime(&(naive + Duration::hours(1)))
                .earliest()
                .map_or_else(|| Utc.from_utc_datetime(&naive), |t| t.with_timezone(&Utc))
        })
}

struct TimeFormat {
    unix: bool,
    tz: Tz,
}

impl TimeFormat {
    fn hour(&self, t: DateTime<Utc>) -> Value {
        if self.unix {
            json!(t.timestamp())
        } else {
            json!(t
                .with_timezone(&self.tz)
                .format("%Y-%m-%dT%H:%M")
                .to_string())
        }
    }

    fn day(&self, date: NaiveDate) -> Value {
        if self.unix {
            json!(local_midnight(self.tz, date).timestamp())
        } else {
            json!(date.format("%Y-%m-%d").to_string())
        }
    }

    fn unit(&self) -> &'static str {
        if self.unix {
            "unixtime"
        } else {
            "iso8601"
        }
    }
}

pub async fn forecast(
    State(state): State<AppState>,
    Query(params): Query<Params>,
) -> Result<Json<Value>, ApiError> {
    let started = std::time::Instant::now();
    let (Some(lat), Some(lon)) = (params.latitude, params.longitude) else {
        return Err(ApiError::bad_request(
            "Parameter 'latitude' and 'longitude' are required",
        ));
    };
    let units = Units::parse(&params)?;
    let hourly_vars = parse_vars(&params.hourly, HourlyVar::parse)?;
    let current_vars = parse_vars(&params.current, HourlyVar::parse)?;
    let daily_vars = parse_vars(&params.daily, DailyVar::parse)?;
    let (tz, tz_name) = parse_timezone(params.timezone.as_deref())?;
    let fmt = TimeFormat {
        unix: match params.timeformat.as_deref().unwrap_or("iso8601") {
            "iso8601" => false,
            "unixtime" => true,
            other => return Err(ApiError::bad_request(format!("Invalid timeformat {other}"))),
        },
        tz,
    };
    let forecast_days = params.forecast_days.unwrap_or(7);
    let past_days = params.past_days.unwrap_or(0);
    if forecast_days > MAX_FORECAST_DAYS || past_days > MAX_PAST_DAYS {
        return Err(ApiError::bad_request(format!(
            "forecast_days must be 0-{MAX_FORECAST_DAYS} and past_days 0-{MAX_PAST_DAYS}"
        )));
    }

    let snap = state.require_snapshot()?;
    let resolved = resolve(&snap, LocationQuery::Coords(lat, lon))?;
    let meta = &resolved.location.location_meta_data;
    let (loc_lat, loc_lon) = (meta.display_lat as f64, meta.display_lon as f64);
    let by_time: HashMap<i64, &HourlyPoint> = resolved
        .hourly
        .iter()
        .map(|p| (p.time_utc().timestamp(), p))
        .collect();

    let now = state.now();
    let now_local = now.with_timezone(&tz);
    let first_day = now_local.date_naive() - Duration::days(past_days as i64);
    let days: Vec<NaiveDate> = first_day
        .iter_days()
        .take((past_days + forecast_days) as usize)
        .collect();

    let mut body = Map::new();
    body.insert("latitude".into(), json!(round(loc_lat, 4)));
    body.insert("longitude".into(), json!(round(loc_lon, 4)));
    body.insert(
        "generationtime_ms".into(),
        json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    body.insert(
        "utc_offset_seconds".into(),
        json!(now_local.offset().fix().local_minus_utc()),
    );
    body.insert("timezone".into(), json!(tz_name));
    body.insert(
        "timezone_abbreviation".into(),
        json!(now_local.format("%Z").to_string()),
    );
    body.insert("elevation".into(), json!(meta.display_height));

    if params.current_weather == Some(true) || !current_vars.is_empty() {
        let hour_start = now.timestamp() - now.timestamp().rem_euclid(3600);
        let point = by_time.get(&hour_start).copied();
        let hour = DateTime::from_timestamp(hour_start, 0).expect("valid timestamp");
        let value =
            |v: HourlyVar| point.map_or(Value::Null, |p| v.value(p, &units, loc_lat, loc_lon));

        if !current_vars.is_empty() {
            let mut cur_units = Map::new();
            let mut cur = Map::new();
            cur_units.insert("time".into(), json!(fmt.unit()));
            cur_units.insert("interval".into(), json!("seconds"));
            cur.insert("time".into(), fmt.hour(hour));
            cur.insert("interval".into(), json!(3600));
            for (name, var) in &current_vars {
                cur_units.insert((*name).into(), json!(var.unit(&units, name)));
                cur.insert((*name).into(), value(*var));
            }
            body.insert("current_units".into(), Value::Object(cur_units));
            body.insert("current".into(), Value::Object(cur));
        }
        if params.current_weather == Some(true) {
            body.insert(
                "current_weather_units".into(),
                json!({
                    "time": fmt.unit(), "interval": "seconds",
                    "temperature": units.temp_unit(), "windspeed": units.wind_unit(),
                    "winddirection": "°", "is_day": "", "weathercode": "wmo code",
                }),
            );
            body.insert(
                "current_weather".into(),
                json!({
                    "time": fmt.hour(hour),
                    "interval": 3600,
                    "temperature": value(HourlyVar::Temperature),
                    "windspeed": value(HourlyVar::WindSpeed),
                    "winddirection": value(HourlyVar::WindDirection),
                    "is_day": value(HourlyVar::IsDay),
                    "weathercode": value(HourlyVar::WeatherCode),
                }),
            );
        }
    }

    if !hourly_vars.is_empty() {
        let start = days.first().map_or(now, |d| local_midnight(tz, *d));
        let end = days
            .last()
            .map_or(now, |d| local_midnight(tz, *d + Duration::days(1)));
        let hours: Vec<DateTime<Utc>> = (0..)
            .map(|h| start + Duration::hours(h))
            .take_while(|t| *t < end)
            .collect();

        let mut h_units = Map::new();
        let mut hourly = Map::new();
        h_units.insert("time".into(), json!(fmt.unit()));
        hourly.insert(
            "time".into(),
            hours.iter().map(|t| fmt.hour(*t)).collect::<Value>(),
        );
        for (name, var) in &hourly_vars {
            h_units.insert((*name).into(), json!(var.unit(&units, name)));
            let values: Value = hours
                .iter()
                .map(|t| {
                    by_time
                        .get(&t.timestamp())
                        .map_or(Value::Null, |p| var.value(p, &units, loc_lat, loc_lon))
                })
                .collect();
            hourly.insert((*name).into(), values);
        }
        body.insert("hourly_units".into(), Value::Object(h_units));
        body.insert("hourly".into(), Value::Object(hourly));
    }

    if !daily_vars.is_empty() {
        let buckets: Vec<Vec<&HourlyPoint>> = days
            .iter()
            .map(|d| {
                let (from, to) = (
                    local_midnight(tz, *d),
                    local_midnight(tz, *d + Duration::days(1)),
                );
                resolved
                    .hourly
                    .iter()
                    .filter(|p| (from..to).contains(&p.time_utc()))
                    .collect()
            })
            .collect();

        let mut d_units = Map::new();
        let mut daily = Map::new();
        d_units.insert("time".into(), json!(fmt.unit()));
        daily.insert(
            "time".into(),
            days.iter().map(|d| fmt.day(*d)).collect::<Value>(),
        );
        for (name, var) in &daily_vars {
            d_units.insert((*name).into(), json!(var.unit(&units)));
            daily.insert(
                (*name).into(),
                buckets
                    .iter()
                    .map(|b| var.value(b, &units))
                    .collect::<Value>(),
            );
        }
        body.insert("daily_units".into(), Value::Object(d_units));
        body.insert("daily".into(), Value::Object(daily));
    }

    Ok(Json(Value::Object(body)))
}

#[cfg(test)]
mod tests {
    use crate::server::test_support::*;
    use axum::http::StatusCode;

    // 01:22 Israel time on 2025-03-09.
    const NOW: &str = "2025-03-08T23:22:00Z";
    const TLV: &str = "latitude=32.08&longitude=34.78";

    #[tokio::test]
    async fn hourly_gmt_defaults() {
        let (status, json) = get_json(
            state_at(NOW),
            &format!("/v1/forecast?{TLV}&hourly=temperature_2m,precipitation,weather_code"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["timezone"], "GMT");
        assert_eq!(json["utc_offset_seconds"], 0);
        assert_eq!(json["latitude"], 32.0821);
        assert_eq!(json["hourly_units"]["temperature_2m"], "°C");
        assert_eq!(json["hourly_units"]["weather_code"], "wmo code");
        let time = json["hourly"]["time"].as_array().unwrap();
        assert_eq!(time.len(), 7 * 24);
        // GMT day starts at 2025-03-08 00:00 UTC.
        assert_eq!(time[0], "2025-03-08T00:00");
        let temps = json["hourly"]["temperature_2m"].as_array().unwrap();
        assert_eq!(temps.len(), time.len());
        assert!(temps[0].is_number());
    }

    #[tokio::test]
    async fn hourly_values_follow_ims_blocks() {
        let (_, json) = get_json(
            state_at(NOW),
            &format!("/v1/forecast?{TLV}&hourly=temperature_2m,precipitation&timezone=auto&forecast_days=1"),
        )
        .await;
        assert_eq!(json["timezone"], "Asia/Jerusalem");
        assert_eq!(json["timezone_abbreviation"], "IST");
        assert_eq!(json["utc_offset_seconds"], 7200);
        let time = json["hourly"]["time"].as_array().unwrap();
        assert_eq!(time.len(), 24);
        assert_eq!(time[0], "2025-03-09T00:00");
        let rain: Vec<f64> = json["hourly"]["precipitation"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let temps: Vec<f64> = json["hourly"]["temperature_2m"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        // Each hour of the 02:00-07:00 IMS block carries 1/6 of that block's rain.
        let block = &crate::parse_forecast(FIXTURE).unwrap().location[1]
            .location_data
            .forecast
            .iter()
            .find(|f| f.forecast_time == "2025-03-09T02:00:00+02:00")
            .unwrap()
            .clone();
        for (h, r) in rain.iter().enumerate().take(8).skip(2) {
            assert!((r - (block.rain / 6.0) as f64).abs() < 0.01, "hour {h}");
        }
        // Block start hours carry the IMS temperature exactly.
        assert_eq!(temps[2], crate::server::round(block.temperature as f64, 1));
    }

    #[tokio::test]
    async fn missing_hours_are_null() {
        let (_, json) = get_json(
            state_at(NOW),
            &format!("/v1/forecast?{TLV}&hourly=temperature_2m&forecast_days=16"),
        )
        .await;
        let temps = json["hourly"]["temperature_2m"].as_array().unwrap();
        assert_eq!(temps.len(), 16 * 24);
        assert!(temps.last().unwrap().is_null());
    }

    #[tokio::test]
    async fn daily_and_current() {
        let (status, json) = get_json(
            state_at(NOW),
            &format!(
                "/v1/forecast?{TLV}&timezone=Asia/Jerusalem&current=temperature_2m,is_day\
                 &current_weather=true\
                 &daily=temperature_2m_max,temperature_2m_min,precipitation_sum,weather_code,uv_index_max,wind_direction_10m_dominant"
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["current"]["time"], "2025-03-09T01:00");
        assert_eq!(json["current"]["interval"], 3600);
        assert!(json["current"]["temperature_2m"].is_number());
        assert_eq!(json["current"]["is_day"], 0);
        assert!(json["current_weather"]["weathercode"].is_number());
        let daily = &json["daily"];
        assert_eq!(daily["time"][0], "2025-03-09");
        assert_eq!(daily["time"].as_array().unwrap().len(), 7);
        let max = daily["temperature_2m_max"][0].as_f64().unwrap();
        let min = daily["temperature_2m_min"][0].as_f64().unwrap();
        assert!(max >= min);
        assert!(daily["weather_code"][0].is_number());
    }

    #[tokio::test]
    async fn unit_conversions() {
        let base = format!("/v1/forecast?{TLV}&current=temperature_2m,wind_speed_10m");
        let (_, c) = get_json(state_at(NOW), &base).await;
        let (_, f) = get_json(
            state_at(NOW),
            &format!("{base}&temperature_unit=fahrenheit&wind_speed_unit=ms"),
        )
        .await;
        let tc = c["current"]["temperature_2m"].as_f64().unwrap();
        let tf = f["current"]["temperature_2m"].as_f64().unwrap();
        assert!((tf - (tc * 9.0 / 5.0 + 32.0)).abs() < 0.2);
        let kmh = c["current"]["wind_speed_10m"].as_f64().unwrap();
        let ms = f["current"]["wind_speed_10m"].as_f64().unwrap();
        assert!((ms - kmh / 3.6).abs() < 0.1);
        assert_eq!(f["current_units"]["temperature_2m"], "°F");
        assert_eq!(f["current_units"]["wind_speed_10m"], "m/s");
    }

    #[tokio::test]
    async fn unixtime_format() {
        let (_, json) = get_json(
            state_at(NOW),
            &format!(
                "/v1/forecast?{TLV}&hourly=temperature_2m&timeformat=unixtime&forecast_days=1"
            ),
        )
        .await;
        assert_eq!(json["hourly_units"]["time"], "unixtime");
        // 2025-03-08T00:00:00Z
        assert_eq!(json["hourly"]["time"][0], 1741392000);
    }

    #[tokio::test]
    async fn errors() {
        for (uri, needle) in [
            ("/v1/forecast?hourly=temperature_2m", "latitude"),
            (
                &format!("/v1/forecast?{TLV}&hourly=cape") as &str,
                "invalid String value cape",
            ),
            (
                &format!("/v1/forecast?{TLV}&timezone=Mars/Base"),
                "Invalid timezone",
            ),
            (
                &format!("/v1/forecast?{TLV}&forecast_days=40"),
                "forecast_days",
            ),
            (
                "/v1/forecast?latitude=51.5&longitude=-0.12",
                "no IMS forecast location",
            ),
        ] {
            let (status, json) = get_json(state_at(NOW), uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(json["error"], true);
            assert!(
                json["reason"].as_str().unwrap().contains(needle),
                "{uri}: {json}"
            );
        }
    }
}
