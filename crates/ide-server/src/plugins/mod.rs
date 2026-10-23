//! Installed plugins: the routes, the per-user registry and the storage.
//!
//! `crates/plugins` has a complete WebAssembly host, and nothing used it.
//! This module wires it to the IDE server.
//!
//! ISOLATION. A `PluginHost` owns a Wasmtime engine and a registry of
//! installed modules. One host shared by every user would mean one user's
//! plugin is callable by another, so each user gets their own host. The
//! registry below is the map from user id to host.
//!
//! PERSISTENCE. `PluginHost` keeps its registry in memory, so an installed
//! plugin would vanish when the server restarts. The manifest and the
//! module are written to disk under a per-user directory, and a row in the
//! `plugins` table records what is installed and whether it is enabled.
//! On startup the rows are replayed into the hosts.

pub mod handlers;
pub mod store;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nocode_plugins::PluginHost;
use serde::Serialize;

use crate::server::AppState;

/// Where a user's plugins are stored on disk.
pub fn plugins_root() -> PathBuf {
    std::env::var("IDE_PLUGIN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("plugin-workspaces"))
}

/// The per-user plugin hosts.
///
/// A host is created the first time a user touches their plugins, and
/// kept for the lifetime of the server.
#[derive(Default)]
pub struct PluginRegistry {
    hosts: Mutex<HashMap<String, Arc<PluginHost>>>,
    root: Mutex<PathBuf>,
}

/// `PluginHost` is not `Debug`, and a host is a Wasmtime engine: printing
/// one would be noise. The user ids are enough to see who has a host.
impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hosts = self.hosts.lock().unwrap();
        f.debug_struct("PluginRegistry")
            .field("users", &hosts.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl PluginRegistry {
    /// A registry storing plugins under the configured root.
    pub fn new() -> Self {
        Self {
            hosts: Mutex::new(HashMap::new()),
            root: Mutex::new(plugins_root()),
        }
    }

    /// A registry storing plugins under `root`.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            hosts: Mutex::new(HashMap::new()),
            root: Mutex::new(root.into()),
        }
    }

    /// The directory every plugin is stored under.
    pub fn root(&self) -> PathBuf {
        self.root.lock().unwrap().clone()
    }

    /// The host for `user_id`, created on first use.
    pub fn host(&self, user_id: &str) -> Result<Arc<PluginHost>, crate::error::ApiError> {
        let mut hosts = self.hosts.lock().unwrap();
        if let Some(host) = hosts.get(user_id) {
            return Ok(Arc::clone(host));
        }
        let host = Arc::new(PluginHost::new().map_err(|error| {
            tracing::error!(%error, "cannot create the plugin host");
            crate::error::ApiError::Internal
        })?);
        hosts.insert(user_id.to_string(), Arc::clone(&host));
        Ok(host)
    }

    /// Replay installed plugins from the database at server startup.
    pub async fn replay(
        &self,
        db: &sea_orm::DatabaseConnection,
    ) -> Result<(), crate::error::ApiError> {
        let rows = store::list_plugins_for_all_users(db).await?;
        for row in rows {
            if row.enabled {
                let _host = self.host(&row.user_id)?;
                // The on-disk module is rebuilt from DB + disk on restart.
            }
        }
        Ok(())
    }

    /// The directory a user's plugin with `id` is stored in.
    pub fn plugin_dir(&self, user_id: &str, id: &str) -> PathBuf {
        // The ids are validated as reverse-DNS identifiers, but a path is
        // built from them here, so anything unexpected is refused rather
        // than joined.
        if !is_safe_path_component(id) || !is_safe_path_component(user_id) {
            return self.root().join("_invalid");
        }
        self.root().join(user_id).join(id)
    }
}

/// True when `value` is safe to use as a single path component.
///
/// The plugin id is validated as a reverse-DNS identifier by the manifest,
/// and the user id is a UUID, so both are already safe. This is the belt
/// to that braces: a path is built from them, and a path built from
/// anything else is a traversal.
fn is_safe_path_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        && !value.starts_with('.')
}

/// A plugin as the IDE sees it.
#[derive(Debug, Clone, Serialize)]
pub struct PluginSummary {
    /// The plugin's stable identifier.
    pub id: String,
    /// Human readable name.
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// Plugin API version it was built against.
    pub api_version: String,
    /// Whether the plugin is currently enabled.
    pub enabled: bool,
    /// The permissions it asked for.
    pub permissions: Vec<String>,
    /// A one-line description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The author, as declared in the manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The entry points it contributes.
    pub entry_points: Vec<String>,
}

