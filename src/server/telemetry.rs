//! OpenTelemetry for `weather serve`: traces, metrics and logs over OTLP/HTTP protobuf.
//!
//! Configured only by the standard `OTEL_*` environment variables. With none of
//! `OTEL_EXPORTER_OTLP_{,TRACES_,METRICS_,LOGS_}ENDPOINT` set nothing is exported and the
//! service behaves exactly as before. Setup failures and a sink being down never stop the
//! service: exporters run on their own threads and drop data when their queue is full.

use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::propagation::Extractor;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{KeyValue, global};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::{Instrument, field::Empty};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, filter::filter_fn};

/// Target of the per-request spans: exported, but not printed by the stderr logger.
const HTTP_TARGET: &str = "otel_http";
/// Short, so a dead sink cannot hold up a flush for the SDK's 10 s default.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(3);

/// Flushes and stops the exporters. Call once, after the server has stopped.
#[derive(Default)]
pub struct Telemetry {
    traces: Option<SdkTracerProvider>,
    metrics: Option<SdkMeterProvider>,
    logs: Option<SdkLoggerProvider>,
}

impl Telemetry {
    pub fn shutdown(self) {
        // stderr only: logging to OTLP while it shuts down would loop.
        if let Some(Err(e)) = self.traces.map(|p| p.shutdown()) {
            eprintln!("otel: trace shutdown: {e}");
        }
        if let Some(Err(e)) = self.metrics.map(|p| p.shutdown()) {
            eprintln!("otel: metric shutdown: {e}");
        }
        if let Some(Err(e)) = self.logs.map(|p| p.shutdown()) {
            eprintln!("otel: log shutdown: {e}");
        }
    }
}

fn env(key: &str) -> Option<String> {
    crate::non_empty_env(key)
}

/// Exporting for a signal needs the generic or the per-signal endpoint, and no `none` exporter.
fn signal_enabled(signal: &str, exporter_var: &str) -> bool {
    env("OTEL_SDK_DISABLED").is_none_or(|v| !v.eq_ignore_ascii_case("true"))
        && env(exporter_var).is_none_or(|v| !v.eq_ignore_ascii_case("none"))
        && (env("OTEL_EXPORTER_OTLP_ENDPOINT").is_some()
            || env(&format!("OTEL_EXPORTER_OTLP_{signal}_ENDPOINT")).is_some())
}

/// Only set our timeout when the operator did not pick one.
fn timeout_for(signal: &str) -> Duration {
    if env("OTEL_EXPORTER_OTLP_TIMEOUT").is_some()
        || env(&format!("OTEL_EXPORTER_OTLP_{signal}_TIMEOUT")).is_some()
    {
        Duration::from_secs(10)
    } else {
        EXPORT_TIMEOUT
    }
}

