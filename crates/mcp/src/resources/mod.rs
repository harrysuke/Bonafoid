//! Read-only MCP resources that ground agent tool calls in the product's
//! real JSON contract.

pub mod catalogue;
pub mod examples;
pub mod schema;

use crate::error::McpError;

/// Metadata advertised by `resources/list`.
#[derive(Debug, Clone)]
pub struct ResourceMeta {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
}

/// A resource body returned by `resources/read`.
#[derive(Debug, Clone)]
pub struct Resource {
    pub meta: ResourceMeta,
    pub text: String,
}

pub const RESOURCES: [ResourceMeta; 7] = [
    ResourceMeta {
        uri: "bpmn://schema/workflow",
        name: "Workflow JSON Schema",
        description: "Generated JSON Schema for the native Workflow JSON format",
        mime_type: "application/schema+json",
    },
    ResourceMeta {
        uri: "bpmn://schema/project",
        name: "Project JSON Schema",
        description: "Generated JSON Schema showing where workflows live in a project",
        mime_type: "application/schema+json",
    },
    ResourceMeta {
        uri: "bpmn://catalog/node-types",
        name: "Node type catalogue",
        description: "All 16 node types with config keys, defaults and examples",
        mime_type: "application/json",
    },
    ResourceMeta {
        uri: "bpmn://examples/json/minimal",
        name: "Minimal workflow JSON",
        description: "Smallest valid workflow: start -> end",
        mime_type: "application/json",
    },
    ResourceMeta {
        uri: "bpmn://examples/json/gateway",
        name: "Gateway workflow JSON",
        description: "Start -> user task -> exclusive gateway -> two ends",
        mime_type: "application/json",
    },
    ResourceMeta {
        uri: "bpmn://examples/xml/minimal",
        name: "Minimal BPMN 2.0 XML",
        description: "Exporter output including BPMNDiagram and dc:Bounds",
        mime_type: "application/xml",
    },
    ResourceMeta {
        uri: "bpmn://guide/authoring",
        name: "Authoring guide",
        description: "Preferred tool sequence, condition syntax and coordinate rules",
        mime_type: "text/markdown",
    },
];

/// All resource descriptors as a JSON array for the protocol response.
pub fn list_json() -> serde_json::Value {
    serde_json::json!(RESOURCES
        .iter()
        .map(|meta| serde_json::json!({
            "uri": meta.uri,
            "name": meta.name,
            "description": meta.description,
            "mimeType": meta.mime_type,
        }))
        .collect::<Vec<_>>())
}

/// Read one resource by URI.
pub fn read(uri: &str) -> Result<Resource, McpError> {
    let meta = RESOURCES
        .iter()
        .find(|meta| meta.uri == uri)
        .ok_or_else(|| McpError::InvalidParam {
            name: "uri",
            detail: format!("unknown resource `{uri}`"),
        })?;
    let text = match uri {
        "bpmn://schema/workflow" => schema::workflow_schema()?,
        "bpmn://schema/project" => schema::project_schema()?,
        "bpmn://catalog/node-types" => {
            serde_json::to_string_pretty(&catalogue::catalogue_json()).map_err(McpError::from)?
        }
        "bpmn://examples/json/minimal" => examples::MINIMAL_WORKFLOW_JSON.to_string(),
        "bpmn://examples/json/gateway" => examples::GATEWAY_WORKFLOW_JSON.to_string(),
        "bpmn://examples/xml/minimal" => examples::MINIMAL_XML.to_string(),
        "bpmn://guide/authoring" => examples::AUTHORING_GUIDE.to_string(),
        _ => unreachable!("all resource URIs are handled above"),
    };
    Ok(Resource {
        meta: meta.clone(),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_resource_is_readable_and_parses() {
        for meta in &RESOURCES {
            let resource = read(meta.uri).unwrap();
            assert!(!resource.text.is_empty(), "{}", meta.uri);
            match meta.mime_type {
                "application/schema+json" | "application/json" => {
                    serde_json::from_str::<serde_json::Value>(&resource.text).unwrap();
                }
                "application/xml" => {
                    assert!(resource.text.contains("<definitions"));
                }
                _ => {}
            }
        }
    }

    #[test]
    fn unknown_resource_is_an_invalid_param() {
        assert!(read("bpmn://nope").is_err());
    }
}
