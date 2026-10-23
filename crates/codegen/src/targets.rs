//! Multi-target build artifacts for generated applications.
//!
//! A single project model can be built for three targets:
//! - **Web (WASM):** the Leptos/Vite frontend in `frontend/`
//! - **PWA:** installable web app served from `frontend/public/`
//! - **Desktop (Tauri):** a native shell in `desktop/`
//!
//! This module produces the PWA and desktop scaffolding.

use std::collections::HashMap;
use std::path::PathBuf;

use nocode_model::Project;
use serde_json::json;

/// Progressive Web App assets.
#[derive(Debug, Default, Clone)]
pub struct PwaOutput {
    pub manifest: String,
    pub service_worker: String,
    pub offline_page: String,
    /// The install icons, as binary PNG bytes.
    pub icons: Vec<(PathBuf, Vec<u8>)>,
}

impl PwaOutput {
    /// Files to write, relative to the frontend directory.
    pub fn files(&self) -> HashMap<PathBuf, String> {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("public/manifest.webmanifest"),
            self.manifest.clone(),
        );
        files.insert(PathBuf::from("public/sw.js"), self.service_worker.clone());
        files.insert(
            PathBuf::from("public/offline.html"),
            self.offline_page.clone(),
        );
        files
    }
}

/// A solid-brand PNG icon with a white disc in the middle.
///
/// The PWA manifest names icon files, and a manifest that points at files
/// that do not exist makes the app uninstallable. Rather than ship a
/// manifest that 404s, the icons are generated: a real PNG, encoded here so
/// the generator needs no image library.
///
/// The encoding is deliberately simple: 8-bit RGBA, no interlacing, and a
/// zlib stream built from stored (uncompressed) deflate blocks. That is a
/// valid zlib stream, so any decoder reads it.
fn png_icon(size: u32) -> Vec<u8> {
    // The brand colour the IDE uses for its primary actions.
    const BACKGROUND: [u8; 4] = [0x02, 0x84, 0xc7, 0xff];
    const FOREGROUND: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

    // The raw image data: one filter byte (0 = None) per row, then RGBA.
    let mut raw = Vec::with_capacity((size * (1 + size * 4)) as usize);
    let centre = (size as f64 - 1.0) / 2.0;
    let radius = size as f64 * 0.32;
    for y in 0..size {
        raw.push(0); // filter: None
        for x in 0..size {
            let dx = x as f64 - centre;
            let dy = y as f64 - centre;
            let pixel = if dx * dx + dy * dy <= radius * radius {
                FOREGROUND
            } else {
                BACKGROUND
            };
            raw.extend_from_slice(&pixel);
        }
    }

    // zlib stream: 2-byte header, stored deflate blocks, Adler-32.
    let mut zlib = vec![0x78, 0x01];
    let mut offset = 0usize;
    while offset < raw.len() {
        let len = (raw.len() - offset).min(0xffff);
        let last = offset + len == raw.len();
        zlib.push(if last { 1 } else { 0 }); // BFINAL, BTYPE=00 (stored)
        zlib.extend_from_slice(&(len as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(len as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::new();
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, adaptive, no interlace
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib);
    write_chunk(&mut png, b"IEND", &[]);
    png
}

/// Append one PNG chunk: length, type, data, CRC.
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// The CRC-32 (IEEE) of `data`.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// The Adler-32 checksum of `data`, as zlib requires.
fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Desktop (Tauri) shell scaffolding.
#[derive(Debug, Default, Clone)]
pub struct DesktopOutput {
    pub tauri_config: String,
    pub cargo_toml: String,
    pub build_rs: String,
    pub main_rs: String,
    pub capabilities: String,
}

impl DesktopOutput {
    /// Files to write, relative to the `desktop` directory.
    pub fn files(&self) -> HashMap<PathBuf, String> {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("tauri.conf.json"), self.tauri_config.clone());
        files.insert(
            PathBuf::from("src-tauri/Cargo.toml"),
            self.cargo_toml.clone(),
        );
        files.insert(PathBuf::from("src-tauri/build.rs"), self.build_rs.clone());
        files.insert(PathBuf::from("src-tauri/src/main.rs"), self.main_rs.clone());
        files.insert(
            PathBuf::from("src-tauri/capabilities/default.json"),
            self.capabilities.clone(),
        );
        files
    }
}

/// All multi-target artifacts for a project.
#[derive(Debug, Default, Clone)]
pub struct TargetsOutput {
    pub pwa: PwaOutput,
    pub desktop: DesktopOutput,
}

impl TargetsOutput {
    /// Total number of generated target files.
    pub fn file_count(&self) -> usize {
        self.pwa.files().len() + self.desktop.files().len()
    }
}

/// Generate PWA and desktop artifacts for `project`.
pub fn generate_targets(project: &Project) -> TargetsOutput {
    TargetsOutput {
        pwa: generate_pwa(project),
        desktop: generate_desktop(project),
    }
}

fn generate_pwa(project: &Project) -> PwaOutput {
    let manifest = serde_json::to_string_pretty(&json!({
        "name": project.name,
        "short_name": project.slug,
        "description": project.description,
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "orientation": "any",
        "background_color": "#ffffff",
        "theme_color": "#0f172a",
        "icons": [
            { "src": "/icons/icon-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/icons/icon-512.png", "sizes": "512x512", "type": "image/png" },
            { "src": "/icons/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable" }
        ]
    }))
    .unwrap_or_else(|_| "{}".to_string());

    let cache_name = format!("{}-cache-v1", project.slug);
    let service_worker = format!(
        r#"// Service worker for {name}
const CACHE_NAME = "{cache_name}";
const APP_SHELL = ["/", "/index.html", "/offline.html", "/manifest.webmanifest"];

self.addEventListener("install", (event) => {{
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(APP_SHELL))
  );
  self.skipWaiting();
}});

self.addEventListener("activate", (event) => {{
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(
        keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))
      )
    )
  );
  self.clients.claim();
}});

