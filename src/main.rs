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

// fn which calculates average temperature for a location
fn average_temperature(location: &weather::Location) -> i16 {
    location.location_data.forecast.iter().map(|f| f.temperature).sum::<i16>() / location.location_data.forecast.len() as i16
}