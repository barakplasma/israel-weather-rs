//! The forecast as the UI uses it: hourly rows and days, built from an Open-Meteo response
//! requested in Israel local time.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Timelike, Weekday};
use weather_api::openmeteo::{DailyVar, Forecast, ForecastQuery, HourlyVar, Time};

const HOURLY: [HourlyVar; 10] = [
    HourlyVar::Temperature,
    HourlyVar::ApparentTemperature,
    HourlyVar::Precipitation,
    HourlyVar::PrecipitationProbability,
    HourlyVar::WeatherCode,
    HourlyVar::WindSpeed,
    HourlyVar::WindGusts,
    HourlyVar::RelativeHumidity,
    HourlyVar::UvIndex,
    HourlyVar::IsDay,
];
const DAILY: [DailyVar; 5] = [
    DailyVar::TemperatureMax,
    DailyVar::TemperatureMin,
    DailyVar::WeatherCode,
    DailyVar::PrecipitationSum,
    DailyVar::PrecipitationProbabilityMax,
];

pub fn query(latitude: f64, longitude: f64) -> ForecastQuery {
    ForecastQuery {
        latitude,
        longitude,
        hourly: HOURLY.to_vec(),
        daily: DAILY.to_vec(),
        timezone: Some("Asia/Jerusalem".into()),
        forecast_days: Some(7),
        ..Default::default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hour {
    /// Israel local time.
    pub time: NaiveDateTime,
    pub temperature: f64,
    pub feels_like: f64,
    /// mm in this hour.
    pub precipitation: f64,
    /// %; only where IMS published hourly data.
    pub chance: Option<f64>,
    /// WMO code.
    pub code: u8,
    pub wind: f64,
    pub gust: Option<f64>,
    pub humidity: f64,
    pub uv: Option<f64>,
    pub is_day: bool,
}

impl Hour {
    /// Rain is measurable or likely.
    pub fn is_wet(&self) -> bool {
        self.precipitation > 0.0 || self.chance.unwrap_or(0.0) >= 40.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub min: f64,
    pub max: f64,
    pub code: u8,
    pub rain: f64,
    pub chance: f64,
    pub hours: Vec<Hour>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// From the current hour on.
    pub hours: Vec<Hour>,
    pub days: Vec<Day>,
    pub now: NaiveDateTime,
}

fn local(t: &Time, utc_offset: i32) -> Option<NaiveDateTime> {
    match t {
        Time::Iso(s) => NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
            .or_else(|_| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap())
            })
            .ok(),
        Time::Unix(secs) => {
            DateTime::from_timestamp(secs + utc_offset as i64, 0).map(|t| t.naive_utc())
        }
    }
}

impl Model {
    /// `now_ms` is the browser's clock (ms since the epoch).
    pub fn new(f: &Forecast, now_ms: f64) -> Self {
        let now =
            DateTime::from_timestamp_millis(now_ms as i64 + f.utc_offset_seconds as i64 * 1000)
                .map(|t| t.naive_utc())
                .unwrap_or_default();
        let hour_start = now.date().and_hms_opt(now.hour(), 0, 0).unwrap_or(now);
        let mut all = vec![];
        if let Some(h) = &f.hourly {
            let col = |v: HourlyVar| h.f64s(v.name());
            let [temp, feels, precip, chance, code, wind, gust, hum, uv, day] = HOURLY.map(col);
            for (i, t) in h.time.iter().enumerate() {
                let (Some(time), Some(Some(temperature))) =
                    (local(t, f.utc_offset_seconds), temp.get(i))
                else {
                    continue; // hours IMS has no forecast for are null
                };
                let at = |c: &[Option<f64>]| c.get(i).copied().flatten();
                all.push(Hour {
                    time,
                    temperature: *temperature,
                    feels_like: at(&feels).unwrap_or(*temperature),
                    precipitation: at(&precip).unwrap_or(0.0),
                    chance: at(&chance),
                    code: at(&code).unwrap_or(3.0) as u8,
                    wind: at(&wind).unwrap_or(0.0),
                    gust: at(&gust),
                    humidity: at(&hum).unwrap_or(0.0),
                    uv: at(&uv),
                    is_day: at(&day).unwrap_or(1.0) > 0.0,
                });
            }
        }
        let mut days = vec![];
        if let Some(d) = &f.daily {
            let [max, min, code, rain, chance] = DAILY.map(|v| d.f64s(v.name()));
            for (i, t) in d.time.iter().enumerate() {
                let Some(date) = local(t, f.utc_offset_seconds).map(|t| t.date()) else {
                    continue;
                };
                let hours: Vec<Hour> = all
                    .iter()
                    .filter(|h| h.time.date() == date)
                    .cloned()
                    .collect();
                let (Some(Some(mx)), Some(Some(mn))) = (max.get(i), min.get(i)) else {
                    continue;
                };
                if hours.is_empty() {
                    continue;
                }
                let at = |c: &[Option<f64>]| c.get(i).copied().flatten();
                days.push(Day {
                    date,
                    min: *mn,
                    max: *mx,
                    code: at(&code).unwrap_or(3.0) as u8,
                    rain: at(&rain).unwrap_or(0.0),
                    chance: at(&chance).unwrap_or(if at(&rain).unwrap_or(0.0) > 0.0 {
                        100.0
                    } else {
                        0.0
                    }),
                    hours,
                });
            }
        }
        Self {
            hours: all.into_iter().filter(|h| h.time >= hour_start).collect(),
            days: days.into_iter().filter(|d| d.date >= now.date()).collect(),
            now,
        }
    }
}

pub fn weekday(d: NaiveDate) -> &'static str {
    match d.weekday() {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

/// "this evening", "tomorrow morning", "Tuesday afternoon".
pub fn when(t: NaiveDateTime, now: NaiveDateTime) -> String {
    let h = t.hour();
    let part = match h {
        0..5 => "night",
        5..12 => "morning",
        12..17 => "afternoon",
        17..21 => "evening",
        _ => "night",
    };
    let days = (t.date() - now.date()).num_days();
    match days {
        0 if part == "night" => "tonight".into(),
        0 => format!("this {part}"),
        1 => format!("tomorrow {part}"),
        _ => format!("{} {part}", weekday(t.date())),
    }
}

pub fn hh(t: NaiveDateTime) -> String {
    t.format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forecast() -> Forecast {
        serde_json::from_str(
            r#"{"latitude":32,"longitude":34.8,"generationtime_ms":0,"utc_offset_seconds":7200,
            "timezone":"Asia/Jerusalem","timezone_abbreviation":"IST","elevation":10,
            "hourly":{"time":["2026-10-28T20:00","2026-10-28T21:00","2026-10-28T22:00","2026-10-29T00:00"],
              "temperature_2m":[20,21,null,18],"apparent_temperature":[20,21,null,18],
              "precipitation":[0,0.4,null,0],"precipitation_probability":[5,60,null,null],
              "weather_code":[0,61,null,3],"wind_speed_10m":[5,6,null,7],"wind_gusts_10m":[null,null,null,null],
              "relative_humidity_2m":[60,70,null,80],"uv_index":[0,0,null,0],"is_day":[0,0,null,0]},
            "daily":{"time":["2026-10-28","2026-10-29"],"temperature_2m_max":[28,26],
              "temperature_2m_min":[20,18],"weather_code":[61,3],"precipitation_sum":[0.4,0],
              "precipitation_probability_max":[60,null]}}"#,
        )
        .unwrap()
    }

    #[test]
    fn drops_past_and_missing_hours() {
        // 2026-10-28T19:10Z = 21:10 Israel (UTC+2).
        let now = DateTime::parse_from_rfc3339("2026-10-28T19:10:00.358Z").unwrap();
        let m = Model::new(&forecast(), now.timestamp_millis() as f64);
        assert_eq!(hh(m.now), "21:10");
        assert_eq!(m.hours.len(), 2);
        assert_eq!(hh(m.hours[0].time), "21:00");
        assert!(m.hours[0].is_wet());
        assert_eq!(m.days.len(), 2);
        assert_eq!(
            m.days[0].hours.len(),
            2,
            "today keeps its past hours for the graph"
        );
        assert_eq!(m.days[1].chance, 0.0);
    }

    #[test]
    fn when_names_day_parts() {
        let t = |s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap();
        let now = t("2026-10-28T10:00");
        assert_eq!(when(t("2026-10-28T19:00"), now), "this evening");
        assert_eq!(when(t("2026-10-28T23:00"), now), "tonight");
        assert_eq!(when(t("2026-10-29T07:00"), now), "tomorrow morning");
        assert_eq!(when(t("2026-10-31T13:00"), now), "Saturday afternoon");
    }
}
