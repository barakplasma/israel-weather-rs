#![allow(unused_parens)]
use serde_json;
use weather::get_israeli_weather_forecast;

use clap::Parser;

/// Downloads and Caches Israeli weather forecast from https://ims.gov.il and prints the next forecast for a location as json
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Location to check weather for
    #[arg(short, long, default_value_t=("Tel Aviv Coast".to_string()))]
    location: String,

    /// Check next n hours ahead
    #[arg(short, long, default_value_t = 6)]
    next: u8,

    /// Ignore location and print all weather data
    #[arg(short, long, default_value_t = false)]
    all: bool,

    /// Offline mode
    #[arg(short, long, default_value_t = false)]
    offline: bool,
}

fn main() {
    let args = Args::parse();

    let forecasts = get_israeli_weather_forecast(args.offline).expect("failed to get forecast");

    let now = chrono::Utc::now();

    if args.all {
        let json = serde_json::json!(forecasts);
        // pretty print as json
        println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .expect("failed to serialize weather forecast to json")
        );
        return;
    }

    // get desired location and next forecast
    let desired_location = forecasts
        .location
        .iter()
        .find(|location| location.location_meta_data.location_name_eng == args.location)
        .expect("failed to find location specified");
    let mut forecast_iter = desired_location.location_data.forecast.iter();
    forecast_iter
        .position(|forecast| {
            chrono::DateTime::parse_from_rfc3339(&forecast.forecast_time)
                .expect("failed to parse forecast datetime")
                > now.checked_sub_signed(chrono::Duration::minutes(5*60+59))
                    .expect("failed to subtract 5 hours and 59 minutes from now to reset to next forecast")
        })
        .expect("failed to find next forecast");

    let json = serde_json::json!(forecast_iter
        .take((args.next / 6) as usize)
        .collect::<Vec<_>>());
    // pretty print as json
    println!(
        "{}",
        serde_json::to_string_pretty(&json).expect("failed to serialize weather forecast to json")
    );
}
