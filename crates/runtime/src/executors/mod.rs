//! The control-only executors, and the wiring that makes them work.
//!
//! Of the nine node kinds, two do not touch the outside world: `approval` asks
//! a human, `subflow` asks another workflow. They are here rather than in a
//! host because a graph containing them must mean the same thing everywhere —
//! a headless `bonafoid run`, the IDE, and a compiled bundle.
//!
//! The other seven need a filesystem, a shell, a database, or a model, and
//! arrive in Phase 36.

mod approval;
mod subflow;

pub use approval::ApprovalExecutor;
pub use subflow::SubflowExecutor;

use std::sync::{Arc, RwLock, Weak};

use crate::executor::ExecutorRegistry;
use crate::limits::RunLimits;
use crate::time::Clock;

impl ExecutorRegistry {
    /// Add `approval` and `subflow`, and return the registry ready to use.
    ///
    /// The `Arc` is not a convenience: a `subflow` node has to run its child on
    /// a runner built from *this* registry, so the registry must be able to
    /// hand out a handle to itself. That is done through a `Weak` filled in
    /// after the `Arc` exists, which is why this is a constructor rather than a
    /// `register` call the host could get half-right.
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use nocode_runtime::{ExecutorRegistry, RunLimits, SystemClock};
    ///
    /// let registry = ExecutorRegistry::new()
    ///     .with_control_executors(RunLimits::default(), Arc::new(SystemClock));
    /// assert!(registry.contains(nocode_model::AgentNodeKind::Subflow));
    /// ```
    pub fn with_control_executors(self, limits: RunLimits, clock: Arc<dyn Clock>) -> Arc<Self> {
        let mut registry = self;
        let handle: Arc<RwLock<Weak<ExecutorRegistry>>> = Arc::new(RwLock::new(Weak::new()));

        registry.register(Arc::new(ApprovalExecutor::new()));
        registry.register(Arc::new(SubflowExecutor::new(
            handle.clone(),
            limits,
            clock,
        )));

        let registry = Arc::new(registry);
        if let Ok(mut slot) = handle.write() {
            *slot = Arc::downgrade(&registry);
        }
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use nocode_model::AgentNodeKind;

    #[test]
    fn both_control_executors_are_registered() {
        let registry =
            ExecutorRegistry::new().with_control_executors(RunLimits::default(), Arc::new(crate::time::SystemClock));
        assert_eq!(
            registry.kinds(),
            vec![AgentNodeKind::Approval, AgentNodeKind::Subflow],
            "and in palette order"
        );
    }

    #[test]
    fn the_subflow_executor_can_see_its_own_registry() {
        // Without this, a subflow could not run a child, and a nested subflow
        // would be impossible rather than merely discouraged.
        let registry =
            ExecutorRegistry::new().with_control_executors(RunLimits::default(), Arc::new(crate::time::SystemClock));
        let executor = registry.get(AgentNodeKind::Subflow).expect("registered");
        assert!(
            executor
                .config_schema()
                .get("properties")
                .and_then(|properties| properties.get("workflow_id"))
                .is_some(),
            "the inspector needs a schema"
        );
        assert_eq!(
            Arc::strong_count(&registry),
            1,
            "the handle must be weak, or the registry can never be dropped"
        );
    }

    #[test]
    fn a_host_registration_survives_the_control_executors() {
        let registry = ExecutorRegistry::new()
            .with_control_executors(RunLimits::default(), Arc::new(crate::time::SystemClock));
        assert!(registry.contains(AgentNodeKind::Approval));
        assert!(!registry.contains(AgentNodeKind::Llm), "Phase 36 adds these");
    }
}
