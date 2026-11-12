//! GitHub REST client for deployment integration.
//!
//! Uses a personal access token (PAT) for authentication. The token is
//! NEVER logged, returned, placed on a command line, or embedded in a URL.
//!
//! TLS is mandatory: `api.github.com` is HTTPS, and the original plain
//! `HttpConnector` could not make a single real request. `with_base_url`
//! exists so tests can point the same client at a local plain-HTTP server.

use crate::{redact, valid_branch, valid_repo_component};
use http::{Method, Request, StatusCode, Uri};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, instrument};
use zeroize::Zeroizing;

pub const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = "nocode-ide/0.1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Error bodies are untrusted: cap them before they reach a log or a response.
const MAX_ERROR_CHARS: usize = 256;

type Connector = hyper_rustls::HttpsConnector<HttpConnector>;
type HttpClient = Client<Connector, Full<Bytes>>;

#[derive(Debug, Error)]
pub enum GitHubError {
    #[error("HTTP {status}: {message}")]
    Http { status: StatusCode, message: String },
    #[error("request failed: {0}")]
    Request(String),
    #[error("GitHub request timed out")]
    Timeout,
    #[error("invalid URI: {0}")]
    Uri(String),
    #[error("invalid response JSON: {0}")]
    Json(String),
    #[error("missing personal access token")]
    MissingToken,
    #[error("repository not found or token has no access")]
    NotFound,
    #[error("GitHub rate limit reached; retry shortly")]
    RateLimited,
    #[error("invalid GitHub configuration: {0}")]
    Config(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    pub owner: String,
    pub repo: String,
    pub branch: String,
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            owner: String::new(),
            repo: String::new(),
            branch: "main".to_string(),
        }
    }
}

impl GitHubConfig {
    /// Validate before any part of the config reaches a URL, a git command
    /// or a generated workflow. Returns a human-readable reason on failure.
    pub fn validate(&self) -> Result<(), GitHubError> {
        if !valid_repo_component(&self.owner) {
            return Err(GitHubError::Config("invalid repository owner".into()));
        }
        if !valid_repo_component(&self.repo) {
            return Err(GitHubError::Config("invalid repository name".into()));
        }
        if !valid_branch(&self.branch) {
            return Err(GitHubError::Config("invalid branch name".into()));
        }
        Ok(())
    }

    /// The clean remote URL. It contains NO credentials — a credential
    /// helper supplies them at push time (see [`crate::git`]).
    pub fn remote_url(&self) -> String {
        format!("https://github.com/{}/{}.git", self.owner, self.repo)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    /// The commit this run belongs to. Required so a deployment never
    /// reports another commit's CI result as its own.
    #[serde(default)]
    pub head_sha: String,
    #[serde(default)]
    pub head_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRunsResponse {
    #[serde(default)]
    total_count: u32,
    #[serde(default)]
    workflow_runs: Vec<WorkflowRun>,
}

/// GitHub REST API client.
///
/// `Debug` is hand-written: deriving it would print the bearer token.
pub struct GitHubClient {
    token: Zeroizing<String>,
    config: GitHubConfig,
    base_url: String,
    client: HttpClient,
}

impl fmt::Debug for GitHubClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubClient")
            .field("owner", &self.config.owner)
            .field("repo", &self.config.repo)
            .field("branch", &self.config.branch)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl GitHubClient {
    /// Create a client against the real GitHub API.
    pub fn new(token: String, config: GitHubConfig) -> Result<Self, GitHubError> {
        Self::with_base_url(token, config, GITHUB_API_BASE)
    }

    /// Create a client against an arbitrary base URL (tests point this at a
    /// local HTTP server). `https_or_http` keeps production HTTPS working.
    pub fn with_base_url(
        token: String,
        config: GitHubConfig,
        base_url: impl Into<String>,
    ) -> Result<Self, GitHubError> {
        if token.trim().is_empty() {
            return Err(GitHubError::MissingToken);
        }
        config.validate()?;
        let base_url = base_url.into().trim_end_matches('/').to_string();

        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .build();
        let client = Client::builder(TokioExecutor::new()).build(connector);

        Ok(Self {
            token: Zeroizing::new(token),
            config,
            base_url,
            client,
        })
    }

    pub fn config(&self) -> &GitHubConfig {
        &self.config
    }

    /// Verify the token can reach the repository.
    #[instrument(skip(self))]
    pub async fn verify_repo(&self) -> Result<RepoInfo, GitHubError> {
        let uri = self.repo_uri(None)?;
        let req = self.build_request(Method::GET, uri, None)?;
        let (status, body) = self.send(req).await?;
        match status {
            StatusCode::OK => self.parse(&body),
            StatusCode::NOT_FOUND => Err(GitHubError::NotFound),
            _ => Err(http_error(status, &body)),
        }
    }

    /// List recent runs of the generated deploy workflow.
    #[instrument(skip(self))]
    pub async fn list_workflow_runs(&self) -> Result<Vec<WorkflowRun>, GitHubError> {
        let uri = self.repo_uri(Some("/actions/workflows/deploy.yml/runs?per_page=20"))?;
        let req = self.build_request(Method::GET, uri, None)?;
        let (status, body) = self.send(req).await?;
        match status {
            StatusCode::OK => {
                let resp: WorkflowRunsResponse = self.parse(&body)?;
                Ok(resp.workflow_runs)
            }
            StatusCode::NOT_FOUND => Ok(Vec::new()),
            _ => Err(http_error(status, &body)),
        }
    }

    /// Get a single workflow run by id.
    #[instrument(skip(self))]
    pub async fn get_workflow_run(&self, run_id: u64) -> Result<WorkflowRun, GitHubError> {
        let uri = self.repo_uri(Some(&format!("/actions/runs/{run_id}")))?;
        let req = self.build_request(Method::GET, uri, None)?;
        let (status, body) = self.send(req).await?;
        match status {
            StatusCode::OK => self.parse(&body),
            StatusCode::NOT_FOUND => Err(GitHubError::NotFound),
            _ => Err(http_error(status, &body)),
        }
    }

    fn repo_uri(&self, suffix: Option<&str>) -> Result<Uri, GitHubError> {
        let url = format!(
            "{}/repos/{}/{}{}",
            self.base_url,
            self.config.owner,
            self.config.repo,
            suffix.unwrap_or_default()
        );
        Self::parse_uri(&url)
    }

    /// Parse a URI with an explicit result type. A bare `.parse().map_err(...)`
    /// leaves the error type ambiguous to rustc, and the URL itself is built
    /// from user-controlled config so redact it before it reaches an error.
    fn parse_uri(url: &str) -> Result<Uri, GitHubError> {
        url.parse::<Uri>()
            .map_err(|e: http::uri::InvalidUri| GitHubError::Uri(redact(&e.to_string())))
    }

    fn build_request(
        &self,
        method: Method,
        uri: Uri,
        body: Option<Vec<u8>>,
    ) -> Result<Request<Full<Bytes>>, GitHubError> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {}", self.token.as_str()));

        if body.is_some() {
            builder = builder.header("Content-Type", "application/json");
        }
        let payload = Full::new(Bytes::from(body.unwrap_or_default()));
        builder
            .body(payload)
            .map_err(|e| GitHubError::Request(redact(&e.to_string())))
    }

    async fn send(&self, req: Request<Full<Bytes>>) -> Result<(StatusCode, Vec<u8>), GitHubError> {
        let resp = tokio::time::timeout(REQUEST_TIMEOUT, self.client.request(req))
            .await
            .map_err(|_| GitHubError::Timeout)?
            .map_err(|e| GitHubError::Request(redact(&e.to_string())))?;
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| GitHubError::Request(redact(&e.to_string())))?
            .to_bytes()
            .to_vec();
        debug!(%status, bytes = body.len(), "GitHub API response");
        Ok((status, body))
    }

