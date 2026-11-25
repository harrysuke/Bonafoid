//! Per-project write leases.
//!
//! Two tabs (or two people) editing one project is the only way the agent can
//! silently lose work: each run loads its own snapshot and whichever saves last
//! wins. The revision guard catches the second save, but only after a model has
//! spent tokens and a human has waited. A lease turns that contention into a
//! `409 project_locked` *before* the stream opens, which reads as a rule rather
//! than a broken product.
//!
//! # Scope
//!
//! A lease is held by a **run**, not by a session. The chat handler names the
//! project session after the lease token, so a session cannot outlive its right
//! to write: once the lease is released (or expires) nothing creates another
//! session with that token, and the next run gets a new one.
//!
//! # Why a trait
//!
//! The in-memory store is correct for the current single-process deployment and
//! is what the tests exercise. A multi-instance deployment needs the same
//! test-and-set in SQL (`INSERT ... ON CONFLICT ... WHERE expires_at < :now`),
//! and that is a storage concern, not an orchestration one. Plan §15.2.2,
//! Phase 33.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use uuid::Uuid;

/// Default lease lifetime.
///
/// Long enough for one slow model turn plus its tool calls, short enough that a
/// crashed process does not wedge a project for a human.
pub const DEFAULT_TTL: Duration = Duration::from_secs(90);

/// How often a live run renews its lease. Must be well under [`DEFAULT_TTL`]
/// so one missed renewal is not fatal.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// A held lease. `token` identifies the holder; nothing else may write.
#[derive(Debug, Clone)]
pub struct ProjectLease {
    pub tenant_id: String,
    pub project_id: String,
    /// Opaque holder token. Also used as the project session id.
    pub token: String,
    /// What is holding it — the run id, so a 409 can name the culprit.
    pub holder: String,
    /// Not serialized: the lease never leaves this process.
    pub expires_at: Instant,
}

impl ProjectLease {
    /// Time left before the lease can be stolen.
    pub fn expires_in(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }
}

/// Why a lease could not be taken, or could not be renewed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    /// Someone else holds it and it has not expired.
    Locked {
        holder: String,
        expires_in: Duration,
    },
    /// The store itself failed (a poisoned lock, or a database error).
    Internal(String),
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeaseError::Locked { holder, expires_in } => write!(
                f,
                "another run ({holder}) is generating this project; it expires in {}s",
                expires_in.as_secs().max(1)
            ),
            LeaseError::Internal(message) => write!(f, "lease store error: {message}"),
        }
    }
}

impl std::error::Error for LeaseError {}

/// Where leases live. Implemented in memory today, in SQL in Phase 33.
#[async_trait]
pub trait LeaseStore: Send + Sync {
    /// Take the lease, or report who holds it.
    async fn try_acquire(
        &self,
        tenant_id: &str,
        project_id: &str,
        holder: &str,
        ttl: Duration,
    ) -> Result<ProjectLease, LeaseError>;

    /// Extend a lease this caller still holds.
    ///
    /// Renewing a lease whose token is still ours is always safe: if nobody
    /// took it while we were late, nobody is about to.
    async fn heartbeat(&self, lease: &ProjectLease, ttl: Duration) -> Result<(), LeaseError>;

    /// Give the lease back. Never fails: releasing is best-effort, and a lease
    /// that is not released expires by itself.
    async fn release(&self, lease: &ProjectLease);
}

/// A lease map guarded by a mutex. Correct for one server process.
#[derive(Default)]
pub struct MemoryLeaseStore {
    leases: Mutex<HashMap<String, ProjectLease>>,
}

impl MemoryLeaseStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(tenant_id: &str, project_id: &str) -> String {
        // The unit separator cannot appear in a tenant or project id, so two
        // different pairs can never collide into one key.
        format!("{tenant_id}\u{1f}{project_id}")
    }
}

#[async_trait]
impl LeaseStore for MemoryLeaseStore {
    async fn try_acquire(
        &self,
        tenant_id: &str,
        project_id: &str,
        holder: &str,
        ttl: Duration,
    ) -> Result<ProjectLease, LeaseError> {
        let now = Instant::now();
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| LeaseError::Internal("lease store lock poisoned".into()))?;

        if let Some(existing) = leases.get(&Self::key(tenant_id, project_id)) {
            if existing.expires_at > now {
                return Err(LeaseError::Locked {
                    holder: existing.holder.clone(),
                    expires_in: existing.expires_in(now),
                });
            }
        }

        let lease = ProjectLease {
            tenant_id: tenant_id.to_string(),
            project_id: project_id.to_string(),
            token: Uuid::new_v4().to_string(),
            holder: holder.to_string(),
            expires_at: now + ttl,
        };
        leases.insert(Self::key(tenant_id, project_id), lease.clone());
        Ok(lease)
    }

    async fn heartbeat(&self, lease: &ProjectLease, ttl: Duration) -> Result<(), LeaseError> {
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| LeaseError::Internal("lease store lock poisoned".into()))?;
        let entry = leases
            .get_mut(&Self::key(&lease.tenant_id, &lease.project_id))
            .ok_or_else(|| LeaseError::Internal("lease disappeared".into()))?;
        if entry.token != lease.token {
            return Err(LeaseError::Internal("lease was taken over".into()));
        }
        entry.expires_at = Instant::now() + ttl;
        Ok(())
    }

    async fn release(&self, lease: &ProjectLease) {
        let Ok(mut leases) = self.leases.lock() else {
            return;
        };
        let key = Self::key(&lease.tenant_id, &lease.project_id);
        // Only remove our own lease: if it expired and someone else took it,
        // dropping ours must not delete theirs.
        if leases.get(&key).map(|entry| entry.token.as_str()) == Some(lease.token.as_str()) {
            leases.remove(&key);
        }
    }
}

