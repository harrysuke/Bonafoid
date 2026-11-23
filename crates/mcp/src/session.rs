//! Stateful workflow sessions with revisions, bounded undo/redo and
//! per-caller isolation.
//!
//! Sessions live in memory only. `save_workflow` is the explicit durability
//! boundary. HTTP callers are scoped by tenant and user in [`SessionKey`], so
//! one user cannot read or mutate another user's session by guessing a UUID.

use chrono::Utc;
use nocode_model::Workflow;
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
            .mutate(&key, None, |wf| {
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
}
