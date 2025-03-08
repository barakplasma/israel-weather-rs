#![allow(unused_parens)]
use israel_weather_rs::get_israeli_weather_forecast;
use serde_json;
use tracing::debug;

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

    let weather_data = get_israeli_weather_forecast(args.offline).expect("failed to get forecast");

    let now = chrono::Utc::now();

    if args.all {
        let json = serde_json::json!(weather_data);
        debug!("{}", &json);
        println!(
            "{}",
            serde_json::to_string_pretty(&json).expect("could not pretty print json")
        );
        return;
    }

    // get desired location and next forecast
    let desired_location = weather_data
        .location
        .iter()
        .find(|location| location.location_meta_data.location_name_eng == args.location)
        .expect("failed to find location specified");

    let next_forecasts: Vec<_> = desired_location.location_data.forecast
        .iter()
        .filter(|forecast| {
            chrono::DateTime::parse_from_rfc3339(&forecast.forecast_time)
                .expect("failed to parse forecast datetime")
                > now
        })
        .take((args.next / 6) as usize)
        .collect();

    debug!("{:?}", next_forecasts);

    let forecast_json =
        serde_json::to_string_pretty(&next_forecasts).expect("could not pretty print json");

    println!("{}", forecast_json);
}
