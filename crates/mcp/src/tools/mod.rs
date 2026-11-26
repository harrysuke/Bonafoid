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
use serde_json::{Map, Value};

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
///
/// The result is passed through [`portable_schema`] because `schemars` emits
/// boolean subschemas for a `serde_json::Value` field, and a model gateway may
/// refuse to accept the whole tool list because of one of them.
pub fn schema_for<T: JsonSchema>() -> Value {
    let schema =
        serde_json::to_value(schemars::schema_for!(T)).expect("parameter schema serialises");
    portable_schema(schema)
}

/// Rewrite boolean subschemas into their object form, recursively.
///
/// `schemars` renders a `serde_json::Value` field as the schema `true`, meaning
/// "any JSON value". That is legal draft-07, and it is what a parameter like
/// `config: Option<Value>` should accept — but it is a boolean where several
/// gateways expect an object, and they reject the entire request rather than
/// the one tool. Cohere's OpenAI-compatibility layer answers
///
/// ```text
/// HTTP 422: invalid type: parameter 'parameters' is of type bool but should
/// be of type Object
/// ```
///
/// for `add_node`, `update_node`, `update_edge`, `simulate_workflow`,
/// `add_variable` and `update_variable` — the six tools with a `Value` field.
///
/// `{}` means exactly the same thing ("any value") and every validator accepts
/// it, so the rewrite is meaning-preserving rather than a restriction. A
/// `false` subschema becomes `{"not": {}}`, which is also the same "matches
/// nothing" in object form.
///
/// Only *schema positions* are rewritten. A boolean keyword value such as
/// `"uniqueItems": true` or `"additionalProperties": false` is left alone: the
/// first is not a schema, and the second is the idiomatic closed-object form
/// that every gateway already understands.
pub fn portable_schema(mut value: Value) -> Value {
    normalize_schema(&mut value);
    value
}

/// Keywords whose value is a map of schemas.
const SCHEMA_MAPS: &[&str] = &[
    "properties",
    "patternProperties",
    "definitions",
    "$defs",
    "dependentSchemas",
];

/// Keywords whose value is a single schema.
const SCHEMA_NODES: &[&str] = &[
    "items",
    "additionalItems",
    "contains",
    "propertyNames",
    "not",
    "if",
    "then",
    "else",
    "unevaluatedItems",
];

/// Keywords whose value is a list of schemas.
const SCHEMA_LISTS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

fn normalize_schema(value: &mut Value) {
    // A boolean schema is rewritten to its object equivalent. The replacement
    // is built from a shared borrow first, so the assignment below cannot
    // conflict with the match.
    let replacement = match &*value {
        Value::Bool(true) => Some(Value::Object(Map::new())),
        Value::Bool(false) => Some(serde_json::json!({ "not": {} })),
        _ => None,
    };
    if let Some(replacement) = replacement {
        *value = replacement;
        return;
    }

    if let Value::Object(map) = value {
        for keyword in SCHEMA_MAPS {
            if let Some(Value::Object(children)) = map.get_mut(*keyword) {
                for child in children.values_mut() {
                    normalize_schema(child);
                }
            }
        }
        for keyword in SCHEMA_NODES {
            if let Some(child) = map.get_mut(*keyword) {
                normalize_schema(child);
            }
        }
        for keyword in SCHEMA_LISTS {
            if let Some(Value::Array(children)) = map.get_mut(*keyword) {
                for child in children.iter_mut() {
                    normalize_schema(child);
                }
            }
        }
    }
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

    /// The regression guard for the Cohere 422: no tool may advertise a boolean
    /// schema anywhere, because a gateway that validates deeply rejects the
    /// whole request over one of them.
    #[test]
    fn no_tool_advertises_a_boolean_schema() {
        fn walk(value: &Value, path: &str) {
            match value {
                Value::Bool(otherwise) => {
                    panic!("{path} is the boolean schema {otherwise}; expected an object")
                }
                Value::Object(map) => {
                    for keyword in SCHEMA_MAPS {
                        if let Some(Value::Object(children)) = map.get(*keyword) {
                            for (name, child) in children {
                                walk(child, &format!("{path}/{keyword}/{name}"));
                            }
                        }
                    }
                    for keyword in SCHEMA_NODES {
                        if let Some(child) = map.get(*keyword) {
                            walk(child, &format!("{path}/{keyword}"));
                        }
                    }
                    for keyword in SCHEMA_LISTS {
                        if let Some(Value::Array(children)) = map.get(*keyword) {
                            for (index, child) in children.iter().enumerate() {
                                walk(child, &format!("{path}/{keyword}/{index}"));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        for tool in definitions() {
            walk(&tool.input_schema, &tool.name);
        }
    }

    #[test]
    fn a_value_field_becomes_an_any_schema() {
        // `{}` and `true` both mean "any value"; only the object form is
        // accepted by every gateway.
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "config": true, "count": { "type": "integer" } },
            "additionalProperties": false,
        });
        let portable = portable_schema(schema);
        assert_eq!(portable["properties"]["config"], serde_json::json!({}));
        // A real schema is untouched.
        assert_eq!(
            portable["properties"]["count"],
            serde_json::json!({ "type": "integer" })
        );
        // A boolean *keyword*, not a schema, is left alone: `false` here is the
        // idiomatic closed-object form.
        assert_eq!(portable["additionalProperties"], serde_json::json!(false));
    }

    #[test]
    fn nested_boolean_schemas_are_rewritten_at_every_depth() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "list": { "type": "array", "items": true },
                "one_of": { "anyOf": [ true, { "type": "string" } ] },
                "map": { "type": "object", "patternProperties": { ".*": true } },
            },
            "definitions": { "Anything": true },
        });
        let portable = portable_schema(schema);
        assert_eq!(portable["properties"]["list"]["items"], serde_json::json!({}));
        assert_eq!(portable["properties"]["one_of"]["anyOf"][0], serde_json::json!({}));
        assert_eq!(
            portable["properties"]["map"]["patternProperties"][".*"],
            serde_json::json!({})
        );
        assert_eq!(portable["definitions"]["Anything"], serde_json::json!({}));
    }

    #[test]
    fn a_false_subschema_becomes_matches_nothing() {
        let portable = portable_schema(serde_json::json!({ "properties": { "never": false } }));
        assert_eq!(
            portable["properties"]["never"],
            serde_json::json!({ "not": {} })
        );
    }

    #[test]
    fn normalising_twice_changes_nothing() {
        let once = portable_schema(definitions()[0].input_schema.clone());
        let twice = portable_schema(once.clone());
        assert_eq!(once, twice);
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
