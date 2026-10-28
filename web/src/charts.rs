//! The 24-hour condition strip and the per-day graph, both scrubbable by pointer or touch.

use chrono::Timelike;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::PointerEvent;

use crate::icons::Sky;
use crate::model::{Hour, hh};

/// Rain bar height (0..1) on a log scale that tops out at 10 mm/h.
fn level(mm: f64) -> f64 {
    ((mm.ln_1p()) / 10f64.ln_1p()).min(1.0)
}

/// Pointer position across the element the handler is attached to, 0..1.
fn fraction(ev: &PointerEvent) -> f64 {
    let Some(el) = ev
        .current_target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return 0.0;
    };
    let r = el.get_bounding_client_rect();
    ((ev.client_x() as f64 - r.left()) / r.width().max(1.0)).clamp(0.0, 0.9999)
}

fn details(h: &Hour) -> String {
    let rain = if h.precipitation > 0.0 {
        format!(" · {:.1} mm", h.precipitation)
    } else {
        String::new()
    };
    let chance = h.chance.map(|c| format!(" · {c:.0}%")).unwrap_or_default();
    format!(
        "{} · {} · {:.0}°{rain}{chance}",
        hh(h.time),
        Sky::of(h.code, h.precipitation).label(),
        h.temperature
    )
}

/// Tooltip over a chart for the hour under the pointer.
#[component]
fn Readout(hour: Signal<Option<(f64, Hour)>>) -> impl IntoView {
    move || {
        hour.get().map(|(_, h)| {
            view! {
                <div class="readout">
                    {details(&h)}
                    <span class="sub">
                        {format!("Feels {:.0}° · Wind {:.0} km/h", h.feels_like, h.wind)}
                        {h.gust.map(|g| format!(" (gusts {g:.0})"))}
                        {format!(" · Humidity {:.0}%", h.humidity)}
                        {h.uv.filter(|u| *u > 0.0).map(|u| format!(" · UV {u:.0}"))}
                    </span>
                </div>
            }
        })
    }
}

struct Run {
    sky: Sky,
    start: usize,
    end: usize,
}

fn runs(hours: &[Hour]) -> Vec<Run> {
    let mut out: Vec<Run> = vec![];
    for (k, h) in hours.iter().enumerate() {
        let sky = Sky::of(h.code, h.precipitation);
        match out.last_mut() {
            Some(r) if r.sky == sky => r.end = k + 1,
            _ => out.push(Run {
                sky,
                start: k,
                end: k + 1,
            }),
        }
    }
    out
}

/// Tick labels: "Now", rain start/end hours (merged when close), regular ticks elsewhere.
fn ticks(hours: &[Hour], px: f64) -> Vec<(f64, String, bool)> {
    let n = hours.len();
    if n == 0 {
        return vec![];
    }
    let every = if px < 500.0 { 6 } else { 3 };
    let wet = |k: usize| k < n && Sky::of(hours[k].code, hours[k].precipitation).is_wet();
    // Rain ending with the last hour ends at the strip's end, one hour after it.
    let at = |k: usize| {
        let t = hours
            .get(k)
            .map_or(hours[n - 1].time + chrono::Duration::hours(1), |h| h.time);
        t.format("%H").to_string()
    };
    let close = |a: usize, b: usize| (a.abs_diff(b) as f64) * (px / n as f64) < 36.0;
    let edges: Vec<usize> = (1..n).filter(|&k| wet(k) != wet(k - 1)).collect();
    let mut out = vec![(0.0, "Now".to_string(), false)];
    let mut i = 0;
    while i < edges.len() {
        let a = edges[i];
        match edges.get(i + 1) {
            Some(&b) if wet(a) && close(a, b) => {
                out.push((
                    (a + b) as f64 / 2.0 / n as f64,
                    format!("{}–{}", at(a), at(b)),
                    true,
                ));
                i += 2;
            }
            _ => {
                out.push((a as f64 / n as f64, at(a), true));
                i += 1;
            }
        }
    }
    for k in (every..n).step_by(every) {
        if !edges.iter().any(|&e| close(e, k)) {
            out.push((k as f64 / n as f64, at(k), false));
        }
    }
    out
}

#[component]
fn Ticks(items: Vec<(f64, String, bool)>) -> impl IntoView {
    view! {
        <div class="ticks">
            {items
                .into_iter()
                .enumerate()
                .map(|(i, (p, t, wet))| {
                    let first = if i == 0 { "transform:none;" } else { "" };
                    view! {
                        <span class:wet=wet style=format!("{first}left:{}%", p * 100.0)>{t}</span>
                    }
                })
                .collect_view()}
        </div>
    }
}

