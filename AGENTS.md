# Agent guide

## IMS upstream research and related implementations

Before adding or changing IMS website integrations, read [the IMS endpoint reference](docs/ims-website-api.md) and [the Python binding plan](docs/python-bindings.md).

- [t0mer/py-weatheril](https://github.com/t0mer/py-weatheril): MIT-licensed Python client; useful reference for per-location current analysis and forecasts, bilingual metadata, regional warnings and radar/satellite images.
- [t0mer/ims-api](https://github.com/t0mer/ims-api): FastAPI wrapper around py-weatheril; useful for understanding public route conventions. No explicit license at the reviewed revision: study the implementation freely as a technical reference (routes, behavior, algorithms, upstream request patterns), but independently implement the Rust version; do not copy its source, location table or README unless the author confirms a license. The MIT license on py-weatheril does not automatically license its wrapper.
- The reference contains pinned source links and attribution. Preserve the upstream MIT notice when incorporating substantial portions of py-weatheril. IMS data/image reuse is a separate licensing question.

Prefer extending the existing Rust implementation over a Python runtime dependency. Keep bulk forecast ingestion, XML fallback, persistent caching, Open-Meteo/MET Norway compatibility, and offline operation intact. Treat undocumented IMS shapes as provisional until fixture-tested. Do not call IMS for every downstream request; use bounded, shared caches. Record freshness and provenance; never silently turn missing current conditions into zeros or observations.

Relevant code: `src/ims_json.rs`, `src/lib.rs`, `src/hourly.rs`, `src/server/mod.rs`, `src/server/api.rs`, `crates/weather-api/src/ims.rs`, `tests/fixtures/`. Prefer offline tests; make live probes opt-in. Python bindings should work with the server feature disabled and share the same Rust data/cache layer.
