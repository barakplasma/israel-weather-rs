//! `/status`: what the server has loaded and cached, refresh history and request metrics.

use chrono::{DateTime, Utc};
use leptos::prelude::*;
use weather_api::status::Status;

use crate::get_json;

const REFRESH_EVERY_MS: u64 = 30_000;

fn bytes(n: u64) -> String {
    match n {
        0..1_024 => format!("{n} B"),
        1_024..1_048_576 => format!("{:.1} KiB", n as f64 / 1_024.0),
        _ => format!("{:.1} MiB", n as f64 / 1_048_576.0),
    }
}

fn duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} d {h} h"),
    }
}

/// UTC timestamp shown in Israel time; the page has no timezone tables, so this uses the
/// browser's offset, which is right for anyone looking at it from Israel.
fn time(t: Option<DateTime<Utc>>) -> String {
    t.map_or("–".into(), |t| {
        let offset_min = -js_sys::Date::new_0().get_timezone_offset() as i64;
        (t + chrono::Duration::minutes(offset_min))
            .format("%a %d %b %H:%M")
            .to_string()
    })
}

#[component]
fn Row(k: &'static str, v: String) -> impl IntoView {
    view! { <tr><th>{k}</th><td>{v}</td></tr> }
}

#[component]
pub fn StatusPage() -> impl IntoView {
    document().set_title("Server status · Israel Weather");
    let tick = RwSignal::new(0u32);
    let _ = set_interval_with_handle(
        move || tick.update(|t| *t += 1),
        std::time::Duration::from_millis(REFRESH_EVERY_MS),
    );
    let status = LocalResource::new(move || {
        tick.track();
        get_json::<Status>("/api/status")
    });
    view! {
        <header>
            <h1>"Server status"</h1>
            <a class="button" href="/">"← Forecast"</a>
        </header>
        <Transition fallback=|| view! { <div class="card skeleton"></div> }>
            {move || status.get().map(|r| match r {
                Err(e) => view! { <p class="error">{e}</p> }.into_any(),
                Ok(s) => view! { <StatusView s=s/> }.into_any(),
            })}
        </Transition>
    }
}

