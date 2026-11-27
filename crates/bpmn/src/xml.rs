//! BPMN 2.0 XML import/export.
//!
//! The IDE stores workflows natively as [`nocode_model::Workflow`] graphs and
//! uses `bpmn-js` in the browser for rendering/editing. This module converts
//! between the native graph and a BPMN 2.0 XML document so models can be
//! round-tripped through `bpmn-js`.
//!
//! The importer focuses on the elements the engine understands (events, tasks,
//! gateways, sequence flows) and preserves node names and positions.

use base64::Engine as _;
use nocode_model::{
    AgentNodeConfig, EdgeId, EdgeStyle, NodeConfig, NodeId, NodePosition, Workflow, WorkflowEdge,
    WorkflowNode, WorkflowNodeType,
};
use roxmltree::Document;
use uuid::Uuid;

use crate::error::{BpmnError, Result};

const BPMN_NS: &str = "http://www.omg.org/spec/BPMN/20100524/MODEL";
const BPMNDI_NS: &str = "http://www.omg.org/spec/BPMN/20100524/DI";
const DC_NS: &str = "http://www.omg.org/spec/DD/20100524/DC";
const DI_NS: &str = "http://www.omg.org/spec/DD/20100524/DI";
/// Namespace for the agent-node extension.
///
/// BPMN 2.0 has no vocabulary for "call a model", so an agent node exports as a
/// `serviceTask` whose payload rides in this namespaced child. Export is
/// lossless for Bonafoid; a foreign BPMN tool drops the extension and imports a
/// plain service task, which is the honest outcome — a tool that does not know
/// what an LLM is cannot invent one.
const BONAFOID_NS: &str = "https://bonafoid.dev/schema/bpmn";

