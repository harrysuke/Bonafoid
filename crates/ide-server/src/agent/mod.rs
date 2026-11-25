//! `POST /api/v1/projects/:id/agent/chat` — the agent, streamed.
//!
//! The endpoint is a thin adapter. All the behaviour that matters (bounded
//! turns, per-step tool allowlists, validate-then-save) lives in
//! `nocode_agent::run`, which is testable without HTTP. This module only:
//!
//! 1. authenticates and authorizes the caller;
//! 2. fails *before* the stream starts when the feature is unconfigured or the
//!    project is missing, so the browser gets a real status code;
//! 3. opens the project session and spawns the run;
//! 4. forwards `AgentEvent`s as SSE frames until the run ends.
//!
//! # Why a channel and not a callback
//!
//! `run` owns its progress; the HTTP response owns the socket. A bounded
//! `mpsc` between them gives backpressure for free: if the browser stops
//! reading, `emit` waits, and the run slows instead of buffering the whole
//! timeline in memory.
//!
//! # Cancellation
//!
//! Dropping the response body drops the receiver, the next `emit` fails, and
//! the run stops with `RunStatus::Cancelled` — no tool call is left running and
//! the audit row records why.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::mpsc;

use nocode_agent::{AgentEvent, RunContext, RunRequest};
use nocode_mcp::lease::{LeaseError, LeaseGuard, DEFAULT_TTL};

use crate::auth::AuthUser;
use crate::error::{require_csrf, ApiError};
use crate::server::AppState;

mod provider;
pub mod runtime;
pub mod schema;
mod store;

pub use provider::SeaOrmProjectProvider;
pub use runtime::AgentRuntime;

/// Queue depth between the run and the socket.
///
/// Large enough that a burst of `model_patch` events never blocks a run that is
/// making progress, small enough that a stalled browser cannot pin much memory.
const EVENT_BUFFER: usize = 256;

/// How often to send an SSE comment while the model is thinking.
///
/// A turn can take 30 seconds. Without this, proxies with a 60-second idle
/// timeout would cut the stream mid-build.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Longest accepted prompt, in characters. Well above any real request and well
/// below anything that would make the audit row unreasonable.
const MAX_PROMPT_CHARS: usize = 4_000;

/// How long a run may hold its project lease without renewing it.
///
/// The guard renews every 30 s, so this only expires a run that has stopped
/// making progress — a crashed process, or a machine that went to sleep.
const LEASE_TTL: Duration = DEFAULT_TTL;

/// The agent routes, merged into the main router.
pub fn agent_router() -> Router<AppState> {
    Router::new().route("/api/v1/projects/:id/agent/chat", post(chat))
}

/// Body of the chat request.
#[derive(Debug, Deserialize)]
pub struct ChatBody {
    /// What the user asked for, in natural language.
    pub prompt: String,
    /// `pipeline` (default) or `auto`.
    #[serde(default)]
    pub mode: Option<String>,
}