/// A held lease that renews itself and releases on drop.
///
/// Holding the guard for the duration of the run is the whole mechanism: a
/// panic, an early return or a cancelled stream all drop it, and `Drop` gives
/// the lease back immediately instead of waiting for the TTL.
pub struct LeaseGuard {
    store: Arc<dyn LeaseStore>,
    lease: ProjectLease,
    heartbeat: Option<tokio::task::JoinHandle<()>>,
}

impl LeaseGuard {
    /// Acquire a lease, starting its heartbeat.
    pub async fn acquire(
        store: Arc<dyn LeaseStore>,
        tenant_id: &str,
        project_id: &str,
        holder: &str,
        ttl: Duration,
    ) -> Result<Self, LeaseError> {
        let lease = store
            .try_acquire(tenant_id, project_id, holder, ttl)
            .await?;
        let heartbeat = spawn_heartbeat(store.clone(), lease.clone(), ttl);
        Ok(Self {
            store,
            lease,
            heartbeat,
        })
    }

    pub fn lease(&self) -> &ProjectLease {
        &self.lease
    }

    /// The session id this run should use: the lease token.
    pub fn token(&self) -> &str {
        &self.lease.token
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.heartbeat.take() {
            handle.abort();
        }
        // `release` is async, and `Drop` cannot await. A detached task is
        // correct here: the lease has a TTL, so the worst case of losing the
        // spawn is that the project stays locked until it expires.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let store = self.store.clone();
            let lease = self.lease.clone();
            runtime.spawn(async move { store.release(&lease).await });
        }
    }
}

fn spawn_heartbeat(
    store: Arc<dyn LeaseStore>,
    lease: ProjectLease,
    ttl: Duration,
) -> Option<tokio::task::JoinHandle<()>> {
    let runtime = tokio::runtime::Handle::try_current().ok()?;
    // A test may pass a very short TTL; `interval` panics on a zero period.
    let period = HEARTBEAT_INTERVAL
        .min(ttl / 3)
        .max(Duration::from_millis(5));
    Some(runtime.spawn(async move {
        let mut ticker = tokio::time::interval(period);
        // The first tick of an interval completes immediately; skip it so the
        // lease is not renewed before any time has passed.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if store.heartbeat(&lease, ttl).await.is_err() {
                // Taken over or expired: stop renewing. The run's own save
                // will fail on the revision guard if it still tries to write.
                return;
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Arc<MemoryLeaseStore> {
        Arc::new(MemoryLeaseStore::new())
    }

    #[tokio::test]
    async fn a_second_acquire_is_refused_while_the_first_is_live() {
        let store = store();
        let first = store
            .try_acquire("t1", "p1", "run-a", Duration::from_secs(30))
            .await
            .unwrap();
        let second = store
            .try_acquire("t1", "p1", "run-b", Duration::from_secs(30))
            .await;
        assert!(matches!(
            second,
            Err(LeaseError::Locked { holder, .. }) if holder == "run-a"
        ));
        assert_eq!(first.project_id, "p1");
    }

    #[tokio::test]
    async fn a_lease_is_scoped_by_tenant() {
        let store = store();
        store
            .try_acquire("t1", "p1", "run-a", Duration::from_secs(30))
            .await
            .unwrap();
        // Same project id, different tenant: a different key.
        assert!(store
            .try_acquire("t2", "p1", "run-b", Duration::from_secs(30))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn an_expired_lease_can_be_taken() {
        let store = store();
        store
            .try_acquire("t1", "p1", "run-a", Duration::from_millis(20))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(store
            .try_acquire("t1", "p1", "run-b", Duration::from_secs(30))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn heartbeat_keeps_a_lease_alive() {
        let store = store();
        let lease = store
            .try_acquire("t1", "p1", "run-a", Duration::from_millis(40))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;
        store
            .heartbeat(&lease, Duration::from_millis(40))
            .await
            .unwrap();
        // Past the original deadline, still ours.
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(matches!(
            store
                .try_acquire("t1", "p1", "run-b", Duration::from_secs(30))
                .await,
            Err(LeaseError::Locked { .. })
        ));
    }

    #[tokio::test]
    async fn dropping_the_guard_releases_immediately() {
        let store = store();
        {
            let _guard =
                LeaseGuard::acquire(store.clone(), "t1", "p1", "run-a", Duration::from_secs(30))
                    .await
                    .unwrap();
            assert!(store
                .try_acquire("t1", "p1", "run-b", Duration::from_secs(30))
                .await
                .is_err());
        }
        // The release is spawned; give it a turn to run.
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(store
            .try_acquire("t1", "p1", "run-b", Duration::from_secs(30))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn a_stale_holder_cannot_release_someone_elses_lease() {
        let store = store();
        let stale = store
            .try_acquire("t1", "p1", "run-a", Duration::from_millis(10))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;
        store
            .try_acquire("t1", "p1", "run-b", Duration::from_secs(30))
            .await
            .unwrap();
        store.release(&stale).await;
        assert!(matches!(
            store
                .try_acquire("t1", "p1", "run-c", Duration::from_secs(30))
                .await,
            Err(LeaseError::Locked { holder, .. }) if holder == "run-b"
        ));
    }

    #[tokio::test]
    async fn the_guard_renews_itself() {
        let store = store();
        // TTL 60 ms with a 20 ms heartbeat: the lease must outlive the TTL.
        let _guard = LeaseGuard::acquire(
            store.clone(),
            "t1",
            "p1",
            "run-a",
            Duration::from_millis(60),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(store
            .try_acquire("t1", "p1", "run-b", Duration::from_secs(30))
            .await
            .is_err());
    }
}
