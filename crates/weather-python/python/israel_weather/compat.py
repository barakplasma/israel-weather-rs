"""WeatherIL-style objects. Network and cache operations use the native Rust client.

Inspired by Tomer Klein's MIT-licensed py-weatheril; see repository attribution.
"""
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any
from urllib.parse import urljoin, urlsplit
from zoneinfo import ZoneInfo
import math

from ._native import Client, WeatherUnavailableError

TZ = ZoneInfo("Asia/Jerusalem")
HE_DAYS = ["יום שני", "יום שלישי", "יום רביעי", "יום חמישי", "יום שישי", "שבת", "יום ראשון"]


def _number(value, kind=float, default=None):
    try:
        value = kind(value)
        return default if not math.isfinite(value) or value in (-999, -8991) else value
    except (ValueError, TypeError, OverflowError):
        return default


def _time(value):
    try:
        dt = datetime.fromisoformat(value)
        return dt.replace(tzinfo=TZ) if dt.tzinfo is None else dt.astimezone(TZ)
    except (TypeError, ValueError):
        return None


def _mapping(value):
    return value if isinstance(value, dict) else {}


@dataclass
class Hourly:
    language: str = "he"
    hour: str = ""
    forecast_time: datetime | None = None
    created: datetime | None = None
    temperature: int | None = None
    weather_code: int | None = None
    heat_stress_level: int | None = None
    pm10: int | None = None
    relative_humidity: int | None = None
    wind_speed: int | None = None
    wind_direction_id: int | None = None
    wind_chill: int | None = None
    u_v_index: int | None = None
    u_v_i_max: int | None = None
    gust_speed: int | None = None
    precise_temperature: float | None = None
    heat_stress: float | None = None
    rain: float | None = None
    rain_chance: float | None = None
    wave_height: float | None = None
    weather: str = "Nothing"
    wind_direction: int = -1


@dataclass
class Daily:
    language: str = "he"
    date: datetime | None = None
    lid: str = "0"
    weather_code: int | None = None
    minimum_temperature: int | None = None
    maximum_temperature: int | None = None
    maximum_uvi: int | None = None
    u_v_i_factor: float | None = None
    description: str = ""
    hours: list[Hourly] = field(default_factory=list)
    day: str = ""
    location: str = "Nothing"
    weather: str = "Nothing"


@dataclass
class Forecast:
    days: list[Daily] = field(default_factory=list)


@dataclass
class Weather:
    language: str = "he"
    lid: str = "0"
    humidity: int | None = None
    rain_chance: int | None = None
    due_point_temp: int | None = None
    wind_speed: int | None = None
    wind_chill: int | None = None
    wind_direction_id: int | None = None
    heat_stress_level: int | None = None
    u_v_index: int | None = None
    u_v_i_max: int | None = None
    min_temp: int | None = None
    max_temp: int | None = None
    pm10: int | None = None
    weather_code: int | None = None
    gust_speed: int | None = None
    rain: float | None = None
    temperature: float | None = None
    feels_like: float | None = None
    u_v_i_factor: float | None = None
    wave_height: float | None = None
    forecast_time: datetime | None = None
    modified_at: datetime | None = None
    u_v_level: str | None = None
    json: dict[str, Any] = field(default_factory=dict)
    location: str = "Nothing"
    description: str = "Nothing"
    wind_direction: int = -1


@dataclass
class Warning:
    language: str = "he"
    location_id: int = 0
    wid: int | None = None
    alert_id: int | None = None
    severity_id: int | None = None
    warning_type_id: int | None = None
    valid_from_unix: int | None = None
    sent: datetime | None = None
    valid_from: datetime | None = None
    valid_to: datetime | None = None
    full_en: str = ""
    full_he: str = ""
    text: str = ""
    text_full: str = ""
    region_name: str = ""
    severity: str = ""
    warning_type: str = ""
    groups: list[str] = field(default_factory=list)
    regions: list[str] = field(default_factory=list)