async fn chat(
    State(state): State<AppState>,
    user: AuthUser,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ChatBody>,
) -> Result<Response, ApiError> {
    // Bearer tokens skip the double-submit check; cookie sessions must echo it.
    require_csrf(&headers)?;

    let caller = crate::mcp::caller_for(&user);
    if !caller.has_scope("project:write") {
        return Err(ApiError::Forbidden(
            "the assistant needs the project:write scope".into(),
        ));
    }

    let prompt = body.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(ApiError::BadRequest("prompt is empty".into()));
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(ApiError::BadRequest(format!(
            "prompt is longer than {MAX_PROMPT_CHARS} characters"
        )));
    }
    let mode = nocode_agent::Mode::parse(body.mode.as_deref().unwrap_or_default())
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;

    // A missing provider is an operator task, not a bug: 503 with the fix.
    let provider = state.agent.provider()?.clone();

    // The run id is minted first: it is the lease holder, the audit key and the
    // id the browser receives, and all three must agree.
    let run_id = uuid::Uuid::new_v4().to_string();

    // One writer per project. Taken before the project is even read, so a
    // second tab gets a 409 rather than a stream that dies mid-build — and
    // before any tokens are spent.
    let lease = LeaseGuard::acquire(
        state.leases.clone(),
        &user.tenant_id,
        &project_id,
        &run_id,
        LEASE_TTL,
    )
    .await
    .map_err(|error| match error {
        LeaseError::Locked { holder, expires_in } => ApiError::ProjectLocked {
            holder,
            expires_in_seconds: expires_in.as_secs().max(1),
        },
        LeaseError::Internal(message) => ApiError::Internal(message),
    })?;

    // Load before streaming so a bad id is a 404, not an empty stream.
    let row =
        crate::projects::store::get_project(&state.db, &user.tenant_id, &user.id, &project_id)
            .await?
            .ok_or(ApiError::NotFound)?;
    let project: nocode_model::Project = serde_json::from_str(&row.data).map_err(|error| {
        tracing::error!(error = %error, project_id, "stored project JSON is corrupt");
        ApiError::Internal
    })?;
    let revision = row.summary.revision.max(0) as u64;

    // Seed the session from the row we just read rather than calling
    // `project_open`: one query instead of two, and no tool envelope to parse.
    // The session is named after the lease token, so it cannot outlive the
    // right to write.
    let session_id = lease.token().to_string();
    state
        .mcp
        .executor
        .projects
        .create_with_id(&caller.owner, &session_id, project, revision)
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    let session_key = caller.session_key(&session_id);

    if let Err(error) = store::insert_run(
        &state.db,
        &run_id,
        &user.tenant_id,
        &project_id,
        &user.id,
        mode.as_str(),
        &prompt,
    )
    .await
    {
        // No run is about to happen, so do not leave a session (or a lease)
        // behind for a project nobody is editing.
        let _ = state.mcp.executor.projects.discard(&session_key);
        return Err(error);
    }

    let (tx, rx) = mpsc::channel(EVENT_BUFFER);
    let ctx = RunContext {
        run_id: run_id.clone(),
        provider,
        executor: state.mcp.executor.clone(),
        caller,
        session_id,
        limits: state.agent.limits(),
        chat: state.agent.chat(),
        events: tx,
    };
    let request = RunRequest { prompt, mode };

    // The run owns the sender, so the stream ends when it finishes or is
    // dropped. Nothing else can close it early.
    let db = state.db.clone();
    let projects = state.mcp.executor.projects.clone();
    tokio::spawn(async move {
        let report = nocode_agent::run(ctx, request).await;
        tracing::info!(
            run_id = %report.run_id,
            status = report.status.as_str(),
            revision = report.revision,
            rebased = report.rebased,
            tokens_in = report.usage.tokens_in,
            tokens_out = report.usage.tokens_out,
            "agent run finished"
        );
        if let Err(error) = store::finish_run(
            &db,
            &report.run_id,
            report.status.as_str(),
            report.revision,
            report.saved,
            report.usage,
            report.error.as_deref(),
        )
        .await
        {
            tracing::warn!(error = %error, run_id = %report.run_id, "could not close the run row");
        }
        // The session's only reason to exist was this run. Dropping it here
        // also means a second tab cannot inherit a half-edited model.
        let _ = projects.discard(&session_key);
        // Explicit, so the lease is released before the response body is
        // dropped; `LeaseGuard::drop` would do it anyway.
        drop(lease);
    });

    let mut response = Sse::new(EventStream { rx })
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("ping"))
        .into_response();
    // nginx buffers proxied responses by default, which turns a live stream
    // into one large body delivered at the end.
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

/// A `Stream` over the run's event channel.
///
/// Hand-written for one reason: `futures_util::stream::unfold` would add the
/// `futures-macro` proc-macro crate to the build for a dozen lines of
/// `poll_recv` plumbing. The stream ends when the run drops its sender.
struct EventStream {
    rx: mpsc::Receiver<AgentEvent>,
}

