//! The plugin HTTP handlers.

use axum::extract::{Multipart, Path, State};
use axum::http::HeaderMap;
use axum::Json;
use nocode_plugins::PluginManifest;

use crate::auth::AuthUser;
use crate::error::{require_csrf, ApiError};
use crate::server::AppState;

use super::store;
use super::PluginSummary;

/// The largest plugin module the server accepts.
const MAX_MODULE_BYTES: usize = 16 * 1024 * 1024;

/// The largest manifest the server accepts.
const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// `GET /api/v1/plugins` — the plugins this user has installed.
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<PluginSummary>>, ApiError> {
    let rows = store::list_plugins(&state.db, &user.id).await?;
    let host = state.plugins.host(&user.id)?;

    let mut summaries = Vec::with_capacity(rows.len());
    for row in rows {
        let manifest = match PluginManifest::from_json(&row.manifest) {
            Ok(manifest) => manifest,
            Err(error) => {
                // A manifest that no longer parses is a stored-row problem,
                // not a request problem. Say which plugin, and carry on with
                // the rest rather than failing the whole list.
                tracing::error!(plugin_id = %row.id, %error, "stored plugin manifest is corrupt");
                continue;
            }
        };
        // The host is the source of truth for "is it loaded right now"; the
        // row records what the user asked for.
        let loaded = host.manifest(&row.id).is_some();
        summaries.push(PluginSummary::from_manifest(
            &manifest,
            row.enabled && loaded,
        ));
    }
    Ok(Json(summaries))
}

/// `POST /api/v1/plugins` — install a plugin from an uploaded manifest and
/// module.
///
/// The request is multipart with two parts: `manifest` (the manifest JSON)
/// and `module` (the WebAssembly bytes).
pub async fn install(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    mut multipart: Multipart,
) -> Result<(axum::http::StatusCode, Json<PluginSummary>), ApiError> {
    require_csrf(&headers)?;

    let mut manifest_json: Option<String> = None;
    let mut module: Option<Vec<u8>> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::BadRequest(format!("malformed upload: {error}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "manifest" => {
                let bytes = read_field(&mut field, MAX_MANIFEST_BYTES).await?;
                manifest_json = Some(String::from_utf8_lossy(&bytes).into_owned());
            }
            "module" => {
                module = Some(read_field(&mut field, MAX_MODULE_BYTES).await?);
            }
            _ => {}
        }
    }

    let manifest_json = manifest_json
        .ok_or_else(|| ApiError::BadRequest("the upload has no manifest part".into()))?;
    let module =
        module.ok_or_else(|| ApiError::BadRequest("the upload has no module part".into()))?;

    // The manifest is validated before anything is written: an invalid id, a
    // bad version or an unsupported api_version must not leave a directory
    // behind.
    let manifest = PluginManifest::from_json(&manifest_json)
        .map_err(|error| ApiError::BadRequest(format!("invalid plugin manifest: {error}")))?;

    let host = state.plugins.host(&user.id)?;
    if host.manifest(&manifest.id).is_some() {
        return Err(ApiError::Conflict(format!(
            "plugin {} is already installed",
            manifest.id
        )));
    }

    // Compile the module first: a module that does not compile must not be
    // recorded as installed.
    let compiled =
        nocode_plugins::load_module_from_bytes(host.engine(), &module).map_err(|error| {
            tracing::warn!(plugin_id = %manifest.id, %error, "the plugin module did not compile");
            ApiError::BadRequest(format!("the plugin module did not compile: {error}"))
        })?;

    // Store the manifest and the module on disk, so the plugin survives a
    // restart.
    let dir = state.plugins.plugin_dir(&user.id, &manifest.id);
    let manifest_path = dir.join("plugin.json");
    let module_path = dir.join(&manifest.entry_point);
    if let Err(error) =
        write_plugin_files(&dir, &manifest_path, &module_path, &manifest_json, &module)
    {
        tracing::error!(%error, dir = %dir.display(), "cannot store the plugin");
        super::remove_plugin_files(&state.plugins.root(), &dir);
        return Err(ApiError::Internal);
    }

    host.install(manifest.clone(), compiled, Some(module_path.clone()))
        .map_err(|error| {
            tracing::warn!(plugin_id = %manifest.id, %error, "the plugin was refused by the host");
            super::remove_plugin_files(&state.plugins.root(), &dir);
            match error {
                nocode_plugins::PluginError::AlreadyInstalled(id) => {
                    ApiError::Conflict(format!("plugin {id} is already installed"))
                }
                nocode_plugins::PluginError::PermissionDenied { .. } => ApiError::Forbidden(
                    "the plugin asks for permissions this server does not grant".into(),
                ),
                other => ApiError::BadRequest(format!("the plugin was refused: {other}")),
            }
        })?;

    store::insert_plugin(&state.db, &user.id, &manifest.id, &manifest_json).await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(PluginSummary::from_manifest(&manifest, true)),
    ))
}