fn scrub_handlers(
    pick: impl Fn(f64) -> Option<usize> + Copy + 'static,
    hours: StoredValue<Vec<Hour>>,
    set: WriteSignal<Option<(f64, Hour)>>,
) -> (impl Fn(PointerEvent) + Copy, impl Fn(PointerEvent) + Copy) {
    let move_ = move |ev: PointerEvent| {
        let f = fraction(&ev);
        let picked = pick(f).and_then(|k| hours.with_value(|h| h.get(k).cloned()));
        set.set(picked.map(|h| (f, h)));
    };
    let leave = move |ev: PointerEvent| {
        // On touch, keep the last tapped hour visible.
        if ev.pointer_type() != "touch" {
            set.set(None);
        }
    };
    (move_, leave)
}

/// Next 24 hours as one strip; wet runs show per-hour rain amount bars over a chance band.
#[component]
pub fn Strip(hours: Vec<Hour>, width: Signal<f64>) -> impl IntoView {
    let n = hours.len().max(1);
    let (readout, set_readout) = signal(None);
    let stored = StoredValue::new(hours.clone());
    let (on_move, on_leave) =
        scrub_handlers(move |f| Some((f * n as f64) as usize), stored, set_readout);
    let segs = runs(&hours)
        .into_iter()
        .map(|r| {
            let left = r.start as f64 / n as f64 * 100.0;
            let w = (r.end - r.start) as f64 / n as f64;
            let len = r.end - r.start;
            let bars = r.sky.is_wet().then(|| {
                hours[r.start..r.end]
                    .iter()
                    .enumerate()
                    .map(|(k, h)| {
                        let bw = 100.0 / len as f64;
                        let x = k as f64 * bw;
                        let c = h.chance.unwrap_or(0.0);
                        let l = level(h.precipitation);
                        view! {
                            <i class="chance" style=format!("left:{x}%;width:{bw}%;height:{c}%")></i>
                            <i class="bar" style=format!("left:{}%;width:{}%;height:{}%", x + bw * 0.2, bw * 0.6, l * 100.0)></i>
                        }
                    })
                    .collect_view()
            });
            let label = move || {
                (!r.sky.is_wet() && w * width.get() > r.sky.label().len() as f64 * 7.0 + 12.0)
                    .then_some(r.sky.label())
            };
            view! {
                <div class="seg" style=format!("left:{left}%;width:{}%;background:{}", w * 100.0, r.sky.var())>
                    {bars}
                    <span>{label}</span>
                </div>
            }
        })
        .collect_view();
    let tick_hours = hours.clone();
    view! {
        <div class="chart" on:pointermove=on_move on:pointerdown=on_move on:pointerleave=on_leave>
            <div class="strip" role="img" aria-label="Conditions over the next 24 hours">{segs}</div>
            {move || readout.get().map(|(f, _)| {
                let k = (f * n as f64) as usize;
                view! { <div class="cursor" style=format!("left:{}%", (k as f64 + 0.5) / n as f64 * 100.0)></div> }
            })}
            <Readout hour=readout.into()/>
        </div>
        {move || view! { <Ticks items=ticks(&tick_hours, width.get())/> }}
    }
}

