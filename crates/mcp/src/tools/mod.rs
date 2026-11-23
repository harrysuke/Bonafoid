//! Tool registry. Each sub-module owns a cohesive set of tools; adding an SDK
//! later only changes the protocol boundary, never these handlers.

pub mod definitions;
pub mod document;
pub mod graph;
pub mod quality;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::error::McpError;
use crate::{CallerContext, McpServer};

/// MCP `tools/list` entry.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
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
    tools
}

type Handler = fn(&McpServer, &CallerContext, &str, Value) -> Option<Result<Value, McpError>>;

/// Dispatch a tool call by name.
pub fn dispatch(
    server: &McpServer,
    caller: &CallerContext,
    name: &str,
    arguments: Value,
) -> Result<Value, McpError> {
    const HANDLERS: [Handler; 4] = [
        document::dispatch,
        graph::dispatch,
        quality::dispatch,
        definitions::dispatch,
    ];
    for handler in HANDLERS {
        if let Some(result) = handler(server, caller, name, arguments.clone()) {
            return result;
        }
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
        let server = McpServer::new(crate::Config::from_env());
        assert!(matches!(
            dispatch(&server, &CallerContext::local(), "nope", Value::Null),
            Err(McpError::UnknownTool(_))
        ));
    }
}
