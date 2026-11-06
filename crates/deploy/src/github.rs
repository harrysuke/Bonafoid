//! GitHub REST client for deployment integration.
//!
//! Uses a personal access token (PAT) for authentication.
//! The token is NEVER logged, returned, or placed on a command line.

use http::{Method, Request, StatusCode, Uri};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, error, instrument};

const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = "nocode-ide/0.1";

#[derive(Debug, Error)]
pub enum GitHubError {
    #[error("HTTP error: {status} — {message}")]
    Http { status: StatusCode, message: String },
    #[error("request failed: {0}")]
    Request(#[from] hyper::Error),
    #[error("URI parse failed: {0}")]
    Uri(#[from] http::uri::InvalidUri),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing personal access token")]
    MissingToken,
    #[error("repository not found or token has no access")]
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    pub owner: String,
    pub repo: String,
    pub branch: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRunsResponse {
    total_count: u32,
    workflow_runs: Vec<WorkflowRun>,
}

/// GitHub REST API client.
pub struct GitHubClient {
    token: String,
    config: GitHubConfig,
    client: hyper_util::client::legacy::Client<hyper_util::rt::TokioExecutor, Full<Bytes>>,
}

impl GitHubClient {
    /// Create a new client. The token is the user's personal access token.
    pub fn new(token: String, config: GitHubConfig) -> Result<Self, GitHubError> {
        if token.is_empty() {
            return Err(GitHubError::MissingToken);
        }
        let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(
                hyper_util::client::legacy::connect::HttpConnector::new(),
            );
        Ok(Self {
            token,
            config,
            client,
        })
    }

    /// Verify the token can reach the repository.
    #[instrument(skip(self))]
    pub async fn verify_repo(&self) -> Result<RepoInfo, GitHubError> {
        let uri: Uri = format!(
            "{}/repos/{}/{}",
            GITHUB_API_BASE, self.config.owner, self.config.repo
        )
        .parse()?;
        let req = self.build_request(Method::GET, uri, None)?;
        let (status, body) = self.send(req).await?;
        match status {
            StatusCode::OK => Ok(serde_json::from_slice(&body)?),
            StatusCode::NOT_FOUND => Err(GitHubError::NotFound),
            _ => Err(GitHubError::Http {
                status,
                message: String::from_utf8_lossy(&body).to_string(),
            }),
        }
    }

    /// List recent workflow runs for the deploy workflow.
    #[instrument(skip(self))]
    pub async fn list_workflow_runs(&self) -> Result<Vec<WorkflowRun>, GitHubError> {
        let uri: Uri = format!(
            "{}/repos/{}/{}/actions/workflows/deploy.yml/runs?branch={}&per_page=10",
            GITHUB_API_BASE,
            self.config.owner,
            self.config.repo,
            self.config.branch
        )
        .parse()?;
        let req = self.build_request(Method::GET, uri, None)?;
        let (status, body) = self.send(req).await?;
        match status {
            StatusCode::OK => {
                let resp: WorkflowRunsResponse = serde_json::from_slice(&body)?;
                Ok(resp.workflow_runs)
            }
            StatusCode::NOT_FOUND => Ok(vec![]),
            _ => Err(GitHubError::Http {
                status,
                message: String::from_utf8_lossy(&body).to_string(),
            }),
        }
    }

    /// Get a single workflow run by id.
    #[instrument(skip(self))]
    pub async fn get_workflow_run(&self, run_id: u64) -> Result<WorkflowRun, GitHubError> {
        let uri: Uri = format!(
            "{}/repos/{}/{}/actions/runs/{}",
            GITHUB_API_BASE, self.config.owner, self.config.repo, run_id
        )
        .parse()?;
        let req = self.build_request(Method::GET, uri, None)?;
        let (status, body) = self.send(req).await?;
        match status {
            StatusCode::OK => Ok(serde_json::from_slice(&body)?),
            StatusCode::NOT_FOUND => Err(GitHubError::NotFound),
            _ => Err(GitHubError::Http {
                status,
                message: String::from_utf8_lossy(&body).to_string(),
            }),
        }
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
            .header("Authorization", format!("Bearer {}", self.token));

        if body.is_some() {
            builder = builder.header("Content-Type", "application/json");
        }

        let body = match body {
            Some(b) => Full::new(Bytes::from(b)),
            None => Full::new(Bytes::new()),
        };

        builder.body(body).map_err(|e| {
            error!("request build error: {}", e);
            GitHubError::Http {
                status: StatusCode::BAD_REQUEST,
                message: e.to_string(),
            }
        })
    }

    async fn send(
        &self,
        req: Request<Full<Bytes>>,
    ) -> Result<(StatusCode, Vec<u8>), GitHubError> {
        let resp = self.client.request(req).await?;
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| GitHubError::Http {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            })?
            .to_bytes()
            .to_vec();
        debug!("GitHub API response: {}", status);
        Ok((status, body))
    }
}

/// Redacted version of GitHub config for API responses.
/// The token field is completely absent.
#[derive(Debug, Clone, Serialize)]
pub struct GitHubConfigResponse {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub verified: bool,
}

impl From<&GitHubConfig> for GitHubConfigResponse {
    fn from(config: &GitHubConfig) -> Self {
        Self {
            owner: config.owner.clone(),
            repo: config.repo.clone(),
            branch: config.branch.clone(),
            verified: false,
        }
    }
}