impl futures_core::Stream for EventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // `EventStream` is `Unpin` (its only field is), so a mutable reference
        // is available without unsafe.
        match self.get_mut().rx.poll_recv(cx) {
            Poll::Ready(Some(event)) => Poll::Ready(Some(Ok(frame(&event)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// One SSE frame: `event:` is the kind, `data:` is the JSON payload.
///
/// The payload also carries `type`, so a client that ignores the `event:` field
/// (or a proxy that drops it) still gets a tagged object.
fn frame(event: &AgentEvent) -> Event {
    let payload = serde_json::to_string(event).unwrap_or_else(|error| {
        // Serialising a struct of primitives cannot fail; if it somehow does,
        // send a valid `error` event rather than a malformed frame.
        tracing::error!(%error, "could not serialise an agent event");
        r#"{"type":"error","message":"could not serialise an event","recoverable":false,"kind":"internal"}"#
            .to_string()
    });
    Event::default().event(event.kind()).data(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex, OnceLock};

    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sea_orm::ConnectionTrait;
    use tower::ServiceExt;

    use nocode_agent::llm::{ChatRequest, ChatResponse, LlmProvider, StopReason, ToolCall};
    use nocode_agent::{AgentError, Limits, Usage};
    use nocode_model::Project;

    use crate::auth::jwt::{now_secs, Claims};
    use crate::auth::JwtKeys;

    /// One scripted assistant turn.
    enum Turn {
        Tools(Vec<(&'static str, serde_json::Value)>),
        Text(&'static str),
    }

    struct Scripted {
        turns: Mutex<Vec<Turn>>,
    }

    #[async_trait]
    impl LlmProvider for Scripted {
        fn name(&self) -> &'static str {
            "scripted"
        }

        fn model(&self) -> &str {
            "scripted-1"
        }

        async fn complete(&self, _request: ChatRequest) -> Result<ChatResponse, AgentError> {
            let turn = self
                .turns
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| AgentError::Internal("script exhausted".into()))?;
            let usage = Usage {
                tokens_in: 10,
                tokens_out: 5,
                tool_calls: 0,
                turns: 0,
            };
            Ok(match turn {
                Turn::Tools(calls) => ChatResponse {
                    text: None,
                    tool_calls: calls
                        .into_iter()
                        .enumerate()
                        .map(|(index, (name, arguments))| ToolCall {
                            id: format!("call_{index}"),
                            name: name.to_string(),
                            arguments,
                        })
                        .collect(),
                    usage,
                    stop_reason: StopReason::ToolUse,
                },
                Turn::Text(text) => ChatResponse {
                    text: Some(text.to_string()),
                    tool_calls: Vec::new(),
                    usage,
                    stop_reason: StopReason::EndTurn,
                },
            })
        }
    }

    fn keys() -> Arc<JwtKeys> {
        static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
        KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("test keys")))
            .clone()
    }

    fn bearer(roles: Vec<String>) -> String {
        let claims = Claims::access_for(
            "user-1",
            "agent@example.com",
            "default".into(),
            roles,
            None,
            "jti",
            now_secs(),
        );
        format!("Bearer {}", keys().encode(&claims).unwrap())
    }

    /// A database with one user, one tenant and one project, plus a router
    /// whose assistant is `runtime`.
    async fn app(runtime: AgentRuntime) -> (axum::Router, sea_orm::DatabaseConnection) {
        let db = crate::db::connect_sqlite_memory().await.expect("db");
        // The tenancy and project tables carry foreign keys, so the rows have
        // to exist before the project does.
        db.execute_unprepared(
            "INSERT OR IGNORE INTO tenants (id, name, slug) VALUES ('default', 'Default', 'default')",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT OR IGNORE INTO users (id, email, password_hash) VALUES ('user-1', 'agent@example.com', 'x')",
        )
        .await
        .unwrap();

        let project = Project::new("Expense tracker");
        let now = chrono::Utc::now().to_rfc3339();
        crate::projects::store::insert_project(
            &db,
            "project-1",
            "default",
            "user-1",
            &project.name,
            &project.slug,
            None,
            &project.version,
            &serde_json::to_string(&project).unwrap(),
            &now,
            &now,
        )
        .await
        .expect("insert project");

        let state = crate::server::AppState::new(db.clone(), keys()).with_agent(Arc::new(runtime));
        (crate::server::build_router(state, "apps/ide-web/dist"), db)
    }

    fn chat_request(body: &str, token: Option<String>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/v1/projects/project-1/agent/chat")
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", token);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn chat_requires_authentication() {
        let (app, _db) = app(AgentRuntime::disabled("test".into())).await;
        let response = app
            .oneshot(chat_request(r#"{"prompt":"build"}"#, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_member_without_the_write_scope_is_refused() {
        let (app, _db) = app(AgentRuntime::disabled("test".into())).await;
        let response = app
            .oneshot(chat_request(
                r#"{"prompt":"build"}"#,
                Some(bearer(vec!["member".into()])),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn an_unconfigured_assistant_is_a_503_not_an_empty_stream() {
        let (app, _db) = app(AgentRuntime::disabled("set AGENT_PROVIDER".into())).await;
        let response = app
            .oneshot(chat_request(
                r#"{"prompt":"build"}"#,
                Some(bearer(vec!["owner".into()])),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn an_empty_prompt_is_rejected_before_the_provider_is_needed() {
        let (app, _db) = app(AgentRuntime::disabled("test".into())).await;
        let response = app
            .oneshot(chat_request(
                r#"{"prompt":"   "}"#,
                Some(bearer(vec!["owner".into()])),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_scripted_run_streams_frames_saves_and_is_audited() {
        // The script is a stack, so the last entry is served first.
        let scripted = Arc::new(Scripted {
            turns: Mutex::new(vec![
                Turn::Text("Added a Note entity."),
                Turn::Tools(vec![(
                    "entity_create",
                    serde_json::json!({ "name": "Note" }),
                )]),
            ]),
        });

        // The script never mentions the session id: the orchestrator injects
        // it, which is exactly the behaviour this test is here to prove.
        let (app, db) = app(AgentRuntime::with_provider(scripted, Limits::default())).await;

        let response = app
            .oneshot(chat_request(
                r#"{"prompt":"Add a Note entity","mode":"auto"}"#,
                Some(bearer(vec!["owner".into()])),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("sse body");
        let text = String::from_utf8_lossy(&body);
        for expected in [
            "event: run_started",
            "event: step_started",
            "event: tool_call",
            "event: model_patch",
            "event: validation",
            "event: project_updated",
            "event: run_finished",
        ] {
            assert!(text.contains(expected), "missing {expected} in:\n{text}");
        }
        assert!(text.contains("\"status\":\"succeeded\""), "{text}");

        // The audit row was written at start and completed at the end.
        let rows = db
            .query_all(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT status, saved, revision, tokens_in FROM agent_runs",
            ))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let status: String = rows[0].try_get("", "status").unwrap();
        let saved: i64 = rows[0].try_get("", "saved").unwrap();
        let revision: i64 = rows[0].try_get("", "revision").unwrap();
        let tokens_in: i64 = rows[0].try_get("", "tokens_in").unwrap();
        assert_eq!(status, "succeeded");
        assert_eq!(saved, 1);
        assert_eq!(revision, 2);
        assert_eq!(tokens_in, 20, "two turns of 10 input tokens");
    }
}
