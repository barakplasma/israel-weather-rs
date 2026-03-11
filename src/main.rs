use clap::Parser;
use tracing::debug;

/// Downloads and Caches Israeli weather forecast from https://ims.gov.il and prints the next forecast for a location as json
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Location to check weather for
    #[arg(short, long, default_value_t = String::from("Tel Aviv Coast"))]
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

    let json = if args.all {
        serde_json::to_string_pretty(&weather_data)
    } else {
        let desired_location = israel_weather_rs::find_location(&args.location, &weather_data);
        let next_forecasts = israel_weather_rs::forecasts_for_location_for_next_n_hours(
            args.next,
            desired_location,
            chrono::Utc::now(),
        );
        debug!("{:?}", next_forecasts);
        serde_json::to_string_pretty(&next_forecasts)
    };

    println!("{}", json.expect("could not serialize to json"));
}
