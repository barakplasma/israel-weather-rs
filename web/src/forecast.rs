//! The forecast page: location picker, warnings, the "now" card and the week list.

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use weather_api::geocoding::{Place, SearchResponse};
use weather_api::ims::Extras;
use weather_api::openmeteo::Forecast;

use crate::charts::{DayGraph, Strip};
use crate::get_json;
use crate::icons::{Icon, label};
use crate::model::{Model, query, rain_summary, weekday};

const PLACE_KEY: &str = "place";
const DEFAULT_PLACE: &str = "Tel Aviv Coast";

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

fn saved_place() -> Option<Place> {
    let s = storage()?.get_item(PLACE_KEY).ok()??;
    serde_json::from_str(&s).ok()
}

fn save_place(p: &Place) {
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(p)) {
        let _ = s.set_item(PLACE_KEY, &json);
    }
}

/// Whether the user already allowed location access, so we can use it without a prompt.
async fn geolocation_granted() -> bool {
    let Ok(permissions) = window().navigator().permissions() else {
        return false;
    };
    let desc = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&desc, &"name".into(), &"geolocation".into());
    let Ok(query) = permissions.query(&desc) else {
        return false;
    };
    wasm_bindgen_futures::JsFuture::from(query)
        .await
        .is_ok_and(|s| {
            s.unchecked_into::<web_sys::PermissionStatus>().state()
                == web_sys::PermissionState::Granted
        })
}

fn enc(s: &str) -> String {
    js_sys::encode_uri_component(s).into()
}

async fn search(name: &str, count: u32) -> Result<Vec<Place>, String> {
    let r: SearchResponse =
        get_json(&format!("/v1/search?name={}&count={count}", enc(name))).await?;
    Ok(r.results)
}

fn card_width() -> f64 {
    let w = window()
        .inner_width()
        .ok()
        .and_then(|w| w.as_f64())
        .unwrap_or(800.0);
    w.min(760.0) - 64.0
}