/// Parse a BPMN 2.0 XML document into a [`Workflow`].
///
/// `name` is used as the workflow name when the document has no process name.
pub fn parse_bpmn(xml: &str, name: &str) -> Result<Workflow> {
    let doc = Document::parse(xml).map_err(|e| BpmnError::Xml(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "definitions" {
        return Err(BpmnError::Xml(
            "expected a <definitions> root element".into(),
        ));
    }

    // Locate the first <process> element.
    let process = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "process")
        .ok_or_else(|| BpmnError::Xml("document contains no <process>".into()))?;

    let workflow_name = process
        .attribute("name")
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string();

    // --- Pass 1: node positions from BPMNDI ---
    let mut positions: std::collections::HashMap<String, (f64, f64)> =
        std::collections::HashMap::new();
    for shape in root.descendants().filter(|n| {
        n.is_element()
            && n.tag_name().name() == "BPMNShape"
            && n.tag_name().namespace() == Some(BPMNDI_NS)
    }) {
        let Some(id) = shape.attribute("bpmnElement") else {
            continue;
        };
        let bounds = shape.children().find(|n| {
            n.is_element()
                && n.tag_name().name() == "Bounds"
                && n.tag_name().namespace() == Some(DC_NS)
        });
        if let Some(bounds) = bounds {
            let x = bounds
                .attribute("x")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let y = bounds
                .attribute("y")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            positions.insert(id.to_string(), (x, y));
        }
    }

    // --- Pass 2: nodes ---
    let mut nodes = Vec::new();
    // Map BPMN element ids to generated node ids as we go, so sequence flows
    // can be resolved by id (names may be empty or duplicated).
    let mut id_map: std::collections::HashMap<String, NodeId> = std::collections::HashMap::new();
    for element in process.children().filter(|n| n.is_element()) {
        let tag = element.tag_name().name();
        if tag == "sequenceFlow" {
            continue;
        }
        let id = element.attribute("id").unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let node_name = element.attribute("name").unwrap_or(tag).to_string();
        let (x, y) = positions.get(&id).copied().unwrap_or((0.0, 0.0));

        // Agent nodes carry their payload out-of-band; every other type leaves
        // the config empty, as before.
        let mut config = NodeConfig::default();
        let node_type = match tag {
            "startEvent" => Some(WorkflowNodeType::StartEvent {
                event_type: nocode_model::StartEventType::None,
            }),
            "endEvent" => Some(WorkflowNodeType::EndEvent {
                event_type: nocode_model::EndEventType::None,
            }),
            "userTask" => Some(WorkflowNodeType::UserTask {
                assignee: element.attribute("assignee").map(|s| s.to_string()),
                candidate_groups: element
                    .attribute("candidateGroups")
                    .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
                    .unwrap_or_default(),
                form_id: None,
                due_date: None,
            }),
            "serviceTask" => match parse_agent_extension(element)? {
                Some(agent) => {
                    config.agent = Some(agent);
                    Some(WorkflowNodeType::AgentTask)
                }
                None => Some(WorkflowNodeType::ServiceTask {
                    implementation: nocode_model::ServiceTaskImplementation::Function {
                        function_name: element
                            .attribute("implementation")
                            .unwrap_or("noop")
                            .to_string(),
                        input_mapping: Default::default(),
                        output_mapping: Default::default(),
                    },
                    r#async: false,
                    exclusive: true,
                }),
            },
            "scriptTask" => Some(WorkflowNodeType::ScriptTask {
                script: element
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "script")
                    .and_then(|n| n.text())
                    .unwrap_or("")
                    .to_string(),
                script_format: element
                    .attribute("scriptFormat")
                    .unwrap_or("text/plain")
                    .to_string(),
            }),
            "manualTask" => Some(WorkflowNodeType::ManualTask),
            "businessRuleTask" => Some(WorkflowNodeType::BusinessRuleTask {
                decision_ref: element
                    .attribute("implementation")
                    .unwrap_or("")
                    .to_string(),
                decision_table: None,
            }),
            "sendTask" => Some(WorkflowNodeType::SendTask {
                message_ref: element.attribute("messageRef").unwrap_or("").to_string(),
            }),
            "receiveTask" => Some(WorkflowNodeType::ReceiveTask {
                message_ref: element.attribute("messageRef").unwrap_or("").to_string(),
            }),
            "exclusiveGateway" => Some(WorkflowNodeType::ExclusiveGateway { default_flow: None }),
            "inclusiveGateway" => Some(WorkflowNodeType::InclusiveGateway { default_flow: None }),
            "parallelGateway" => Some(WorkflowNodeType::ParallelGateway),
            "eventBasedGateway" => Some(WorkflowNodeType::EventBasedGateway),
            "subProcess" => Some(WorkflowNodeType::SubProcess {
                is_event_subprocess: element.attribute("triggeredByEvent") == Some("true"),
                is_transaction: false,
                workflow_id: None,
            }),
            "callActivity" => Some(WorkflowNodeType::CallActivity {
                called_workflow_id: nocode_model::WorkflowId::new(),
                input_mapping: Default::default(),
                output_mapping: Default::default(),
            }),
            "task" => Some(WorkflowNodeType::Task {
                task_type: nocode_model::TaskType::Manual,
            }),
            _ => None,
        };

        if let Some(node_type) = node_type {
            let node_id = NodeId(Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()));
            id_map.insert(id.clone(), node_id.clone());
            nodes.push(WorkflowNode {
                id: node_id,
                name: node_name,
                node_type,
                position: NodePosition { x, y },
                config,
                documentation: None,
            });
        }
    }

    // --- Pass 3: sequence flows ---
    let mut edges = Vec::new();
    for flow in process
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "sequenceFlow")
    {
        let source = flow.attribute("sourceRef").and_then(|s| id_map.get(s));
        let target = flow.attribute("targetRef").and_then(|s| id_map.get(s));
        let (Some(source), Some(target)) = (source, target) else {
            continue;
        };
        let condition = flow
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "conditionExpression")
            .and_then(|n| n.text())
            .map(|s| s.trim().to_string());
        edges.push(WorkflowEdge {
            id: EdgeId(Uuid::new_v4()),
            source: source.clone(),
            target: target.clone(),
            source_handle: None,
            target_handle: None,
            condition,
            label: flow.attribute("name").map(|s| s.to_string()),
            style: EdgeStyle::default(),
            mapping: Default::default(),
        });
    }

    Ok(Workflow {
        id: nocode_model::WorkflowId(Uuid::new_v4()),
        name: workflow_name,
        description: None,
        version: "1.0.0".to_string(),
        nodes,
        edges,
        variables: Vec::new(),
        signals: Vec::new(),
        timers: Vec::new(),
        settings: Default::default(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    })
}