/// `POST /api/v1/plugins/:id/enable` — turn a plugin on or off.
pub async fn set_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<EnableRequest>,
) -> Result<Json<PluginSummary>, ApiError> {
    require_csrf(&headers)?;

    let row = store::get_plugin(&state.db, &user.id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let manifest = PluginManifest::from_json(&row.manifest)
        .map_err(|error| ApiError::BadRequest(format!("stored manifest is corrupt: {error}")))?;

    let host = state.plugins.host(&user.id)?;
    if body.enabled {
        // Enabling a plugin that was never loaded needs the module back.
        if host.manifest(&id).is_none() {
            let module_path = state
                .plugins
                .plugin_dir(&user.id, &id)
                .join(&manifest.entry_point);
            let bytes = std::fs::read(&module_path).map_err(|error| {
                tracing::error!(%error, path = %module_path.display(), "the plugin module is missing");
                ApiError::Conflict("the plugin module is missing; install it again".into())
            })?;
            let compiled =
                nocode_plugins::load_module_from_bytes(host.engine(), &bytes).map_err(|error| {
                    ApiError::BadRequest(format!("the plugin module did not compile: {error}"))
                })?;
            host.install(manifest.clone(), compiled, Some(module_path))
                .map_err(|error| ApiError::Conflict(format!("the plugin was refused: {error}")))?;
        }
        host.set_enabled(&id, true)
            .map_err(|_| ApiError::NotFound)?;
    } else {
        host.set_enabled(&id, false)
            .map_err(|_| ApiError::NotFound)?;
    }

    store::set_enabled(&state.db, &user.id, &id, body.enabled).await?;

    Ok(Json(PluginSummary::from_manifest(&manifest, body.enabled)))
}

/// `DELETE /api/v1/plugins/:id` — uninstall a plugin.
pub async fn uninstall(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_csrf(&headers)?;

    let removed = store::delete_plugin(&state.db, &user.id, &id).await?;
    if !removed {
        return Err(ApiError::NotFound);
    }

    // A plugin that was never loaded is still uninstalled: the row is gone,
    // which is what the user asked for.
    if let Ok(host) = state.plugins.host(&user.id) {
        let _ = host.uninstall(&id);
    }
    super::remove_plugin_files(
        &state.plugins.root(),
        &state.plugins.plugin_dir(&user.id, &id),
    );

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Body of `POST /api/v1/plugins/:id/enable`.
#[derive(Debug, serde::Deserialize)]
pub struct EnableRequest {
    /// Whether the plugin should be enabled.
    pub enabled: bool,
}

/// Read one multipart field, refusing anything over `limit`.
async fn read_field(
    field: &mut axum::extract::multipart::Field<'_>,
    limit: usize,
) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| ApiError::BadRequest(format!("cannot read the upload: {error}")))?
    {
        if bytes.len() + chunk.len() > limit {
            return Err(ApiError::BadRequest(format!(
                "the upload is larger than the {limit} byte limit"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Write the manifest and the module into the plugin's directory.
fn write_plugin_files(
    dir: &std::path::Path,
    manifest_path: &std::path::Path,
    module_path: &std::path::Path,
    manifest_json: &str,
    module: &[u8],
) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(manifest_path, manifest_json.as_bytes())?;
    std::fs::write(module_path, module)?;
    Ok(())
}
