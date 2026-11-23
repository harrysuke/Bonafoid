//! Phase 11: version control and deployment environments.
//!
//! # Commit-graph model
//!
//! `versioning/schema.rs` defines a UNIQUE index on
//! `project_revisions.parent_id`, so a revision cannot have two children. The
//! graph is therefore a **chain**, not a DAG. The HTTP layer appends every new
//! revision with the project's current tip as its parent and moves only the
//! branch pointer being operated on; two branches never share a newly written
//! child. A fast-forward merge just advances the target pointer to the source
//! head. A conflicted merge appends one NEW revision onto the chain with an
//! explicit `theirs`/`ours` policy and then advances the target pointer —
//! there is no implicit last-write-wins and no history rewriting. Branches are
//! isolated pointers, so a commit on one never changes another's head.

pub mod handlers;
pub mod schema;
pub mod store;
pub mod vcs;

#[cfg(test)]
mod tests;

use axum::routing::{delete, get, patch, post};
use axum::Router;

use crate::server::AppState;

/// The 16 versioning API routes, merged into the main server router.
pub fn versioning_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/versioning/branches",
            get(handlers::list_branches).post(handlers::create_branch),
        )
        .route(
            "/api/v1/versioning/branches/:name/merge",
            post(handlers::merge),
        )
        .route("/api/v1/versioning/history", get(handlers::history))
        .route(
            "/api/v1/versioning/revisions/:id",
            get(handlers::get_revision),
        )
        .route("/api/v1/versioning/commits", post(handlers::commit))
        .route("/api/v1/versioning/diff", get(handlers::diff))
        .route("/api/v1/versioning/restore", post(handlers::restore))
        .route(
            "/api/v1/versioning/environments",
            get(handlers::list_environments).post(handlers::create_environment),
        )
        .route(
            "/api/v1/versioning/environments/:name",
            patch(handlers::patch_environment),
        )
        .route(
            "/api/v1/versioning/environments/:name/promote",
            post(handlers::promote),
        )
        .route(
            "/api/v1/versioning/environments/:name/rollback",
            post(handlers::rollback),
        )
        .route(
            "/api/v1/versioning/environments/:name/secrets",
            get(handlers::list_secrets).post(handlers::create_secret),
        )
        .route(
            "/api/v1/versioning/environments/:name/secrets/:key",
            delete(handlers::delete_secret),
        )
}
