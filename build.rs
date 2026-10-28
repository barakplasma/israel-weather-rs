//! Build metadata for `/api/status`.

use std::process::Command;

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let commit = std::env::var("GITHUB_SHA")
        .ok()
        .map(|s| s.chars().take(12).collect())
        .or_else(|| output("git", &["rev-parse", "--short=12", "HEAD"]));
    if let Some(c) = commit {
        println!("cargo:rustc-env=WEATHER_GIT_COMMIT={c}");
    }
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = output(&rustc, &["--version"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=WEATHER_RUSTC={version}");
    println!(
        "cargo:rustc-env=WEATHER_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
}