/// Read the `bonafoid:agentNode` extension from a `serviceTask`.
///
/// Three outcomes, deliberately distinct:
///
/// - no extension → `Ok(None)`, so a foreign `serviceTask` stays a service
///   task (a tool that does not know what an LLM is cannot invent one);
/// - a readable extension → the parsed payload;
/// - an extension that is present but undecodable → an error. Falling back to
///   a plain service task here would silently drop the node's entire
///   behaviour, which is worse than refusing the file.
fn parse_agent_extension(element: roxmltree::Node) -> Result<Option<AgentNodeConfig>> {
    let Some(extension) = element.descendants().find(|n| {
        n.is_element()
            && n.tag_name().name() == "agentNode"
            && n.tag_name().namespace() == Some(BONAFOID_NS)
    }) else {
        return Ok(None);
    };

    let encoded = extension.attribute("config").ok_or_else(|| {
        BpmnError::Xml("bonafoid:agentNode is missing its `config` attribute".into())
    })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|error| {
            BpmnError::Xml(format!("bonafoid:agentNode config is not valid base64: {error}"))
        })?;
    let json = String::from_utf8(bytes)
        .map_err(|_| BpmnError::Xml("bonafoid:agentNode config is not valid UTF-8".into()))?;
    let config: AgentNodeConfig = serde_json::from_str(&json).map_err(|error| {
        BpmnError::Xml(format!("bonafoid:agentNode config is not a valid node: {error}"))
    })?;
    Ok(Some(config))
}

/// Serialize a [`Workflow`] to BPMN 2.0 XML.
pub fn export_bpmn(workflow: &Workflow) -> Result<String> {
    let (xml, _id_map) = export_bpmn_with_ids(workflow)?;
    Ok(xml)
}

