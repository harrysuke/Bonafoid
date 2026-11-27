//! `bonafoid` — run and resume a workflow from a terminal.
//!
//! This is the reference *host* for [`nocode_runtime`]: it does the four things
//! a host must do and nothing else.
//!
//! 1. Load a workflow from a file.
//! 2. Choose where journals live.
//! 3. Answer or refuse approvals.
//! 4. Turn a [`RunReport`] into an exit code.
//!
//! The IDE does the same four things in-process, against the same crate — which
//! is the point of the seam: a graph that runs in the IDE runs here, and the
//! journal one writes the other can resume.
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | completed |
//! | 1 | failed, or the arguments were wrong |
//! | 2 | suspended on a human gate — run it again to continue |
//!
//! `2` rather than `0` for a suspension because a suspended run has not
//! finished, and a CI step that treats "parked on an approval" as success is a
//! CI step that ships without the approval.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use nocode_model::Workflow;
use nocode_runtime::{
    AlwaysApproves, ExecutorRegistry, NoApprovals, RunContext, RunInput, RunLimits, RunOutcomeKind,
    RunPaths, RunPhase, RunReport, RunSession, SystemClock,
};

const USAGE: &str = "\
bonafoid — run and resume workflows

USAGE:
    bonafoid run <workflow.json> [options]
    bonafoid resume <run-id> <workflow.json> [options]
    bonafoid runs [options]

OPTIONS:
    --id <id>            Run id. Defaults to a slug of the workflow name.
    --runs <dir>         Where journals live. Defaults to .bonafoid/runs.
    --workspace <dir>    The `${workspace}` root. Defaults to the workflow's directory.
    --yes                Answer every approval with `approve`. Without it, a gate
                         suspends the run and the command exits 2.
    --max-steps <n>      Node executions before the run stops. Default 500.
    --json               Print the report as JSON.
    -h, --help           This text.

A workflow file is either a workflow document or `{ \"workflow\": ... }`.";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let cli = match Cli::parse(&args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("bonafoid: {error:#}");
            return ExitCode::FAILURE;
        }
    };

    match cli.dispatch().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("bonafoid: {error:#}");
            ExitCode::FAILURE
        }
    }
}

struct Cli {
    command: Command,
    options: Options,
}

enum Command {
    Run { workflow: PathBuf },
    Resume { run_id: String, workflow: PathBuf },
    Runs,
}

struct Options {
    id: Option<String>,
    runs: PathBuf,
    workspace: Option<PathBuf>,
    yes: bool,
    max_steps: usize,
    json: bool,
}

impl Options {
    fn paths(&self) -> RunPaths {
        RunPaths::new(&self.runs)
    }

    fn limits(&self) -> RunLimits {
        RunLimits {
            max_steps: self.max_steps,
            ..RunLimits::default()
        }
    }

    fn session(&self) -> RunSession {
        let limits = self.limits();
        RunSession::new(
            ExecutorRegistry::new().with_control_executors(limits.clone(), Arc::new(SystemClock)),
            limits,
            Arc::new(SystemClock),
            self.paths(),
        )
    }

