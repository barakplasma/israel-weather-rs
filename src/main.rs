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
    println!("Average Temperature: {}", location.location_data.forecast.iter().map(|f| f.temperature).sum::<i16>() / location.location_data.forecast.len() as i16);
}