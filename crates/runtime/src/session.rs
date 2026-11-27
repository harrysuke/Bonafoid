//! Running a workflow as a host, in one call.
//!
//! [`GraphRunner`](crate::GraphRunner) is deliberately fussy: it wants a
//! registry, a limits value, a clock, a context, a journal, and a workflow, and
//! it will not decide for you where a journal lives or whether a run should be
//! started or resumed. A host that gets any of that wrong does not get a
//! compile error — it gets a run that silently starts over.
//!
//! This module is the host-shaped layer: a [`RunPaths`] convention, a
//! [`RunSession`] that starts or resumes, and a [`RunReport`] that a UI can
//! render without knowing anything about journals.
//!
//! ```no_run
//! use std::sync::Arc;
//! use nocode_runtime::{
//!     ExecutorRegistry, RunContext, RunLimits, RunPaths, RunSession, SystemClock,
//! };
//!
//! # async fn go(workflow: &nocode_model::Workflow) -> Result<(), Box<dyn std::error::Error>> {
//! let session = RunSession::new(
//!     ExecutorRegistry::new()
//!         .with_control_executors(RunLimits::default(), Arc::new(SystemClock)),
//!     RunLimits::default(),
//!     Arc::new(SystemClock),
//!     RunPaths::new(".bonafoid/runs"),
//! );
//! let cx = RunContext::new("r1", ".");
//! let report = session.start(workflow, "r1", Default::default(), &cx).await?;
//! if report.is_suspended() {
//!     // Show `report.wait` as a card; call `session.resume(..)` when answered.
//! }
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use nocode_model::Workflow;

use crate::context::{RunContext, RunInput};
use crate::error::{JournalError, RunError};
use crate::executor::{ExecutorRegistry, Usage, WaitState};
use crate::journal::{Journal, JournalEvent, JsonlJournal, NodeStatus, RunJournal};
use crate::limits::RunLimits;
use crate::runner::{GraphRunner, RunOutcome};
use crate::time::Clock;

/// Where run journals live.
///
/// One file per run, named for the run id. The id comes from the host, not from
/// here, because a host already has an identity for the run it is about to
/// start (a request id, a timestamp, a user's click) and a second one would
/// only need reconciling.
#[derive(Debug, Clone)]
pub struct RunPaths {
    root: PathBuf,
}

impl RunPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The journal for `run_id`.
    ///
    /// The id is used as a file name, so it is filtered to `[A-Za-z0-9._-]`:
    /// a run id from a URL must not be able to walk out of the directory.
    pub fn journal(&self, run_id: &str) -> PathBuf {
        self.root.join(format!("{}.jsonl", sanitise(run_id)))
    }

    /// Every journal in the directory, newest first.
    ///
    /// Unreadable files are skipped rather than failing the listing: one
    /// corrupt journal must not hide the rest of a user's runs.
    pub fn list(&self) -> Vec<RunSummary> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut summaries: Vec<RunSummary> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
            .filter_map(|entry| RunSummary::read(&entry.path()).ok())
            .collect();
        summaries.sort_by(|left, right| right.modified.cmp(&left.modified));
        summaries
    }
}

/// One journalled run, as the run list shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub workflow_name: String,
    pub phase: RunPhase,
    /// Records written, which is a decent proxy for progress.
    pub records: usize,
    pub journal: PathBuf,
    /// The file's modification time, for ordering.
    #[serde(skip)]
    pub modified: Option<std::time::SystemTime>,
}

impl RunSummary {
    fn read(path: &Path) -> Result<Self, JournalError> {
        let journal = JsonlJournal::load(path)?;
        Ok(Self {
            run_id: journal.run_id.clone(),
            workflow_name: journal.workflow_name.clone(),
            phase: if journal.is_finished() {
                RunPhase::Finished
            } else {
                RunPhase::Resumable
            },
            records: journal.records.len(),
            journal: path.to_path_buf(),
            modified: std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok(),
        })
    }
}

/// Whether a journalled run can still be continued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    /// No `run_finished`: the process stopped, whether it meant to or not.
    Resumable,
    Finished,
}

/// How one attempt at a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcomeKind {
    Completed,
    /// Parked on a human gate. Not a failure, and not an error value.
    Suspended,
    Failed,
    Cancelled,
}

