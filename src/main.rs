use std::process::ExitCode;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use tracing::debug;

/// Downloads and Caches Israeli weather forecast from https://ims.gov.il and prints the next forecast for a location as json
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

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
    #[arg(short, long, default_value_t = false, global = true)]
    offline: bool,

    /// List available location names and exit
    #[arg(long, default_value_t = false, conflicts_with = "all")]
    list_locations: bool,

    /// Pretend the current time is this RFC 3339 timestamp (e.g. 2025-03-08T23:00:00Z)
    #[arg(long, value_parser = parse_now)]
    now: Option<DateTime<Utc>>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a web server with a web UI plus Open-Meteo and MET Norway compatible APIs
    #[cfg(feature = "server")]
    Serve(ServeArgs),
}

#[cfg(feature = "server")]
#[derive(clap::Args, Debug)]
struct ServeArgs {
    /// Address to listen on
    #[arg(long, env = "WEATHER_LISTEN", default_value = "127.0.0.1:8080")]
    listen: std::net::SocketAddr,

    /// Cron expression (Israel time) for refreshing the forecast.
    /// 5 fields ("7 * * * *") or 6 with seconds ("0 7 * * * *")
    #[arg(long, env = "WEATHER_SCHEDULE", default_value = israel_weather_rs::server::DEFAULT_SCHEDULE)]
    schedule: String,
}

#[cfg(feature = "server")]
fn serve(args: ServeArgs, offline: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Validate before starting the runtime so typos fail fast.
    israel_weather_rs::server::parse_schedule(&args.schedule)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(israel_weather_rs::server::serve(
            israel_weather_rs::server::ServeOptions {
                listen: args.listen,
                schedule: args.schedule,
                offline,
            },
        ))
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
    let args = Args::parse();
    let result = match args.command {
        #[cfg(feature = "server")]
        Some(Command::Serve(serve_args)) => {
            israel_weather_rs::init_logging_with_default("info");
            serve(serve_args, args.offline).map(|()| String::new())
        }
        None => {
            israel_weather_rs::init_logging();
            run(args)
        }
    };
    match result {
        Ok(output) if output.is_empty() => ExitCode::SUCCESS,
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
