# Native Python bindings (PyO3 + Maturin)

Status: prerelease candidate; CI must pass before distribution.

```mermaid
flowchart TD
    IMS[IMS XML and website JSON] --> Core[Rust parsers and persistent cache]
    Core --> CLI[weather CLI]
    Core --> Server[Axum server]
    Core --> Client[Instance-scoped Rust Client]
    Client --> Bind[PyO3 native extension]
    Bind --> Python[israel_weather]
```

The synchronous cache store is shared by the native client and server; both use the existing parser/validation logic. The native crate is isolated from the default Rust workspace and disables the server feature. Client settings are instance-scoped: locale, cache directory, offline flag and upstream URLs. No process environment changes.

The client exposes refresh(), locations(), forecast(location_id), current_analysis(location_id), warnings(), radar_images() and website(endpoint, location_id=None). Normalized forecast uses XML expansion and the existing merge algorithm, with XML fallback. Native requests return payload, fetched_at and stale. The native warning envelope is unfiltered; radar is a manifest. Ordinary Python dictionaries/lists, typed stubs and ISO timestamps are provided. The GIL is released during blocking Rust operations.

## Distribution and tests

Official PyO3/maturin-action builds cp310-abi3 wheels on Linux x86_64 and ARM64 only. CI checks wheel metadata, clean installation, pip dependencies, fixture parity, threading, offline fallback, locale isolation and unknown field preservation on Python 3.10 and 3.14. Artifacts are attached to a release only after all platform jobs pass. No automatic PyPI publishing. Separate lockfile should be committed once CI resolves it. See [official PyO3 guide](https://pyo3.rs/) and [Maturin](https://www.maturin.rs/).

Future: filtered warnings, richer native Python payload stubs, datetime objects, macOS/Windows wheels, and an optional py-weatheril adapter.

## Compatibility facade

`from israel_weather import WeatherIL` provides the same positional constructor (location, language="he", cache_expiration_in_sec=30), four primary methods and named result dataclasses with timezone-aware dates. It uses the native Client, not requests. Expose the raw and normalized extensions through Client and weather.client. Missing values are None; image lists are supported but animation and private helper modules are not in the prerelease. No conflicting `weatheril` import is installed. Credit: [py-weatheril](https://github.com/t0mer/py-weatheril) (MIT).

## BoxD CI

Linux-only wheel builds prefer isolated ephemeral BoxD VMs for trusted push/release/manual jobs when the repository secret BOXD_API_KEY exists. GitHub Actions orchestrates, copies source into the VM, retrieves tested wheels and removes the VM. Fork/untrusted pull requests or missing credentials use the official Maturin action on native Linux runners instead. The portable `scripts/build-python-linux.sh` uses manylinux 2_28, the Rust toolchain and both CPython test versions; ARM64 may require QEMU on x86_64 BoxD hosts. BoxD capability and authentication must be verified before calling the integration operational. Do not expose BoxD credentials to untrusted PR code.
