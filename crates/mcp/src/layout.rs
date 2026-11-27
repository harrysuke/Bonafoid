//! Deterministic layered auto-layout.
//!
//! Coordinates are derived from graph topology only. Re-running layout on
//! the same graph always yields byte-identical positions, which is what keeps
//! BPMN DI bounds valid after incremental agent edits.

use indexmap::IndexMap;
use nocode_model::{NodeId, NodePosition, Workflow};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Direction of the primary flow axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum LayoutDirection {
    /// Left to right (default): depth becomes x.
    #[default]
    LR,
    /// Top to bottom: depth becomes y.
    TB,
}

/// Spacing controls for [`apply_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutOptions {
    #[serde(default)]
    pub direction: LayoutDirection,
    #[serde(default = "default_column_width")]
    pub column_width: f64,
    #[serde(default = "default_row_spacing")]
    pub row_spacing: f64,
    #[serde(default = "default_margin")]
    pub margin: f64,
}

pub fn default_column_width() -> f64 {
    220.0
}

pub fn default_row_spacing() -> f64 {
    140.0
}

pub fn default_margin() -> f64 {
    80.0
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            direction: LayoutDirection::default(),
            column_width: default_column_width(),
            row_spacing: default_row_spacing(),
            margin: default_margin(),
        }
    }
}

/// Compact position returned by `auto_layout`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePositionDto {
    pub node_id: String,
    pub x: f64,
    pub y: f64,
}

/// Recompute every node position from the workflow's edges.
pub fn apply_layout(workflow: &mut Workflow, options: &LayoutOptions) -> Vec<NodePositionDto> {
    let count = workflow.nodes.len();
    if count == 0 {
        return Vec::new();
    }

    let index: HashMap<NodeId, usize> = workflow
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.clone(), i))
        .collect();

    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut indegree = vec![0usize; count];
    for edge in &workflow.edges {
        if let (Some(&source), Some(&target)) = (index.get(&edge.source), index.get(&edge.target)) {
            successors[source].push(target);
            indegree[target] += 1;
        }
    }

    // Kahn layering for the acyclic part, stable in node declaration order.
    let mut depth = vec![0usize; count];
    let mut visited = vec![false; count];
    let mut queue: VecDeque<usize> = (0..count).filter(|i| indegree[*i] == 0).collect();
    while let Some(current) = queue.pop_front() {
        if visited[current] {
            continue;
        }
        visited[current] = true;
        for &next in &successors[current] {
            depth[next] = depth[next].max(depth[current] + 1);
            indegree[next] = indegree[next].saturating_sub(1);
            if indegree[next] == 0 {
                queue.push_back(next);
            }
        }
    }

    // Nodes in cycles never leave the queue. Relax them a bounded number of
    // times so layout still terminates and stays deterministic.
    for _ in 0..count {
        let mut changed = false;
        for edge in &workflow.edges {
            if let (Some(&source), Some(&target)) =
                (index.get(&edge.source), index.get(&edge.target))
            {
                if !visited[target] && depth[target] < depth[source] + 1 {
                    depth[target] = depth[source] + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut layers: IndexMap<usize, Vec<usize>> = IndexMap::new();
    for (i, layer) in depth.iter().enumerate() {
        layers.entry(*layer).or_default().push(i);
    }

    let mut positions = Vec::with_capacity(count);
    for (&layer, members) in &layers {
        for (slot, &node_index) in members.iter().enumerate() {
            let cross = options.margin
                + (slot as f64 - (members.len() - 1) as f64 / 2.0) * options.row_spacing;
            let along = options.margin + layer as f64 * options.column_width;
            let (x, y) = match options.direction {
                LayoutDirection::LR => (along, cross),
                LayoutDirection::TB => (cross, along),
            };
            workflow.nodes[node_index].position = NodePosition { x, y };
            positions.push(NodePositionDto {
                node_id: workflow.nodes[node_index].id.to_string(),
                x,
                y,
            });
        }
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::WorkflowNodeType;

    fn node(name: &str) -> nocode_model::WorkflowNode {
        nocode_model::WorkflowNode {
            id: NodeId::new(),
            name: name.into(),
            node_type: WorkflowNodeType::ManualTask,
            position: NodePosition::default(),
            config: Default::default(),
            documentation: None,
        }
    }

    fn chain() -> Workflow {
        let mut workflow = Workflow::new("layout");
        let nodes: Vec<_> = (0..4).map(|i| node(&format!("n{i}"))).collect();
        for i in 0..nodes.len() - 1 {
            workflow.edges.push(nocode_model::WorkflowEdge {
                id: nocode_model::EdgeId::new(),
                source: nodes[i].id.clone(),
                target: nodes[i + 1].id.clone(),
                source_handle: None,
                target_handle: None,
                condition: None,
                label: None,
                style: Default::default(),
                mapping: Default::default(),
            });
        }
        workflow.nodes = nodes;
        workflow
    }

    #[test]
    fn layout_is_deterministic() {
        let mut first = chain();
        let mut second = first.clone();
        let a = apply_layout(&mut first, &LayoutOptions::default());
        let b = apply_layout(&mut second, &LayoutOptions::default());
        assert_eq!(
            a.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>(),
            b.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>()
        );
        assert!(a.windows(2).all(|pair| pair[0].x < pair[1].x));
    }

    #[test]
    fn gateway_branches_fan_out_symmetrically() {
        let mut workflow = chain();
        let branch = node("branch");
        let end = node("end");
        let fork = workflow.nodes[1].id.clone();
        let tail = workflow.nodes[3].id.clone();
        workflow.edges.clear();
        workflow.edges.push(nocode_model::WorkflowEdge {
            id: nocode_model::EdgeId::new(),
            source: fork.clone(),
            target: branch.id.clone(),
            source_handle: None,
            target_handle: None,
            condition: None,
            label: None,
            style: Default::default(),
            mapping: Default::default(),
        });
        workflow.edges.push(nocode_model::WorkflowEdge {
            id: nocode_model::EdgeId::new(),
            source: fork,
            target: end.id.clone(),
            source_handle: None,
            target_handle: None,
            condition: None,
            label: None,
            style: Default::default(),
            mapping: Default::default(),
        });
        workflow.edges.push(nocode_model::WorkflowEdge {
            id: nocode_model::EdgeId::new(),
            source: branch.id.clone(),
            target: tail.clone(),
            source_handle: None,
            target_handle: None,
            condition: None,
            label: None,
            style: Default::default(),
            mapping: Default::default(),
        });
        workflow.edges.push(nocode_model::WorkflowEdge {
            id: nocode_model::EdgeId::new(),
            source: end.id.clone(),
            target: tail,
            source_handle: None,
            target_handle: None,
            condition: None,
            label: None,
            style: Default::default(),
            mapping: Default::default(),
        });
        workflow.nodes.push(branch);
        workflow.nodes.push(end);
        let positions = apply_layout(&mut workflow, &LayoutOptions::default());
        let ys: Vec<f64> = positions.iter().map(|p| p.y).collect();
        assert!(ys.iter().any(|y| *y < 80.0) && ys.iter().any(|y| *y > 80.0));
    }

    #[test]
    fn top_to_bottom_swaps_axes() {
        let mut workflow = chain();
        let positions = apply_layout(
            &mut workflow,
            &LayoutOptions {
                direction: LayoutDirection::TB,
                ..Default::default()
            },
        );
        assert!(positions.windows(2).all(|pair| pair[0].y < pair[1].y));
    }
}
