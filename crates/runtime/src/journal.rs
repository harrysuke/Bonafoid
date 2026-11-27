//! The journal — the run's only durable state.
//!
//! One append-only JSONL file per run. Three rules from §7.3, all load-bearing:
//!
//! - **Intent before effect.** A side effect that happened without an intent
//!   record is a bug, not a crash artifact.
//! - **Outcome after effect.** A crash between the two leaves the node
//!   `Unknown`, which [`crate::runner`] refuses to guess about.
//! - **No secrets, no full output.** Outputs are capped and scrubbed before
//!   they reach here; the caller owns that, and [`JournalRecord`] has nowhere
//!   to hide a credential.
//!
//! A suspended run writes *no* `run_finished` record. That is the whole
//! suspension protocol: "not finished" means "resumable", and adding a
//! `run_suspended` record would give two ways to say the same thing.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use nocode_model::Workflow;

use crate::error::JournalError;
use crate::executor::Usage;

/// How a node attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Ok,
    Error,
    /// Parked until something outside the run answers (§7.4).
    Waiting,
    /// Skipped because an approval timed out with `on_timeout: skip`.
    Skipped,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Completed,
    Failed,
    Cancelled,
}

/// One journalled event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEvent {
    RunStarted {
        run_id: String,
        workflow_id: String,
        workflow_name: String,
        /// Guards resume against an edited graph.
        graph_hash: String,
        #[serde(default)]
        input: Value,
    },
    NodeIntent {
        node: String,
        node_name: String,
        /// The executor kind, so a resumed run can tell what was about to run.
        kind: String,
        attempt: u32,
        /// Hash of the interpolated config, so an edited config is visible.
        input_hash: String,
    },
    NodeOutcome {
        node: String,
        node_name: String,
        status: NodeStatus,
        attempt: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// When a `waiting` outcome began, for timeout accounting.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<DateTime<Utc>>,
    },
    RunFinished {
        status: RunStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl JournalEvent {
    /// The `type` tag, without a serialisation round-trip.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::NodeIntent { .. } => "node_intent",
            Self::NodeOutcome { .. } => "node_outcome",
            Self::RunFinished { .. } => "run_finished",
        }
    }

    /// Whether this record must be durable before the caller proceeds.
    ///
    /// Only intents: they are the promise that a side effect is about to
    /// happen. Outcomes are re-derivable from the world, intents are not.
    fn requires_fsync(&self) -> bool {
        matches!(self, Self::NodeIntent { .. })
    }
}

/// A sequenced, timestamped event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalRecord {
    pub seq: u64,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub event: JournalEvent,
}

/// Somewhere to write the run's history.
///
/// Synchronous on purpose: the writes are a few hundred bytes at a node
/// boundary, and the file must be fsynced before the effect anyway. An async
/// trait would add a `spawn_blocking` hop per record for no gain.
pub trait Journal: Send + Sync {
    /// Append one event, assigning its sequence number and timestamp.
    fn append(&self, event: JournalEvent) -> Result<JournalRecord, JournalError>;

    /// Every record written so far, oldest first.
    fn records(&self) -> Result<Vec<JournalRecord>, JournalError>;

    fn path(&self) -> Option<&Path> {
        None
    }
}

/// A journal that keeps records in memory. For tests and for a dry run.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    records: Mutex<Vec<JournalRecord>>,
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Journal for MemoryJournal {
    fn append(&self, event: JournalEvent) -> Result<JournalRecord, JournalError> {
        let mut records = self.records.lock().expect("journal lock");
        let record = JournalRecord {
            seq: records.len() as u64 + 1,
            ts: Utc::now(),
            event,
        };
        records.push(record.clone());
        Ok(record)
    }

    fn records(&self) -> Result<Vec<JournalRecord>, JournalError> {
        Ok(self.records.lock().expect("journal lock").clone())
    }
}

