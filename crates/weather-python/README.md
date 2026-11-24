# israel-weather-rs (Python)

Native PyO3 client; no running server, subprocess, or Python HTTP dependency.

```python
from israel_weather import Client
client = Client(language="en", cache_dir="/tmp/ims-cache")
print(client.locations())
print(client.forecast(2)["hourly"][0])
print(client.current_analysis(2))
```

Use offline=True to prohibit network calls. XML is loaded lazily; call refresh() to reload it. Normalized forecasts overlay native hourly data on XML, with XML fallback. Native website responses contain payload, fetched_at (UTC ISO 8601) and stale; native fields and sentinels remain intact. Dates and times in normalized results are ISO 8601 strings. locations() returns XML locations; website("locations_info") returns localized metadata. website() supports all ten native endpoints. warnings() returns the unfiltered warning envelope; radar_images() returns the manifest, not image bytes. Missing native data raises WeatherUnavailableError; invalid input raises ValueError. For async applications use asyncio.to_thread. Rust I/O releases the GIL.

Build locally using maturin develop in a virtual environment. CI validates and tests ABI3 wheels on Python 3.10 and 3.14. Download wheels from GitHub Actions or Releases; PyPI publication is not enabled.