    /// The run context for `run_id`, with the workspace taken from the flag or
    /// from the workflow file's own directory.
    fn context(&self, run_id: &str, workflow_path: &Path) -> RunContext {
        let workspace = self
            .workspace
            .clone()
            .or_else(|| {
                workflow_path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(|parent| parent.to_path_buf())
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let context = RunContext::new(run_id, workspace);
        if self.yes {
            context.with_approvals(Arc::new(AlwaysApproves))
        } else {
            context.with_approvals(Arc::new(NoApprovals))
        }
    }
}

impl Cli {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Options {
            id: None,
            runs: PathBuf::from(".bonafoid/runs"),
            workspace: None,
            yes: false,
            max_steps: 500,
            json: false,
        };

        let mut positional: Vec<String> = Vec::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--id" => {
                    options.id = Some(
                        iter.next()
                            .ok_or_else(|| anyhow!("`--id` needs a value"))?
                            .clone(),
                    )
                }
                "--runs" => {
                    options.runs =
                        PathBuf::from(iter.next().ok_or_else(|| anyhow!("`--runs` needs a value"))?)
                }
                "--workspace" => {
                    options.workspace = Some(PathBuf::from(
                        iter.next()
                            .ok_or_else(|| anyhow!("`--workspace` needs a value"))?,
                    ))
                }
                "--max-steps" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| anyhow!("`--max-steps` needs a number"))?;
                    options.max_steps = value
                        .parse()
                        .with_context(|| format!("`{value}` is not a number"))?;
                }
                "--yes" => options.yes = true,
                "--json" => options.json = true,
                other if other.starts_with('-') => bail!("unknown option `{other}`\n\n{USAGE}"),
                other => positional.push(other.to_string()),
            }
        }

        let Some(head) = positional.first() else {
            bail!("no command\n\n{USAGE}");
        };
        let command = match head.as_str() {
            "run" => Command::Run {
                workflow: PathBuf::from(
                    positional
                        .get(1)
                        .ok_or_else(|| anyhow!("`run` needs a workflow file\n\n{USAGE}"))?,
                ),
            },
            "resume" => Command::Resume {
                run_id: positional
                    .get(1)
                    .ok_or_else(|| anyhow!("`resume` needs a run id\n\n{USAGE}"))?
                    .clone(),
                workflow: PathBuf::from(
                    positional
                        .get(2)
                        .ok_or_else(|| anyhow!("`resume` needs a workflow file\n\n{USAGE}"))?,
                ),
            },
            "runs" => Command::Runs,
            other => bail!("unknown command `{other}`\n\n{USAGE}"),
        };

        Ok(Self { command, options })
    }

    async fn dispatch(self) -> Result<ExitCode> {
        match &self.command {
            Command::Runs => {
                self.list_runs();
                Ok(ExitCode::SUCCESS)
            }
            Command::Run { workflow } => {
                let workflow = load_workflow(workflow)?;
                let id = self
                    .options
                    .id
                    .clone()
                    .unwrap_or_else(|| slug(&workflow.name));
                let path = self.options.workflow_path(&self.command);
                let context = self.options.context(&id, &path);
                let report = self
                    .options
                    .session()
                    .start(&workflow, &id, RunInput::empty(), &context)
                    .await?;
                self.report(&report)
            }
            Command::Resume { run_id, workflow } => {
                let workflow = load_workflow(workflow)?;
                let path = self.options.workflow_path(&self.command);
                let context = self.options.context(run_id, &path);
                let report = self
                    .options
                    .session()
                    .resume(&workflow, run_id, &context)
                    .await?;
                self.report(&report)
            }
        }
    }

    fn report(&self, report: &RunReport) -> Result<ExitCode> {
        if self.options.json {
            println!("{}", serde_json::to_string_pretty(report)?);
        } else {
            println!("run      {}", report.run_id);
            println!("status   {}", status_name(report.kind));
            if let Some(wait) = &report.wait {
                println!("waiting  {}", wait.title);
                if let Some(body) = &wait.body {
                    println!("         {body}");
                }
            }
            if let Some(error) = &report.error {
                println!("error    {error}");
            }
            println!("steps    {}", report.steps);
            if report.usage.total_tokens() > 0 {
                println!(
                    "usage    {} tokens, ${:.4}",
                    report.usage.total_tokens(),
                    report.usage.cost_usd
                );
            }
            if report.output != Value::Null {
                println!("output   {}", serde_json::to_string(&report.output)?);
            }
            println!("journal  {}", report.journal.display());
            if report.is_suspended() {
                println!("\nRe-run this command to continue once it is answered.");
            }
        }

        Ok(match report.kind {
            RunOutcomeKind::Completed => ExitCode::SUCCESS,
            RunOutcomeKind::Suspended => ExitCode::from(2),
            _ => ExitCode::FAILURE,
        })
    }

    fn list_runs(&self) {
        let paths = self.options.paths();
        let runs = paths.list();
        if runs.is_empty() {
            println!("no runs in {}", paths.root().display());
            return;
        }
        for run in runs {
            let phase = match run.phase {
                RunPhase::Resumable => "resumable",
                RunPhase::Finished => "finished",
            };
            println!(
                "{:<28} {:<10} {:<4} {}",
                run.run_id, phase, run.records, run.workflow_name
            );
        }
    }
}