/// The append-only JSONL file.
#[derive(Debug)]
pub struct JsonlJournal {
    path: PathBuf,
    file: Mutex<File>,
    seq: AtomicU64,
}

impl JsonlJournal {
    /// Create a new journal, truncating any existing file at `path`.
    pub fn create(path: impl Into<PathBuf>) -> Result<Self, JournalError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
            seq: AtomicU64::new(0),
        })
    }

    /// Open an existing journal for appending, continuing its sequence.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, JournalError> {
        let path = path.into();
        let existing = read_file(&path)?;
        let seq = existing.last().map(|record| record.seq).unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
            seq: AtomicU64::new(seq),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the journal at `path` as a resumable view.
    pub fn load(path: impl AsRef<Path>) -> Result<RunJournal, JournalError> {
        RunJournal::from_records(read_file(path.as_ref())?)
    }
}

impl Journal for JsonlJournal {
    fn append(&self, event: JournalEvent) -> Result<JournalRecord, JournalError> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let record = JournalRecord {
            seq,
            ts: Utc::now(),
            event,
        };

        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');

        let mut file = self.file.lock().expect("journal lock");
        file.write_all(&line)?;
        if record.event.requires_fsync() {
            // The promise that a side effect is about to happen must survive a
            // power cut; the outcome record need not.
            file.sync_data()?;
        }
        Ok(record)
    }

    fn records(&self) -> Result<Vec<JournalRecord>, JournalError> {
        read_file(&self.path)
    }

    fn path(&self) -> Option<&Path> {
        Some(&self.path)
    }
}

fn read_file(path: &Path) -> Result<Vec<JournalRecord>, JournalError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: JournalRecord =
            serde_json::from_str(line).map_err(|error| JournalError::Corrupt {
                line: index + 1,
                detail: error.to_string(),
            })?;
        records.push(record);
    }
    Ok(records)
}

/// What the journal knows about one node.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeState {
    Succeeded { output: Value, usage: Usage },
    Failed { code: String, message: String, attempt: u32 },
    /// Parked, with the attempt number so a resume can re-present the card.
    Waiting {
        since: DateTime<Utc>,
        attempt: u32,
    },
    /// An intent with no outcome: the side effect may or may not have happened.
    Unknown { attempt: u32 },
}

/// A journal loaded for replay.
#[derive(Debug, Clone)]
pub struct RunJournal {
    pub run_id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub graph_hash: String,
    pub input: Value,
    pub records: Vec<JournalRecord>,
    pub finished: Option<RunStatus>,
}