/// Everything a UI needs to show after a run attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub kind: RunOutcomeKind,
    /// The card to present, when `kind` is `Suspended`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitState>,
    pub output: Value,
    pub usage: Usage,
    pub steps: usize,
    pub journal: PathBuf,
    /// Why the run stopped, when it did not complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunReport {
    pub fn is_suspended(&self) -> bool {
        self.kind == RunOutcomeKind::Suspended
    }

    pub fn is_complete(&self) -> bool {
        self.kind == RunOutcomeKind::Completed
    }

    fn suspended(outcome: RunOutcome, journal: PathBuf) -> Self {
        Self {
            run_id: outcome.run_id,
            kind: RunOutcomeKind::Suspended,
            wait: outcome.suspended,
            output: outcome.output,
            usage: outcome.usage,
            steps: outcome.steps,
            journal,
            error: None,
        }
    }

    fn completed(outcome: RunOutcome, journal: PathBuf) -> Self {
        Self {
            run_id: outcome.run_id,
            kind: RunOutcomeKind::Completed,
            wait: None,
            output: outcome.output,
            usage: outcome.usage,
            steps: outcome.steps,
            journal,
            error: None,
        }
    }

    /// A failure, described in the terms the user needs: which node, what code.
    ///
    /// The run id is not in the error value, so it is passed in — a report
    /// without one cannot be resumed, and the UI needs it to link back.
    ///
    /// Progress is read back from the journal rather than left at zero: a run
    /// that failed on its fifth node did five steps, and a report that says
    /// otherwise is worse than no number at all.
    fn failed(run_id: &str, error: &RunError, journal: PathBuf) -> Self {
        let (steps, usage) = progress(&journal);
        Self {
            run_id: run_id.to_string(),
            kind: match error {
                RunError::Cancelled => RunOutcomeKind::Cancelled,
                _ => RunOutcomeKind::Failed,
            },
            wait: None,
            output: Value::Null,
            usage,
            steps,
            journal,
            error: Some(describe(error)),
        }
    }
}

/// What the journal says was accomplished, for a report about a run that stopped.
///
/// Best-effort: a failure to read the journal must not turn a run failure into a
/// reporting panic, so an unreadable file reports nothing rather than erroring.
fn progress(path: &Path) -> (usize, Usage) {
    let Ok(journal) = JsonlJournal::load(path) else {
        return (0, Usage::default());
    };
    let mut steps = 0;
    let mut usage = Usage::default();
    for record in &journal.records {
        if let JournalEvent::NodeOutcome {
            status,
            usage: used,
            ..
        } = &record.event
        {
            if *status == NodeStatus::Ok {
                steps += 1;
                if let Some(used) = used {
                    usage.tokens_in += used.tokens_in;
                    usage.tokens_out += used.tokens_out;
                    usage.cost_usd += used.cost_usd;
                    usage.tool_calls += used.tool_calls;
                }
            }
        }
    }
    (steps, usage)
}

/// One sentence a user can act on.
fn describe(error: &RunError) -> String {
    match error {
        RunError::Node { node_name, error, .. } => {
            format!("`{node_name}` failed: {}", error.message)
        }
        RunError::NeedsAttention { node_name, .. } => format!(
            "`{node_name}` started but did not finish, and it is not idempotent, so it will \
             not be run again automatically"
        ),
        other => other.to_string(),
    }
}

/// Runs workflows and keeps their journals.
///
/// Holds the registry, limits, and clock rather than a runner, because a runner
/// is bound to one journal and this type outlives any single run.
pub struct RunSession {
    registry: Arc<ExecutorRegistry>,
    limits: RunLimits,
    clock: Arc<dyn Clock>,
    paths: RunPaths,
}

impl RunSession {
    pub fn new(
        registry: Arc<ExecutorRegistry>,
        limits: RunLimits,
        clock: Arc<dyn Clock>,
        paths: RunPaths,
    ) -> Self {
        Self {
            registry,
            limits,
            clock,
            paths,
        }
    }

    pub fn paths(&self) -> &RunPaths {
        &self.paths
    }

    pub fn limits(&self) -> &RunLimits {
        &self.limits
    }

    fn runner(&self, journal: Arc<dyn Journal>) -> GraphRunner {
        GraphRunner::new(
            self.registry.clone(),
            journal,
            self.limits.clone(),
            self.clock.clone(),
        )
    }

