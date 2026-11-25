//! Tool registry. Each sub-module owns a cohesive set of tools; adding an SDK
//! later only changes the protocol boundary, never these handlers.
//!
//! Two dispatch shapes:
//!
//! - [`dispatch`] runs the synchronous tools (the 23 shipped BPMN tools).
//! - [`tools::project::dispatch`] is async, because project storage is.
//!
//! `McpToolExecutor::call` picks between them, so callers never have to know.

pub mod backend;
pub mod definitions;
pub mod document;
pub mod domain;
pub mod graph;
pub mod project;
pub mod quality;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::error::{self, McpError};
use crate::executor::McpToolExecutor;
use crate::CallerContext;

/// MCP `tools/list` entry.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// MCP wire name is camelCase; a plain `input_schema` key would make every
    /// client treat the tool as taking no arguments.
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Generate an `inputSchema` from the handler's typed parameter struct.
pub fn schema_for<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("parameter schema serialises")
}

/// Deserialise tool arguments, rejecting missing or unexpected fields.
pub fn parse_args<T: DeserializeOwned>(arguments: Value) -> Result<T, McpError> {
    serde_json::from_value(arguments).map_err(McpError::from)
}

/// All tools advertised by the server.
pub fn definitions() -> Vec<ToolDefinition> {
    let mut tools = Vec::new();
    tools.extend(document::definitions());
    tools.extend(graph::definitions());
    tools.extend(quality::definitions());
    tools.extend(definitions::definitions());
    tools.extend(project::definitions());
    tools.extend(domain::definitions());
    tools.extend(backend::definitions());
    tools
}

type Handler = fn(&McpToolExecutor, &CallerContext, &str, Value) -> Option<Result<Value, McpError>>;

/// Dispatch a synchronous tool call by name.
///
/// On success the returned value is the MCP tool result envelope
/// (`{ content, isError }`) ready to be placed in a `tools/call` response;
/// domain failures are returned as `Err` so the transport can decide between a
/// tool result and a protocol-level error.
pub fn dispatch(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    name: &str,
    arguments: Value,
) -> Result<Value, McpError> {
    const HANDLERS: [Handler; 6] = [
        document::dispatch,
        graph::dispatch,
        quality::dispatch,
        definitions::dispatch,
        domain::dispatch,
        backend::dispatch,
    ];
    for handler in HANDLERS {
        if let Some(result) = handler(executor, caller, name, arguments.clone()) {
            // `Ok` is formatted as a tool result; `Err` stays an error so the
            // transport can classify it (client vs server) before rendering.
            return result.map(|value| error::into_tool_result(Ok::<Value, McpError>(value)));
        }
    }
    // Project tools exist but are async; reaching here means the caller used
    // the sync path for one, which is a programming error worth naming.
    if project::handles(name) {
        return Err(McpError::Internal(format!(
            "`{name}` needs project storage and must be called asynchronously"
        )));
    }
    Err(McpError::UnknownTool(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_have_unique_names_and_schemas() {
        let tools = definitions();
        assert!(tools.len() >= 14);
        for tool in &tools {
            assert!(
                tool.input_schema.get("properties").is_some(),
                "{}",
                tool.name
            );
            assert!(tool.description.len() > 20, "{}", tool.name);
        }
        let mut names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
    }

    #[test]
    fn unknown_tool_is_reported() {
        let executor = McpToolExecutor::new(std::sync::Arc::new(crate::Config::from_env()));
        assert!(matches!(
            dispatch(&executor, &CallerContext::local(), "nope", Value::Null),
            Err(McpError::UnknownTool(_))
        ));
    }

    #[test]
    fn a_project_tool_on_the_sync_path_says_why() {
        let executor = McpToolExecutor::new(std::sync::Arc::new(crate::Config::from_env()));
        let error = dispatch(
            &executor,
            &CallerContext::local(),
            "project_get",
            Value::Null,
        )
        .unwrap_err();
        assert!(matches!(error, McpError::Internal(_)));
        assert!(error.to_string().contains("asynchronously"));
    }
}
