use serde_json;
use weather::get_israeli_weather_forecast;

fn main() {
    let forecasts = get_israeli_weather_forecast().expect("failed to get forecast");

    let json = serde_json::json!(forecasts);
    // pretty print as json
    println!(
        "{}",
        serde_json::to_string_pretty(&json).expect("failed to serialize weather forecast to json")
    );
}
