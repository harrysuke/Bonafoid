//! Stateful workflow sessions with revisions, bounded undo/redo and
//! per-caller isolation.
//!
//! Sessions live in memory only. `save_workflow` is the explicit durability
//! boundary. HTTP callers are scoped by tenant and user in [`SessionKey`], so
//! one user cannot read or mutate another user's session by guessing a UUID.

use chrono::Utc;
use nocode_model::{Project, Workflow};
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

use crate::error::McpError;

/// A session belongs to exactly one caller identity. The stdio binary uses
/// the fixed owner `local`; HTTP uses `tenant_id:user_id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub owner: String,
    pub id: String,
}

impl SessionKey {
    pub fn new(owner: impl AsRef<str>, id: impl AsRef<str>) -> Self {
        Self {
            owner: owner.as_ref().to_string(),
            id: id.as_ref().to_string(),
        }
    }
}

/// One editable workflow and its change history.
#[derive(Debug, Clone)]
pub struct WorkflowSession {
    pub workflow: Workflow,
    pub revision: u64,
    pub undo: Vec<Workflow>,
    pub redo: Vec<Workflow>,
}

impl WorkflowSession {
    fn new(workflow: Workflow) -> Self {
        Self {
            workflow,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

/// In-memory store shared by both transports.
pub struct SessionStore {
    sessions: RwLock<HashMap<SessionKey, WorkflowSession>>,
    undo_depth: usize,
}

impl SessionStore {
    pub fn new(undo_depth: usize) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            undo_depth,
        }
    }

    /// Create a session and return its generated id and initial revision.
    pub fn create(&self, owner: &str, workflow: Workflow) -> Result<(String, u64), McpError> {
        let id = Uuid::new_v4().to_string();
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        sessions.insert(SessionKey::new(owner, &id), WorkflowSession::new(workflow));
        Ok((id, 0))
    }

    /// Insert a session with a caller-chosen id (used by tests and imports).
    pub fn insert(&self, key: SessionKey, workflow: Workflow) -> Result<u64, McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        sessions.insert(key, WorkflowSession::new(workflow));
        Ok(0)
    }

    /// Read without mutating. The closure is short-lived and never awaits.
    pub fn with_read<T>(
        &self,
        key: &SessionKey,
        f: impl FnOnce(&WorkflowSession) -> Result<T, McpError>,
    ) -> Result<T, McpError> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        let session = sessions
            .get(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        f(session)
    }

    /// Mutate a session atomically.
    ///
    /// The previous workflow is snapshotted *before* the closure runs and is
    /// restored if the closure fails, so a rejected edit cannot leave a
    /// half-applied graph behind. Revisions are only advanced on success.
    pub fn mutate<T>(
        &self,
        key: &SessionKey,
        expected_revision: Option<u64>,
        f: impl FnOnce(&mut Workflow) -> Result<T, McpError>,
    ) -> Result<(T, u64), McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;

        if let Some(expected) = expected_revision {
            if expected != session.revision {
                return Err(McpError::StaleRevision {
                    expected,
                    current: session.revision,
                });
            }
        }

        let snapshot = session.workflow.clone();
        match f(&mut session.workflow) {
            Ok(value) => {
                session.undo.push(snapshot);
                if session.undo.len() > self.undo_depth {
                    let excess = session.undo.len() - self.undo_depth;
                    session.undo.drain(..excess);
                }
                session.redo.clear();
                session.workflow.updated_at = Utc::now();
                session.revision += 1;
                Ok((value, session.revision))
            }
            Err(error) => {
                session.workflow = snapshot;
                Err(error)
            }
        }
    }

    /// Replace a whole workflow (load/import) without adding an undo entry.
    pub fn replace(&self, key: &SessionKey, workflow: Workflow) -> Result<u64, McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        session.undo.push(session.workflow.clone());
        if session.undo.len() > self.undo_depth {
            let excess = session.undo.len() - self.undo_depth;
            session.undo.drain(..excess);
        }
        session.redo.clear();
        session.workflow = workflow;
        session.revision += 1;
        Ok(session.revision)
    }

    pub fn undo(&self, key: &SessionKey) -> Result<u64, McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        let Some(previous) = session.undo.pop() else {
            return Err(McpError::InvalidParam {
                name: "session_id",
                detail: "nothing to undo".into(),
            });
        };
        session.redo.push(session.workflow.clone());
        session.workflow = previous;
        session.revision += 1;
        Ok(session.revision)
    }

    pub fn redo(&self, key: &SessionKey) -> Result<u64, McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        let Some(next) = session.redo.pop() else {
            return Err(McpError::InvalidParam {
                name: "session_id",
                detail: "nothing to redo".into(),
            });
        };
        session.undo.push(session.workflow.clone());
        session.workflow = next;
        session.revision += 1;
        Ok(session.revision)
    }

    pub fn discard(&self, key: &SessionKey) -> Result<(), McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("session store lock poisoned".into()))?;
        sessions
            .remove(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        Ok(())
    }
}

