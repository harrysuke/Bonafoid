//! JSON Schemas generated directly from the Rust model — no hand-maintained
//! copies that could drift from the product's wire format.

use crate::error::McpError;

/// `bpmn://schema/workflow`
pub fn workflow_schema() -> Result<String, McpError> {
    let schema = schemars::schema_for!(nocode_model::Workflow);
    serde_json::to_string_pretty(&schema).map_err(McpError::from)
}

/// `bpmn://schema/project`
pub fn project_schema() -> Result<String, McpError> {
    let schema = schemars::schema_for!(nocode_model::Project);
    serde_json::to_string_pretty(&schema).map_err(McpError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_match_schemars_byte_for_byte() {
        let workflow = schemars::schema_for!(nocode_model::Workflow);
        assert_eq!(
            workflow_schema().unwrap(),
            serde_json::to_string_pretty(&workflow).unwrap()
        );
        let project = schemars::schema_for!(nocode_model::Project);
        assert_eq!(
            project_schema().unwrap(),
            serde_json::to_string_pretty(&project).unwrap()
        );
    }
}
