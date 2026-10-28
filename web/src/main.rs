//! Web UI for `weather serve`: a Leptos client-side app that talks to the server's
//! Open-Meteo compatible API, using the response types from the shared `weather-api` crate.

mod charts;
mod forecast;
mod icons;
mod model;
mod status;

use leptos::prelude::*;
use serde::de::DeserializeOwned;

/// GET a same-origin JSON endpoint; non-2xx bodies carry `{"error": true, "reason": ...}`.
pub async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let res = gloo_net::http::Request::get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        let reason = res
            .json::<weather_api::ApiError>()
            .await
            .map(|e| e.reason)
            .unwrap_or_else(|_| format!("{} {}", res.status(), res.status_text()));
        return Err(reason);
    }
    res.json().await.map_err(|e| e.to_string())
}

#[component]
fn App() -> impl IntoView {
    let path = window().location().pathname().unwrap_or_default();
    view! {
        <main>
            {if path.trim_end_matches('/') == "/status" {
                status::StatusPage().into_any()
            } else {
                forecast::ForecastPage().into_any()
            }}
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    // Offline support; failures (e.g. plain http on a LAN address) just mean no offline mode.
    let _ = window().navigator().service_worker().register("/sw.js");
    leptos::mount::mount_to_body(App);
}
