#![allow(unused_parens)]
use clap::Parser;
use serde_json;
use tracing::debug;

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

    let weather_data = israel_weather_rs::get_israeli_weather_forecast(args.offline)
        .expect("failed to get forecast");

    if args.all {
        let json = serde_json::json!(weather_data);
        debug!("{}", &json);
        println!(
            "{}",
            serde_json::to_string_pretty(&json).expect("could not pretty print json")
        );
        return;
    }

    let desired_location = israel_weather_rs::find_location(&args.location, &weather_data);

    let next_forecasts = israel_weather_rs::forecasts_for_location_for_next_n_hours(
        args.next,
        &desired_location,
        chrono::Utc::now(),
    );

    debug!("{:?}", next_forecasts);

    let forecast_json =
        serde_json::to_string_pretty(&next_forecasts).expect("could not pretty print json");

    println!("{}", forecast_json);
}
