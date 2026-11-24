use std::path::PathBuf;

use israel_weather_rs::Error;
use israel_weather_rs::client::{Client as CoreClient, ClientConfig};
use israel_weather_rs::ims_website::Endpoint;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use serde_json::Value;

pyo3::create_exception!(_native, WeatherUnavailableError, PyRuntimeError);

fn error(error: Error) -> PyErr {
    match error {
        Error::LocationNotFound { .. } => PyValueError::new_err(error.to_string()),
        _ => WeatherUnavailableError::new_err(error.to_string()),
    }
}

fn to_python(py: Python<'_>, value: Value) -> PyResult<Py<PyAny>> {
    Ok(py
        .import("json")?
        .call_method1("loads", (value.to_string(),))?
        .unbind())
}

#[pyclass(module = "israel_weather._native")]
struct Client {
    core: CoreClient,
}

#[pymethods]
impl Client {
    #[new]
    #[pyo3(signature = (language="en", cache_dir=None, offline=false, base_url=None, xml_url=None))]
    fn new(
        language: &str,
        cache_dir: Option<PathBuf>,
        offline: bool,
        base_url: Option<String>,
        xml_url: Option<String>,
    ) -> PyResult<Self> {
        if !matches!(language, "en" | "he") {
            return Err(PyValueError::new_err("language must be en or he"));
        }
        let mut config = ClientConfig {
            language: language.into(),
            offline,
            ..ClientConfig::default()
        };
        if let Some(path) = cache_dir {
            config.cache_dir = path;
        }
        if let Some(url) = base_url {
            config.ims_base_url = url;
        }
        if let Some(url) = xml_url {
            config.xml_url = url;
        }
        Ok(Self {
            core: CoreClient::new(config).map_err(error)?,
        })
    }

    fn refresh(&self, py: Python<'_>) -> PyResult<usize> {
        py.detach(|| self.core.refresh()).map_err(error)
    }

    fn locations(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let value = py.detach(|| self.core.locations()).map_err(error)?;
        to_python(py, value)
    }

    fn forecast(&self, py: Python<'_>, location_id: i16) -> PyResult<Py<PyAny>> {
        if location_id <= 0 {
            return Err(PyValueError::new_err("location_id must be positive"));
        }
        let value = py.detach(|| self.core.forecast(location_id)).map_err(error)?;
        to_python(py, value)
    }

    #[pyo3(signature = (endpoint, location_id=None))]
    fn website(
        &self,
        py: Python<'_>,
        endpoint: &str,
        location_id: Option<i16>,
    ) -> PyResult<Py<PyAny>> {
        let endpoint = match endpoint {
            "now_analysis" => Endpoint::Current,
            "full_forecast_data" => Endpoint::Forecast,
            "locations_info" => Endpoint::Locations,
            "warnings" => Endpoint::Warnings,
            "warnings_metadata" => Endpoint::WarningsMetadata,
            "weather_codes" => Endpoint::WeatherCodes,
            "wind_directions" => Endpoint::WindDirections,
            "regions" => Endpoint::Regions,
            "sea_regions" => Endpoint::SeaRegions,
            "radar_satellite" => Endpoint::RadarSatellite,
            _ => return Err(PyValueError::new_err("unsupported IMS endpoint")),
        };
        if matches!(endpoint, Endpoint::Current | Endpoint::Forecast) != location_id.is_some()
            || location_id.is_some_and(|id| id <= 0)
        {
            return Err(PyValueError::new_err("invalid location ID for endpoint"));
        }
        let value = py
            .detach(|| self.core.website(endpoint, location_id))
            .map_err(error)?;
        to_python(py, value)
    }

    fn current_analysis(&self, py: Python<'_>, location_id: i16) -> PyResult<Py<PyAny>> {
        self.website(py, "now_analysis", Some(location_id))
    }

    fn warnings(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.website(py, "warnings", None)
    }

    fn radar_images(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.website(py, "radar_satellite", None)
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Client>()?;
    m.add(
        "WeatherUnavailableError",
        m.py().get_type::<WeatherUnavailableError>(),
    )?;
    Ok(())
}