impl Options {
    fn workflow_path(&self, command: &Command) -> PathBuf {
        match command {
            Command::Run { workflow } | Command::Resume { workflow, .. } => workflow.clone(),
            Command::Runs => PathBuf::from("."),
        }
    }
}

fn status_name(kind: RunOutcomeKind) -> &'static str {
    match kind {
        RunOutcomeKind::Completed => "completed",
        RunOutcomeKind::Suspended => "suspended",
        RunOutcomeKind::Failed => "failed",
        RunOutcomeKind::Cancelled => "cancelled",
    }
}

fn load_workflow(path: &Path) -> Result<Workflow> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let value: Value =
        serde_json::from_str(&text).with_context(|| format!("{} is not JSON", path.display()))?;

    // Accept a bare workflow or a `{ "workflow": ... }` envelope, because the
    // export endpoint and a project file disagree about which one they write.
    let candidate = match value.get("workflow") {
        Some(inner) => inner.clone(),
        None => value,
    };
    serde_json::from_value(candidate)
        .with_context(|| format!("{} is not a workflow document", path.display()))
}

/// A file-name-safe default run id from a workflow name.
///
/// Runs of separators collapse, so `Ship it / now!` is `ship-it-now` rather
/// than `ship-it---now`: the id ends up in a file name a person has to read.
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "run".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_is_a_safe_file_name() {
        assert_eq!(slug("Diamond"), "diamond");
        assert_eq!(slug("Ship it / now!"), "ship-it-now");
        assert_eq!(slug("  "), "run");
        assert_eq!(slug("../../etc"), "etc");
    }

    #[test]
    fn the_parser_reads_run_and_its_options() {
        let args = strings(&["run", "flow.json", "--id", "r1", "--yes"]);
        let cli = Cli::parse(&args).unwrap();
        assert!(matches!(cli.command, Command::Run { .. }));
        assert_eq!(cli.options.id.as_deref(), Some("r1"));
        assert!(cli.options.yes);
    }

    #[test]
    fn the_parser_reads_resume_with_both_positional_arguments() {
        let args = strings(&["resume", "r1", "flow.json", "--json"]);
        let cli = Cli::parse(&args).unwrap();
        match &cli.command {
            Command::Resume { run_id, workflow } => {
                assert_eq!(run_id, "r1");
                assert_eq!(workflow, Path::new("flow.json"));
            }
            _ => panic!("expected resume"),
        }
        assert!(cli.options.json);
    }

    #[test]
    fn an_unknown_option_is_refused_with_the_usage() {
        let error = Cli::parse(&strings(&["run", "flow.json", "--nope"]))
            .err()
            .expect("an unknown option is refused")
            .to_string();
        assert!(error.contains("--nope"), "{error}");
        assert!(error.contains("USAGE"), "{error}");
    }

    #[test]
    fn a_missing_argument_is_a_usage_error_not_a_panic() {
        assert!(Cli::parse(&strings(&["run"])).is_err());
        assert!(Cli::parse(&strings(&["resume", "r1"])).is_err());
        assert!(Cli::parse(&strings(&[])).is_err());
    }

    #[test]
    fn both_workflow_document_shapes_load() {
        let dir = tempfile::tempdir().unwrap();
        let workflow = Workflow::new("Bare");

        let bare = dir.path().join("bare.json");
        std::fs::write(&bare, serde_json::to_string(&workflow).unwrap()).unwrap();
        assert_eq!(load_workflow(&bare).unwrap().name, "Bare");

        let wrapped = dir.path().join("wrapped.json");
        std::fs::write(
            &wrapped,
            serde_json::to_string(&serde_json::json!({ "workflow": workflow })).unwrap(),
        )
        .unwrap();
        assert_eq!(load_workflow(&wrapped).unwrap().name, "Bare");
    }

    #[test]
    fn a_file_that_is_not_a_workflow_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("junk.json");
        std::fs::write(&path, r#"{"hello":"world"}"#).unwrap();
        let error = load_workflow(&path).unwrap_err().to_string();
        assert!(error.contains("not a workflow document"), "{error}");
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }
}
