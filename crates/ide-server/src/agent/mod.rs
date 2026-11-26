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
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::mpsc;

use nocode_agent::{AgentEvent, RunContext, RunRequest};
use nocode_mcp::lease::{LeaseError, LeaseGuard, DEFAULT_TTL};

use crate::auth::AuthUser;
use crate::error::{require_csrf, ApiError};
use crate::server::AppState;

pub mod access;
mod conversation;
mod keys;
mod provider;
pub mod runtime;
pub mod schema;
mod store;

pub use conversation::{Conversation, ConversationMessage};
pub use provider::SeaOrmProjectProvider;
pub use runtime::{AgentRuntime, CatalogView};

/// Remove a deleted project's conversations.
///
/// Exposed for the projects delete handler; kept here so the conversation
/// module stays private and the SQL has one home.
pub async fn delete_conversations_for_project(
    db: &sea_orm::DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
) -> Result<(), ApiError> {
    conversation::delete_for_project(db, tenant_id, project_id).await
}

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
///
/// `chat` is the stream; the conversation routes are the drawer's history. Both
/// are scoped by the project id in the path *and* by the caller's tenant, so a
/// conversation id from another project is a 404 rather than a leak.
pub fn agent_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects/:id/agent/chat", post(chat))
        .route(
            "/api/v1/projects/:id/agent/conversations",
            get(list_conversations),
        )
        .route(
            "/api/v1/projects/:id/agent/conversations/:cid",
            get(get_conversation)
                .patch(rename_conversation)
                .delete(delete_conversation),
        )
        // The provider picker. Not project-scoped: a key belongs to the
        // deployment, not to a project, and the drawer asks once per session.
        .route(
            "/api/v1/agent/providers",
            get(list_providers).post(create_provider),
        )
        // What this build *could* run, whether or not a key row exists yet.
        // The picker needs it to offer a provider that was never configured.
        .route("/api/v1/agent/providers/catalog", get(list_platforms))
        .route(
            "/api/v1/agent/providers/:id",
            patch(update_provider).delete(delete_provider),
        )
        // A stored key is a claim. This turns it into a fact.
        .route("/api/v1/agent/providers/:id/test", post(probe_provider))
}