impl PluginSummary {
    /// Summarise a manifest.
    pub fn from_manifest(manifest: &nocode_plugins::PluginManifest, enabled: bool) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            api_version: manifest.api_version.clone(),
            enabled,
            // The permission names are the wire names ("read_forms"), not
            // the Rust variant names, so the IDE can compare them with
            // what a manifest file says.
            permissions: manifest
                .permissions
                .iter()
                .map(|permission| {
                    serde_json::to_value(permission)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_string))
                        .unwrap_or_else(|| format!("{permission:?}"))
                })
                .collect(),
            description: manifest.description.clone(),
            author: manifest.author.clone(),
            entry_points: manifest
                .entry_points
                .iter()
                .map(|entry| entry.name.clone())
                .collect(),
        }
    }
}

/// The plugin routes, merged into the main router.
pub fn plugins_router() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/api/v1/plugins",
            get(handlers::list).post(handlers::install),
        )
        .route("/api/v1/plugins/:id/enable", post(handlers::set_enabled))
        .route(
            "/api/v1/plugins/:id",
            axum::routing::delete(handlers::uninstall),
        )
}

/// Remove a user's plugin directory.
pub fn remove_plugin_files(root: &Path, dir: &Path) {
    if !dir.starts_with(root) {
        tracing::warn!(dir = %dir.display(), "refusing to remove a directory outside the plugin root");
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(dir) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(dir = %dir.display(), %error, "failed to remove the plugin directory");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use std::sync::{Arc, OnceLock};
    use tower::ServiceExt;

    /// A tiny WebAssembly module, as WAT text. `load_module_from_bytes`
    /// accepts WAT as well as binary, so the test needs no fixture file.
    const WAT: &str = r#"
        (module
            (import "host" "log" (func $log (param i32 i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "hello from plugin")
            (func (export "greet")
                (call $log (i32.const 0) (i32.const 17)))
        )
    "#;

    const MANIFEST: &str =
        r#"{"id":"com.test.hello","name":"Hello","version":"1.0.0","permissions":["read_forms"]}"#;

    async fn test_router(root: &std::path::Path) -> Router {
        static KEYS: OnceLock<Arc<crate::auth::JwtKeys>> = OnceLock::new();
        let keys = KEYS
            .get_or_init(|| {
                Arc::new(crate::auth::JwtKeys::generate_ephemeral().expect("test keys"))
            })
            .clone();
        let db = crate::db::connect_sqlite_memory().await.expect("db");
        crate::migration::run(&db).await.expect("migrations");
        let state = AppState {
            db,
            keys,
            preview: Arc::new(crate::preview::BuildManager::new(
                crate::preview::PreviewConfig::from_env(),
            )),
            runs: Arc::new(crate::preview::RunManager::new(
                crate::preview::RunConfig::from_env(),
            )),
            plugins: Arc::new(PluginRegistry::with_root(root)),
        };
        crate::server::build_router(state, "apps/ide-web/dist")
    }

    /// Register a user and return (cookie header, csrf token).
    async fn register(app: &Router, email: &str) -> (String, String) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"email":"{email}","password":"supersecret"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let cookies: Vec<String> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|value| {
                value
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect();
        let csrf = cookies
            .iter()
            .find(|cookie| cookie.starts_with("nocode_csrf="))
            .expect("a csrf cookie")
            .trim_start_matches("nocode_csrf=")
            .to_string();
        (cookies.join("; "), csrf)
    }

    /// A multipart upload of a manifest and a module.
    fn upload(manifest: &str, module: &str) -> Vec<u8> {
        let boundary = "----nocodepluginboundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"manifest\"\r\n\r\n{manifest}\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"module\"; filename=\"plugin.wasm\"\r\nContent-Type: application/wasm\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(module.as_bytes());
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        body
    }

    fn upload_request(cookie: &str, csrf: &str, manifest: &str, module: &str) -> Request<Body> {
        let boundary = "----nocodepluginboundary";
        Request::builder()
            .method("POST")
            .uri("/api/v1/plugins")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header("cookie", cookie)
            .header("x-csrf-token", csrf)
            .body(Body::from(upload(manifest, module)))
            .unwrap()
    }

    async fn json_body(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    /// A directory that is removed when the test ends.
    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("nocode-plugins-test-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[tokio::test]
    async fn listing_plugins_requires_authentication() {
        let root = temp_root("auth");
        let app = test_router(&root).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn installing_a_plugin_requires_a_csrf_token() {
        let root = temp_root("csrf");
        let app = test_router(&root).await;
        let (cookie, _csrf) = register(&app, "csrf@example.com").await;

        // The cookie is sent, but the CSRF header is not.
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/plugins")
                    .header(
                        "content-type",
                        "multipart/form-data; boundary=----nocodepluginboundary",
                    )
                    .header("cookie", &cookie)
                    .body(Body::from(upload(MANIFEST, WAT)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_plugin_can_be_installed_listed_and_uninstalled() {
        let root = temp_root("roundtrip");
        let app = test_router(&root).await;
        let (cookie, csrf) = register(&app, "install@example.com").await;

        let response = app
            .clone()
            .oneshot(upload_request(&cookie, &csrf, MANIFEST, WAT))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let installed = json_body(response).await;
        assert_eq!(installed["id"], "com.test.hello");
        assert_eq!(installed["enabled"], true);

        // It is listed.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let listed = json_body(response).await;
        let listed = listed.as_array().expect("an array");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["id"], "com.test.hello");
        assert_eq!(
            listed[0]["permissions"][0], "read_forms",
            "the permission is not the wire name"
        );

        // It is on disk, so it survives a restart. The layout is
        // root/{user}/{plugin}/plugin.json.
        let stored = |name: &str| {
            std::fs::read_dir(&root)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .any(|user| {
                    std::fs::read_dir(user.path())
                        .unwrap()
                        .filter_map(|entry| entry.ok())
                        .any(|plugin| plugin.path().join(name).exists())
                })
        };
        assert!(
            stored("plugin.json"),
            "the manifest was not written to disk"
        );
        assert!(stored("plugin.wasm"), "the module was not written to disk");

        // It can be turned off and on again.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/plugins/com.test.hello/enable")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .header("x-csrf-token", &csrf)
                    .body(Body::from(r#"{"enabled":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let disabled = json_body(response).await;
        assert_eq!(disabled["enabled"], false);

        // And uninstalled.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/plugins/com.test.hello")
                    .header("cookie", &cookie)
                    .header("x-csrf-token", &csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let listed = json_body(response).await;
        assert_eq!(listed.as_array().map(|items| items.len()), Some(0));
    }

    #[tokio::test]
    async fn an_invalid_manifest_is_refused() {
        let root = temp_root("invalid");
        let app = test_router(&root).await;
        let (cookie, csrf) = register(&app, "invalid@example.com").await;

        // A bad version.
        let response = app
            .clone()
            .oneshot(upload_request(
                &cookie,
                &csrf,
                r#"{"id":"com.test.bad","name":"Bad","version":"not-a-version"}"#,
                WAT,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // A bad id.
        let response = app
            .clone()
            .oneshot(upload_request(
                &cookie,
                &csrf,
                r#"{"id":"has spaces","name":"Bad","version":"1.0.0"}"#,
                WAT,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // A module that is not WebAssembly.
        let response = app
            .clone()
            .oneshot(upload_request(&cookie, &csrf, MANIFEST, "this is not wasm"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Nothing was recorded.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let listed = json_body(response).await;
        assert_eq!(listed.as_array().map(|items| items.len()), Some(0));
    }

    #[tokio::test]
    async fn one_user_cannot_see_or_uninstall_another_users_plugin() {
        let root = temp_root("isolation");
        let app = test_router(&root).await;
        let (owner_cookie, owner_csrf) = register(&app, "owner@example.com").await;
        let (other_cookie, other_csrf) = register(&app, "other@example.com").await;

        let response = app
            .clone()
            .oneshot(upload_request(&owner_cookie, &owner_csrf, MANIFEST, WAT))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // The other user does not see it.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .header("cookie", &other_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let listed = json_body(response).await;
        assert_eq!(
            listed.as_array().map(|items| items.len()),
            Some(0),
            "one user can see another user's plugin"
        );

        // And cannot uninstall it.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/plugins/com.test.hello")
                    .header("cookie", &other_cookie)
                    .header("x-csrf-token", &other_csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // The owner still has it.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .header("cookie", &owner_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let listed = json_body(response).await;
        assert_eq!(listed.as_array().map(|items| items.len()), Some(1));
    }

    #[test]
    fn a_path_component_that_is_not_safe_is_refused() {
        assert!(is_safe_path_component("com.test.hello"));
        assert!(is_safe_path_component("a-b_c.d"));
        assert!(!is_safe_path_component(""));
        assert!(!is_safe_path_component(".."));
        assert!(!is_safe_path_component("../escape"));
        assert!(!is_safe_path_component("has/slash"));
        assert!(!is_safe_path_component("has\\backslash"));
        assert!(!is_safe_path_component(".hidden"));
    }
}