/// One day: thin condition strip, temperature curve on a week-wide scale, rain bars + chance.
#[component]
pub fn DayGraph(
    hours: Vec<Hour>,
    t_min: f64,
    t_max: f64,
    r_max: f64,
    today: bool,
) -> impl IntoView {
    const W: f64 = 720.0;
    const H: f64 = 150.0;
    let slot = |h: &Hour| h.time.hour() as f64 / 24.0;
    let x = |f: f64| f * W;
    let y_t = |v: f64| 8.0 + (1.0 - (v - t_min) / (t_max - t_min).max(1.0)) * (H * 0.62 - 8.0);
    let bh = |v: f64, max: f64| v / max * H * 0.38;
    let bw = W / 24.0;

    let night = hours
        .iter()
        .filter(|h| !h.is_day)
        .map(|h| view! { <rect x=x(slot(h)) y="0" width=bw height=H fill="var(--night)"/> })
        .collect_view();
    let rain = hours
        .iter()
        .map(|h| {
            let c = h.chance.unwrap_or(0.0);
            let (cx, rx) = (x(slot(h)), x(slot(h)) + bw * 0.2);
            view! {
                {(c > 0.0).then(|| view! { <rect x=cx y=H - bh(c, 100.0) width=bw height=bh(c, 100.0) fill="var(--rain)" opacity=".15"/> })}
                {(h.precipitation > 0.0).then(|| view! {
                    <rect x=rx y=H - bh(h.precipitation, r_max) width=bw * 0.6 height=bh(h.precipitation, r_max) rx="2" fill="var(--rain)"/>
                })}
            }
        })
        .collect_view();
    let pts: Vec<(f64, f64)> = hours
        .iter()
        .map(|h| (x(slot(h) + 0.5 / 24.0), y_t(h.temperature)))
        .collect();
    let line = pts
        .iter()
        .map(|(a, b)| format!("{a:.1},{b:.1}"))
        .collect::<Vec<_>>()
        .join(" L");
    let (first, last) = (
        pts.first().map_or(0.0, |p| p.0),
        pts.last().map_or(0.0, |p| p.0),
    );
    let now_x = today.then(|| hours.first().map(|h| x(slot(h)))).flatten();

    let (readout, set_readout) = signal(None);
    let stored = StoredValue::new(hours.clone());
    let pick = move |f: f64| {
        let want = (f * 24.0) as u32;
        stored.with_value(|hs| hs.iter().position(|h| h.time.hour() == want))
    };
    let (on_move, on_leave) = scrub_handlers(pick, stored, set_readout);
    let cursor = move || {
        readout
            .get()
            .map(|(_, h)| x(slot(&h) + 0.5 / 24.0))
            .map(|cx| view! { <line x1=cx x2=cx y1="0" y2=H stroke="var(--fg)" stroke-width="1.5" vector-effect="non-scaling-stroke"/> })
    };
    let strip_runs = runs(&hours)
        .into_iter()
        .map(|r| {
            let left = slot(&hours[r.start]) * 100.0;
            let w = (r.end - r.start) as f64 / 24.0 * 100.0;
            view! { <div class="seg" style=format!("left:{left}%;width:{w}%;background:{}", r.sky.var())></div> }
        })
        .collect_view();
    view! {
        <div class="strip thin">{strip_runs}</div>
        <div class="chart" on:pointermove=on_move on:pointerdown=on_move on:pointerleave=on_leave>
            <svg viewBox=format!("0 0 {W} {H}") preserveAspectRatio="none" class="daygraph" role="img" aria-label="Temperature and rain by hour">
                <defs>
                    <linearGradient id="tg" x1="0" x2="0" y1="0" y2="1">
                        <stop offset="0" stop-color="var(--warm)" stop-opacity=".35"/>
                        <stop offset="1" stop-color="var(--warm)" stop-opacity="0"/>
                    </linearGradient>
                </defs>
                {night}
                {rain}
                {(pts.len() > 1).then(|| view! {
                    <path d=format!("M{first:.1},{H} L{line} L{last:.1},{H} Z") fill="url(#tg)"/>
                    <path d=format!("M{line}") fill="none" stroke="var(--warm)" stroke-width="2.5" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke"/>
                })}
                {now_x.map(|nx| view! { <line x1=nx x2=nx y1="0" y2=H stroke="var(--fg)" stroke-dasharray="3 3" opacity=".5" vector-effect="non-scaling-stroke"/> })}
                {cursor}
            </svg>
            <Readout hour=readout.into()/>
        </div>
        <Ticks items=[0, 6, 12, 18].map(|h| (h as f64 / 24.0, format!("{h:02}"), false)).to_vec()/>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn hour(k: i64, precipitation: f64) -> Hour {
        Hour {
            time: NaiveDateTime::parse_from_str("2026-10-28T21:00", "%Y-%m-%dT%H:%M").unwrap()
                + chrono::Duration::hours(k),
            temperature: 20.0,
            feels_like: 20.0,
            precipitation,
            chance: None,
            code: 0,
            wind: 0.0,
            gust: None,
            humidity: 50.0,
            uv: None,
            is_day: false,
        }
    }

    #[test]
    fn rain_edges_get_ticks_and_short_showers_merge() {
        // Rain 06-10 and a one-hour shower at 14.
        let hours: Vec<Hour> = (0..24)
            .map(|k| {
                hour(
                    k,
                    if (9..13).contains(&k) || k == 17 {
                        1.0
                    } else {
                        0.0
                    },
                )
            })
            .collect();
        let t = ticks(&hours, 360.0);
        let wet: Vec<&str> = t.iter().filter(|x| x.2).map(|x| x.1.as_str()).collect();
        assert_eq!(wet, ["06", "10", "14–15"]);
        assert_eq!(t[0].1, "Now");
        // The regular 03 tick is 3 h (45 px) from the 06 edge, so it stays.
        assert!(t.iter().any(|x| x.1 == "03" && !x.2));
    }

    #[test]
    fn runs_merge_equal_sky() {
        let hours: Vec<Hour> = (0..5)
            .map(|k| hour(k, if k >= 3 { 1.0 } else { 0.0 }))
            .collect();
        let r = runs(&hours);
        assert_eq!(r.len(), 2);
        assert_eq!((r[1].start, r[1].end), (3, 5));
    }
}
