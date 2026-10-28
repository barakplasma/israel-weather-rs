//! Build metadata for `/api/status`, and (with the `server` feature) the embedded web UI.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn build_info() {
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

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webmanifest" => "application/manifest+json",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if !p.file_name().unwrap().to_string_lossy().starts_with('.') {
            out.push(p);
        }
    }
}

/// Writes `$OUT_DIR/web_assets.rs`: `(url path, content type, bytes, gzipped bytes)` for every
/// file trunk put in `web/dist` (or `$WEATHER_WEB_DIST`).
fn embed_web_ui() {
    println!("cargo:rerun-if-env-changed=WEATHER_WEB_DIST");
    let dist = std::env::var("WEATHER_WEB_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist"));
    println!("cargo:rerun-if-changed={}", dist.display());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut list = vec![];
    if dist.join("index.html").exists() {
        files(&dist, &mut list);
    } else if std::env::var("PROFILE").as_deref() == Ok("release") {
        panic!(
            "web UI not built: run `trunk build --release` in web/ first (or set WEATHER_WEB_DIST)"
        );
    } else {
        // Debug builds and tests work without trunk.
        println!("cargo:warning=web/dist missing, embedding a placeholder page");
        let placeholder = out_dir.join("index.html");
        std::fs::write(
            &placeholder,
            "<!doctype html><title>Israel Weather</title>\
             <p>Web UI not built. Run <code>trunk build</code> in <code>web/</code>.</p>",
        )
        .unwrap();
        list.push(placeholder);
    }
    let base = if list.len() == 1 && !list[0].starts_with(&dist) {
        out_dir.clone()
    } else {
        dist.clone()
    };
    let mut code = String::from("&[\n");
    for (i, path) in list.iter().enumerate() {
        let url = format!("/{}", path.strip_prefix(&base).unwrap().to_string_lossy());
        let bytes = std::fs::read(path).unwrap();
        let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::best());
        gz.write_all(&bytes).unwrap();
        let gz_path = out_dir.join(format!("asset-{i}.gz"));
        std::fs::write(&gz_path, gz.finish().unwrap()).unwrap();
        code += &format!(
            "    ({url:?}, {:?}, include_bytes!({:?}), include_bytes!({:?})),\n",
            content_type(path),
            path.display().to_string(),
            gz_path.display().to_string(),
        );
    }
    code += "]\n";
    std::fs::write(out_dir.join("web_assets.rs"), code).unwrap();
}

fn main() {
    build_info();
    if std::env::var_os("CARGO_FEATURE_SERVER").is_some() {
        embed_web_ui();
    }
}
