import json
from pathlib import Path
from israel_weather import WeatherIL, Weather, Forecast, RadarSatellite

ROOT = Path(__file__).resolve().parents[3]


def put(cache, name, data):
    (cache / f"israel-weather-rs-website-en-{name}.json").write_text(json.dumps(data), encoding="utf-8")


def test_weatheril_public_api(tmp_path):
    bulk = json.loads((ROOT / "tests/fixtures/ims_full_forecast_data.json").read_text())
    put(tmp_path, "full_forecast_data-2", {"data": bulk["data"]["info"]["2"]})
    put(tmp_path, "now_analysis-2", {"data": {"2": {"lid": "2", "temperature": "22.5", "relative_humidity": "70", "rain": "-999", "forecast_time": "2026-10-01 12:00:00"}}})
    put(tmp_path, "locations_info", {"data": {"2": {"lid": "2", "name": "Tel Aviv", "rid": "103"}}})
    put(tmp_path, "weather_codes", {"data": {"1250": {"weather_code": "1250", "desc": "Clear"}}})
    put(tmp_path, "wind_directions", {"data": {"1": {"direction": "0"}}})
    put(tmp_path, "radar_satellite", {"data": {"types": {"IMSRadar": [{"file_name": "/example.gif"}]}}})
    for name in ("warnings", "warnings_metadata"):
        put(tmp_path, name, json.loads((ROOT / f"tests/fixtures/ims_{name}.json").read_text()))
    weather = WeatherIL("2", "en", 30, cache_dir=tmp_path, offline=True, xml_url=str(ROOT / "isr_cities_1week_6hr_forecast.xml"))
    current = weather.get_current_analysis()
    assert isinstance(current, Weather)
    assert current.temperature == 22.5 and current.humidity == 70 and current.rain is None
    assert current.location == "Tel Aviv" and current.forecast_time.tzinfo is not None
    forecast = weather.get_forecast()
    assert isinstance(forecast, Forecast) and forecast.days and any(day.hours for day in forecast.days)
    assert isinstance(weather.get_warnings(), list)
    radar = weather.get_radar_images()
    assert isinstance(radar, RadarSatellite) and radar.imsradar_images == ["https://ims.gov.il/example.gif"]
    assert weather.client.forecast(2)["hourly"]