self.addEventListener("fetch", (event) => {{
  const {{ request }} = event;
  if (request.method !== "GET") return;

  if (request.mode === "navigate") {{
    event.respondWith(
      fetch(request).catch(() => caches.match("/offline.html"))
    );
    return;
  }}

  event.respondWith(
    caches.match(request).then((cached) => {{
      if (cached) return cached;
      return fetch(request).then((response) => {{
        const copy = response.clone();
        caches.open(CACHE_NAME).then((cache) => cache.put(request, copy));
        return response;
      }});
    }})
  );
}});
"#,
        name = project.name,
        cache_name = cache_name,
    );

    let offline_page = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{name} — Offline</title>
  <style>
    body {{ font-family: system-ui, sans-serif; display: grid; place-items: center; height: 100vh; margin: 0; }}
    main {{ text-align: center; }}
  </style>
</head>
<body>
  <main>
    <h1>{name}</h1>
    <p>You are offline. Reconnect to continue.</p>
  </main>
</body>
</html>
"#,
        name = project.name,
    );

    // The icons the manifest names. They are generated rather than
    // referenced, so the manifest never points at a file that is missing.
    let icons = vec![
        (PathBuf::from("public/icons/icon-192.png"), png_icon(192)),
        (PathBuf::from("public/icons/icon-512.png"), png_icon(512)),
    ];

    PwaOutput {
        manifest,
        service_worker,
        offline_page,
        icons,
    }
}

