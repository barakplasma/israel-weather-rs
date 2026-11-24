//! The OpenAPI document, generated from the handlers' `#[utoipa::path]` and the wire types'
//! `ToSchema` derives, so it cannot drift from the code. Served at `/openapi.json`.

use axum::Json;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "israel-weather-rs",
        description = "IMS forecasts behind Open-Meteo and MET Norway compatible APIs"
    ),
    paths(
        super::openmeteo::forecast,
        super::metno::compact,
        super::api::search,
        super::api::extras,
        super::api::status,
        super::ims_website::current,
        super::ims_website::forecast,
        super::ims_website::locations,
        super::ims_website::warnings,
        super::ims_website::warnings_metadata,
        super::ims_website::weather_codes,
        super::ims_website::wind_directions,
        super::ims_website::regions,
        super::ims_website::sea_regions,
        super::ims_website::radar_satellite,
    ),
    components(schemas(weather_api::ApiError))
)]
struct Doc;

pub async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(Doc::openapi())
}

#[cfg(test)]
mod tests {
    use crate::server::test_support::*;

    #[tokio::test]
    async fn every_route_is_documented() {
        let (status, doc) = get_json(state_at("2026-09-30T12:00:00Z"), "/openapi.json").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        for path in [
            "/v1/forecast",
            "/v1/search",
            "/api/ims",
            "/api/status",
            "/{language}/now_analysis/{lid}",
            "/{language}/full_forecast_data/{lid}",
            "/{language}/locations_info",
            "/{language}/warnings",
            "/{language}/warnings_metadata",
            "/{language}/weather_codes",
            "/{language}/wind_directions",
            "/{language}/regions",
            "/{language}/sea_regions",
            "/{language}/radar_satellite",
            "/weatherapi/locationforecast/2.0/compact",
        ] {
            assert!(doc["paths"][path]["get"].is_object(), "{path} missing");
        }
        assert!(doc["components"]["schemas"]["Forecast"].is_object());
    }
}
