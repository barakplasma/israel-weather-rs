"""Third-party Open-Meteo and MET Norway clients against `weather serve`.

Run against a live server: WEATHER_BASE_URL=http://127.0.0.1:8080 pytest
"""

import asyncio
import datetime as dt
import os
import zoneinfo

import aiohttp
import metno
import metno_locationforecast
import pytest
from open_meteo import DailyParameters, HourlyParameters, OpenMeteo
from yarl import URL

BASE = os.environ.get("WEATHER_BASE_URL", "")
pytestmark = pytest.mark.skipif(not BASE, reason="WEATHER_BASE_URL not set")

TEL_AVIV = (32.08, 34.78)
ISRAEL = zoneinfo.ZoneInfo("Asia/Jerusalem")


class LocalOpenMeteo(OpenMeteo):
    """The client has its hosts built in; send its requests to our server instead."""

    async def _request(self, url: URL) -> str:
        base = URL(BASE)
        return await super()._request(url.with_scheme(base.scheme).with_host(base.host).with_port(base.port))


def run(coro):
    return asyncio.run(coro)


async def _open_meteo(call):
    async with aiohttp.ClientSession() as session:
        return await call(LocalOpenMeteo(session=session))


def test_open_meteo_forecast():
    lat, lon = TEL_AVIV
    f = run(
        _open_meteo(
            lambda om: om.forecast(
                latitude=lat,
                longitude=lon,
                timezone="Asia/Jerusalem",
                forecast_days=2,
                current=[HourlyParameters.TEMPERATURE_2M, HourlyParameters.WEATHER_CODE],
                hourly=[
                    HourlyParameters.TEMPERATURE_2M,
                    HourlyParameters.PRECIPITATION,
                    HourlyParameters.PRECIPITATION_PROBABILITY,
                    HourlyParameters.WEATHER_CODE,
                    HourlyParameters.WIND_SPEED_10M,
                ],
                daily=[
                    DailyParameters.TEMPERATURE_2M_MAX,
                    DailyParameters.TEMPERATURE_2M_MIN,
                    DailyParameters.PRECIPITATION_SUM,
                ],
            )
        )
    )
    assert f.timezone == "Asia/Jerusalem"
    assert abs(f.latitude - lat) < 0.2 and abs(f.longitude - lon) < 0.2
    assert len(f.hourly.time) == 48
    assert len(f.hourly.temperature_2m) == 48
    # Local times, like Open-Meteo: naive, in the requested zone.
    assert f.hourly.time[0].hour == 0
    assert f.hourly_units.temperature_2m == "°C"
    assert len(f.daily.time) == 2
    assert f.daily.temperature_2m_max[0] >= f.daily.temperature_2m_min[0]
    assert f.current.temperature_2m is not None


def test_open_meteo_geocoding():
    g = run(_open_meteo(lambda om: om.geocoding(name="Haifa", count=3)))
    assert g.results, "Haifa should match"
    haifa = g.results[0]
    assert haifa.name.startswith("Haifa")
    assert haifa.country_code == "IL"
    assert 32 < haifa.latitude < 33.5

    none = run(_open_meteo(lambda om: om.geocoding(name="Atlantis")))
    assert not none.results


def test_pymetno_complete():
    lat, lon = TEL_AVIV

    async def fetch():
        async with aiohttp.ClientSession() as session:
            data = metno.MetWeatherData(
                {"lat": lat, "lon": lon},
                session,
                api_url=f"{BASE}/weatherapi/locationforecast/2.0/complete",
            )
            assert await data.fetching_data()
            return data

    data = run(fetch())
    now = data.get_current_weather()
    assert now["temperature"] is not None
    assert now["condition"], "symbol_code should map to a Home Assistant condition"
    hourly = data.get_forecast(ISRAEL, hourly=True)
    assert len(hourly) >= 20
    assert all(h["temperature"] is not None for h in hourly)
    daily = data.get_forecast(ISRAEL)
    assert len(daily) >= 4
    assert all(d["temperature"] >= d["templow"] for d in daily)


def test_metno_locationforecast_compact(tmp_path):
    lat, lon = TEL_AVIV
    place = metno_locationforecast.Place("Tel Aviv", lat, lon)
    forecast = metno_locationforecast.Forecast(
        place,
        user_agent="israel-weather-rs compat test https://github.com/barakplasma/israel-weather-rs",
        forecast_type="compact",
        save_location=str(tmp_path),
        base_url=f"{BASE}/weatherapi/locationforecast/2.0/",
    )
    assert forecast.update() == "Data-Modified"
    d = forecast.data
    assert d.expires > d.updated_at
    assert len(d.intervals) > 24
    first = d.intervals[0]
    assert "air_temperature" in first.variables
    assert first.variables["air_temperature"].units == "celsius"
    assert d.intervals[0].start_time <= dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=1)
    # A second update within the expiry window must not refetch.
    assert forecast.update() == "Data-Not-Expired"