#[component]
fn StatusView(s: Status) -> impl IntoView {
    let d = s.data;
    let srv = s.server;
    let total_hourly = d.hourly_ims + d.hourly_interpolated;
    let ims_share = if total_hourly == 0 {
        0.0
    } else {
        d.hourly_ims as f64 / total_hourly as f64 * 100.0
    };
    let failed_last = s
        .refreshes
        .first()
        .is_some_and(|r| r.sources.iter().any(|x| x.error.is_some()));
    view! {
        <div class="tiles">
            <div class="card tile">
                <span class="muted">"Data"</span>
                <b class:bad=!d.loaded>{if d.loaded { "Loaded" } else { "Not loaded" }}</b>
                <span class="muted">{format!("fetched {}", time(d.fetched_at))}</span>
            </div>
            <div class="card tile">
                <span class="muted">"Locations"</span>
                <b>{d.locations}</b>
                <span class="muted">{format!("{total_hourly} hourly rows")}</span>
            </div>
            <div class="card tile">
                <span class="muted">"Cache"</span>
                <b>{bytes(s.cache.total_bytes)}</b>
                <span class="muted">{format!("{} files", s.cache.files.len())}</span>
            </div>
            <div class="card tile">
                <span class="muted">"Uptime"</span>
                <b>{duration(srv.uptime_seconds)}</b>
                <span class="muted" class:bad=failed_last>{if failed_last { "last refresh had errors" } else { "last refresh ok" }}</span>
            </div>
        </div>

        <section class="card">
            <h2>"Forecast data"</h2>
            <div class="share" title=format!("{ims_share:.0}% of hourly rows are IMS values")>
                <i style=format!("width:{ims_share}%")></i>
            </div>
            <p class="muted">{format!("{ims_share:.0}% of hourly rows are IMS values, the rest interpolated from the 6-hourly XML")}</p>
            <table>
                <Row k="IMS hourly rows" v=d.hourly_ims.to_string()/>
                <Row k="Interpolated rows" v=d.hourly_interpolated.to_string()/>
                <Row k="XML samples" v=d.xml_samples.to_string()/>
                <Row k="Daily summaries" v=d.daily_summaries.to_string()/>
                <Row k="Country forecasts" v=d.country_forecasts.to_string()/>
                <Row k="Warnings" v=d.warnings.map_or("feed not loaded".into(), |w| w.to_string())/>
                <Row k="Forecast range" v=format!("{} → {}", time(d.first_hour), time(d.last_hour))/>
                <Row k="XML fetched" v=time(d.fetched_at)/>
                <Row k="IMS hourly fetched" v=time(d.ims_hourly_fetched_at)/>
                <Row k="Next refresh" v=time(d.next_refresh)/>
                <Row k="Schedule" v=d.schedule/>
            </table>
        </section>

        <section class="card">
            <h2>"Refresh history"</h2>
            {if s.refreshes.is_empty() {
                view! { <p class="muted">"No refreshes since start."</p> }.into_any()
            } else {
                view! {
                    <table class="grid">
                        <tr><th>"Started"</th><th>"Took"</th><th>"Sources"</th></tr>
                        {s.refreshes.into_iter().map(|r| view! {
                            <tr>
                                <td>{time(Some(r.started_at))}</td>
                                <td>{format!("{:.1} s", r.duration_ms as f64 / 1000.0)}</td>
                                <td>
                                    {r.sources.into_iter().map(|x| {
                                        let ok = x.error.is_none();
                                        view! {
                                            <span class="pill" class:bad=!ok title=x.error.unwrap_or_default()>
                                                {if ok { "✓ " } else { "✗ " }}{x.name}
                                            </span>
                                        }
                                    }).collect_view()}
                                </td>
                            </tr>
                        }).collect_view()}
                    </table>
                }.into_any()
            }}
        </section>

        <section class="card">
            <h2>"Requests since start"</h2>
            <table class="grid">
                <tr><th>"Route"</th><th>"Count"</th><th>"Errors"</th><th>"Mean"</th><th>"Max"</th></tr>
                {s.requests.into_iter().map(|r| view! {
                    <tr>
                        <td><code>{r.route}</code></td>
                        <td>{r.count}</td>
                        <td class:bad={r.errors > 0}>{r.errors}</td>
                        <td>{format!("{:.1} ms", r.mean_ms)}</td>
                        <td>{format!("{:.1} ms", r.max_ms)}</td>
                    </tr>
                }).collect_view()}
            </table>
        </section>

        <section class="card">
            <h2>"Cache files"</h2>
            <p class="muted"><code>{s.cache.dir}</code></p>
            <table class="grid">
                <tr><th>"File"</th><th>"Size"</th><th>"Modified"</th></tr>
                {s.cache.files.into_iter().map(|f| {
                    let shown = f.source.unwrap_or(f.name);
                    view! {
                        <tr>
                            <td><code>{shown}</code></td>
                            <td>{bytes(f.bytes)}</td>
                            <td>{time(f.modified)}</td>
                        </tr>
                    }
                }).collect_view()}
            </table>
        </section>

        <section class="card">
            <h2>"Server"</h2>
            <table>
                <Row k="Version" v=srv.version/>
                <Row k="Commit" v=srv.git_commit.unwrap_or_else(|| "–".into())/>
                <Row k="Target" v=srv.target/>
                <Row k="Compiler" v=srv.rustc/>
                <Row k="Started" v=time(Some(srv.started_at))/>
                <Row k="Memory (RSS)" v=srv.rss_bytes.map_or("–".into(), bytes)/>
                <Row k="PID" v=srv.pid.to_string()/>
            </table>
        </section>
    }
}