#[component]
pub fn ForecastPage() -> impl IntoView {
    let place = RwSignal::new(saved_place());
    let error = RwSignal::new(None::<String>);
    let suggestions = RwSignal::new(Vec::<Place>::new());
    let width = RwSignal::new(card_width());
    let _resize = window_event_listener(ev::resize, move |_| width.set(card_width()));

    if place.get_untracked().is_none() {
        spawn_local(async move {
            match search(DEFAULT_PLACE, 1).await {
                Ok(mut r) if !r.is_empty() => place.set(Some(r.remove(0))),
                Ok(_) => error.set(Some(format!("{DEFAULT_PLACE} not found"))),
                Err(e) => error.set(Some(e)),
            }
        });
    }
    Effect::new(move || {
        if let Some(p) = place.get() {
            save_place(&p);
            document().set_title(&format!("{} · Israel Weather", p.name));
        }
    });

    let forecast = LocalResource::new(move || {
        let p = place.get();
        async move {
            let p = p?;
            let q = query(p.latitude, p.longitude).to_query();
            Some(get_json::<Forecast>(&format!("/v1/forecast?{q}")).await)
        }
    });
    let extras = LocalResource::new(move || {
        let p = place.get();
        async move {
            let p = p?;
            get_json::<Extras>(&format!(
                "/api/ims?latitude={}&longitude={}",
                p.latitude, p.longitude
            ))
            .await
            .ok()
        }
    });

    let on_input = move |ev: ev::Event| {
        let v = event_target_value(&ev);
        spawn_local(async move {
            if v.trim().chars().count() >= 2 {
                suggestions.set(search(&v, 10).await.unwrap_or_default());
            }
        });
    };
    let on_change = move |ev: ev::Event| {
        let v = event_target_value(&ev);
        spawn_local(async move {
            match search(&v, 1).await {
                Ok(mut r) if !r.is_empty() => {
                    error.set(None);
                    place.set(Some(r.remove(0)));
                }
                Ok(_) => error.set(Some(format!("No location matches \"{v}\""))),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let near_me = move || {
        let Ok(geo) = window().navigator().geolocation() else {
            error.set(Some("Location is not available in this browser".into()));
            return;
        };
        let ok = Closure::once_into_js(move |pos: web_sys::Position| {
            let c = pos.coords();
            let (lat, lon) = (c.latitude(), c.longitude());
            spawn_local(async move {
                match get_json::<Extras>(&format!("/api/ims?latitude={lat}&longitude={lon}")).await
                {
                    Ok(e) => {
                        error.set(None);
                        place.set(Some(e.location));
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        });
        let err = Closure::once_into_js(move |e: web_sys::PositionError| {
            error.set(Some(format!(
                "Could not get your location: {}",
                e.message()
            )));
        });
        let _ = geo.get_current_position_with_error_callback(
            ok.unchecked_ref(),
            Some(err.unchecked_ref()),
        );
    };
    // With location already allowed, always start from where the user is.
    spawn_local(async move {
        if geolocation_granted().await {
            near_me();
        }
    });

    view! {
        <header>
            <h1><Icon code=80 size=30/>"Israel Weather"</h1>
            <div class="picker">
                <input
                    list="places"
                    placeholder="Search location…"
                    autocomplete="off"
                    aria-label="Location"
                    prop:value=move || place.get().map(|p| p.name).unwrap_or_default()
                    on:input=on_input
                    on:change=on_change
                />
                <datalist id="places">
                    {move || suggestions.get().into_iter().map(|p| view! { <option value=p.name></option> }).collect_view()}
                </datalist>
                <button on:click=move |_| near_me() title="Use my location">"Near me"</button>
            </div>
        </header>
        {move || error.get().map(|e| view! { <p class="error">{e}</p> })}
        {move || extras.get().flatten().map(|e| view! { <Warnings extras=e/> })}
        <Transition fallback=|| view! { <div class="card skeleton"></div> }>
            {move || {
                forecast.get().flatten().map(|r| match r {
                    Err(e) => view! { <p class="error">{e}</p> }.into_any(),
                    Ok(f) => {
                        let m = Model::new(&f, js_sys::Date::now());
                        let name = place.get_untracked().map(|p| p.name).unwrap_or_default();
                        let country = extras.get().flatten().and_then(|e| e.country_forecast);
                        view! {
                            <Now model=m.clone() name=name country=country.map(|c| c.description) width=width.into()/>
                            <Week model=m/>
                        }
                        .into_any()
                    }
                })
            }}
        </Transition>
        <Footer place=place.into()/>
    }
}

#[component]
fn Warnings(extras: Extras) -> impl IntoView {
    extras
        .warnings
        .into_iter()
        .map(|w| {
            let range = format!(
                "{} – {}",
                w.valid_from.format("%a %H:%M"),
                w.valid_to.format("%a %H:%M")
            );
            view! {
                <div class="card warning" style=format!("--wcolor:{}", w.color)>
                    <h3>{format!("{}: {}", w.severity, w.warning_type)}</h3>
                    <div class="muted">{range}</div>
                    <p>{w.text}</p>
                </div>
            }
        })
        .collect_view()
}

#[component]
fn Now(model: Model, name: String, country: Option<String>, width: Signal<f64>) -> impl IntoView {
    let Some(cur) = model.hours.first().cloned() else {
        return view! { <div class="card">"No forecast for the coming hours."</div> }.into_any();
    };
    let summary = format!(
        "{}. {}",
        label(cur.code, cur.ims),
        rain_summary(&model.hours, model.now)
    );
    let strip: Vec<_> = model.hours.iter().take(24).cloned().collect();
    view! {
        <section class="card now">
            <div class="muted">{name}</div>
            <div class="now-row">
                <Icon code=cur.code ims=cur.ims is_day=cur.is_day size=72/>
                <div class="temp">{format!("{:.0}°", cur.temperature)}</div>
                <div>
                    <div class="summary">{summary}</div>
                    <div class="muted facts">
                        {format!("Feels {:.0}° · Wind {:.0} km/h · Humidity {:.0}%", cur.feels_like, cur.wind, cur.humidity)}
                        {cur.uv.filter(|u| *u > 0.0).map(|u| format!(" · UV {u:.0}"))}
                    </div>
                </div>
            </div>
            <div class="timeline"><Strip hours=strip width=width/></div>
            {country.map(|c| view! { <p class="muted country">{format!("IMS: {c}")}</p> })}
        </section>
    }
    .into_any()
}

#[component]
fn Week(model: Model) -> impl IntoView {
    let all = model.days.iter().flat_map(|d| d.hours.iter());
    let t_min = all.clone().map(|h| h.temperature).fold(f64::MAX, f64::min);
    let t_max = all.clone().map(|h| h.temperature).fold(f64::MIN, f64::max);
    let (lo, hi) = model.days.iter().fold((f64::MAX, f64::MIN), |(lo, hi), d| {
        (lo.min(d.min), hi.max(d.max))
    });
    let span = (hi - lo).max(1.0);
    let today = model.now.date();
    let rows = model
        .days
        .into_iter()
        .enumerate()
        .map(|(i, d)| {
            let is_today = d.date == today;
            let name = if is_today { "Today".to_string() } else { weekday(d.date)[..3].to_string() };
            let (left, right) = ((d.min - lo) / span * 100.0, (d.max - lo) / span * 100.0);
            let title = format!("Rain: {:.0}% chance, {:.2} mm", d.chance, d.rain);
            let mm = match d.rain {
                r if r <= 0.0 => None,
                r if r < 0.1 => Some("<0.1 mm".to_string()),
                r => Some(format!("{r:.1} mm")),
            };
            let wet = d.chance > 0.0 || mm.is_some() || d.hours.iter().any(|h| h.rain_possible());
            view! {
                <details open=i == 0>
                    <summary>
                        <Icon code=d.code ims=d.ims size=32/>
                        <span class="name">{name}</span>
                        <span class="range">
                            <span class="lo">{format!("{:.0}°", d.min)}</span>
                            <span class="track"><i style=format!("left:{left}%;width:{}%", right - left)></i></span>
                            <span>{format!("{:.0}°", d.max)}</span>
                        </span>
                        <span class="rain" class:wet=wet title=title.clone() aria-label=title>
                            {if wet { format!("{:.0}%", d.chance) } else { "–".into() }}
                            {mm.map(|m| view! { <small>{m}</small> })}
                        </span>
                    </summary>
                    <div class="daybox">
                        <DayGraph hours=d.hours t_min=t_min t_max=t_max today=is_today/>
                    </div>
                </details>
            }
        })
        .collect_view();
    view! {
        <section class="card week">
            <div class="week-head muted" aria-hidden="true">
                <span></span>
                <span></span>
                <span>"Low · High"</span>
                <span>"Rain"</span>
            </div>
            {rows}
        </section>
        <div class="legend muted">
            <span><b style="background:var(--warm)"></b>"Temperature"</span>
            <span><b style="background:var(--rain)"></b>"Rain"</span>
            <span><b style="background:var(--rain);opacity:.25"></b>"Chance of rain"</span>
            <span><b style="background:var(--night)"></b>"Night"</span>
        </div>
    }
}

#[component]
fn Footer(place: Signal<Option<Place>>) -> impl IntoView {
    let om = move || {
        place.get().map_or("#".into(), |p| {
            format!("/v1/forecast?{}", query(p.latitude, p.longitude).to_query())
        })
    };
    let met = move || {
        place.get().map_or("#".into(), |p| {
            format!(
                "/weatherapi/locationforecast/2.0/compact?lat={}&lon={}",
                p.latitude, p.longitude
            )
        })
    };
    view! {
        <footer class="card muted">
            <p>
                "Data: " <a href="https://ims.gov.il">"Israel Meteorological Service"</a>
                ". Tap or drag across a chart for exact values. " <a href="/status">"Server status"</a>
            </p>
            <ul>
                <li>"Open-Meteo: " <a href=om><code>"/v1/forecast"</code></a> ", " <a href="/v1/search?name=tel"><code>"/v1/search"</code></a></li>
                <li>"MET Norway: " <a href=met><code>"/weatherapi/locationforecast/2.0/compact"</code></a></li>
                <li>"IMS extras: " <code>"/api/ims"</code> " · status: " <a href="/api/status"><code>"/api/status"</code></a></li>
            </ul>
        </footer>
    }
}
