//! Wire types shared by the `weather serve` HTTP server and its embedded web UI.
//!
//! The server serializes these and the WASM client deserializes the same structs, so a change
//! to a response shape fails to compile on whichever side was not updated.
//! Only depends on serde/chrono so it builds for `wasm32-unknown-unknown`.

pub mod geocoding;
pub mod ims;
pub mod openmeteo;
pub mod status;

/// Error body in Open-Meteo's style, returned with a non-2xx status by every endpoint.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ApiError {
    pub error: bool,
    pub reason: String,
}
