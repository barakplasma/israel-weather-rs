from os import PathLike
from typing import Any, Literal, TypedDict

class WeatherUnavailableError(RuntimeError): ...

class Location(TypedDict):
    location_id: int
    name: str
    latitude: float
    longitude: float
    elevation: float

class HourlyPoint(TypedDict):
    time: str
    temperature: float
    relative_humidity: float
    dew_point: float
    feels_like: float
    wind_speed: float
    wind_gust: float | None
    wind_direction: float
    uv_index: float | None
    uv_index_max: float | None
    precipitation: float
    precipitation_probability: float | None
    weather_code: int
    source: Literal["ims", "interpolated"]

class DailySummary(TypedDict):
    date: str
    min_temp: float | None
    max_temp: float | None
    weather_code: int | None
    uv_index_max: float | None

class Forecast(TypedDict):
    location_id: int
    hourly: list[HourlyPoint]
    daily: list[DailySummary]
    native_fetched_at: str | None
    native_stale: bool | None

class WebsiteResult(TypedDict):
    payload: dict[str, Any]
    fetched_at: str
    stale: bool

class Client:
    def __init__(self, language: Literal["en", "he"] = "en", cache_dir: str | PathLike[str] | None = None, offline: bool = False, base_url: str | None = None, xml_url: str | None = None) -> None: ...
    def refresh(self) -> int: ...
    def locations(self) -> list[Location]: ...
    def forecast(self, location_id: int) -> Forecast: ...
    def current_analysis(self, location_id: int) -> WebsiteResult: ...
    def warnings(self) -> WebsiteResult: ...
    def radar_images(self) -> WebsiteResult: ...
    def website(self, endpoint: str, location_id: int | None = None) -> WebsiteResult: ...