@dataclass
class RadarSatellite:
    imsradar_images: list[str] = field(default_factory=list)
    radar_images: list[str] = field(default_factory=list)
    middle_east_satellite_images: list[str] = field(default_factory=list)
    europe_satellite_images: list[str] = field(default_factory=list)


class WeatherIL:
    def __init__(self, location, language="he", cache_expiration_in_sec=30, *, cache_dir=None, offline=False, base_url=None, xml_url=None):
        self.location = str(int(location))
        if not 0 < int(self.location) <= 32767:
            raise ValueError("location must be a positive IMS ID")
        self.language = language
        self.offline = offline
        self.client = Client(language, cache_dir, offline, base_url, xml_url, cache_expiration_in_sec)

    def _data(self, endpoint, lid=None):
        try:
            return self.client.website(endpoint, lid)["payload"]["data"]
        except WeatherUnavailableError:
            return {}

    def _metadata(self):
        location = _mapping(_mapping(self._data("locations_info")).get(self.location))
        codes = _mapping(self._data("weather_codes"))
        wind = _mapping(self._data("wind_directions"))
        if not location:
            try:
                location = next(({"name": p["name"]} for p in self.client.locations() if str(p["location_id"]) == self.location), {})
            except WeatherUnavailableError:
                pass
        return location, codes, wind

    @staticmethod
    def _description(codes, code):
        return _mapping(codes.get(str(code))).get("desc", "Nothing")

    @staticmethod
    def _direction(wind, code):
        return _number(_mapping(wind.get(str(code))).get("direction"), int, -1)

    def get_current_analysis(self) -> Weather | None:
        raw = _mapping(_mapping(self._data("now_analysis", int(self.location))).get(self.location))
        if not raw:
            return None
        location, codes, wind = self._metadata()
        code = _number(raw.get("weather_code"), int)
        direction = _number(raw.get("wind_direction_id"), int)
        ints = ["rain_chance", "wind_speed", "wind_chill", "heat_stress_level", "u_v_index", "u_v_i_max", "min_temp", "max_temp", "pm10", "gust_speed"]
        floats = ["rain", "temperature", "feels_like", "u_v_i_factor", "wave_height"]
        return Weather(
            language=self.language, lid=str(raw.get("lid", self.location)),
            humidity=_number(raw.get("relative_humidity"), int),
            due_point_temp=_number(raw.get("due_point_Temp"), int),
            forecast_time=_time(raw.get("forecast_time")), modified_at=_time(raw.get("modified")),
            u_v_level=raw.get("u_v_level"), json=raw, weather_code=code, wind_direction_id=direction,
            location=location.get("name", "Nothing"), description=self._description(codes, code),
            wind_direction=self._direction(wind, direction),
            **{k: _number(raw.get(k), int) for k in ints},
            **{k: _number(raw.get(k)) for k in floats},
        )

    def get_forecast(self) -> Forecast:
        raw = _mapping(self._data("full_forecast_data", int(self.location)))
        if not raw:
            return Forecast()
        location, codes, wind = self._metadata()
        result = Forecast()
        ints = ["temperature", "heat_stress_level", "pm10", "relative_humidity", "wind_speed", "wind_chill", "u_v_index", "u_v_i_max", "gust_speed"]
        floats = ["precise_temperature", "heat_stress", "rain", "rain_chance", "wave_height"]
        for day, record in sorted(raw.items()):
            date = _time(day)
            if date is None:
                continue
            record = _mapping(record)
            daily = _mapping(record.get("daily"))
            code = _number(daily.get("weather_code"), int)
            hours = []
            for hour, entry in sorted(_mapping(record.get("hourly")).items()):
                entry = _mapping(entry)
                hcode = _number(entry.get("weather_code"), int)
                direction = _number(entry.get("wind_direction_id"), int)
                hours.append(Hourly(
                    language=self.language, hour=hour, forecast_time=_time(entry.get("forecast_time")),
                    created=_time(entry.get("created")), weather_code=hcode, wind_direction_id=direction,
                    weather=self._description(codes, hcode), wind_direction=self._direction(wind, direction),
                    **{k: _number(entry.get(k), int) for k in ints},
                    **{k: _number(entry.get(k)) for k in floats},
                ))
            result.days.append(Daily(
                language=self.language, date=date, lid=str(daily.get("lid", self.location)), weather_code=code,
                minimum_temperature=_number(daily.get("minimum_temperature"), int),
                maximum_temperature=_number(daily.get("maximum_temperature"), int),
                maximum_uvi=_number(daily.get("maximum_uvi"), int),
                u_v_i_factor=_number(daily.get("u_v_i_factor")),
                description=str(_mapping(record.get("country")).get("description") or "").rstrip(),
                hours=hours, day=HE_DAYS[date.weekday()] if self.language == "he" else date.strftime("%A"),
                location=location.get("name", "Nothing"), weather=self._description(codes, code),
            ))
        return result

    def get_warnings(self) -> list[Warning]:
        locations = _mapping(self._data("locations_info"))
        location = _mapping(locations.get(self.location))
        if not location or location.get("rid") is None:
            raise ValueError(f"Location or region not found for ID {self.location}")
        rid = "r-" + str(location["rid"]).removeprefix("r-")
        metadata = _mapping(self._data("warnings_metadata"))
        regions = _mapping(metadata.get("regions"))
        if not regions:
            region_list = self._data("regions")
            regions = {str(item["rid"]): item for item in region_list if isinstance(item, dict) and "rid" in item} if isinstance(region_list, list) else {}
        if rid not in regions:
            raise ValueError(f"Region not found for ID {rid}")
        days = _mapping(_mapping(self._data("warnings")).get("full_warnings_data"))
        warnings = []
        for day in days.values():
            for alert in _mapping(_mapping(day).get(rid)).values():
                alert = _mapping(alert)
                severity = _mapping(_mapping(metadata.get("warning_severity")).get(str(alert.get("severity_id"))))
                kind = _mapping(_mapping(metadata.get("ims_warning_type")).get(str(alert.get("warning_type_id"))))
                text = str(alert.get("text_full") or alert.get("full_en" if self.language == "en" else "full_he") or "").strip()
                warnings.append(Warning(
                    language=self.language, location_id=int(self.location),
                    **{k: _number(alert.get(k), int) for k in ("wid", "alert_id", "severity_id", "warning_type_id", "valid_from_unix")},
                    **{k: _time(alert.get(k)) for k in ("sent", "valid_from", "valid_to")},
                    **{k: str(alert.get(k) or "") for k in ("full_en", "full_he", "text")},
                    text_full=text, region_name=_mapping(regions.get(rid)).get("name", ""),
                    severity=severity.get("severity_name", ""), warning_type=kind.get("name", ""),
                    groups=[_mapping(_mapping(metadata.get("warning_groups")).get("g-" + str(g))).get("name", str(g)) for g in alert.get("groups") or []],
                    regions=[_mapping(regions.get("r-" + str(r))).get("name", str(r)) for r in alert.get("regions") or []],
                ))
        return warnings

    def get_radar_images(self) -> RadarSatellite:
        kinds = _mapping(_mapping(self._data("radar_satellite")).get("types"))

        def urls(items):
            result = []
            for item in items if isinstance(items, list) else []:
                name = _mapping(item).get("file_name")
                if isinstance(name, str) and name:
                    url = urljoin("https://ims.gov.il/", name)
                    parts = urlsplit(url)
                    if parts.scheme == "https" and parts.netloc == "ims.gov.il":
                        result.append(url)
            return result

        return RadarSatellite(
            imsradar_images=urls(kinds.get("IMSRadar")), radar_images=urls(kinds.get("radar")),
            middle_east_satellite_images=urls(_mapping(kinds.get("satellite")).get("NATURAL")),
            europe_satellite_images=urls(kinds.get("EUROPE")),
        )