/// `GET /api/v1/agent/providers` — what the assistant can run, and with which
/// models.
///
/// No key material is in the response: `ProviderInfo` has no field for it. The
/// route is authenticated because the list is a deployment capability, not
/// public information.
async fn list_providers(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<CatalogView>, ApiError> {
    access::require_owner(&state.db, &user).await?;
    Ok(Json(state.agent.catalog_view().await))
}

/// `GET /api/v1/agent/providers/catalog` — every platform this build supports.
///
/// Same table the request builder uses, so the list cannot claim support the
/// adapter does not have. Keyless platforms appear here too: they are usable
/// without a key row, which is exactly what the picker should show.
async fn list_platforms(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<keys::PlatformInfo>>, ApiError> {
    access::require_owner(&state.db, &user).await?;
    Ok(Json(keys::supported_platforms()))
}

/// A row id from the path. Non-numeric ids cannot name a key row, so they are
/// a bad request rather than a 404: the caller has the wrong *shape*, not a
/// stale reference.
fn row_id(raw: &str) -> Result<i64, ApiError> {
    raw.trim()
        .parse::<i64>()
        .map_err(|_| ApiError::BadRequest(format!("`{raw}` is not a provider row id")))
}

/// The picker's entry for one row, after a write.
///
/// The write routes answer with the row rather than a bare 204 because the
/// client cannot compute `unavailable` or the resolved default model: those
/// come from the catalog, and a second round trip just to re-read one row
/// would let the two answers disagree.
async fn row_view(state: &AppState, id: i64) -> Result<keys::ProviderInfo, ApiError> {
    state
        .agent
        .catalog_view()
        .await
        .providers
        .into_iter()
        .find(|provider| provider.id == id.to_string())
        .ok_or(ApiError::Internal)
}

/// `POST /api/v1/agent/providers` — add a provider key.
///
/// Validation happens before anything is sealed or stored, so a rejected draft
/// leaves no row behind. The credential is sealed with this server's vault;
/// the plaintext never reaches the database, a log line or the response.
async fn create_provider(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Json(body): Json<keys::KeyDraft>,
) -> Result<(StatusCode, Json<keys::ProviderInfo>), ApiError> {
    require_csrf(&headers)?;
    access::require_owner(&state.db, &user).await?;

    let id = keys::insert_key(&state.db, &user.id, &body).await?;
    // Without this the picker would keep serving the list it built before the
    // row existed, and the operator would see their new key vanish.
    state.agent.invalidate_catalog().await;
    Ok((StatusCode::CREATED, Json(row_view(&state, id).await?)))
}

/// `PATCH /api/v1/agent/providers/:id` — change one row.
///
/// `api_key: ""` clears the credential and `base_url: ""` drops an override;
/// omitting a field leaves it as it was. That distinction is why this is a
/// patch and not a put: a caller that only wants to disable a row must not
/// have to resend the key it cannot see.
async fn update_provider(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<keys::KeyPatch>,
) -> Result<Json<keys::ProviderInfo>, ApiError> {
    require_csrf(&headers)?;
    access::require_owner(&state.db, &user).await?;

    let id = row_id(&id)?;
    keys::update_key(&state.db, id, &body).await?;
    state.agent.invalidate_catalog().await;
    Ok(Json(row_view(&state, id).await?))
}

/// `DELETE /api/v1/agent/providers/:id` — remove a row and its sealed key.
async fn delete_provider(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    require_csrf(&headers)?;
    access::require_owner(&state.db, &user).await?;

    keys::delete_key(&state.db, row_id(&id)?).await?;
    state.agent.invalidate_catalog().await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/agent/providers/:id/test` — spend one tiny completion to find
/// out whether the stored credential actually works.
///
/// A 200 with `ok: false` is the normal failure shape: the provider answered,
/// and the answer was "no". Only a row that cannot even be built (no vault, no
/// base URL) is a 503.
async fn probe_provider(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<keys::ProbeOutcome>, ApiError> {
    require_csrf(&headers)?;
    access::require_owner(&state.db, &user).await?;

    let outcome = keys::probe_key(&state.db, row_id(&id)?).await?;
    // The check wrote a new `status`, so the cached list is now stale.
    state.agent.invalidate_catalog().await;
    Ok(Json(outcome))
}

/// Body of the chat request.
#[derive(Debug, Deserialize)]
pub struct ChatBody {
    /// What the user asked for, in natural language.
    pub prompt: String,
    /// `pipeline` (default) or `auto`.
    #[serde(default)]
    pub mode: Option<String>,
    /// Continue an existing conversation. Absent means "start a new one".
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Which provider to run, from `GET /agent/providers`.
    ///
    /// Absent means "the deployment's default", which is what a client that
    /// predates the picker sends — and it must keep working.
    #[serde(default)]
    pub provider: Option<String>,
    /// Model override for this run. Absent means the provider's default.
    #[serde(default)]
    pub model: Option<String>,
}

/// Body of a rename request.
#[derive(Debug, Deserialize)]
pub struct RenameBody {
    pub title: String,
}

/// Longest accepted title, in characters.
const MAX_TITLE_CHARS: usize = 60;

/// How many conversations the history menu lists.
const MAX_CONVERSATIONS: i64 = 50;

/// How many messages a restore returns.
const MAX_RESTORED_MESSAGES: i64 = 200;

async fn chat(
    State(state): State<AppState>,
    user: AuthUser,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ChatBody>,
) -> Result<Response, ApiError> {
    // Bearer tokens skip the double-submit check; cookie sessions must echo it.
    require_csrf(&headers)?;

    // The credentials are sealed, but a sealed key is still spendable: only
    // the operator's chosen user may reach the provider at all.
    access::require_owner(&state.db, &user).await?;

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
    // Resolving before the lease means a bad selection costs no tokens and
    // does not take the project away from another tab.
    let resolved = state
        .agent
        .provider_for(body.provider.as_deref(), body.model.as_deref())
        .await?;
    let provider = resolved.provider.clone();

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
        LeaseError::Internal(message) => {
            // The client gets a bare 500: an internal message can name tables
            // and rows, and `ApiError::Internal` is deliberately payload-free.
            tracing::error!(%message, "could not acquire the project lease");
            ApiError::Internal
        }
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
        .map_err(|error| {
            tracing::error!(%error, "could not open a project session for the run");
            ApiError::Internal
        })?;
    let session_key = caller.session_key(&session_id);

    // The conversation is resolved before the run row is written, because
    // `run_started` has to carry its id and the audit row has to be traceable to
    // it. A caller that supplies an id we do not own gets a 404 *before* any
    // tokens are spent.
    let conversation_id = match body.conversation_id.as_deref() {
        Some(id) => {
            if !conversation::valid_id(id) {
                let _ = state.mcp.executor.projects.discard(&session_key);
                return Err(ApiError::NotFound);
            }
            let owned = conversation::get(
                &state.db,
                &user.tenant_id,
                &user.id,
                &project_id,
                id,
            )
            .await?;
            if owned.is_none() {
                let _ = state.mcp.executor.projects.discard(&session_key);
                return Err(ApiError::NotFound);
            }
            id.to_string()
        }
        None => {
            conversation::create(
                &state.db,
                &user.tenant_id,
                &user.id,
                &project_id,
                &prompt,
            )
            .await?
        }
    };

    // Memory is best-effort: a read failure degrades to "no history" rather
    // than refusing the run, because the project alone is still enough to work.
    let memory = match conversation::context(&state.db, &conversation_id).await {
        Ok(memory) => memory,
        Err(error) => {
            tracing::warn!(%error, conversation_id, "could not load conversation memory");
            conversation::Memory::default()
        }
    };

    if let Err(error) = store::insert_run(
        &state.db,
        &run_id,
        &user.tenant_id,
        &project_id,
        &user.id,
        mode.as_str(),
        &prompt,
        &conversation_id,
    )
    .await
    {
        // No run is about to happen, so do not leave a session (or a lease)
        // behind for a project nobody is editing.
        let _ = state.mcp.executor.projects.discard(&session_key);
        return Err(error);
    }

    let (tx, rx) = mpsc::channel(EVENT_BUFFER);
    // A second handle on the channel, held until the transcript is stored. The
    // SSE body closes when every sender is dropped, so keeping one alive makes
    // "the stream ended" mean "the conversation is durable": a client that
    // reloads the moment `run_finished` arrives finds its messages.
    let stream_guard = tx.clone();
    let ctx = RunContext {
        run_id: run_id.clone(),
        // Cloned for the summariser below: the run owns one handle, the
        // spawned task keeps another.
        provider: provider.clone(),
        executor: state.mcp.executor.clone(),
        caller,
        session_id,
        limits: state.agent.limits(),
        chat: state.agent.chat(),
        events: tx,
        history: memory.history,
        summary: memory.summary,
        conversation_id: Some(conversation_id.clone()),
    };
    let request = RunRequest { prompt, mode };

    // The run owns the sender, so the stream ends when it finishes or is
    // dropped. Nothing else can close it early.
    let db = state.db.clone();
    let projects = state.mcp.executor.projects.clone();
    // The same provider the run used summarises the conversation later: a
    // second provider would mean a second key and a second model for one
    // thread, which is exactly the inconsistency the picker exists to avoid.
    let summariser = provider.clone();
    let conversation = conversation_id.clone();
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

        // Memory is written *after* the audit row and never fails the run: the
        // user already has their answer and the project is already saved. A
        // lost transcript costs the next turn its context, which is worse than
        // nothing only if it is silent — hence the log.
        if let Err(error) =
            conversation::append_transcript(&db, &conversation, &report.transcript).await
        {
            tracing::warn!(%error, conversation_id = %conversation, "could not store the transcript");
        }
        // Compaction runs after the append so the count it sees is the count
        // the next turn will read.
        match conversation::compact(&db, Some(summariser.as_ref()), &conversation).await {
            Ok(true) => tracing::info!(conversation_id = %conversation, "compacted the conversation"),
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, conversation_id = %conversation, "compaction failed")
            }
        }

        // The session's only reason to exist was this run. Dropping it here
        // also means a second tab cannot inherit a half-edited model.
        let _ = projects.discard(&session_key);
        // Explicit, so the lease is released before the response body is
        // dropped; `LeaseGuard::drop` would do it anyway.
        drop(lease);
        // Last: closes the stream now that memory is written.
        drop(stream_guard);
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

/* ------------------------------------------------------------------ *
 * Conversation history
 * ------------------------------------------------------------------ */

/// A project the caller can read, or a 404.
///
/// Shared by every conversation route: the path carries a project id, and a
/// conversation must never be reachable through a project the caller does not
/// own.
async fn require_project(
    state: &AppState,
    user: &AuthUser,
    project_id: &str,
) -> Result<(), ApiError> {
    let row =
        crate::projects::store::get_project(&state.db, &user.tenant_id, &user.id, project_id).await?;
    row.map(|_| ()).ok_or(ApiError::NotFound)
}

/// `GET /api/v1/projects/:id/agent/conversations`
async fn list_conversations(
    State(state): State<AppState>,
    user: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
    access::require_owner(&state.db, &user).await?;
    require_project(&state, &user, &project_id).await?;
    let rows = conversation::list(
        &state.db,
        &user.tenant_id,
        &user.id,
        &project_id,
        MAX_CONVERSATIONS,
    )
    .await?;
    Ok(Json(rows))
}

/// `GET /api/v1/projects/:id/agent/conversations/:cid`
///
/// What the drawer restores on mount: the messages of the last conversation, so
/// a reload no longer loses the thread.
async fn get_conversation(
    State(state): State<AppState>,
    user: AuthUser,
    Path((project_id, conversation_id)): Path<(String, String)>,
) -> Result<Json<ConversationDetail>, ApiError> {
    access::require_owner(&state.db, &user).await?;
    require_project(&state, &user, &project_id).await?;
    if !conversation::valid_id(&conversation_id) {
        return Err(ApiError::NotFound);
    }
    let found = conversation::get(
        &state.db,
        &user.tenant_id,
        &user.id,
        &project_id,
        &conversation_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    let messages = conversation::messages(&state.db, &conversation_id, MAX_RESTORED_MESSAGES).await?;
    Ok(Json(ConversationDetail {
        conversation: found,
        messages,
    }))
}

/// `PATCH /api/v1/projects/:id/agent/conversations/:cid`
async fn rename_conversation(
    State(state): State<AppState>,
    user: AuthUser,
    Path((project_id, conversation_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<RenameBody>,
) -> Result<Json<Conversation>, ApiError> {
    require_csrf(&headers)?;
    access::require_owner(&state.db, &user).await?;
    require_project(&state, &user, &project_id).await?;
    let title = body.title.trim();
    if title.is_empty() {
        return Err(ApiError::BadRequest("title is empty".into()));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "title is longer than {MAX_TITLE_CHARS} characters"
        )));
    }
    if !conversation::valid_id(&conversation_id) {
        return Err(ApiError::NotFound);
    }
    let renamed = conversation::rename(
        &state.db,
        &user.tenant_id,
        &user.id,
        &project_id,
        &conversation_id,
        title,
    )
    .await?;
    if !renamed {
        return Err(ApiError::NotFound);
    }
    let found = conversation::get(
        &state.db,
        &user.tenant_id,
        &user.id,
        &project_id,
        &conversation_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(found))
}

/// `DELETE /api/v1/projects/:id/agent/conversations/:cid`
async fn delete_conversation(
    State(state): State<AppState>,
    user: AuthUser,
    Path((project_id, conversation_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    require_csrf(&headers)?;
    access::require_owner(&state.db, &user).await?;
    require_project(&state, &user, &project_id).await?;
    if !conversation::valid_id(&conversation_id) {
        return Err(ApiError::NotFound);
    }
    let deleted = conversation::delete(
        &state.db,
        &user.tenant_id,
        &user.id,
        &project_id,
        &conversation_id,
    )
    .await?;
    if !deleted {
        return Err(ApiError::NotFound);
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// One conversation with its messages.
#[derive(Debug, serde::Serialize)]
pub struct ConversationDetail {
    #[serde(flatten)]
    pub conversation: Conversation,
    pub messages: Vec<ConversationMessage>,
}

/// A `Stream` over the run's event channel.
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
        /// Every request the provider was handed, for assertions about what the
        /// model actually saw.
        requests: Mutex<Vec<ChatRequest>>,
    }

    impl Scripted {
        fn new(turns: Vec<Turn>) -> Self {
            Self {
                turns: Mutex::new(turns),
                requests: Mutex::new(Vec::new()),
            }
        }

        /// The user messages of the *first* request, in order.
        fn first_request_prompts(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .first()
                .map(|request| {
                    request
                        .messages
                        .iter()
                        .filter_map(|message| match message {
                            nocode_agent::llm::ChatMessage::User(text) => Some(text.clone()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }

        /// The user messages of request `index`.
        fn prompts_at(&self, index: usize) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .get(index)
                .map(|request| {
                    request
                        .messages
                        .iter()
                        .filter_map(|message| match message {
                            nocode_agent::llm::ChatMessage::User(text) => Some(text.clone()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl LlmProvider for Scripted {
        fn name(&self) -> &'static str {
            "scripted"
        }

        fn model(&self) -> &str {
            "scripted-1"
        }

        async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, AgentError> {
            self.requests.lock().unwrap().push(request);
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

    fn bearer_for(user_id: &str, email: &str, roles: Vec<String>) -> String {
        let claims = Claims::access_for(
            user_id,
            email,
            "default".into(),
            roles,
            None,
            "jti",
            now_secs(),
        );
        format!("Bearer {}", keys().encode(&claims).unwrap())
    }

    fn bearer(roles: Vec<String>) -> String {
        bearer_for("user-1", "agent@example.com", roles)
    }

    /// The id the project row and the project document agree on.
    const PROJECT_ID: &str = "11111111-1111-4111-8111-111111111111";

    /// A database with one user, one tenant and one project.
    async fn seeded_db() -> sea_orm::DatabaseConnection {
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
        // The fixture user is the owner, so the routes stay reachable by the
        // existing tests. A second, non-owner user is used below to prove the
        // allowlist actually refuses everyone else.
        db.execute_unprepared(
            "INSERT OR IGNORE INTO agent_key_access (user_id, email) VALUES ('user-1', 'agent@example.com')",
        )
        .await
        .unwrap();

        // The row id and the project's own id must agree: `rebase` re-loads the
        // project by the id inside the document, and a fixture where the two
        // differ turns every stale save into "not found".
        let mut project = Project::new("Expense tracker");
        project.id = nocode_model::ProjectId(uuid::Uuid::parse_str(PROJECT_ID).unwrap());
        let now = chrono::Utc::now().to_rfc3339();
        crate::projects::store::insert_project(
            &db,
            PROJECT_ID,
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

        db
    }

    /// A router over an already-seeded database.
    ///
    /// Separate from [`app`] so a test can build a runtime that reads the same
    /// rows — the key-management routes need the database *and* the runtime
    /// pointed at it.
    fn router_over(db: sea_orm::DatabaseConnection, runtime: AgentRuntime) -> axum::Router {
        let state = crate::server::AppState::new(db, keys()).with_agent(Arc::new(runtime));
        crate::server::build_router(state, "apps/ide-web/dist")
    }

    /// The common case: a fresh database and a router whose assistant is
    /// `runtime`.
    async fn app(runtime: AgentRuntime) -> (axum::Router, sea_orm::DatabaseConnection) {
        let db = seeded_db().await;
        let router = router_over(db.clone(), runtime);
        (router, db)
    }

    /// A router whose assistant reads `agent_api_keys`, sharing the database.
    async fn app_with_keys() -> (axum::Router, sea_orm::DatabaseConnection) {
        let db = seeded_db().await;
        let runtime = AgentRuntime::from_env_and_db(db.clone());
        let router = router_over(db.clone(), runtime);
        (router, db)
    }

    fn chat_request(body: &str, token: Option<String>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/projects/{PROJECT_ID}/agent/chat"))
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
    async fn a_user_outside_the_owner_allowlist_cannot_run_the_agent() {
        let (app, _db) = app(AgentRuntime::disabled("test".into())).await;
        let response = app
            .oneshot(chat_request(
                r#"{"prompt":"build"}"#,
                Some(bearer_for(
                    "user-2",
                    "someone-else@example.com",
                    vec!["owner".into()],
                )),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn the_provider_picker_is_owner_only() {
        let (app, _db) = app(AgentRuntime::disabled("test".into())).await;
        let request = Request::builder()
            .uri("/api/v1/agent/providers")
            .header(
                "authorization",
                bearer_for(
                    "user-2",
                    "someone-else@example.com",
                    vec!["owner".into()],
                ),
            )
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
    async fn the_provider_picker_lists_the_configured_provider_without_a_key() {
        let scripted = Arc::new(Scripted::new(vec![]));
        let (app, _db) = app(AgentRuntime::with_provider(scripted, Limits::default())).await;

        let request = Request::builder()
            .uri("/api/v1/agent/providers")
            .header("authorization", bearer(vec!["owner".into()]))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("json body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert_eq!(value["providers"][0]["id"], "env");
        assert_eq!(value["providers"][0]["platform"], "scripted");
        assert_eq!(value["default"], "env");
        // Nothing usable is missing, so the drawer shows no empty state.
        assert!(value["reason"].is_null(), "{value}");
        // The response has no field that could carry a key.
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("api_key"), "{text}");
        assert!(!text.contains("ciphertext"), "{text}");
    }

    #[tokio::test]
    async fn the_platform_catalog_lists_every_supported_provider() {
        let (app, _db) = app(AgentRuntime::disabled("test".into())).await;
        let request = Request::builder()
            .uri("/api/v1/agent/providers/catalog")
            .header("authorization", bearer(vec!["owner".into()]))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("json body");
        let platforms: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("valid JSON");
        // Every platform the request builder can reach is advertised here, so
        // the picker cannot be narrower than the adapter.
        for expected in ["google", "cohere", "kilo", "opencode", "cloudflare", "custom"] {
            assert!(
                platforms
                    .iter()
                    .any(|platform| platform["platform"] == expected),
                "{expected} is missing from the catalog"
            );
        }
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("api_key"), "{text}");
    }

    #[tokio::test]
    async fn the_picker_requires_authentication() {
        let (app, _db) = app(AgentRuntime::disabled("test".into())).await;
        let request = Request::builder()
            .uri("/api/v1/agent/providers")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_run_can_name_the_provider_the_picker_offered() {
        let scripted = Arc::new(Scripted::new(vec![Turn::Text("Done.")]));
        let (app, _db) = app(AgentRuntime::with_provider(scripted, Limits::default())).await;

        // `env` is the picker's id for the server's own provider. Selecting it
        // must behave exactly like sending no selection at all.
        let response = app
            .oneshot(chat_request(
                r#"{"prompt":"build","mode":"auto","provider":"env"}"#,
                Some(bearer(vec!["owner".into()])),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("sse body");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("\"provider\":\"scripted\""), "{text}");
        assert!(text.contains("\"status\":\"succeeded\""), "{text}");
    }

    #[tokio::test]
    async fn a_scripted_run_streams_frames_saves_and_is_audited() {
        // The script is a stack, so the last entry is served first.
        let scripted = Arc::new(Scripted::new(vec![
            Turn::Text("Added a Note entity."),
            Turn::Tools(vec![(
                "entity_create",
                serde_json::json!({ "name": "Note" }),
            )]),
        ]));

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

    /// The `conversation_id` of the `run_started` frame in an SSE body.
    fn conversation_id_of(body: &str) -> String {
        let marker = "\"conversation_id\":\"";
        let start = body.find(marker).expect("run_started carries a conversation id") + marker.len();
        body[start..]
            .split('"')
            .next()
            .expect("a closing quote")
            .to_string()
    }

    fn get_request(uri: &str, token: String) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", token)
            .body(Body::empty())
            .unwrap()
    }

    async fn body_text(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[tokio::test]
    async fn a_follow_up_run_is_given_the_previous_turn() {
        // A stack: the last entry is served first, so run one gets
        // "Created the Expense entity." and run two gets the follow-up answer.
        let scripted = Arc::new(Scripted::new(vec![
            Turn::Text("The threshold is now 1000."),
            Turn::Text("Created the Expense entity with a 500 threshold."),
        ]));
        let (app, _db) = app(AgentRuntime::with_provider(scripted.clone(), Limits::default())).await;

        let first = app
            .clone()
            .oneshot(chat_request(
                r#"{"prompt":"Build an expense tracker that escalates above 500","mode":"auto"}"#,
                Some(bearer(vec!["owner".into()])),
            ))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let conversation_id = conversation_id_of(&body_text(first).await);

        let second = app
            .oneshot(chat_request(
                &format!(
                    r#"{{"prompt":"Now make the threshold 1000","mode":"auto","conversation_id":"{conversation_id}"}}"#
                ),
                Some(bearer(vec!["owner".into()])),
            ))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);
        let second_body = body_text(second).await;
        assert!(
            second_body.contains(&conversation_id),
            "the second run must stay in the same conversation: {second_body}"
        );

        // Request 0 is run one, request 1 is run two. Run two's prompt must
        // carry the earlier turn *before* the new one, so "the threshold" has
        // something to resolve against.
        assert_eq!(scripted.request_count(), 2, "one turn per run");
        let prompts = scripted.prompts_at(1);
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        assert!(
            prompts[0].contains("escalates above 500"),
            "the earlier turn is missing: {prompts:?}"
        );
        assert!(prompts[1].contains("make the threshold 1000"), "{prompts:?}");
    }

    #[tokio::test]
    async fn a_reload_restores_the_conversation() {
        let scripted = Arc::new(Scripted::new(vec![Turn::Text("Added the Note entity.")]));
        let (app, _db) = app(AgentRuntime::with_provider(scripted, Limits::default())).await;
        let token = bearer(vec!["owner".into()]);

        let response = app
            .clone()
            .oneshot(chat_request(
                r#"{"prompt":"Add a Note entity","mode":"auto"}"#,
                Some(token.clone()),
            ))
            .await
            .unwrap();
        let conversation_id = conversation_id_of(&body_text(response).await);

        // The drawer's history menu.
        let list = app
            .clone()
            .oneshot(get_request(
                &format!("/api/v1/projects/{PROJECT_ID}/agent/conversations"),
                token.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let listed = body_text(list).await;
        assert!(listed.contains(&conversation_id), "{listed}");
        assert!(listed.contains("Add a Note entity"), "{listed}");

        // The restore after a reload.
        let detail = app
            .oneshot(get_request(
                &format!(
                    "/api/v1/projects/{PROJECT_ID}/agent/conversations/{conversation_id}"
                ),
                token,
            ))
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::OK);
        let restored = body_text(detail).await;
        assert!(restored.contains("\"role\":\"user\""), "{restored}");
        assert!(restored.contains("\"role\":\"assistant\""), "{restored}");
        assert!(restored.contains("Add a Note entity"), "{restored}");
        assert!(restored.contains("Added the Note entity."), "{restored}");
    }

    #[tokio::test]
    async fn a_conversation_from_another_project_is_a_404() {
        let scripted = Arc::new(Scripted::new(vec![Turn::Text("done")]));
        let (app, _db) = app(AgentRuntime::with_provider(scripted, Limits::default())).await;
        let token = bearer(vec!["owner".into()]);

        let response = app
            .clone()
            .oneshot(chat_request(
                r#"{"prompt":"Add a Note entity","mode":"auto"}"#,
                Some(token.clone()),
            ))
            .await
            .unwrap();
        let conversation_id = conversation_id_of(&body_text(response).await);

        // Same id, different project in the path: not found, not a leak.
        let detail = app
            .oneshot(get_request(
                &format!("/api/v1/projects/another/agent/conversations/{conversation_id}"),
                token,
            ))
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::NOT_FOUND);
    }

    /* -------------------------------------------------------------- *
     * Key management
     * -------------------------------------------------------------- */

    fn provider_request(
        method: &str,
        uri: &str,
        body: Option<&str>,
        token: String,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", token);
        let builder = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        };
        builder.unwrap()
    }

    /// The picker's write path end to end, on a platform that needs no
    /// credential so the test does not depend on a vault key being present.
    #[tokio::test]
    async fn a_provider_can_be_added_disabled_and_removed() {
        let (app, _db) = app_with_keys().await;
        let token = bearer(vec!["owner".into()]);

        let created = app
            .clone()
            .oneshot(provider_request(
                "POST",
                "/api/v1/agent/providers",
                Some(r#"{"platform":"kilo"}"#),
                token.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let body = body_text(created).await;
        let value: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(value["platform"], "kilo");
        assert_eq!(value["keyless"], true);
        assert_eq!(value["status"], "unknown", "a new row has not been checked");
        assert!(value["unavailable"].is_null(), "{value}");
        // The row is answered from the catalog, so the drawer does not have to
        // guess the default model.
        assert!(!value["model"].as_str().unwrap_or_default().is_empty());
        // Nothing in the response could carry a credential.
        assert!(!body.contains("api_key"), "{body}");
        assert!(!body.contains("ciphertext"), "{body}");

        let id = value["id"].as_str().expect("id").to_string();
        let uri = format!("/api/v1/agent/providers/{id}");

        let disabled = app
            .clone()
            .oneshot(provider_request(
                "PATCH",
                &uri,
                Some(r#"{"enabled":false}"#),
                token.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(disabled.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_str(&body_text(disabled).await).expect("json");
        assert_eq!(value["enabled"], false);
        assert_eq!(value["unavailable"], "disabled");

        let removed = app
            .clone()
            .oneshot(provider_request("DELETE", &uri, None, token.clone()))
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);

        let listed = app
            .oneshot(get_request("/api/v1/agent/providers", token))
            .await
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&body_text(listed).await).expect("json");
        // Counted by platform rather than by length: a deployment that also
        // sets `AGENT_PROVIDER` legitimately lists an `env` row first.
        let rows = value["providers"].as_array().expect("providers");
        assert!(
            rows.iter().all(|row| row["platform"] != "kilo"),
            "the removed row is still listed: {value}"
        );
    }

    /// The regression the `RwLock` in `AgentRuntime` exists for: the catalog is
    /// cached, so a write that did not invalidate it would leave the operator
    /// staring at a list without the key they just added.
    #[tokio::test]
    async fn a_new_provider_is_visible_without_a_restart() {
        let (app, _db) = app_with_keys().await;
        let token = bearer(vec!["owner".into()]);

        // First read fills the cache with an empty catalog.
        let empty = app
            .clone()
            .oneshot(get_request("/api/v1/agent/providers", token.clone()))
            .await
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&body_text(empty).await).expect("json");
        assert!(
            value["providers"]
                .as_array()
                .expect("providers")
                .iter()
                .all(|row| row["platform"] != "kilo"),
            "{value}"
        );

        let created = app
            .clone()
            .oneshot(provider_request(
                "POST",
                "/api/v1/agent/providers",
                Some(r#"{"platform":"kilo"}"#),
                token.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);

        let listed = app
            .oneshot(get_request("/api/v1/agent/providers", token))
            .await
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&body_text(listed).await).expect("json");
        let added = value["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .find(|row| row["platform"] == "kilo")
            .unwrap_or_else(|| {
                panic!("the picker still serves the list it built before the write: {value}")
            });
        // A usable row is the default choice when the environment sets none.
        assert!(added["unavailable"].is_null(), "{added}");
        assert!(!value["default"].is_null(), "{value}");
    }

    /// Validation runs before anything is sealed or stored, so a rejected
    /// draft is a 400 and leaves the table untouched.
    #[tokio::test]
    async fn a_draft_that_needs_a_key_is_refused() {
        let (app, db) = app_with_keys().await;
        let response = app
            .oneshot(provider_request(
                "POST",
                "/api/v1/agent/providers",
                Some(r#"{"platform":"groq"}"#),
                bearer(vec!["owner".into()]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_text(response).await;
        assert!(body.contains("API key"), "{body}");

        let rows = db
            .query_one(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM agent_api_keys",
                vec![],
            ))
            .await
            .unwrap()
            .and_then(|row| row.try_get::<i64>("", "n").ok())
            .unwrap_or(-1);
        assert_eq!(rows, 0, "a rejected draft must not store a row");
    }

    #[tokio::test]
    async fn key_management_is_owner_only() {
        let (app, _db) = app_with_keys().await;
        let response = app
            .oneshot(provider_request(
                "POST",
                "/api/v1/agent/providers",
                Some(r#"{"platform":"kilo"}"#),
                bearer_for(
                    "user-2",
                    "someone-else@example.com",
                    vec!["owner".into()],
                ),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// A key row is deployment-wide storage, so the write routes are behind
    /// the same authentication as everything else.
    #[tokio::test]
    async fn adding_a_provider_requires_authentication() {
        let (app, _db) = app_with_keys().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agent/providers")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"platform":"kilo"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_provider_id_that_is_not_a_number_is_a_bad_request() {
        let (app, _db) = app_with_keys().await;
        let response = app
            .oneshot(provider_request(
                "DELETE",
                "/api/v1/agent/providers/not-a-row",
                None,
                bearer(vec!["owner".into()]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn deleting_a_missing_provider_is_a_404() {
        let (app, _db) = app_with_keys().await;
        let response = app
            .oneshot(provider_request(
                "DELETE",
                "/api/v1/agent/providers/999",
                None,
                bearer(vec!["owner".into()]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
