//! The binary, end to end.
//!
//! Every layer is exercised: a workflow file is parsed, a session starts a run,
//! the runner writes a journal, and the report becomes an exit code. The graph
//! uses only control nodes — `approval` is the one kind Phase 35 ships that can
//! be answered without a model or a shell — which makes this the smallest graph
//! that proves the host wiring rather than the executors.
//!
//! Run through the real binary (`CARGO_BIN_EXE_bonafoid`) rather than the
//! library, because the things most likely to break here are argument parsing
//! and the exit code, and neither exists in the library.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nocode_model::{
    AgentNodeConfig, ApprovalNode, EdgeId, EndEventType, NodeConfig, NodeId, NodePosition,
    StartEventType, Workflow, WorkflowEdge, WorkflowNode, WorkflowNodeType,
};

fn node(name: &str, node_type: WorkflowNodeType) -> WorkflowNode {
    WorkflowNode {
        id: NodeId::new(),
        name: name.to_string(),
        node_type,
        position: NodePosition::default(),
        config: NodeConfig::default(),
        documentation: None,
    }
}

fn edge(source: &WorkflowNode, target: &WorkflowNode) -> WorkflowEdge {
    WorkflowEdge {
        id: EdgeId::new(),
        source: source.id.clone(),
        target: target.id.clone(),
        source_handle: None,
        target_handle: None,
        condition: None,
        label: None,
        style: Default::default(),
        mapping: Default::default(),
    }
}

/// `start → approval → end`, and nothing that needs a provider.
fn gated() -> Workflow {
    let start = node(
        "Start",
        WorkflowNodeType::StartEvent {
            event_type: StartEventType::None,
        },
    );
    let mut gate = node("Ship it", WorkflowNodeType::AgentTask);
    gate.config.agent = Some(AgentNodeConfig::Approval(ApprovalNode {
        body: Some("Deploy to production?".into()),
        ..Default::default()
    }));
    let end = node(
        "End",
        WorkflowNodeType::EndEvent {
            event_type: EndEventType::None,
        },
    );

    let mut workflow = Workflow::new("Deploy");
    workflow.edges = vec![edge(&start, &gate), edge(&gate, &end)];
    workflow.nodes = vec![start, gate, end];
    workflow
}

fn write_workflow(dir: &Path) -> PathBuf {
    let path = dir.join("deploy.json");
    std::fs::write(&path, serde_json::to_string_pretty(&gated()).unwrap()).unwrap();
    path
}

fn bonafoid(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bonafoid"))
        .args(args)
        .output()
        .expect("the binary should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn an_answered_run_completes_with_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let workflow = write_workflow(dir.path());
    let runs = dir.path().join("runs");

    let output = bonafoid(&[
        "run",
        workflow.to_str().unwrap(),
        "--id",
        "deploy-1",
        "--runs",
        runs.to_str().unwrap(),
        "--workspace",
        dir.path().to_str().unwrap(),
        "--yes",
    ]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("completed"), "{text}");
    assert!(text.contains("deploy-1"), "{text}");
    assert!(runs.join("deploy-1.jsonl").exists(), "a journal was written");
}

#[test]
fn an_unanswered_run_suspends_with_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    let workflow = write_workflow(dir.path());
    let runs = dir.path().join("runs");

    let output = bonafoid(&[
        "run",
        workflow.to_str().unwrap(),
        "--id",
        "deploy-2",
        "--runs",
        runs.to_str().unwrap(),
        "--workspace",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("suspended"), "{text}");
    assert!(text.contains("Ship it"), "the card is shown: {text}");
    assert!(
        text.contains("Re-run this command"),
        "the user is told how to continue: {text}"
    );
    // A suspended run has no `run_finished`, which is what makes it resumable.
    let journal = std::fs::read_to_string(runs.join("deploy-2.jsonl")).unwrap();
    assert!(!journal.contains("run_finished"), "{journal}");
}

#[test]
fn resuming_the_suspended_run_completes_it() {
    let dir = tempfile::tempdir().unwrap();
    let workflow = write_workflow(dir.path());
    let runs = dir.path().join("runs");
    let common = [
        "--runs",
        runs.to_str().unwrap(),
        "--workspace",
        dir.path().to_str().unwrap(),
    ];

    let first = bonafoid(&[
        "run",
        workflow.to_str().unwrap(),
        "--id",
        "deploy-3",
        common[0],
        common[1],
        common[2],
        common[3],
    ]);
    assert_eq!(first.status.code(), Some(2), "{}", stderr(&first));

    let resumed = bonafoid(&[
        "resume",
        "deploy-3",
        workflow.to_str().unwrap(),
        common[0],
        common[1],
        common[2],
        common[3],
        "--yes",
    ]);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr(&resumed));
    assert!(stdout(&resumed).contains("completed"));

    let journal = std::fs::read_to_string(runs.join("deploy-3.jsonl")).unwrap();
    assert!(journal.contains("run_finished"), "{journal}");
    // One start, one finish: the resume continued the same run.
    assert_eq!(journal.matches("run_started").count(), 1, "{journal}");
}

