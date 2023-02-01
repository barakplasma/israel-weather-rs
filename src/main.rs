use weather::get_israeli_weather_forecast;

fn main() {
    let forecasts = get_israeli_weather_forecast().expect("failed to get forecast");

    forecasts.location.iter().for_each(|location| {
        print_location(location);
    });
}

fn print_location(location: &weather::Location) {
    println!("");
    println!("Location: {}", location.location_meta_data.location_name_eng);
    println!("Most Recent Forecast:");
    let most_recent_forecast = location.location_data.forecast.last().expect("failed to get most recent forecast");
    if Some(most_recent_forecast) != None {
        println!("Time: {}", most_recent_forecast.forecast_time);
        println!("Temperature: {}", most_recent_forecast.temperature);
        println!("Wind Direction: {}", most_recent_forecast.wind_direction);
        println!("Wind Speed: {}", most_recent_forecast.wind_speed);
        println!("Rain: {}", most_recent_forecast.rain);
        println!("");
    }
}