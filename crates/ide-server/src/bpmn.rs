//! BPMN 2.0 validation, import and export routes.

use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use nocode_bpmn::engine::{BpmnEngine, ValidationIssue};
use nocode_bpmn::xml::{export_bpmn, parse_bpmn};
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

/// The BPMN routes, merged into the main router.
pub fn bpmn_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/bpmn/validate", post(validate))
        .route("/api/v1/bpmn/import", post(import))
        .route("/api/v1/bpmn/export", post(export))
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