#[test]
fn the_run_list_shows_a_suspended_run_as_resumable() {
    let dir = tempfile::tempdir().unwrap();
    let workflow = write_workflow(dir.path());
    let runs = dir.path().join("runs");

    bonafoid(&[
        "run",
        workflow.to_str().unwrap(),
        "--id",
        "deploy-4",
        "--runs",
        runs.to_str().unwrap(),
        "--workspace",
        dir.path().to_str().unwrap(),
    ]);

    let listed = bonafoid(&["runs", "--runs", runs.to_str().unwrap()]);
    assert_eq!(listed.status.code(), Some(0), "{}", stderr(&listed));
    let text = stdout(&listed);
    assert!(text.contains("deploy-4"), "{text}");
    assert!(text.contains("resumable"), "{text}");
    assert!(text.contains("Deploy"), "the workflow name is shown: {text}");
}

#[test]
fn reusing_a_run_id_is_refused_rather_than_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let workflow = write_workflow(dir.path());
    let runs = dir.path().join("runs");
    let args = [
        "run",
        workflow.to_str().unwrap(),
        "--id",
        "deploy-5",
        "--runs",
        runs.to_str().unwrap(),
        "--workspace",
        dir.path().to_str().unwrap(),
        "--yes",
    ];

    assert_eq!(bonafoid(&args).status.code(), Some(0));
    let second = bonafoid(&args);
    assert_eq!(second.status.code(), Some(1), "{}", stderr(&second));
    assert!(
        stderr(&second).contains("already has a journal"),
        "{}",
        stderr(&second)
    );
}

#[test]
fn the_json_report_is_machine_readable() {
    let dir = tempfile::tempdir().unwrap();
    let workflow = write_workflow(dir.path());
    let runs = dir.path().join("runs");

    let output = bonafoid(&[
        "run",
        workflow.to_str().unwrap(),
        "--id",
        "deploy-6",
        "--runs",
        runs.to_str().unwrap(),
        "--workspace",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(report["kind"], "suspended");
    assert_eq!(report["wait"]["title"], "Ship it");
    assert_eq!(report["wait"]["kind"], "approval");
}

#[test]
fn a_graph_with_no_executor_fails_with_a_readable_message() {
    // Phase 35 ships the control executors only, so an `llm` node has no
    // executor yet. The failure must name the kind, not panic — and it is a
    // *report*, printed with the journal path, not a bare crash.
    let mut workflow = gated();
    let mut llm = node("Draft", WorkflowNodeType::AgentTask);
    llm.config.agent = Some(AgentNodeConfig::Llm(Default::default()));
    let start = workflow.nodes[0].id.clone();
    let end = workflow.nodes[2].id.clone();
    workflow.nodes.push(llm.clone());
    // A dead end would be refused by the validator first, and the point here is
    // the missing executor, so give the node somewhere to go.
    for (source, target) in [(start, llm.id.clone()), (llm.id, end)] {
        workflow.edges.push(WorkflowEdge {
            id: EdgeId::new(),
            source,
            target,
            source_handle: None,
            target_handle: None,
            condition: None,
            label: None,
            style: Default::default(),
            mapping: Default::default(),
        });
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("llm.json");
    std::fs::write(&path, serde_json::to_string(&workflow).unwrap()).unwrap();

    let output = bonafoid(&[
        "run",
        path.to_str().unwrap(),
        "--runs",
        dir.path().join("runs").to_str().unwrap(),
        "--yes",
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("status   failed"), "{text}");
    assert!(
        text.contains("no executor registered"),
        "the failing kind is named: {text}"
    );
    assert!(text.contains("journal"), "the journal is still reported: {text}");
}