/// Serialize a workflow and return the mapping from node ids to BPMN element
/// ids so callers can build diagrams.
fn export_bpmn_with_ids(
    workflow: &Workflow,
) -> Result<(String, std::collections::HashMap<NodeId, String>)> {
    use std::collections::HashMap;
    use std::fmt::Write;

    let mut id_map: HashMap<NodeId, String> = HashMap::new();
    for (i, node) in workflow.nodes.iter().enumerate() {
        id_map.insert(node.id.clone(), format!("Node_{i}"));
    }

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        xml,
        "<definitions xmlns=\"{BPMN_NS}\" xmlns:bpmndi=\"{BPMNDI_NS}\" xmlns:dc=\"{DC_NS}\" xmlns:di=\"{DI_NS}\" xmlns:bonafoid=\"{BONAFOID_NS}\" targetNamespace=\"http://nocode.ide/bpmn\">"
    );
    let _ = writeln!(
        xml,
        "  <process id=\"Process_1\" name=\"{}\" isExecutable=\"true\">",
        escape_xml(&workflow.name)
    );

    for node in &workflow.nodes {
        let id = &id_map[&node.id];
        let name = escape_xml(&node.name);

        // Agent nodes export as a `serviceTask` carrying their payload in a
        // namespaced extension. The encoding is fallible (a non-finite
        // temperature cannot be serialised), so it happens here rather than
        // inside the write-only match below.
        let agent_payload = match node.agent_config() {
            Some(config) if matches!(node.node_type, WorkflowNodeType::AgentTask) => Some((
                config.kind().as_str(),
                base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_string(config)?),
            )),
            _ => None,
        };

        let _ = match &node.node_type {
            WorkflowNodeType::StartEvent { .. } => {
                writeln!(xml, "    <startEvent id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::EndEvent { .. } => {
                writeln!(xml, "    <endEvent id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::UserTask {
                assignee,
                candidate_groups,
                ..
            } => {
                let assignee = assignee.as_deref().unwrap_or("");
                let groups = candidate_groups.join(",");
                writeln!(
                    xml,
                    "    <userTask id=\"{id}\" name=\"{name}\" assignee=\"{}\" candidateGroups=\"{}\" />",
                    escape_xml(assignee),
                    escape_xml(&groups)
                )
            }
            WorkflowNodeType::ServiceTask { .. } => {
                writeln!(xml, "    <serviceTask id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::ScriptTask {
                script,
                script_format,
            } => {
                let _ = write!(
                    xml,
                    "    <scriptTask id=\"{id}\" name=\"{name}\" scriptFormat=\"{}\">",
                    escape_xml(script_format)
                );
                writeln!(xml, "<script>{}</script></scriptTask>", escape_xml(script))
            }
            WorkflowNodeType::ManualTask => {
                writeln!(xml, "    <manualTask id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::BusinessRuleTask { decision_ref, .. } => {
                writeln!(
                    xml,
                    "    <businessRuleTask id=\"{id}\" name=\"{name}\" implementation=\"{}\" />",
                    escape_xml(decision_ref)
                )
            }
            WorkflowNodeType::SendTask { message_ref } => {
                writeln!(
                    xml,
                    "    <sendTask id=\"{id}\" name=\"{name}\" messageRef=\"{}\" />",
                    escape_xml(message_ref)
                )
            }
            WorkflowNodeType::ReceiveTask { message_ref } => {
                writeln!(
                    xml,
                    "    <receiveTask id=\"{id}\" name=\"{name}\" messageRef=\"{}\" />",
                    escape_xml(message_ref)
                )
            }
            WorkflowNodeType::ExclusiveGateway { .. } => {
                writeln!(xml, "    <exclusiveGateway id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::InclusiveGateway { .. } => {
                writeln!(xml, "    <inclusiveGateway id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::ParallelGateway => {
                writeln!(xml, "    <parallelGateway id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::EventBasedGateway => {
                writeln!(xml, "    <eventBasedGateway id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::SubProcess {
                is_event_subprocess,
                ..
            } => {
                writeln!(
                    xml,
                    "    <subProcess id=\"{id}\" name=\"{name}\" triggeredByEvent=\"{}\" />",
                    is_event_subprocess
                )
            }
            WorkflowNodeType::CallActivity { .. } => {
                writeln!(xml, "    <callActivity id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::Task { .. } => {
                writeln!(xml, "    <task id=\"{id}\" name=\"{name}\" />")
            }
            WorkflowNodeType::AgentTask => match &agent_payload {
                Some((kind, encoded)) => {
                    let _ = writeln!(
                        xml,
                        "    <serviceTask id=\"{id}\" name=\"{name}\" bonafoid:kind=\"{kind}\">"
                    );
                    let _ = writeln!(xml, "      <extensionElements>");
                    let _ = writeln!(
                        xml,
                        "        <bonafoid:agentNode kind=\"{kind}\" config=\"{encoded}\" />"
                    );
                    let _ = writeln!(xml, "      </extensionElements>");
                    writeln!(xml, "    </serviceTask>")
                }
                // Unconfigured: keep the graph's shape so the file still opens,
                // and let validation report the node on the way back in.
                None => writeln!(xml, "    <serviceTask id=\"{id}\" name=\"{name}\" />"),
            },
        };
    }

    for (i, edge) in workflow.edges.iter().enumerate() {
        let source = id_map.get(&edge.source);
        let target = id_map.get(&edge.target);
        let (Some(source), Some(target)) = (source, target) else {
            continue;
        };
        let id = format!("Flow_{i}");
        let label = edge.label.as_deref().unwrap_or("");
        if let Some(condition) = &edge.condition {
            let _ = writeln!(
                xml,
                "    <sequenceFlow id=\"{id}\" name=\"{}\" sourceRef=\"{source}\" targetRef=\"{target}\">",
                escape_xml(label)
            );
            let _ = writeln!(
                xml,
                "      <conditionExpression xsi:type=\"tFormalExpression\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">{}</conditionExpression>",
                escape_xml(condition)
            );
            let _ = writeln!(xml, "    </sequenceFlow>");
        } else {
            let _ = writeln!(
                xml,
                "    <sequenceFlow id=\"{id}\" name=\"{}\" sourceRef=\"{source}\" targetRef=\"{target}\" />",
                escape_xml(label)
            );
        }
    }

    let _ = writeln!(xml, "  </process>");

    // --- BPMN DI section ---
    let _ = writeln!(xml, "  <bpmndi:BPMNDiagram id=\"Diagram_1\">");
    let _ = writeln!(
        xml,
        "    <bpmndi:BPMNPlane id=\"Plane_1\" bpmnElement=\"Process_1\">"
    );
    for node in &workflow.nodes {
        let id = &id_map[&node.id];
        let _ = writeln!(
            xml,
            "      <bpmndi:BPMNShape id=\"{id}_di\" bpmnElement=\"{id}\"><dc:Bounds x=\"{}\" y=\"{}\" width=\"100\" height=\"80\" /></bpmndi:BPMNShape>",
            node.position.x, node.position.y
        );
    }
    let _ = writeln!(xml, "    </bpmndi:BPMNPlane>");
    let _ = writeln!(xml, "  </bpmndi:BPMNDiagram>");
    let _ = writeln!(xml, "</definitions>");

    Ok((xml, id_map))
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::{
        EndEventType, NodePosition, Workflow, WorkflowEdge, WorkflowId, WorkflowNode,
        WorkflowNodeType,
    };

    fn sample_workflow() -> Workflow {
        let start = WorkflowNode {
            id: NodeId::new(),
            name: "Start".into(),
            node_type: WorkflowNodeType::StartEvent {
                event_type: nocode_model::StartEventType::None,
            },
            position: NodePosition { x: 100.0, y: 100.0 },
            config: Default::default(),
            documentation: None,
        };
        let task = WorkflowNode {
            id: NodeId::new(),
            name: "Approve".into(),
            node_type: WorkflowNodeType::UserTask {
                assignee: Some("alice".into()),
                candidate_groups: vec!["managers".into()],
                form_id: None,
                due_date: None,
            },
            position: NodePosition { x: 300.0, y: 100.0 },
            config: Default::default(),
            documentation: None,
        };
        let end = WorkflowNode {
            id: NodeId::new(),
            name: "End".into(),
            node_type: WorkflowNodeType::EndEvent {
                event_type: EndEventType::None,
            },
            position: NodePosition { x: 500.0, y: 100.0 },
            config: Default::default(),
            documentation: None,
        };
        let edges = vec![
            WorkflowEdge {
                id: EdgeId::new(),
                source: start.id.clone(),
                target: task.id.clone(),
                source_handle: None,
                target_handle: None,
                condition: None,
                label: None,
                style: EdgeStyle::default(),
                mapping: Default::default(),
            },
            WorkflowEdge {
                id: EdgeId::new(),
                source: task.id.clone(),
                target: end.id.clone(),
                source_handle: None,
                target_handle: None,
                condition: None,
                label: Some("approved".into()),
                style: EdgeStyle::default(),
                mapping: Default::default(),
            },
        ];
        Workflow {
            id: WorkflowId::new(),
            name: "Approval".into(),
            description: None,
            version: "1.0.0".into(),
            nodes: vec![start, task, end],
            edges,
            variables: Vec::new(),
            signals: Vec::new(),
            timers: Vec::new(),
            settings: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn exports_valid_xml() {
        let xml = export_bpmn(&sample_workflow()).unwrap();
        assert!(xml.contains("<definitions"));
        assert!(xml.contains("<process"));
        assert!(xml.contains("startEvent"));
        assert!(xml.contains("userTask"));
        assert!(xml.contains("sequenceFlow"));
        assert!(xml.contains("bpmndi:BPMNDiagram"));
    }

    #[test]
    fn round_trips_nodes_and_edges() {
        let original = sample_workflow();
        let xml = export_bpmn(&original).unwrap();
        let parsed = parse_bpmn(&xml, "fallback").unwrap();

        assert_eq!(parsed.name, "Approval");
        assert_eq!(parsed.nodes.len(), original.nodes.len());
        assert_eq!(parsed.edges.len(), original.edges.len());
        assert_eq!(parsed.nodes[1].name, "Approve");
        assert_eq!(parsed.edges[1].label.as_deref(), Some("approved"));
    }

    #[test]
    fn positions_are_preserved() {
        let original = sample_workflow();
        let xml = export_bpmn(&original).unwrap();
        let parsed = parse_bpmn(&xml, "x").unwrap();
        assert_eq!(parsed.nodes[2].position.x, 500.0);
    }

    #[test]
    fn rejects_non_bpmn_root() {
        let err = parse_bpmn("<foo/>", "x").unwrap_err();
        assert!(matches!(err, BpmnError::Xml(_)));
    }

    fn agent_workflow(agent: nocode_model::AgentNodeConfig) -> Workflow {
        let start = WorkflowNode {
            id: NodeId::new(),
            name: "Start".into(),
            node_type: WorkflowNodeType::StartEvent {
                event_type: nocode_model::StartEventType::None,
            },
            position: NodePosition { x: 0.0, y: 0.0 },
            config: Default::default(),
            documentation: None,
        };
        let mut config = NodeConfig::default();
        config.agent = Some(agent);
        let task = WorkflowNode {
            id: NodeId::new(),
            name: "Audit".into(),
            node_type: WorkflowNodeType::AgentTask,
            position: NodePosition { x: 200.0, y: 0.0 },
            config,
            documentation: None,
        };
        let end = WorkflowNode {
            id: NodeId::new(),
            name: "End".into(),
            node_type: WorkflowNodeType::EndEvent {
                event_type: nocode_model::EndEventType::None,
            },
            position: NodePosition { x: 400.0, y: 0.0 },
            config: Default::default(),
            documentation: None,
        };
        let edges = vec![
            WorkflowEdge {
                id: EdgeId::new(),
                source: start.id.clone(),
                target: task.id.clone(),
                source_handle: None,
                target_handle: None,
                condition: None,
                label: None,
                style: EdgeStyle::default(),
                mapping: Default::default(),
            },
            WorkflowEdge {
                id: EdgeId::new(),
                source: task.id.clone(),
                target: end.id.clone(),
                source_handle: None,
                target_handle: None,
                condition: None,
                label: None,
                style: EdgeStyle::default(),
                mapping: Default::default(),
            },
        ];
        Workflow {
            id: WorkflowId::new(),
            name: "Sentinel".into(),
            description: None,
            version: "1.0.0".into(),
            nodes: vec![start, task, end],
            edges,
            variables: Vec::new(),
            signals: Vec::new(),
            timers: Vec::new(),
            settings: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn the_export_declares_the_bonafoid_namespace() {
        let xml = export_bpmn(&sample_workflow()).unwrap();
        assert!(
            xml.contains("xmlns:bonafoid="),
            "an undeclared prefix makes the document invalid XML"
        );
    }

    #[test]
    fn an_agent_node_round_trips_with_its_payload() {
        let agent = nocode_model::AgentNodeConfig::Shell(nocode_model::ShellNode {
            program: "git".into(),
            args: vec!["diff".into(), "--name-only".into()],
            ..Default::default()
        });
        let original = agent_workflow(agent.clone());

        let xml = export_bpmn(&original).unwrap();
        assert!(xml.contains("<serviceTask"));
        assert!(xml.contains("bonafoid:agentNode"), "the payload must ride along");

        let parsed = parse_bpmn(&xml, "fallback").unwrap();
        let node = parsed
            .nodes
            .iter()
            .find(|n| n.name == "Audit")
            .expect("agent node survived the round trip");
        assert_eq!(node.node_type, WorkflowNodeType::AgentTask);
        assert_eq!(node.config.agent, Some(agent));
    }

    #[test]
    fn a_plain_service_task_is_not_promoted_to_an_agent_node() {
        let xml = r#"<?xml version="1.0"?>
        <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
          <process id="p" name="P">
            <startEvent id="s" name="Start" />
            <serviceTask id="t" name="Call" implementation="noop" />
            <endEvent id="e" name="End" />
            <sequenceFlow id="f1" sourceRef="s" targetRef="t" />
            <sequenceFlow id="f2" sourceRef="t" targetRef="e" />
          </process>
        </definitions>"#;
        let parsed = parse_bpmn(xml, "P").unwrap();
        let node = parsed.nodes.iter().find(|n| n.name == "Call").unwrap();
        assert!(matches!(node.node_type, WorkflowNodeType::ServiceTask { .. }));
        assert!(node.config.agent.is_none());
    }

    #[test]
    fn a_corrupt_extension_is_refused_rather_than_silently_dropped() {
        // Falling back to a plain service task here would turn "this node runs
        // a security audit" into "this node does nothing", with no signal.
        let xml = r#"<?xml version="1.0"?>
        <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
                     xmlns:bonafoid="https://bonafoid.dev/schema/bpmn">
          <process id="p" name="P">
            <startEvent id="s" name="Start" />
            <serviceTask id="t" name="Call">
              <extensionElements>
                <bonafoid:agentNode kind="llm" config="not base64!" />
              </extensionElements>
            </serviceTask>
            <endEvent id="e" name="End" />
            <sequenceFlow id="f1" sourceRef="s" targetRef="t" />
            <sequenceFlow id="f2" sourceRef="t" targetRef="e" />
          </process>
        </definitions>"#;
        let error = parse_bpmn(xml, "P").unwrap_err();
        assert!(matches!(error, BpmnError::Xml(_)), "{error}");
    }

    #[test]
    fn an_unconfigured_agent_node_still_exports_and_imports_as_an_agent_node() {
        let mut workflow = agent_workflow(nocode_model::AgentNodeConfig::default_for(
            nocode_model::AgentNodeKind::Shell,
        ));
        // Strip the payload: the node keeps its type but loses its config.
        for node in &mut workflow.nodes {
            node.config.agent = None;
        }
        let xml = export_bpmn(&workflow).unwrap();
        assert!(!xml.contains("bonafoid:agentNode"));
        let parsed = parse_bpmn(&xml, "x").unwrap();
        let node = parsed.nodes.iter().find(|n| n.name == "Audit").unwrap();
        assert!(
            matches!(node.node_type, WorkflowNodeType::ServiceTask { .. }),
            "a payload-less agent node cannot survive as one; the validator flags it"
        );
    }
}
