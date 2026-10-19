//! BPMN 2.0 validation, import and export routes.

use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use nocode_bpmn::engine::{BpmnEngine, ValidationIssue};
use nocode_bpmn::xml::{export_bpmn, parse_bpmn};
use nocode_ide::bpmn::{run_workflow, RunOptions, RunSummary};
use nocode_model::Workflow;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::server::AppState;

/// Body carrying an inline workflow definition.
#[derive(Debug, Deserialize)]
pub struct WorkflowBody {
    pub workflow: Workflow,
}

/// Result of a validation pass over a workflow.
#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

/// Body of `POST /api/v1/bpmn/import`.
#[derive(Debug, Deserialize)]
pub struct ImportBpmnRequest {
    pub xml: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// A workflow definition returned to the client.
#[derive(Debug, Serialize)]
pub struct WorkflowResponse {
    pub workflow: Workflow,
}

/// Body of `POST /api/v1/bpmn/export`.
#[derive(Debug, Deserialize)]
pub struct ExportBpmnRequest {
    pub workflow: Workflow,
}

/// BPMN 2.0 XML returned to the client.
#[derive(Debug, Serialize)]
pub struct ExportBpmnResponse {
    pub xml: String,
}

/// Body of `POST /api/v1/bpmn/run`.
#[derive(Debug, Deserialize)]
pub struct RunWorkflowRequest {
    pub workflow: Workflow,
    /// Initial process variables (a JSON object).
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
    /// Complete user tasks automatically so the instance runs to the end.
    #[serde(default)]
    pub auto_complete_tasks: Option<bool>,
    /// Safety net against loops created by auto-completion.
    #[serde(default)]
    pub max_rounds: Option<u32>,
}

/// The BPMN routes, merged into the main router.
pub fn bpmn_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/bpmn/validate", post(validate))
        .route("/api/v1/bpmn/import", post(import))
        .route("/api/v1/bpmn/export", post(export))
        .route("/api/v1/bpmn/run", post(run))
}

/// `POST /api/v1/bpmn/validate` — structurally validate a workflow.
async fn validate(_user: AuthUser, Json(body): Json<WorkflowBody>) -> Json<ValidateResponse> {
    let result = BpmnEngine::new().validate(&body.workflow);
    Json(ValidateResponse {
        valid: result.is_valid(),
        issues: result.issues,
    })
}

/// `POST /api/v1/bpmn/import` — parse BPMN 2.0 XML into a workflow.
async fn import(
    _user: AuthUser,
    Json(body): Json<ImportBpmnRequest>,
) -> Result<Json<WorkflowResponse>, ApiError> {
    let name = body.name.as_deref().unwrap_or("Imported Process");
    let workflow =
        parse_bpmn(&body.xml, name).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    Ok(Json(WorkflowResponse { workflow }))
}

/// `POST /api/v1/bpmn/export` — serialize a workflow to BPMN 2.0 XML.
async fn export(
    _user: AuthUser,
    Json(body): Json<ExportBpmnRequest>,
) -> Result<Json<ExportBpmnResponse>, ApiError> {
    let xml =
        export_bpmn(&body.workflow).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    Ok(Json(ExportBpmnResponse { xml }))
}

/// `POST /api/v1/bpmn/run` — execute a workflow with the token engine.
///
/// The simulation is opt-in: without `auto_complete_tasks` the instance starts
/// and parks on its first user task, which is what the IDE's "Run" shows.
async fn run(
    _user: AuthUser,
    Json(body): Json<RunWorkflowRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    let workflow_json = serde_json::to_string(&body.workflow)
        .map_err(|error| ApiError::BadRequest(format!("cannot serialize workflow: {error}")))?;
    let summary = run_workflow(
        &workflow_json,
        body.variables.unwrap_or(serde_json::Value::Null),
        RunOptions {
            auto_complete_tasks: body.auto_complete_tasks.unwrap_or(false),
            max_rounds: body.max_rounds,
        },
    )
    .map_err(ApiError::BadRequest)?;
    Ok(Json(summary))
}
