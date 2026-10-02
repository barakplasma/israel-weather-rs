//! End-to-end tests of the `weather` binary against the bundled fixture, no network needed.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("isr_cities_1week_6hr_forecast.xml")
}

fn weather(args: &[&str]) -> Output {
    let cache = std::env::temp_dir().join(format!("weather-cli-test-{}", std::process::id()));
    Command::new(env!("CARGO_BIN_EXE_weather"))
        .args(args)
        .env("WEATHER_URL", fixture())
        .env("WEATHER_CACHE_DIR", cache)
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to run weather binary")
}

fn stdout_json(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout should be json")
}

#[test]
fn next_forecasts_for_location() {
    let out = weather(&[
        "-l",
        "tel aviv coast",
        "-n",
        "12",
        "--now",
        "2026-09-30T18:45:00Z",
    ]);
    let json = stdout_json(&out);
    let forecasts = json.as_array().unwrap();
    assert_eq!(forecasts.len(), 2);
    assert_eq!(forecasts[0]["ForecastTime"], "2026-10-01T03:00:00+03:00");
    assert!(forecasts[0]["WeatherCodeEnglish"].is_string());
}

#[test]
fn all_prints_every_location() {
    let json = stdout_json(&weather(&["--all"]));
    let locations = json["Location"].as_array().unwrap();
    assert!(locations.len() > 10);
}

#[test]
fn list_locations() {
    let out = weather(&["--list-locations"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let names: Vec<&str> = stdout.lines().collect();
    assert_eq!(names[0], "Jerusalem");
    assert!(names.contains(&"Tel Aviv Coast"));
}

#[test]
fn unknown_location_fails_with_helpful_message() {
    let out = weather(&["-l", "Atlantis"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("\"Atlantis\" not found"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("Jerusalem"), "stderr: {stderr}");
}

#[test]
fn invalid_now_is_rejected() {
    let out = weather(&["--now", "tomorrow"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("RFC 3339"));
}