/* ------------------------------------------------------------------ *
 * Project sessions (Milestone 5)
 * ------------------------------------------------------------------ */

/// One editable project and its change history.
///
/// The whole `Project` is cloned for each undo step. Projects are JSON
/// documents and typical apps are small, so this is the right trade for v1 —
/// see `docs/Agentic-App-Generation-Plan.md` §5.1. Swapping in domain-slice
/// snapshots later does not change this API.
#[derive(Debug, Clone)]
pub struct ProjectSession {
    pub project: Project,
    /// Monotonic edit counter for this session. Starts at the revision the
    /// project was loaded at and advances on every mutation, undo and redo.
    ///
    /// This is **not** the row version: it counts edits, and two sessions over
    /// the same row would produce different numbers. Use `base_revision` or
    /// `saved_revision` for anything that compares against the database.
    pub revision: u64,
    /// The row version this session was loaded from. Immutable, and the last
    /// resort for the optimistic-concurrency guard.
    pub base_revision: u64,
    /// The revision last persisted to storage. `project_save` uses this as the
    /// optimistic-concurrency guard, so a session that has been edited locally
    /// still saves against the row it actually read.
    pub saved_revision: u64,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
}

impl ProjectSession {
    fn new(project: Project, revision: u64) -> Self {
        Self {
            project,
            revision,
            base_revision: revision,
            saved_revision: revision,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The revision a save must name: what this session last wrote, or the row
    /// it loaded when it has not written yet.
    pub fn expected_revision(&self) -> u64 {
        self.saved_revision
    }
}

/// Sibling of [`SessionStore`] for whole-project editing.
///
/// Kept separate on purpose: the 23 shipped BPMN tools and their tests talk to
/// `SessionStore`, and a project session is not a workflow session. Both use
/// the same revision/undo semantics and the same [`SessionKey`] owner scoping.
pub struct ProjectSessionStore {
    sessions: RwLock<HashMap<SessionKey, ProjectSession>>,
    undo_depth: usize,
}

impl ProjectSessionStore {
    pub fn new(undo_depth: usize) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            undo_depth,
        }
    }

    /// Create a session over a loaded project and return its id and revision.
    ///
    /// `revision` is the *database* revision the project was loaded at, so a
    /// stale session can be detected on save.
    pub fn create(
        &self,
        owner: &str,
        project: Project,
        revision: u64,
    ) -> Result<(String, u64), McpError> {
        let id = Uuid::new_v4().to_string();
        self.insert_new(owner, &id, project, revision)?;
        Ok((id, revision))
    }

    /// Create a session under a caller-chosen id.
    ///
    /// The chat endpoint names the session after its project lease token, so a
    /// session cannot outlive the right to write it: a new run gets a new
    /// token, and nothing recreates the old session id.
    pub fn create_with_id(
        &self,
        owner: &str,
        id: &str,
        project: Project,
        revision: u64,
    ) -> Result<u64, McpError> {
        if id.trim().is_empty() {
            return Err(McpError::InvalidParam {
                name: "session_id",
                detail: "a session id cannot be empty".into(),
            });
        }
        self.insert_new(owner, id, project, revision)?;
        Ok(revision)
    }

