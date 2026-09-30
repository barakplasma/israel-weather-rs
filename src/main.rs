use std::process::ExitCode;

use chrono::{DateTime, Utc};
use clap::Parser;
use tracing::debug;

/// Downloads and Caches Israeli weather forecast from https://ims.gov.il and prints the next forecast for a location as json
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Location to check weather for (case-insensitive)
    #[arg(short, long, default_value_t = String::from("Tel Aviv Coast"))]
    location: String,

    /// Check next n hours ahead
    #[arg(short, long, default_value_t = 6)]
    next: u8,

    /// Ignore location and print all weather data
    #[arg(short, long, default_value_t = false)]
    all: bool,

    /// Offline mode: only use the previously cached forecast
    #[arg(short, long, default_value_t = false)]
    offline: bool,

    /// List available location names and exit
    #[arg(long, default_value_t = false, conflicts_with = "all")]
    list_locations: bool,

    /// Pretend the current time is this RFC 3339 timestamp (e.g. 2025-03-08T23:00:00Z)
    #[arg(long, value_parser = parse_now)]
    now: Option<DateTime<Utc>>,
}

fn parse_now(s: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| format!("expected an RFC 3339 timestamp: {e}"))
}

fn run(args: Args) -> Result<String, Box<dyn std::error::Error>> {
    let weather_data = israel_weather_rs::get_israeli_weather_forecast(args.offline)?;

    if args.list_locations {
        return Ok(israel_weather_rs::location_names(&weather_data).join("\n"));
    }

    let json = if args.all {
        serde_json::to_string_pretty(&weather_data)?
    } else {
        let desired_location = israel_weather_rs::find_location(&args.location, &weather_data)?;
        let next_forecasts = israel_weather_rs::forecasts_for_location_for_next_n_hours(
            args.next,
            desired_location,
            args.now.unwrap_or_else(Utc::now),
        );
        debug!("{:?}", next_forecasts);
        serde_json::to_string_pretty(&next_forecasts)?
    };
    Ok(json)
}

fn main() -> ExitCode {
    israel_weather_rs::init_logging();
    match run(Args::parse()) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