fn generate_desktop(project: &Project) -> DesktopOutput {
    let identifier = format!("com.nocode.{}", project.slug.replace('-', "."));

    let tauri_config = serde_json::to_string_pretty(&json!({
        "$schema": "https://schema.tauri.app/config/2",
        "productName": project.name,
        "version": project.version,
        "identifier": identifier,
        "build": {
            "frontendDist": "../frontend/dist",
            "devUrl": "http://localhost:8080",
            "beforeDevCommand": "cd ../frontend && trunk serve --port 8080",
            "beforeBuildCommand": "cd ../frontend && trunk build --release"
        },
        "app": {
            "windows": [
                {
                    "title": project.name,
                    "width": 1280,
                    "height": 800,
                    "resizable": true,
                    "fullscreen": false
                }
            ],
            "security": { "csp": null }
        },
        "bundle": {
            "active": true,
            "targets": "all",
            "icon": [
                "icons/32x32.png",
                "icons/128x128.png",
                "icons/128x128@2x.png",
                "icons/icon.icns",
                "icons/icon.ico"
            ]
        }
    }))
    .unwrap_or_else(|_| "{}".to_string());

    let cargo_toml = format!(
        r#"[package]
name = "{slug}-desktop"
version = "{version}"
edition = "2021"
description = "Desktop shell for {name}"

[build-dependencies]
tauri-build = {{ version = "2", features = [] }}

[dependencies]
tauri = {{ version = "2", features = [] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"

[features]
default = ["custom-protocol"]
custom-protocol = ["tauri/custom-protocol"]
"#,
        slug = project.slug,
        version = project.version,
        name = project.name,
    );

    let build_rs = "fn main() {\n    tauri_build::build()\n}\n".to_string();

    let main_rs = r#"// Prevents an additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the tauri application");
}
"#
    .to_string();

    let capabilities = serde_json::to_string_pretty(&json!({
        "$schema": "../gen/schemas/desktop-schema.json",
        "identifier": "default",
        "description": "Default capability for the main window",
        "windows": ["main"],
        "permissions": ["core:default"]
    }))
    .unwrap_or_else(|_| "{}".to_string());

    DesktopOutput {
        tauri_config,
        cargo_toml,
        build_rs,
        main_rs,
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project() -> Project {
        let mut project = Project::new("Acme Portal");
        project.description = Some("A demo application".to_string());
        project
    }

    #[test]
    fn pwa_manifest_is_valid_json_with_project_identity() {
        let project = sample_project();
        let output = generate_targets(&project);
        let value: serde_json::Value =
            serde_json::from_str(&output.pwa.manifest).expect("valid manifest json");
        assert_eq!(value["name"], "Acme Portal");
        assert_eq!(value["short_name"], "acme-portal");
        assert_eq!(value["display"], "standalone");
        assert!(value["icons"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn service_worker_references_slug_cache() {
        let project = sample_project();
        let output = generate_targets(&project);
        assert!(output.pwa.service_worker.contains("acme-portal-cache-v1"));
        assert!(output.pwa.service_worker.contains("offline.html"));
    }

    #[test]
    fn desktop_config_has_identifier_and_dist() {
        let project = sample_project();
        let output = generate_targets(&project);
        let value: serde_json::Value =
            serde_json::from_str(&output.desktop.tauri_config).expect("valid tauri json");
        assert_eq!(value["identifier"], "com.nocode.acme.portal");
        assert_eq!(value["build"]["frontendDist"], "../frontend/dist");
    }

    #[test]
    fn desktop_cargo_uses_tauri_two() {
        let project = sample_project();
        let output = generate_targets(&project);
        assert!(output
            .desktop
            .cargo_toml
            .contains("tauri = { version = \"2\""));
        assert!(output.desktop.main_rs.contains("tauri::Builder::default()"));
    }

    #[test]
    fn file_maps_have_expected_paths() {
        let project = sample_project();
        let output = generate_targets(&project);
        let pwa = output.pwa.files();
        assert!(pwa.contains_key(&PathBuf::from("public/manifest.webmanifest")));
        assert!(pwa.contains_key(&PathBuf::from("public/sw.js")));

        let desktop = output.desktop.files();
        assert!(desktop.contains_key(&PathBuf::from("tauri.conf.json")));
        assert!(desktop.contains_key(&PathBuf::from("src-tauri/Cargo.toml")));
        assert_eq!(output.file_count(), pwa.len() + desktop.len());
    }
}
