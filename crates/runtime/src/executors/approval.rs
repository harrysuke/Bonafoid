//! The human gate.
//!
//! An `approval` node is the only control-flow node that has to *stop*, and the
//! only one whose answer may arrive after the process that asked has exited.
//! Both facts come from the same rule (§7.1): a run that suspends returns to
//! its caller instead of blocking, so the answer is picked up by the next
//! attempt — a resume one second later or one day later.
//!
//! Nothing here waits. [`ApprovalGate::poll`] is asked once per attempt; if it
//! has no answer the executor returns [`NodeOutcome::Waiting`] and the
//! scheduler journals that and stops. The *next* attempt asks again.

use async_trait::async_trait;
use serde_json::{json, Value};

use nocode_model::{AgentNodeConfig, AgentNodeKind, OnTimeout, WorkflowNode};

use crate::capability::Capability;
use crate::context::{ApprovalDecision, RunContext};
use crate::error::{NodeError, NodeErrorCode};
use crate::executor::{agent_config, NodeContext, NodeExecutor, NodeOutcome, WaitState};
use crate::time::parse_duration;

/// Answers an approval card.
#[derive(Debug, Default, Clone, Copy)]
pub struct ApprovalExecutor;

impl ApprovalExecutor {
    pub fn new() -> Self {
        Self
    }

    fn config<'a>(&self, node: &'a WorkflowNode) -> Result<&'a nocode_model::ApprovalNode, NodeError> {
        match agent_config(node)? {
            AgentNodeConfig::Approval(approval) => Ok(approval),
            other => Err(NodeError::new(
                NodeErrorCode::InvalidConfig,
                format!(
                    "`{}` is a `{}` node; the approval executor cannot run it",
                    node.name,
                    other.kind().as_str()
                ),
            )),
        }
    }
}

