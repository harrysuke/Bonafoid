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

    PwaOutput {
        manifest,
        service_worker,
        offline_page,
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