impl RunJournal {
    pub fn from_records(records: Vec<JournalRecord>) -> Result<Self, JournalError> {
        let started = records
            .iter()
            .find_map(|record| match &record.event {
                JournalEvent::RunStarted {
                    run_id,
                    workflow_id,
                    workflow_name,
                    graph_hash,
                    input,
                } => Some((
                    run_id.clone(),
                    workflow_id.clone(),
                    workflow_name.clone(),
                    graph_hash.clone(),
                    input.clone(),
                )),
                _ => None,
            })
            .ok_or(JournalError::NotStarted)?;

        let finished = records.iter().rev().find_map(|record| match &record.event {
            JournalEvent::RunFinished { status, .. } => Some(*status),
            _ => None,
        });

        Ok(Self {
            run_id: started.0,
            workflow_id: started.1,
            workflow_name: started.2,
            graph_hash: started.3,
            input: started.4,
            records,
            finished,
        })
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    /// The latest state of every node that has an intent record.
    ///
    /// The *last* record for a node wins: an intent followed by an error
    /// followed by a retry's intent is `Unknown`, not `Failed`.
    pub fn nodes(&self) -> HashMap<String, NodeState> {
        let mut states: HashMap<String, NodeState> = HashMap::new();
        for record in &self.records {
            match &record.event {
                JournalEvent::NodeIntent { node, attempt, .. } => {
                    states.insert(node.clone(), NodeState::Unknown { attempt: *attempt });
                }
                JournalEvent::NodeOutcome {
                    node,
                    status,
                    attempt,
                    output,
                    usage,
                    code,
                    message,
                    since,
                    ..
                } => {
                    let state = match status {
                        NodeStatus::Ok => NodeState::Succeeded {
                            output: output.clone().unwrap_or(Value::Null),
                            usage: usage.clone().unwrap_or_default(),
                        },
                        NodeStatus::Skipped => NodeState::Succeeded {
                            output: output.clone().unwrap_or(Value::Null),
                            usage: Usage::default(),
                        },
                        NodeStatus::Error => NodeState::Failed {
                            code: code.clone().unwrap_or_else(|| "internal".to_string()),
                            message: message.clone().unwrap_or_default(),
                            attempt: *attempt,
                        },
                        NodeStatus::Waiting => NodeState::Waiting {
                            since: since.unwrap_or(record.ts),
                            attempt: *attempt,
                        },
                    };
                    states.insert(node.clone(), state);
                }
                _ => {}
            }
        }
        states
    }

    /// How many attempts a node has already had.
    pub fn attempts(&self, node: &str) -> u32 {
        self.records
            .iter()
            .filter(|record| {
                matches!(
                    &record.event,
                    JournalEvent::NodeIntent { node: id, .. } if id == node
                )
            })
            .count() as u32
    }
}

/// A stable hash of a graph, for the resume guard.
///
/// FNV-1a over the canonical JSON, hand-written so the value does not depend on
/// the Rust version: `DefaultHasher`'s output is explicitly not stable across
/// releases, and a journal written yesterday must still match today.
pub fn graph_hash(workflow: &Workflow) -> String {
    let canonical = serde_json::to_string(workflow).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::NodeErrorCode;
    use serde_json::json;

    fn started() -> JournalEvent {
        JournalEvent::RunStarted {
            run_id: "r1".into(),
            workflow_id: "w1".into(),
            workflow_name: "Sentinel".into(),
            graph_hash: "abc".into(),
            input: json!({}),
        }
    }

    #[test]
    fn a_journal_without_run_started_is_refused() {
        let error = RunJournal::from_records(Vec::new()).unwrap_err();
        assert!(matches!(error, JournalError::NotStarted));
    }

    #[test]
    fn an_intent_without_an_outcome_reads_as_unknown() {
        let journal = RunJournal::from_records(vec![
            JournalRecord {
                seq: 1,
                ts: Utc::now(),
                event: started(),
            },
            JournalRecord {
                seq: 2,
                ts: Utc::now(),
                event: JournalEvent::NodeIntent {
                    node: "n1".into(),
                    node_name: "Diff".into(),
                    kind: "shell".into(),
                    attempt: 1,
                    input_hash: "h".into(),
                },
            },
        ])
        .unwrap();

        assert_eq!(
            journal.nodes().get("n1"),
            Some(&NodeState::Unknown { attempt: 1 })
        );
    }

    #[test]
    fn the_last_record_for_a_node_wins() {
        let ts = Utc::now();
        let journal = RunJournal::from_records(vec![
            JournalRecord {
                seq: 1,
                ts,
                event: started(),
            },
            JournalRecord {
                seq: 2,
                ts,
                event: JournalEvent::NodeIntent {
                    node: "n1".into(),
                    node_name: "Diff".into(),
                    kind: "shell".into(),
                    attempt: 1,
                    input_hash: "h".into(),
                },
            },
            JournalRecord {
                seq: 3,
                ts,
                event: JournalEvent::NodeOutcome {
                    node: "n1".into(),
                    node_name: "Diff".into(),
                    status: NodeStatus::Error,
                    attempt: 1,
                    output: None,
                    usage: None,
                    code: Some(NodeErrorCode::NonzeroExit.as_str().into()),
                    message: Some("git failed".into()),
                    detail: None,
                    since: None,
                },
            },
            // A retry started but never reported back.
            JournalRecord {
                seq: 4,
                ts,
                event: JournalEvent::NodeIntent {
                    node: "n1".into(),
                    node_name: "Diff".into(),
                    kind: "shell".into(),
                    attempt: 2,
                    input_hash: "h".into(),
                },
            },
        ])
        .unwrap();

        assert_eq!(
            journal.nodes().get("n1"),
            Some(&NodeState::Unknown { attempt: 2 }),
            "an in-flight retry must not read as a failure"
        );
        assert_eq!(journal.attempts("n1"), 2);
    }

    #[test]
    fn a_waiting_outcome_keeps_its_start_time() {
        let since = Utc::now();
        let journal = RunJournal::from_records(vec![
            JournalRecord {
                seq: 1,
                ts: since,
                event: started(),
            },
            JournalRecord {
                seq: 2,
                ts: since,
                event: JournalEvent::NodeOutcome {
                    node: "n1".into(),
                    node_name: "Approve".into(),
                    status: NodeStatus::Waiting,
                    attempt: 1,
                    output: None,
                    usage: None,
                    code: None,
                    message: None,
                    detail: None,
                    since: Some(since),
                },
            },
        ])
        .unwrap();

        match journal.nodes().get("n1") {
            Some(NodeState::Waiting { since: recorded, .. }) => assert_eq!(*recorded, since),
            other => panic!("expected Waiting, got {other:?}"),
        }
        assert!(!journal.is_finished(), "a suspended run is resumable");
    }

    #[test]
    fn a_finished_run_is_marked_finished() {
        let journal = RunJournal::from_records(vec![
            JournalRecord {
                seq: 1,
                ts: Utc::now(),
                event: started(),
            },
            JournalRecord {
                seq: 2,
                ts: Utc::now(),
                event: JournalEvent::RunFinished {
                    status: RunStatus::Completed,
                    detail: None,
                },
            },
        ])
        .unwrap();
        assert!(journal.is_finished());
        assert_eq!(journal.finished, Some(RunStatus::Completed));
    }

    #[test]
    fn the_file_journal_round_trips_and_continues_its_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.jsonl");

        {
            let journal = JsonlJournal::create(&path).unwrap();
            journal.append(started()).unwrap();
            journal
                .append(JournalEvent::RunFinished {
                    status: RunStatus::Completed,
                    detail: None,
                })
                .unwrap();
        }

        let reopened = JsonlJournal::open(&path).unwrap();
        let record = reopened
            .append(JournalEvent::NodeIntent {
                node: "n1".into(),
                node_name: "x".into(),
                kind: "shell".into(),
                attempt: 1,
                input_hash: "h".into(),
            })
            .unwrap();
        assert_eq!(record.seq, 3, "the sequence must continue across a restart");

        let lines = std::fs::read_to_string(&path).unwrap();
        assert_eq!(lines.lines().count(), 3);
        assert!(lines.lines().next().unwrap().contains("\"type\":\"run_started\""));
    }

    #[test]
    fn a_corrupt_line_names_its_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.jsonl");
        std::fs::write(&path, "{\"seq\":1,\"ts\":\"2026-01-01T00:00:00Z\",\"type\":\"nope\"}\n")
            .unwrap();
        let error = JsonlJournal::load(&path).unwrap_err();
        assert!(matches!(error, JournalError::Corrupt { line: 1, .. }));
    }

    #[test]
    fn the_graph_hash_is_stable_and_changes_with_the_graph() {
        use nocode_model::Workflow;
        let a = Workflow::new("A");
        let b = Workflow::new("A");
        let mut c = Workflow::new("A");
        c.name = "B".into();

        assert_eq!(graph_hash(&a), graph_hash(&a), "deterministic");
        assert_ne!(graph_hash(&a), graph_hash(&c), "content-sensitive");
        // Two separately-constructed identical graphs hash the same only if
        // their ids match, which is the point: an edit is visible.
        assert_ne!(graph_hash(&a), graph_hash(&b));
    }
}
