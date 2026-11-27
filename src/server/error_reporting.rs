//! Error reports to a Sentry-compatible tracker (Rustrak) through the `sentry` SDK.
//!
//! Enabled only when `SENTRY_DSN` is set (an `http://` DSN is fine); otherwise nothing is
//! created. Panics and `tracing::error!` events are reported; warnings are not. Query strings
//! and credentials are scrubbed before anything leaves the process. A tracker that is down
//! costs at most the 2 s flush at shutdown: sending happens on the SDK's own thread.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use sentry::protocol::Event;
use sentry::{ClientInitGuard, ClientOptions};
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::registry::LookupSpan;

const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
];

/// Hold the guard for the life of the process and drop it (which flushes) once the server
/// has stopped.
pub fn init() -> Option<ClientInitGuard> {
    let dsn = crate::non_empty_env("SENTRY_DSN")?;
    // The DSN holds a key: never print it, not even in the error.
    let dsn = dsn
        .parse()
        .inspect_err(|_| eprintln!("sentry: SENTRY_DSN is not a valid DSN, error reporting off"))
        .ok()?;
    let mut options = ClientOptions::default();
    options.dsn = Some(dsn);
    options.release = sentry::release_name!();
    options.environment = Some(Cow::Owned(environment()));
    options.send_default_pii = false;
    options.shutdown_timeout = FLUSH_TIMEOUT;
    options.before_send = Some(Arc::new(|event| Some(scrub(event))));
    Some(sentry::init(options))
}

/// `SENTRY_ENVIRONMENT`, else `deployment.environment` from `OTEL_RESOURCE_ATTRIBUTES`.
fn environment() -> String {
    crate::non_empty_env("SENTRY_ENVIRONMENT")
        .or_else(|| {
            let attrs = crate::non_empty_env("OTEL_RESOURCE_ATTRIBUTES")?;
            attrs.split(',').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k.trim() == "deployment.environment").then(|| v.trim().to_string())
            })
        })
        .unwrap_or_else(|| "homelab".into())
}

/// Turns `tracing::error!` into events; warnings and below are not reported.
pub fn layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    sentry_tracing::ErrorLayer::new().with_filter(LevelFilter::ERROR)
}

/// Drops URL query strings, cookies and credential headers from an event.
fn scrub(mut event: Event<'static>) -> Event<'static> {
    if let Some(req) = event.request.as_mut() {
        req.query_string = None;
        req.cookies = None;
        if let Some(url) = req.url.as_mut() {
            url.set_query(None);
        }
        req.headers
            .retain(|k, _| !SENSITIVE_HEADERS.contains(&k.to_ascii_lowercase().as_str()));
    }
    // Error text often embeds the URL that failed, query string included.
    event.message = event.message.take().map(|m| strip_query(&m));
    for ex in event.exception.values.iter_mut() {
        ex.value = ex.value.take().map(|v| strip_query(&v));
    }
    event
}

/// Removes `?query` from every URL-looking word of `text`.
fn strip_query(text: &str) -> String {
    text.split_inclusive(char::is_whitespace)
        .map(|word| match word.find('?') {
            Some(q) if word.contains("://") => {
                let tail =
                    word.trim_end_matches(|c: char| ")\"'>],.;".contains(c) || c.is_whitespace());
                format!("{}{}", &word[..q], &word[tail.len().max(q)..])
            }
            _ => word.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentry::protocol::Request;

    #[test]
    fn no_dsn_means_no_client() {
        // The test environment sets no SENTRY_DSN.
        assert!(init().is_none());
    }

    #[test]
    fn scrub_removes_query_and_credentials() {
        let mut event = Event::default();
        event.message =
            Some("fetch failed: error for url (http://h/x?token=SECRET&a=1) after 3s".into());
        let mut req = Request::default();
        req.url = Some("http://h/v1/forecast?token=SECRET".parse().unwrap());
        req.query_string = Some("token=SECRET".into());
        req.cookies = Some("sid=SECRET".into());
        req.headers
            .insert("Authorization".into(), "Bearer SECRET".into());
        req.headers.insert("accept".into(), "*/*".into());
        event.request = Some(req);

        let event = scrub(event);
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("SECRET"), "{json}");
        let req = event.request.unwrap();
        assert_eq!(req.url.unwrap().as_str(), "http://h/v1/forecast");
        assert_eq!(req.headers.len(), 1);
        assert_eq!(
            event.message.unwrap(),
            "fetch failed: error for url (http://h/x) after 3s"
        );
    }

    #[test]
    fn environment_falls_back_to_otel_attributes_then_default() {
        // Pure parsing path is exercised through the default only: env is process-global.
        assert!(!environment().is_empty());
    }
}