    fn insert_new(
        &self,
        owner: &str,
        id: &str,
        project: Project,
        revision: u64,
    ) -> Result<(), McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        sessions.insert(
            SessionKey::new(owner, id),
            ProjectSession::new(project, revision),
        );
        Ok(())
    }

    pub fn with_read<T>(
        &self,
        key: &SessionKey,
        f: impl FnOnce(&ProjectSession) -> Result<T, McpError>,
    ) -> Result<T, McpError> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        let session = sessions
            .get(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        f(session)
    }

    /// Mutate a project atomically, rolling back on error.
    ///
    /// `expected_revision` guards against an agent and a human editing the same
    /// session; the check happens before the closure runs so a stale caller
    /// cannot half-apply an edit.
    pub fn mutate<T>(
        &self,
        key: &SessionKey,
        expected_revision: Option<u64>,
        f: impl FnOnce(&mut Project) -> Result<T, McpError>,
    ) -> Result<(T, u64), McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;

        if let Some(expected) = expected_revision {
            if expected != session.revision {
                return Err(McpError::StaleRevision {
                    expected,
                    current: session.revision,
                });
            }
        }

        let snapshot = session.project.clone();
        match f(&mut session.project) {
            Ok(value) => {
                session.undo.push(snapshot);
                if session.undo.len() > self.undo_depth {
                    let excess = session.undo.len() - self.undo_depth;
                    session.undo.drain(..excess);
                }
                session.redo.clear();
                session.project.updated_at = Utc::now();
                session.revision += 1;
                Ok((value, session.revision))
            }
            Err(error) => {
                session.project = snapshot;
                Err(error)
            }
        }
    }

    /// A full copy of the project, used to diff before/after a tool call.
    pub fn snapshot(&self, key: &SessionKey) -> Result<Project, McpError> {
        self.with_read(key, |session| Ok(session.project.clone()))
    }

    /// Record that the session was persisted at `db_revision`.
    ///
    /// Called after a successful save so a second save does not trip the
    /// optimistic-concurrency check against the revision it just wrote.
    pub fn mark_saved(&self, key: &SessionKey, db_revision: u64) -> Result<(), McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        session.saved_revision = db_revision;
        Ok(())
    }

    /// The revision the session's contents were last persisted at.
    pub fn saved_revision(&self, key: &SessionKey) -> Result<u64, McpError> {
        self.with_read(key, |session| Ok(session.saved_revision))
    }

    /// Replace the session's project wholesale.
    ///
    /// Used by a rebase: the merged project is the new working copy, and the
    /// pre-merge state stays reachable through undo. The caller saves
    /// afterwards, which is what advances `saved_revision`.
    pub fn replace(&self, key: &SessionKey, project: Project) -> Result<u64, McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        session.undo.push(session.project.clone());
        if session.undo.len() > self.undo_depth {
            let excess = session.undo.len() - self.undo_depth;
            session.undo.drain(..excess);
        }
        session.redo.clear();
        session.project = project;
        session.revision += 1;
        Ok(session.revision)
    }

    pub fn undo(&self, key: &SessionKey) -> Result<u64, McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        let Some(previous) = session.undo.pop() else {
            return Err(McpError::InvalidParam {
                name: "session_id",
                detail: "nothing to undo".into(),
            });
        };
        session.redo.push(session.project.clone());
        session.project = previous;
        session.revision += 1;
        Ok(session.revision)
    }

    pub fn redo(&self, key: &SessionKey) -> Result<u64, McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        let session = sessions
            .get_mut(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        let Some(next) = session.redo.pop() else {
            return Err(McpError::InvalidParam {
                name: "session_id",
                detail: "nothing to redo".into(),
            });
        };
        session.undo.push(session.project.clone());
        session.project = next;
        session.revision += 1;
        Ok(session.revision)
    }

    pub fn discard(&self, key: &SessionKey) -> Result<(), McpError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| McpError::Internal("project session store lock poisoned".into()))?;
        sessions
            .remove(key)
            .ok_or_else(|| McpError::SessionNotFound(key.id.clone()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow(name: &str) -> Workflow {
        let mut workflow = Workflow::new(name);
        workflow.nodes.clear();
        workflow
    }

    #[test]
    fn revisions_and_revision_conflict() {
        let store = SessionStore::new(2);
        let (id, revision) = store.create("local", workflow("A")).unwrap();
        assert_eq!(revision, 0);
        let key = SessionKey::new("local", &id);
        let (_, revision) = store
            .mutate(&key, Some(0), |wf| {
                wf.name = "B".into();
                Ok(())
            })
            .unwrap();
        assert_eq!(revision, 1);
        assert!(matches!(
            store.mutate(&key, Some(0), |_| Ok(())),
            Err(McpError::StaleRevision {
                expected: 0,
                current: 1
            })
        ));
    }

    #[test]
    fn failed_mutation_does_not_change_workflow() {
        let store = SessionStore::new(2);
        let (id, _) = store.create("local", workflow("A")).unwrap();
        let key = SessionKey::new("local", &id);
        let error = store
            .mutate(&key, None, |wf| -> Result<(), McpError> {
                wf.name = "mutated".into();
                Err(McpError::Internal("nope".into()))
            })
            .unwrap_err();
        assert!(error.to_string().contains("nope"));
        store
            .with_read(&key, |session| {
                assert_eq!(session.workflow.name, "A");
                assert_eq!(session.revision, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn undo_redo_and_bounded_history() {
        let store = SessionStore::new(2);
        let (id, _) = store.create("local", workflow("A")).unwrap();
        let key = SessionKey::new("local", &id);
        for name in ["B", "C", "D"] {
            store
                .mutate(&key, None, |wf| {
                    wf.name = name.into();
                    Ok(())
                })
                .unwrap();
        }
        store.undo(&key).unwrap();
        store
            .with_read(&key, |session| {
                assert_eq!(session.workflow.name, "C");
                assert!(session.undo.len() <= 2);
                Ok(())
            })
            .unwrap();
        store.redo(&key).unwrap();
        store
            .with_read(&key, |session| {
                assert_eq!(session.workflow.name, "D");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn sessions_are_isolated_by_owner() {
        let store = SessionStore::new(2);
        let (id, _) = store.create("tenant-a:user-1", workflow("A")).unwrap();
        let other = SessionKey::new("tenant-b:user-2", &id);
        assert!(matches!(
            store.with_read(&other, |_| Ok(())),
            Err(McpError::SessionNotFound(_))
        ));
    }

    #[test]
    fn project_sessions_share_the_revision_contract() {
        let store = ProjectSessionStore::new(3);
        let (id, revision) = store.create("local", Project::new("App"), 4).unwrap();
        assert_eq!(revision, 4, "a session starts at the revision it loaded");
        let key = SessionKey::new("local", &id);

        let (entity_id, revision) = store
            .mutate(&key, Some(4), |project| {
                let entity = nocode_model::Entity::new("Expense");
                let id = entity.id.clone();
                project.entities.insert(id.clone(), entity);
                Ok(id)
            })
            .unwrap();
        assert_eq!(revision, 5);
        assert!(store
            .snapshot(&key)
            .unwrap()
            .entities
            .contains_key(&entity_id));

        assert!(matches!(
            store.mutate(&key, Some(4), |_| Ok(())),
            Err(McpError::StaleRevision { .. })
        ));
    }

    #[test]
    fn a_failed_project_mutation_rolls_back() {
        let store = ProjectSessionStore::new(3);
        let (id, _) = store.create("local", Project::new("App"), 1).unwrap();
        let key = SessionKey::new("local", &id);
        let error = store
            .mutate(&key, None, |project| -> Result<(), McpError> {
                project.name = "Changed".into();
                Err(McpError::InvalidParam {
                    name: "entity_id",
                    detail: "nope".into(),
                })
            })
            .unwrap_err();
        assert!(error.to_string().contains("nope"));
        assert_eq!(store.snapshot(&key).unwrap().name, "App");
    }

    #[test]
    fn project_sessions_are_owner_scoped() {
        let store = ProjectSessionStore::new(2);
        let (id, _) = store
            .create("tenant-a:user-1", Project::new("App"), 1)
            .unwrap();
        let other = SessionKey::new("tenant-b:user-2", &id);
        assert!(matches!(
            store.snapshot(&other),
            Err(McpError::SessionNotFound(_))
        ));
    }
}