    /// Start a run and write a fresh journal.
    ///
    /// Refuses an id that already has a journal. Silently overwriting one would
    /// destroy the only record of a run that might have had side effects, and
    /// reusing an id is far more likely to be a caller's bug than an intent.
    pub async fn start(
        &self,
        workflow: &Workflow,
        run_id: &str,
        input: RunInput,
        cx: &RunContext,
    ) -> Result<RunReport, RunError> {
        let path = self.paths.journal(run_id);
        if path.exists() {
            return Err(RunError::Journal(JournalError::AlreadyStarted(
                run_id.to_string(),
            )));
        }
        let journal = JsonlJournal::create(&path)?;
        let outcome = self
            .runner(Arc::new(journal))
            .run(workflow, input, cx)
            .await;

        Ok(match outcome {
            Ok(outcome) if outcome.is_suspended() => RunReport::suspended(outcome, path),
            Ok(outcome) => RunReport::completed(outcome, path),
            Err(error) => RunReport::failed(run_id, &error, path),
        })
    }

    /// Continue a run from its journal.
    ///
    /// A run that is already finished is refused by
    /// [`GraphRunner::resume`](crate::GraphRunner::resume); a missing journal is
    /// refused here, because "resume" of something that never started is a
    /// caller's bug, not an empty run.
    pub async fn resume(
        &self,
        workflow: &Workflow,
        run_id: &str,
        cx: &RunContext,
    ) -> Result<RunReport, RunError> {
        let path = self.paths.journal(run_id);
        if !path.exists() {
            return Err(RunError::Journal(JournalError::NotStarted));
        }
        let loaded: RunJournal = JsonlJournal::load(&path)?;
        let journal = JsonlJournal::open(&path)?;
        let outcome = self
            .runner(Arc::new(journal))
            .resume(loaded, workflow, cx)
            .await;

        Ok(match outcome {
            Ok(outcome) if outcome.is_suspended() => RunReport::suspended(outcome, path),
            Ok(outcome) => RunReport::completed(outcome, path),
            Err(error) => RunReport::failed(run_id, &error, path),
        })
    }

    /// Start if there is nothing to resume, resume if there is.
    ///
    /// This is what a "Run" button should call: a user clicking it again after
    /// a crash wants the run continued, and a user clicking it on a fresh id
    /// wants a new run. Making the caller choose is how a UI ends up with a
    /// Resume button nobody presses.
    pub async fn start_or_resume(
        &self,
        workflow: &Workflow,
        run_id: &str,
        input: RunInput,
        cx: &RunContext,
    ) -> Result<RunReport, RunError> {
        if self.paths.journal(run_id).exists() {
            self.resume(workflow, run_id, cx).await
        } else {
            self.start(workflow, run_id, input, cx).await
        }
    }

    /// The last journal record's tag, for a host that wants to show progress
    /// without loading the whole file.
    pub fn last_event(&self, run_id: &str) -> Option<&'static str> {
        let path = self.paths.journal(run_id);
        let journal = JsonlJournal::load(&path).ok()?;
        journal.records.last().map(|record| record.event.tag())
    }

    /// Whether `run_id` has a `run_finished` record.
    pub fn is_finished(&self, run_id: &str) -> bool {
        JsonlJournal::load(self.paths.journal(run_id))
            .map(|journal| journal.is_finished())
            .unwrap_or(false)
    }

    /// Remove a finished run's journal.
    ///
    /// Only finished ones: deleting a resumable journal throws away the record
    /// of a run that may have had side effects, which is the one thing the
    /// journal exists to prevent.
    pub fn discard(&self, run_id: &str) -> Result<(), JournalError> {
        let path = self.paths.journal(run_id);
        let journal = JsonlJournal::load(&path)?;
        if !journal.is_finished() {
            return Err(JournalError::StillResumable(run_id.to_string()));
        }
        std::fs::remove_file(&path)?;
        Ok(())
    }
}

