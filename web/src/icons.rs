//! WMO weather codes (the subset the server emits) to labels, strip colours and SVG icons.

use leptos::prelude::*;

/// Sky state for the condition strips, ordered by severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sky {
    Clear,
    Partly,
    Cloudy,
    Light,
    Rain,
    Heavy,
    Snow,
}

impl Sky {
    pub fn of(code: u8, precipitation: f64) -> Self {
        let s = match code {
            0 | 1 => Self::Clear,
            2 => Self::Partly,
            61 | 80 => Self::Light,
            63 | 66 => Self::Rain,
            65 | 95 => Self::Heavy,
            71 | 73 | 75 => Self::Snow,
            _ => Self::Cloudy,
        };
        // Measurable rain trumps the sky code.
        if precipitation > 0.0 && s < Self::Light {
            Self::Light
        } else {
            s
        }
    }

    pub fn is_wet(self) -> bool {
        self >= Self::Light
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Partly => "Partly cloudy",
            Self::Cloudy => "Cloudy",
            Self::Light => "Light rain",
            Self::Rain => "Rain",
            Self::Heavy => "Heavy rain",
            Self::Snow => "Snow",
        }
    }

    /// CSS custom property holding this state's strip colour.
    pub fn var(self) -> &'static str {
        match self {
            Self::Clear => "var(--s-clear)",
            Self::Partly => "var(--s-partly)",
            Self::Cloudy => "var(--s-cloudy)",
            Self::Light | Self::Rain | Self::Heavy => "var(--rain-bg)",
            Self::Snow => "var(--s-snow)",
        }
    }
}

/// The IMS name when known (it is finer: "Hot", "Dust", "Windy"), else the WMO one.
pub fn label(code: u8, ims: Option<i32>) -> &'static str {
    match ims.map(weather_api::ims::label) {
        Some(name) if name != "Unknown" => return name,
        _ => {}
    }
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Sleet",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        80..=82 => "Showers",
        95..=99 => "Thunderstorms",
        _ => "Cloudy",
    }
}

const SUN: &str = r#"<circle cx="24" cy="24" r="8" fill="var(--sun)"/><g stroke="var(--sun)" stroke-width="3" stroke-linecap="round"><path d="M24 6v4M24 38v4M6 24h4M38 24h4M11.3 11.3l2.8 2.8M33.9 33.9l2.8 2.8M11.3 36.7l2.8-2.8M33.9 14.1l2.8-2.8"/></g>"#;
const MOON: &str = r#"<path d="M30 8a14 14 0 1 0 10 22A12 12 0 0 1 30 8z" fill="var(--moon)"/>"#;
const CLOUD: &str = r#"<path d="M14 38h22a8 8 0 0 0 0-16 11 11 0 0 0-21-2 9 9 0 0 0-1 18z" fill="var(--cloud)" stroke="var(--cloud-edge)" stroke-width="1.5"/>"#;
const SMALL_SUN: &str = r#"<g transform="translate(-6 -6) scale(.75)">"#;
const DROPS: &str = r#"<g stroke="var(--rain)" stroke-width="3" stroke-linecap="round"><path d="M18 41l-2 4M26 41l-2 4M34 41l-2 4"/></g>"#;
const DROP: &str = r#"<g stroke="var(--rain)" stroke-width="3" stroke-linecap="round"><path d="M25 41l-2 4"/></g>"#;
const FLAKES: &str = r#"<g fill="var(--snow)"><circle cx="17" cy="43" r="2"/><circle cx="25" cy="45" r="2"/><circle cx="33" cy="43" r="2"/></g>"#;
const BOLT: &str = r#"<path d="M26 36l-5 8h5l-3 6 8-9h-5l3-5z" fill="var(--sun)"/>"#;
const HEAT: &str = r#"<g stroke="var(--warm)" stroke-width="3" stroke-linecap="round" fill="none"><path d="M12 44q4-3 8 0t8 0 8 0"/></g>"#;
const DUST: &str = r#"<g stroke="var(--dust)" stroke-width="3" stroke-linecap="round"><path d="M8 30h32M12 37h28M8 44h24"/></g>"#;
const WIND: &str = r#"<g stroke="var(--cloud-edge)" stroke-width="3" stroke-linecap="round" fill="none"><path d="M6 20h24a5 5 0 1 0-5-5M6 28h32a5 5 0 1 1-5 5M6 36h18"/></g>"#;
const FLAKE: &str = r#"<g stroke="var(--snow)" stroke-width="3" stroke-linecap="round"><path d="M24 10v32M10 18l28 16M10 34l28-16"/></g>"#;
const FOG: &str = r#"<g stroke="var(--cloud-edge)" stroke-width="3" stroke-linecap="round"><path d="M10 20h28M8 27h32M12 34h24"/></g>"#;

fn svg(code: u8, ims: Option<i32>, is_day: bool) -> String {
    let sky = if is_day { SUN } else { MOON };
    let behind = format!("{SMALL_SUN}{sky}</g>");
    let raised = format!(r#"<g transform="translate(0 -6)">{sky}</g>"#);
    // IMS conditions that WMO folds into clear or cloudy.
    let special = match ims {
        Some(1310 | 1580) => Some(format!("{raised}{HEAT}")),
        Some(1010 | 1570) => Some(format!("{raised}{DUST}")),
        Some(1260) => Some(WIND.to_string()),
        Some(1300 | 1320 | 1590) => Some(FLAKE.to_string()),
        _ => None,
    };
    // The IMS code is per-day for days (WMO's daily code is the worst hour), so it wins.
    let code = ims.map_or(code, weather_api::ims::wmo_code);
    let body = special.unwrap_or_else(|| match code {
        0 => sky.to_string(),
        1 | 2 => format!("{behind}{CLOUD}"),
        45 | 48 => FOG.to_string(),
        61 => format!("{CLOUD}{DROP}"),
        80..=82 => format!("{behind}{CLOUD}{DROP}"),
        63 | 65 | 66 | 67 => format!("{CLOUD}{DROPS}"),
        71..=77 => format!("{CLOUD}{FLAKES}"),
        95..=99 => format!("{CLOUD}{BOLT}"),
        _ => CLOUD.to_string(),
    });
    format!(r#"<svg viewBox="0 0 48 52" aria-hidden="true">{body}</svg>"#)
}

/// A weather icon; `size` in CSS pixels.
#[component]
pub fn Icon(
    code: u8,
    #[prop(default = None)] ims: Option<i32>,
    #[prop(default = true)] is_day: bool,
    size: u32,
) -> impl IntoView {
    view! {
        <span
            class="icon"
            role="img"
            aria-label=label(code, ims)
            style=format!("width:{size}px;height:{size}px")
            inner_html=svg(code, ims, is_day)
        ></span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ims_label_wins_when_known() {
        assert_eq!(label(0, Some(1310)), "Hot");
        assert_eq!(label(3, Some(1570)), "Dust");
        assert_eq!(label(0, None), "Clear");
        assert_eq!(label(0, Some(9999)), "Clear");
    }
}