/// Installs the global logger. Run before the async runtime starts: the blocking HTTP
/// client of the exporters must not be created inside it.
pub fn init(default_filter: &str) -> Telemetry {
    let mut t = Telemetry::default();
    let resource = opentelemetry_sdk::Resource::builder().build();

    if signal_enabled("TRACES", "OTEL_TRACES_EXPORTER") {
        match opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_timeout(timeout_for("TRACES"))
            .build()
        {
            Ok(exporter) => {
                t.traces = Some(
                    SdkTracerProvider::builder()
                        .with_resource(resource.clone())
                        .with_batch_exporter(exporter)
                        .build(),
                );
            }
            Err(e) => eprintln!("otel: trace exporter disabled: {e}"),
        }
    }
    if signal_enabled("METRICS", "OTEL_METRICS_EXPORTER") {
        match opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_timeout(timeout_for("METRICS"))
            .build()
        {
            Ok(exporter) => {
                let provider = SdkMeterProvider::builder()
                    .with_resource(resource.clone())
                    .with_periodic_exporter(exporter)
                    .build();
                global::set_meter_provider(provider.clone());
                t.metrics = Some(provider);
            }
            Err(e) => eprintln!("otel: metric exporter disabled: {e}"),
        }
    }
    if signal_enabled("LOGS", "OTEL_LOGS_EXPORTER") {
        match opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_timeout(timeout_for("LOGS"))
            .build()
        {
            Ok(exporter) => {
                t.logs = Some(
                    SdkLoggerProvider::builder()
                        .with_resource(resource)
                        .with_batch_exporter(exporter)
                        .build(),
                );
            }
            Err(e) => eprintln!("otel: log exporter disabled: {e}"),
        }
    }
    global::set_text_map_propagator(TraceContextPropagator::new());

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let fmt = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_span_events(FmtSpan::CLOSE)
        .json()
        .with_filter(filter_fn(|m| m.target() != HTTP_TARGET));
    let trace_layer = t
        .traces
        .as_ref()
        .map(|p| tracing_opentelemetry::layer().with_tracer(p.tracer("israel-weather-rs")));
    // The exporters' own crates log through `tracing`: keep that out of the OTLP log pipe,
    // where a failing export would log about itself forever.
    let log_layer = t.logs.as_ref().map(|p| {
        OpenTelemetryTracingBridge::new(p).with_filter(filter_fn(|m| {
            !["opentelemetry", "hyper", "reqwest", "h2", "tower"]
                .iter()
                .any(|p| m.target().starts_with(p))
        }))
    });
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt)
        .with(trace_layer)
        .with(log_layer)
        .try_init();
    t
}

struct Headers<'a>(&'a axum::http::HeaderMap);

impl Extractor for Headers<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Route-layer middleware: one server span and one duration sample per request, named by the
/// route pattern. Probes are skipped. The query string is never recorded.
pub async fn http(path: MatchedPath, req: Request, next: Next) -> Response {
    let route = path.as_str().to_owned();
    if route == "/healthz" {
        return next.run(req).await;
    }
    let method = req.method().as_str().to_owned();
    let span = tracing::info_span!(
        target: HTTP_TARGET,
        "request",
        otel.name = %format!("{method} {route}"),
        otel.kind = "server",
        otel.status_code = Empty,
        http.request.method = %method,
        http.route = %route,
        url.path = %req.uri().path(),
        http.response.status_code = Empty,
    );
    let _ = span.set_parent(global::get_text_map_propagator(|p| {
        p.extract(&Headers(req.headers()))
    }));
    let started = Instant::now();
    let response = next.run(req).instrument(span.clone()).await;
    let status = response.status().as_u16();
    span.record("http.response.status_code", status);
    if status >= 500 {
        span.record("otel.status_code", "ERROR");
    }
    global::meter("israel-weather-rs")
        .f64_histogram("http.server.request.duration")
        .with_unit("s")
        .build()
        .record(
            started.elapsed().as_secs_f64(),
            &[
                KeyValue::new("http.request.method", method),
                KeyValue::new("http.route", route),
                KeyValue::new("http.response.status_code", i64::from(status)),
            ],
        );
    response
}

/// Counts a forecast refresh (`result` = `ok` or `failed`) and, on success, how many
/// locations the snapshot holds.
pub fn record_refresh(ok: bool, locations: usize) {
    let meter = global::meter("israel-weather-rs");
    meter.u64_counter("weather.refreshes").build().add(
        1,
        &[KeyValue::new("result", if ok { "ok" } else { "failed" })],
    );
    if ok {
        meter
            .u64_gauge("weather.forecast.locations")
            .build()
            .record(locations as u64, &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_exported_without_an_endpoint() {
        // The test environment sets no OTEL_* variables.
        assert!(!signal_enabled("TRACES", "OTEL_TRACES_EXPORTER"));
        assert!(!signal_enabled("METRICS", "OTEL_METRICS_EXPORTER"));
        assert!(!signal_enabled("LOGS", "OTEL_LOGS_EXPORTER"));
    }
}
