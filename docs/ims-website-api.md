# IMS website API: upstream research and Rust integration specification

Status: design/reference, reviewed 2026-11-24. No new routes implemented by this document. These are undocumented website endpoints inferred from source, not an official IMS API contract or a successful live probe. Verify with fresh IMS responses before shipping.

## Sources and licensing

- Tomer Klein's [py-weatheril](https://github.com/t0mer/py-weatheril/tree/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d), MIT: [license](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/LICENSE), [endpoint constants](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/consts.py#L3-L27), [client](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/__init__.py), [metadata/cache](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/utils.py), [radar](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/radar_satellite.py), [warnings](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/warning.py). Retain the [MIT notice](third-party/py-weatheril-LICENSE.txt) for substantial derived material.
- [ims-api](https://github.com/t0mer/ims-api/tree/8cb4300c93b78a23079aca1e630a1f2140723bd6) wraps that library; [route reference](https://github.com/t0mer/ims-api/blob/8cb4300c93b78a23079aca1e630a1f2140723bd6/app/main.py). No explicit license found at that revision. Document factual interface behavior only; no source or static table copied. Ask the author before incorporating its code.
- This project's Cargo.toml declares MIT. IMS website content, data, imagery, and redistribution/attribution terms must be checked independently of software licenses. Link to imagery by default rather than republishing frames until rights and operational constraints are understood.

## Endpoint inventory

All paths below are HTTP GET under `https://ims.gov.il/{language}/`, where language is `he` or `en`. No token is used by the reviewed client. Preserve the configurable IMS base URL for mocks and mirrors. `lid` is a location identifier, not a station ID; obtain valid IDs from location metadata (e.g. 2, Tel Aviv coast). Responses usually wrap content in `data`; do not assume every data value is an object.

| IMS path | Shape inferred from source | Rust status / action |
| --- | --- | --- |
| ims_full_forecast_data | data.info[lid][YYYY-MM-DD].{daily,hourly,country} | Already parsed; keep as primary all-location source |
| full_forecast_data/{lid} | data[YYYY-MM-DD].{daily,hourly,country} | New per-location parser/fallback; adapt to existing bulk parser, do not fetch all locations individually |
| now_analysis/{lid} | data[string lid] = analysis record | New current analysis source; P1 |
| locations_info | data map of location objects with lid, name, lat, lon, rid, sea_rid | Already consumes region mapping; extend typed bilingual catalog |
| warnings | data.full_warnings_data[day]["r-"+rid][alert key] | Already parsed; preserve general-public filtering and region matching; optionally expose full alerts |
| warnings_metadata | data.{ims_warning_type,warning_groups,warning_severity} | Already consumed; expose typed lookup if useful |
| weather_codes | data map with weather_code and desc | Add locale-specific dynamic lookup; retain bundled code fallback |
| wind_directions | data keyed by code; entry contains direction (azimuth) | Add locale-aware dynamic lookup; retain existing compass fallback |
| regions | data array with rid, name and other metadata | New region catalog; preserve r- prefixed ID |
| sea_regions | data map with rid and other fields | New marine region catalog |
| radar_satellite | data.types (see below) | New radar/satellite image manifest; P2 |

Existing separate XML feed: `https://ims.gov.il/sites/default/files/ims_data/xml_files/isr_cities_1week_6hr_forecast.xml`. Do not remove its six-hour sampling/interpolation fallback.

## Current analysis (P1)

Source: [WeatherIL.get_current_analysis](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/__init__.py#L62-L136). Keys expected in the analysis record:

| Fields (upstream spelling) | Suggested Rust representation |
| --- | --- |
| lid | validated integer ID, accept string/number |
| forecast_time, modified | optional Jerusalem-local timestamp, preserve both; serialize RFC3339 with offset |
| temperature, due_point_Temp, feels_like, wind_chill, min_temp, max_temp | optional numeric °C (verify units/rounding) |
| relative_humidity, rain_chance | optional percent |
| rain, wind_speed, gust_speed, wind_direction_id | optional numeric; use mm and km/h per existing forecast conventions pending live verification; direction lookup separately |
| heat_stress_level, weather_code | optional integral code |
| u_v_index, u_v_i_max, u_v_i_factor, u_v_level | optional numeric or text as appropriate |
| wave_height, pm10 | optional numeric; verify units against IMS documentation |
 
The spelling `due_point_Temp` is intentional; normalize to `dew_point_temperature` internally. Upstream current analysis is modeled data, not necessarily a station observation. Include source, fetched_at, modified_at, forecast_time and stale metadata. Do not silently substitute forecast-derived values; if offering that fallback, mark it explicitly.

Synthetic example (illustrative, not captured from IMS):

```json
{"data":{"2":{"lid":"2","temperature":"22.5","relative_humidity":65,"rain":"-999","due_point_Temp":"15","forecast_time":"2026-11-24 08:00:00","modified":"2026-11-24 07:55:00"}}}
```

## Forecast details and sentinel values

[Daily and hourly extraction](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/__init__.py#L138-L244): daily keys include lid, weather_code, minimum_temperature, maximum_temperature, maximum_uvi, u_v_i_factor. Country description is under country.description. Hourly entries are keyed by hour, with forecast_time and created, temperature and precise_temperature, weather_code, heat_stress and heat_stress_level, pm10, relative_humidity, rain and rain_chance, wind_speed, gust_speed, wind_direction_id, wave_height, wind_chill, u_v_index and u_v_i_max.

Preserve precise_temperature preference, sort/deduplicate by timestamp, reuse the existing JSON/XML merge. Audit fields currently lost during normalization (created, heat_stress_level, wind_chill, daily UV factor, wave height and PM10 in normalized points). The Python reference specifically treats rain=-999, gust_speed=-999 and hourly u_v_index=-8991 as missing. Test those exact values as numeric and string, plus null/empty/malformed values; do not assume all negative values are invalid (temperature can be negative). Avoid the upstream client's zero defaults for missing weather. Use Asia/Jerusalem DST handling, reject or explicitly resolve ambiguous timestamps consistently with existing parse_time.

## Radar and satellite (P2)

[Manifest extraction](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/__init__.py#L246-L303): image lists at `data.types.IMSRadar`, `data.types.radar`, `data.types.satellite.NATURAL`, and `data.types.EUROPE`. Each item has at least `file_name`, normally a path on ims.gov.il. Join against `https://ims.gov.il/`, validate same HTTPS origin, reject external hosts, traversal and unsafe schemes; cap size/frame count and avoid arbitrary URL fetching. Preserve upstream ordering until timestamps/ordering are verified. Expose lists first; animations optional later. No per-request frame downloads.

Static image URLs from upstream constants (relative to `https://ims.gov.il/sites/default/files/ims_data/map_images/`): `IMSRadar4GIS.gif`, `Satellite-map.jpg`, `MIDDLE-EAST-map.jpg`, `EUROPE-map.jpg`. Verify freshness and availability; these are separate from the manifest. Upstream creates GIF animations locally from downloaded frames. If implementing that later, use an optional Rust image/animation feature with bounded decoded pixels and memory; do not require Python or image codecs for basic weather.

## Regional warnings and metadata (P2)

[Warning extraction](https://github.com/t0mer/py-weatheril/blob/76ab1d577d7a5336bbf55e7fab42d27c2f361f0d/weatheril/__init__.py#L352-L398) includes wid, alert_id, severity_id, warning_type_id, sent, valid_from, valid_to, full_en, full_he, text, text_full, valid_from_unix, groups, regions. Keep stable alert ID, severity and bilingual text. Compare Unix and local timestamps; do not assume units without fixtures. Preserve existing general-public filter (group 18), land/sea matching and time validity. New full warning API should clearly state whether filtered and deduplicate alerts spanning dates/regions. Metadata caches must key by language (the Python reference uses shared global maps, risking language bleed). Use dynamic location data rather than maintaining copied tables.

## Proposed Rust HTTP API (new, not yet shipped)

Keep all current paths and semantics. Define DTOs in `crates/weather-api/src/ims.rs` or submodules and register OpenAPI schemas in `src/server/openapi.rs`. Prefer normalized, documented endpoints:

| Local GET path | Behavior |
| --- | --- |
| /api/ims/locations?language=en | IMS location catalog, bilingual metadata optional |
| /api/ims/current/{lid}?language=en | Current analysis, source/timestamps/staleness |
| /api/ims/forecast/{lid}?language=en | Native hourly/daily/country data from bulk snapshot, optionally per-location fallback |
| /api/ims/warnings/{lid}?language=en | Regional current/upcoming alerts |
| /api/ims/radar-satellite?language=en | Manifest, categorized frame URLs and static URLs |
| /api/ims/metadata/{kind}?language=en | Allowlisted weather-codes, wind-directions, regions, sea-regions, warnings-metadata |
 
Optionally offer separate IMS-shaped routes `/{language}/now_analysis/{lid}`, `/{language}/full_forecast_data/{lid}` etc. for drop-in upstream-client integration, only after capturing/validating envelopes; avoid falsely claiming that normalized DTOs are wire-compatible. A separate compatibility layer for ims-api's observed GET routes `/locations`, `/weather/current/{location_id}`, `/weather/forecast/{location_id}`, `/health` is possible if demand warrants it. Its current response has a data envelope, forecast response a days envelope, and its hourly forecast is a reduced projection; do not copy its implementation or bug behavior. Existing /api/ims, /v1/forecast, /v1/search, MET Norway and /healthz stay unchanged.

## Implementation and reliability

1. Refactor `src/ims_json.rs` fetch/cache into typed endpoint keys (kind, validated lid, language); current `cache_file(endpoint)` inserts the endpoint directly into a filename and cannot safely handle slash-containing paths or multiple languages. Keep old cache migration/read compatibility; hash or encode keys, atomically replace only after successful schema parsing, enforce timeouts/body limits, use a reusable blocking client. Never accept an arbitrary URL via a downstream route.
2. Extract reusable parsers for shared hourly/daily shapes; add current analysis DTO/parser and fixture coverage first. Introduce an instance-owned client and cache config usable without Axum. Preserve existing env defaults for CLI, but do not mutate global env from Python.
3. For current analysis, use bounded per-location cache with single-flight, TTL (start at 5 minutes, configurable), and bounded refresh concurrency. Forecast remains hourly bulk (existing schedule); metadata can refresh daily; radar manifest ~5 minutes initially. On-demand lazy fetch preferred to polling every location. Keep offline mode truly network-free; return stale records with original fetch time or typed unavailable error. Distinguish successful cache fallback from successful network fetch in status metrics (current fetch returns only T).
4. Integrate into server state without blocking async executor; share immutable snapshots and run blocking work off-thread. Validate language and location; distinguish 400 invalid input, 404 unknown location, 503 unavailable/no cache. Add source-specific health without breaking readiness for existing forecast when optional radar/current fails.
5. Add metadata and radar parsers independently, then optional compatibility adapters and web UI. Avoid mandatory image dependencies. Document request volumes and respect IMS usage terms.

## Acceptance tests

- Offline fixture tests for each endpoint (fresh minimized IMS fixtures after verifying terms, or synthetic fixtures with explicit provenance), both languages, missing/numeric-string/sentinel cases, unknown codes, malformed envelopes, DST, invalid location and empty image categories.
- Mock HTTP test: request paths, cache isolation by language/lid, no per-location forecast fanout, concurrency/single-flight, stale cache on invalid upstream, no network in offline mode, atomic write and filename safety.
- API tests for OpenAPI/response schema, existing compatibility suite, error statuses and provenance. Opt-in live probe against a small set of locations only; record date, shape and source URL, not an availability promise.
- Radar SSRF/redirect/oversize tests; preserve source attribution. Existing tests: cargo test --workspace excluding web as appropriate, cargo test --features server after building web assets, and compat/ client tests. No new network requirement in default CI.