/// Keep a run id usable as a file name.
///
/// Not a security boundary on its own — the caller's directory choice is — but
/// a run id arriving from a URL should not be able to contain `..` or a path
/// separator, and this is the cheapest place to make that true.
fn sanitise(run_id: &str) -> String {
    let cleaned: String = run_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' || character == '.' {
                character
            } else {
                '-'
            }
        })
        .collect();
    // `..` and `.` are not names; a run id of `..` would otherwise escape.
    if cleaned.is_empty() || cleaned.chars().all(|character| character == '.') {
        "run".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    // `JournalEvent` arrives through `super::*`, which sees the parent module's
    // imports.
    use super::*;

    #[test]
    fn a_run_id_cannot_walk_out_of_the_directory() {
        let paths = RunPaths::new("/runs");
        assert_eq!(paths.journal("../../etc/passwd"), Path::new("/runs/..-..-etc-passwd.jsonl"));
        assert_eq!(paths.journal(".."), Path::new("/runs/run.jsonl"));
        assert_eq!(paths.journal(""), Path::new("/runs/run.jsonl"));
        assert_eq!(
            paths.journal("run-2026.01.01_1"),
            Path::new("/runs/run-2026.01.01_1.jsonl")
        );
    }

    #[test]
    fn an_empty_directory_lists_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunPaths::new(dir.path());
        assert!(paths.list().is_empty());
    }

    #[test]
    fn a_missing_directory_lists_nothing_rather_than_failing() {
        let paths = RunPaths::new("/definitely/not/here");
        assert!(paths.list().is_empty());
    }

    #[test]
    fn a_suspended_journal_lists_as_resumable() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunPaths::new(dir.path());
        let journal = JsonlJournal::create(paths.journal("r1")).unwrap();
        journal
            .append(JournalEvent::RunStarted {
                run_id: "r1".into(),
                workflow_id: "w1".into(),
                workflow_name: "Diamond".into(),
                graph_hash: "abc".into(),
                input: Value::Null,
            })
            .unwrap();
        drop(journal);

        let listed = paths.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].run_id, "r1");
        assert_eq!(listed[0].workflow_name, "Diamond");
        assert_eq!(listed[0].phase, RunPhase::Resumable);
        assert_eq!(listed[0].records, 1);
    }

    #[test]
    fn a_finished_journal_lists_as_finished() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunPaths::new(dir.path());
        let journal = JsonlJournal::create(paths.journal("r1")).unwrap();
        journal
            .append(JournalEvent::RunStarted {
                run_id: "r1".into(),
                workflow_id: "w1".into(),
                workflow_name: "Line".into(),
                graph_hash: "abc".into(),
                input: Value::Null,
            })
            .unwrap();
        journal
            .append(JournalEvent::RunFinished {
                status: crate::journal::RunStatus::Completed,
                detail: None,
            })
            .unwrap();
        drop(journal);

        assert_eq!(paths.list()[0].phase, RunPhase::Finished);
    }

    #[test]
    fn a_failure_report_counts_the_steps_that_did_happen() {
        // A run that failed on its third node did two steps. Reporting zero
        // would be worse than reporting nothing.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        let journal = JsonlJournal::create(&path).unwrap();
        journal
            .append(JournalEvent::RunStarted {
                run_id: "r".into(),
                workflow_id: "w".into(),
                workflow_name: "Line".into(),
                graph_hash: "abc".into(),
                input: Value::Null,
            })
            .unwrap();
        for (node, status) in [("a", NodeStatus::Ok), ("b", NodeStatus::Ok), ("c", NodeStatus::Error)]
        {
            journal
                .append(JournalEvent::NodeOutcome {
                    node: node.into(),
                    node_name: node.into(),
                    status,
                    attempt: 1,
                    output: None,
                    usage: (status == NodeStatus::Ok).then(|| Usage {
                        tokens_in: 10,
                        tokens_out: 5,
                        cost_usd: 0.01,
                        tool_calls: 1,
                    }),
                    code: None,
                    message: None,
                    detail: None,
                    since: None,
                })
                .unwrap();
        }
        drop(journal);

        let report = RunReport::failed("r", &RunError::Cancelled, path.clone());
        assert_eq!(report.steps, 2, "two nodes succeeded");
        assert_eq!(report.usage.total_tokens(), 30, "only the successes count");
        assert_eq!(report.usage.tool_calls, 2);
        assert_eq!(report.kind, RunOutcomeKind::Cancelled);
    }

    #[test]
    fn a_report_about_a_missing_journal_does_not_panic() {
        let report = RunReport::failed(
            "gone",
            &RunError::Cancelled,
            PathBuf::from("/definitely/not/here.jsonl"),
        );
        assert_eq!(report.steps, 0);
        assert!(report.error.is_some());
    }

    #[test]
    fn a_corrupt_journal_does_not_hide_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunPaths::new(dir.path());
        std::fs::write(paths.journal("broken"), "{not json}\n").unwrap();
        let journal = JsonlJournal::create(paths.journal("fine")).unwrap();
        journal
            .append(JournalEvent::RunStarted {
                run_id: "fine".into(),
                workflow_id: "w1".into(),
                workflow_name: "Line".into(),
                graph_hash: "abc".into(),
                input: Value::Null,
            })
            .unwrap();
        drop(journal);

        let listed = paths.list();
        assert_eq!(listed.len(), 1, "the corrupt file is skipped");
        assert_eq!(listed[0].run_id, "fine");
    }
}
