fn main() {
    let weather_url = "https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml";
    let forecast_xml = ureq::get(weather_url)
        .call()
        .expect("failed to fetch forecast")
        .into_string()
        .expect("invalid xml");

    let forecast = roxmltree::Document::parse(&forecast_xml).expect("failed to parse xml");

    let tlv = forecast
        .descendants()
        .find(|node| {
            return node
                .children()
                .find(|node| node.tag_name().name() == "LocationId" && node.text() == Some("2"))
                .is_some();
        })
        .expect("failed to find TLV LocationMetaData")
        .parent()
        .expect("failed to find TLV parent");

    let tlv_forecasts = tlv
        .children()
        .filter(|node| node.tag_name().name() == "LocationData")
        .collect::<Vec<roxmltree::Node>>();

    // let avg_temp = temps.fold((0.0, 0), |(sum, count), temp| (sum + temp, count + 1));
    println!("{:?}", tlv_forecasts);
}