    fn parse<T: serde::de::DeserializeOwned>(&self, body: &[u8]) -> Result<T, GitHubError> {
        serde_json::from_slice(body).map_err(|e| GitHubError::Json(redact(&e.to_string())))
    }
}

/// Build a bounded, redacted error from an untrusted response body.
fn http_error(status: StatusCode, body: &[u8]) -> GitHubError {
    if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        return GitHubError::RateLimited;
    }
    let raw = String::from_utf8_lossy(body);
    let safe = redact(&raw);
    let message: String = safe.chars().take(MAX_ERROR_CHARS).collect();
    GitHubError::Http { status, message }
}

/// Redacted version of GitHub config for API responses.
/// The token field is completely absent.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GitHubConfigResponse {
    pub configured: bool,
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub verified: bool,
}

impl From<&GitHubConfig> for GitHubConfigResponse {
    fn from(config: &GitHubConfig) -> Self {
        Self {
            configured: true,
            owner: config.owner.clone(),
            repo: config.repo.clone(),
            branch: config.branch.clone(),
            verified: false,
        }
    }
}

impl GitHubConfigResponse {
    /// A response for a user who has not configured GitHub yet.
    pub fn absent() -> Self {
        Self {
            branch: "main".to_string(),
            ..Default::default()
        }
    }
}

impl GitHubConfigResponse {
    pub fn verified(mut self) -> Self {
        self.verified = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GitHubConfig {
        GitHubConfig {
            owner: "acme".into(),
            repo: "app".into(),
            branch: "main".into(),
        }
    }

    #[test]
    fn debug_never_prints_the_token() {
        let client = GitHubClient::with_base_url(
            "ghp_supersecret_token".into(),
            config(),
            "http://127.0.0.1:1",
        )
        .unwrap();
        let shown = format!("{client:?}");
        assert!(!shown.contains("supersecret"));
        assert!(shown.contains("acme"));
    }

    #[test]
    fn remote_url_has_no_credentials() {
        assert_eq!(config().remote_url(), "https://github.com/acme/app.git");
    }

    #[test]
    fn config_rejects_injection() {
        let mut bad = config();
        bad.owner = "acme/app".into();
        assert!(bad.validate().is_err());
        let mut bad = config();
        bad.branch = "-u".into();
        assert!(bad.validate().is_err());
        assert!(config().validate().is_ok());
    }

    #[test]
    fn http_error_is_redacted_and_bounded() {
        let body = format!("token ghp_leaked_here {}", "x".repeat(2000));
        match http_error(StatusCode::INTERNAL_SERVER_ERROR, body.as_bytes()) {
            GitHubError::Http { message, .. } => {
                assert!(!message.contains("leaked_here"));
                assert!(message.chars().count() <= MAX_ERROR_CHARS);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(matches!(
            http_error(StatusCode::TOO_MANY_REQUESTS, b"{}"),
            GitHubError::RateLimited
        ));
    }

    #[test]
    fn missing_token_is_rejected() {
        assert!(matches!(
            GitHubClient::new(String::new(), config()),
            Err(GitHubError::MissingToken)
        ));
    }
}
