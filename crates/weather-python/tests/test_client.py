import json
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from importlib.metadata import version

import pytest
from israel_weather import Client, WeatherUnavailableError

ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests" / "fixtures"


def put(cache, name, payload):
    (cache / f"israel-weather-rs-website-en-{name}.json").write_text(
        json.dumps(payload), encoding="utf-8"
    )


def client_for(cache):
    return Client(cache_dir=cache, offline=True, xml_url=str(ROOT / "isr_cities_1week_6hr_forecast.xml"))


def test_installed_package():
    assert version("israel-weather-rs") == "0.7.1a1"


def test_offline_native_and_normalized_forecast(tmp_path):
    bulk = json.loads((FIXTURES / "ims_full_forecast_data.json").read_text())
    put(tmp_path, "full_forecast_data-2", {"data": bulk["data"]["info"]["2"]})
    put(tmp_path, "now_analysis-2", {"data": {"2": {"temperature": "22.5", "rain": "-999", "extra": True}}})
    put(tmp_path, "radar_satellite", {"data": {"types": {"IMSRadar": [{"file_name": "/image.gif"}]}}})
    put(tmp_path, "warnings", json.loads((FIXTURES / "ims_warnings.json").read_text()))
    client = client_for(tmp_path)
    assert client.refresh() > 0
    assert any(p["location_id"] == 2 for p in client.locations())
    result = client.forecast(2)
    assert result["hourly"]
    assert any(p["source"] == "ims" for p in result["hourly"])
    assert datetime.fromisoformat(result["hourly"][0]["time"]).tzinfo is not None
    assert result["daily"]
    assert result["native_fetched_at"]
    assert client.current_analysis(2)["payload"]["data"]["2"]["extra"] is True
    assert client.radar_images()["payload"]["data"]["types"]["IMSRadar"][0]["file_name"] == "/image.gif"
    assert client.warnings()["payload"]["data"]["full_warnings_data"]
    with ThreadPoolExecutor(max_workers=4) as pool:
        assert all(pool.map(lambda _: client.forecast(2)["hourly"], range(4)))


def test_xml_fallback_language_isolation_and_errors(tmp_path):
    client = client_for(tmp_path)
    result = client.forecast(2)
    assert result["hourly"]
    assert any(p["source"] == "interpolated" for p in result["hourly"])
    assert all(p["wind_gust"] is None and p["precipitation_probability"] is None for p in result["hourly"])
    assert result["native_fetched_at"] is None
    put(tmp_path, "now_analysis-2", {"data": {"2": {"temperature": "22.5"}}})
    hebrew = Client(language="he", cache_dir=tmp_path, offline=True)
    with pytest.raises(WeatherUnavailableError):
        hebrew.current_analysis(2)
    with pytest.raises(ValueError):
        client.website("invalid")
    with pytest.raises(ValueError):
        client.forecast(0)
    with pytest.raises(ValueError):
        Client(language="fr")
