//! Durability boundary for `save_workflow`.
//!
//! File writes are confined to `MCP_FILE_ROOT`. Project writes use the
//! existing REST API (GET for the optimistic-concurrency revision, then PUT)
//! over a minimal HTTP/1.1 client so the crate needs no new dependencies.

use chrono::Utc;
use nocode_model::{Project, Workflow};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::error::McpError;

const REQUEST_TIMEOUT_SECS: u64 = 10;

/// A minimal HTTP response: status plus decoded body text.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Perform one JSON request against the IDE server.
///
/// Only plain `http://` is supported; deployments that need TLS should put a
/// local reverse proxy in front of the server and point `MCP_API_BASE_URL` at
/// it. This keeps the MCP crate dependency-free and auditable.
pub fn http_json(
    method: &str,
    base_url: &str,
    path: &str,
    token: &str,
    body: Option<&str>,
) -> Result<HttpResponse, McpError> {
    let (host, port) = parse_base_url(base_url)?;
    let mut stream = TcpStream::connect((host.as_str(), port)).map_err(|error| {
        McpError::ProjectSave(format!("cannot connect to {host}:{port}: {error}"))
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(REQUEST_TIMEOUT_SECS)))?;
    stream.set_write_timeout(Some(Duration::from_secs(REQUEST_TIMEOUT_SECS)))?;

    let payload = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let separator = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| McpError::ProjectSave("malformed HTTP response".into()))?;
    let head = String::from_utf8_lossy(&raw[..separator]);
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| McpError::ProjectSave("missing HTTP status".into()))?;
    let body_bytes = if head.lines().any(|line| {
        line.to_ascii_lowercase()
            .starts_with("transfer-encoding: chunked")
    }) {
        decode_chunked(&raw[separator + 4..])?
    } else {
        raw[separator + 4..].to_vec()
    };
    Ok(HttpResponse {
        status,
        body: String::from_utf8_lossy(&body_bytes).to_string(),
    })
}

fn parse_base_url(base_url: &str) -> Result<(String, u16), McpError> {
    let rest = base_url
        .strip_prefix("http://")
        .ok_or_else(|| McpError::ProjectSave("MCP_API_BASE_URL must start with http://".into()))?;
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err(McpError::ProjectSave("empty MCP_API_BASE_URL host".into()));
    }
    if let Some((host, port)) = authority.split_once(':') {
        let port = port
            .parse::<u16>()
            .map_err(|_| McpError::ProjectSave("invalid MCP_API_BASE_URL port".into()))?;
        Ok((host.to_string(), port))
    } else {
        Ok((authority.to_string(), 80))
    }
}

fn decode_chunked(input: &[u8]) -> Result<Vec<u8>, McpError> {
    let mut output = Vec::new();
    let mut rest = input;
    loop {
        let end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| McpError::ProjectSave("truncated chunked response".into()))?;
        let size_text = String::from_utf8_lossy(&rest[..end]);
        let size_text = size_text.split(';').next().unwrap_or("0").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| McpError::ProjectSave("invalid chunk size".into()))?;
        rest = &rest[end + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size + 2 {
            return Err(McpError::ProjectSave("truncated chunk body".into()));
        }
        output.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
    Ok(output)
}

/// Save a workflow into an existing Bonafoid project via GET + PUT.
pub fn save_to_project(
    config: &Config,
    project_id: &str,
    workflow: &Workflow,
) -> Result<Value, McpError> {
    if !project_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        || project_id.is_empty()
    {
        return Err(McpError::InvalidParam {
            name: "project_id",
            detail: "must be an id containing only letters, digits and dashes".into(),
        });
    }
    let token = config
        .api_token
        .as_ref()
        .ok_or_else(|| McpError::ProjectSave("MCP_API_TOKEN is not configured".into()))?;
    let path = format!("/api/v1/projects/{project_id}");

    let current = http_json("GET", &config.api_base_url, &path, token, None)?;
    if current.status != 200 {
        return Err(McpError::ProjectSave(format!(
            "GET project returned {status}",
            status = current.status
        )));
    }
    let detail: Value = serde_json::from_str(&current.body)?;
    let revision = detail
        .get("revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| McpError::ProjectSave("project response has no revision".into()))?;
    let mut project: Project = serde_json::from_value(
        detail
            .get("data")
            .cloned()
            .ok_or_else(|| McpError::ProjectSave("project response has no data".into()))?,
    )
    .map_err(McpError::from)?;

    project
        .workflows
        .insert(workflow.id.clone(), workflow.clone());
    project.updated_at = Utc::now();
    let body = json!({ "revision": revision, "data": project }).to_string();
    let updated = http_json("PUT", &config.api_base_url, &path, token, Some(&body))?;
    if !(200..300).contains(&updated.status) {
        let snippet: String = updated.body.chars().take(300).collect();
        return Err(McpError::ProjectSave(format!(
            "PUT project returned {status}: {snippet}",
            status = updated.status
        )));
    }
    Ok(json!({
        "saved": "project",
        "project_id": project_id,
        "workflow_id": workflow.id.to_string(),
        "revision": revision + 1,
    }))
}

/// Save a workflow as pretty JSON below the configured writable root.
pub fn save_to_file(root: &Path, relative: &str, workflow: &Workflow) -> Result<PathBuf, McpError> {
    let candidate = Path::new(relative);
    if candidate.as_os_str().is_empty() || candidate.is_absolute() {
        return Err(McpError::InvalidParam {
            name: "path",
            detail: "must be a relative path below MCP_FILE_ROOT".into(),
        });
    }
    for component in candidate.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(McpError::InvalidParam {
                name: "path",
                detail: "parent directories, roots and prefixes are not allowed".into(),
            });
        }
    }

    std::fs::create_dir_all(root)?;
    let canonical_root = std::fs::canonicalize(root)?;
    let target = canonical_root.join(candidate);
    let parent = target.parent().ok_or_else(|| McpError::InvalidParam {
        name: "path",
        detail: "has no parent directory".into(),
    })?;
    std::fs::create_dir_all(parent)?;
    let canonical_parent = std::fs::canonicalize(parent)?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(McpError::InvalidParam {
            name: "path",
            detail: "resolves outside MCP_FILE_ROOT".into(),
        });
    }

    let json = serde_json::to_string_pretty(workflow)?;
    std::fs::write(&target, json)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_base_urls() {
        assert_eq!(
            parse_base_url("http://127.0.0.1:8081").unwrap(),
            ("127.0.0.1".into(), 8081)
        );
        assert_eq!(
            parse_base_url("http://localhost").unwrap(),
            ("localhost".into(), 80)
        );
        assert!(parse_base_url("https://example.com").is_err());
    }

    #[test]
    fn decodes_chunked_bodies() {
        let raw = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(decode_chunked(raw).unwrap(), b"hello world".to_vec());
    }

    #[test]
    fn file_saves_stay_inside_root() {
        let root = tempfile::tempdir().unwrap();
        let workflow = Workflow::new("test");
        let path = save_to_file(root.path(), "nested/workflow.json", &workflow).unwrap();
        assert!(path.exists());
        assert!(save_to_file(root.path(), "../escape.json", &workflow).is_err());
        assert!(save_to_file(root.path(), "/tmp/escape.json", &workflow).is_err());
    }

    #[test]
    fn project_id_is_validated_before_network_use() {
        let config = Config::from_env();
        assert!(matches!(
            save_to_project(&config, "../bad", &Workflow::new("x")),
            Err(McpError::InvalidParam { .. })
        ));
    }
}