#[async_trait]
impl NodeExecutor for ApprovalExecutor {
    fn kind(&self) -> AgentNodeKind {
        AgentNodeKind::Approval
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "The card's headline.",
                    "default": "Approval required"
                },
                "body": {
                    "type": "string",
                    "description": "Detail shown under the title. Interpolated, so it can \
                                    summarise what the next node is about to do."
                },
                "timeout": {
                    "type": "string",
                    "description": "How long to wait, e.g. `1h`. Omit to wait indefinitely.",
                    "examples": ["30m", "1h", "7d"]
                },
                "on_timeout": {
                    "type": "string",
                    "enum": ["fail", "skip"],
                    "default": "fail",
                    "description": "`fail` stops the run; `skip` continues without an answer. \
                                    Skipping a gate is an author's deliberate choice, not a default."
                }
            }
        })
    }

    /// An approval touches nothing: no file, no host, no model. That is exactly
    /// why it is safe to put in front of the nodes that do.
    fn capabilities(&self, _node: &WorkflowNode) -> Vec<Capability> {
        Vec::new()
    }

    fn dry_run(&self, node: &WorkflowNode, _cx: &RunContext) -> Result<(), NodeError> {
        let approval = self.config(node)?;
        if let Some(timeout) = &approval.timeout {
            // Checked here rather than at execution time so the canvas can say
            // `30 minutes` is not a duration before a run is started.
            parse_duration(timeout).ok_or_else(|| {
                NodeError::invalid_config(format!(
                    "`{timeout}` is not a duration; try `30m`, `1h`, or `PT30M`"
                ))
            })?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        node: &WorkflowNode,
        cx: &NodeContext<'_>,
    ) -> Result<NodeOutcome, NodeError> {
        let approval = self.config(node)?;
        // An unset title is indistinguishable from the default on the wire, so
        // the node's own name wins when the author left it alone. A card that
        // says "Ship it" is worth more than one that says "Approval required",
        // and the canvas already forces every node to have a name.
        let raw_title = if approval.title == nocode_model::DEFAULT_APPROVAL_TITLE {
            node.name.clone()
        } else {
            approval.title.clone()
        };
        let title = cx.interpolate(&raw_title)?;
        let body = match &approval.body {
            Some(body) => Some(cx.interpolate(body)?),
            None => None,
        };

        // One question per attempt. A host that answers synchronously (a test,
        // `--yes`) never suspends at all, which keeps the happy path cheap.
        match cx.run.approvals.poll(&cx.run.run_id, &cx.node_id) {
            Some(ApprovalDecision::Approve { comment }) => {
                return Ok(NodeOutcome::done(json!({
                    "approved": true,
                    "comment": comment,
                })));
            }
            // A rejection is a *node error*, not an output, because a run must
            // not walk past a human saying no. It is journalled with code
            // `rejected`, so a host can still tell it apart from a crash, and
            // it is not retryable: asking the same person again in a loop is
            // not a retry strategy.
            Some(ApprovalDecision::Reject { comment }) => {
                let mut error = NodeError::new(
                    NodeErrorCode::Rejected,
                    format!("`{title}` was rejected"),
                );
                if let Some(comment) = comment {
                    error = error.with_detail(comment);
                }
                return Err(error);
            }
            None => {}
        }

        // `waiting_since` is set when the previous attempt ended in `waiting`,
        // so the deadline is measured from the first presentation of the card
        // rather than from this attempt — otherwise a resume would restart the
        // clock and a timeout could never fire.
        let since = cx.waiting_since.unwrap_or_else(|| cx.clock.now());

        if let Some(timeout) = &approval.timeout {
            let limit = parse_duration(timeout).ok_or_else(|| {
                NodeError::invalid_config(format!("`{timeout}` is not a duration"))
            })?;
            let waited = cx
                .clock
                .now()
                .signed_duration_since(since)
                .to_std()
                .unwrap_or_default();
            if waited >= limit {
                return match approval.on_timeout {
                    OnTimeout::Fail => Err(NodeError::timeout(format!(
                        "`{title}` got no answer within {timeout}"
                    ))),
                    OnTimeout::Skip => Ok(NodeOutcome::done(json!({
                        "approved": false,
                        "timed_out": true,
                        "skipped": true,
                    }))),
                };
            }
        }

        Ok(NodeOutcome::Waiting(WaitState::approval(title, body, since)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use nocode_model::{NodeConfig, NodeId, NodePosition, WorkflowNodeType};

    use crate::context::{AlwaysApproves, MapSecrets, NoApprovals, RunContext};
    use crate::interpolate::{Redactor, Scope};
    use crate::time::{Clock, ManualClock};

    struct Rejects;

    impl crate::context::ApprovalGate for Rejects {
        fn poll(&self, _run_id: &str, _node_id: &str) -> Option<ApprovalDecision> {
            Some(ApprovalDecision::Reject {
                comment: Some("not on a Friday".into()),
            })
        }
    }

    fn approval_node(config: nocode_model::ApprovalNode) -> WorkflowNode {
        WorkflowNode {
            id: NodeId::new(),
            name: "Ship it".into(),
            node_type: WorkflowNodeType::AgentTask,
            position: NodePosition::default(),
            config: NodeConfig {
                agent: Some(AgentNodeConfig::Approval(config)),
                ..NodeConfig::default()
            },
            documentation: None,
        }
    }

    fn scope(run: &RunContext) -> Scope {
        Scope::new(
            run.run_id.clone(),
            run.workspace.clone(),
            Default::default(),
            Arc::new(MapSecrets::default()),
        )
    }

    fn context<'a>(
        run: &'a RunContext,
        scope: &'a Scope,
        clock: &'a dyn Clock,
        waiting_since: Option<chrono::DateTime<Utc>>,
        redactor: &'a Redactor,
    ) -> NodeContext<'a> {
        NodeContext {
            run,
            scope,
            clock,
            node_id: "approval-1".into(),
            node_name: "Ship it".into(),
            waiting_since,
            redactor,
        }
    }

    #[tokio::test]
    async fn an_approved_card_finishes_the_node() {
        let node = approval_node(nocode_model::ApprovalNode {
            title: "Ship `${input.version}`?".into(),
            ..Default::default()
        });
        let mut input = indexmap::IndexMap::new();
        input.insert("version".to_string(), json!("2.0"));
        let run = RunContext::new("r1", ".").with_approvals(Arc::new(AlwaysApproves));
        let scope = Scope::new("r1", ".", input, Arc::new(MapSecrets::default()));
        let clock = ManualClock::at(Utc::now());
        let redactor = Redactor::new();
        let cx = context(&run, &scope, &clock, None, &redactor);

        let outcome = ApprovalExecutor::new().execute(&node, &cx).await.unwrap();
        match outcome {
            NodeOutcome::Done(output) => assert_eq!(output.value["approved"], json!(true)),
            NodeOutcome::Waiting(_) => panic!("the gate said yes"),
        }
    }

    #[tokio::test]
    async fn an_unanswered_card_suspends_and_keeps_its_original_start_time() {
        let node = approval_node(nocode_model::ApprovalNode::default());
        let run = RunContext::new("r1", ".");
        let scope = scope(&run);
        let clock = ManualClock::at(Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap());
        let redactor = Redactor::new();
        let since = Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap();
        let cx = context(&run, &scope, &clock, Some(since), &redactor);

        let outcome = ApprovalExecutor::new().execute(&node, &cx).await.unwrap();
        match outcome {
            NodeOutcome::Waiting(wait) => {
                assert_eq!(wait.kind, "approval");
                assert_eq!(
                    wait.since, since,
                    "the clock must not restart on a resume, or a timeout never fires"
                );
            }
            NodeOutcome::Done(_) => panic!("nobody answered"),
        }
    }

    #[tokio::test]
    async fn a_rejection_is_a_non_retryable_error_with_the_comment_kept() {
        let node = approval_node(nocode_model::ApprovalNode::default());
        let run = RunContext::new("r1", ".").with_approvals(Arc::new(Rejects));
        let scope = scope(&run);
        let clock = ManualClock::at(Utc::now());
        let redactor = Redactor::new();
        let cx = context(&run, &scope, &clock, None, &redactor);

        let error = ApprovalExecutor::new()
            .execute(&node, &cx)
            .await
            .unwrap_err();
        assert_eq!(error.code, NodeErrorCode::Rejected);
        assert!(!error.retryable, "asking again is not a retry strategy");
        assert_eq!(error.detail.as_deref(), Some("not on a Friday"));
    }

    #[tokio::test]
    async fn a_deadline_that_has_passed_fails_the_node() {
        let node = approval_node(nocode_model::ApprovalNode {
            timeout: Some("1h".into()),
            on_timeout: OnTimeout::Fail,
            ..Default::default()
        });
        let run = RunContext::new("r1", ".");
        let scope = scope(&run);
        let clock = ManualClock::at(Utc.with_ymd_and_hms(2026, 1, 1, 10, 30, 0).unwrap());
        let redactor = Redactor::new();
        let since = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();
        let cx = context(&run, &scope, &clock, Some(since), &redactor);

        let error = ApprovalExecutor::new()
            .execute(&node, &cx)
            .await
            .unwrap_err();
        assert_eq!(error.code, NodeErrorCode::Timeout);
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn a_deadline_that_has_passed_can_skip_instead() {
        let node = approval_node(nocode_model::ApprovalNode {
            timeout: Some("1h".into()),
            on_timeout: OnTimeout::Skip,
            ..Default::default()
        });
        let run = RunContext::new("r1", ".");
        let scope = scope(&run);
        let clock = ManualClock::at(Utc.with_ymd_and_hms(2026, 1, 1, 10, 30, 0).unwrap());
        let redactor = Redactor::new();
        let since = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();
        let cx = context(&run, &scope, &clock, Some(since), &redactor);

        let outcome = ApprovalExecutor::new().execute(&node, &cx).await.unwrap();
        match outcome {
            NodeOutcome::Done(output) => {
                assert_eq!(output.value["timed_out"], json!(true));
                assert_eq!(output.value["approved"], json!(false));
            }
            NodeOutcome::Waiting(_) => panic!("the deadline has passed"),
        }
    }

    #[tokio::test]
    async fn a_deadline_in_the_future_still_waits() {
        let node = approval_node(nocode_model::ApprovalNode {
            timeout: Some("1h".into()),
            ..Default::default()
        });
        let run = RunContext::new("r1", ".");
        let scope = scope(&run);
        let clock = ManualClock::at(Utc.with_ymd_and_hms(2026, 1, 1, 9, 30, 0).unwrap());
        let redactor = Redactor::new();
        let since = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();
        let cx = context(&run, &scope, &clock, Some(since), &redactor);

        assert!(matches!(
            ApprovalExecutor::new().execute(&node, &cx).await.unwrap(),
            NodeOutcome::Waiting(_)
        ));
    }

    #[test]
    fn a_bad_timeout_is_caught_before_the_run_starts() {
        let node = approval_node(nocode_model::ApprovalNode {
            timeout: Some("30 minutes".into()),
            ..Default::default()
        });
        let run = RunContext::new("r1", ".");
        let error = ApprovalExecutor::new().dry_run(&node, &run).unwrap_err();
        assert_eq!(error.code, NodeErrorCode::InvalidConfig);
        assert!(error.message.contains("30 minutes"));
    }

    #[tokio::test]
    async fn the_default_gate_suspends_rather_than_guessing() {
        // `NoApprovals` is the headless default; a node must not read "nobody
        // is listening" as consent.
        let node = approval_node(nocode_model::ApprovalNode::default());
        let run = RunContext::new("r1", ".").with_approvals(Arc::new(NoApprovals));
        let scope = scope(&run);
        let clock = ManualClock::at(Utc::now());
        let redactor = Redactor::new();
        let cx = context(&run, &scope, &clock, None, &redactor);
        let outcome = ApprovalExecutor::new().execute(&node, &cx).await.unwrap();
        assert!(matches!(outcome, NodeOutcome::Waiting(_)));
    }
}
