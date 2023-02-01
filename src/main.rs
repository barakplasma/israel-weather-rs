use weather::get_israeli_weather_forecast;

fn main() {
    let forecasts = get_israeli_weather_forecast().expect("failed to get forecast");

    println!("{:#?}", forecasts.location)
}