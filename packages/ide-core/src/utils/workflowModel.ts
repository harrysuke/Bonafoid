/**
 * Process builder model (M7 #4).
 *
 * Maps the generated Rust `Workflow` contract onto React Flow's node/edge
 * graph and back. Positions live on `WorkflowNode.position`, so canvas state
 * round-trips through project JSON without a separate layout file.
 */

import type {
  EdgeStyle,
  NodeConfig,
  NodeId,
  NodePosition,
  Workflow,
  WorkflowEdge,
  WorkflowNode,
  WorkflowNodeType,
  WorkflowSettings,
} from '../types/model.generated';
import { generateId } from './helpers';

export type NodePaletteCategory = 'events' | 'tasks' | 'gateways' | 'composite';

export interface NodePaletteItem {
  type: WorkflowNodeType['type'];
  category: NodePaletteCategory;
  label: string;
  /** Tailwind color hints used by the canvas nodes and the minimap. */
  color: string;
}

export const NODE_PALETTE: NodePaletteItem[] = [
  { type: 'StartEvent', category: 'events', label: 'Start', color: '#22c55e' },
  { type: 'EndEvent', category: 'events', label: 'End', color: '#ef4444' },
  { type: 'UserTask', category: 'tasks', label: 'User task', color: '#3b82f6' },
  { type: 'ServiceTask', category: 'tasks', label: 'Service task', color: '#6366f1' },
  { type: 'ScriptTask', category: 'tasks', label: 'Script task', color: '#8b5cf6' },
  { type: 'BusinessRuleTask', category: 'tasks', label: 'Business rule', color: '#a855f7' },
  { type: 'SendTask', category: 'tasks', label: 'Send message', color: '#0ea5e9' },
  { type: 'ReceiveTask', category: 'tasks', label: 'Receive message', color: '#38bdf8' },
  { type: 'ManualTask', category: 'tasks', label: 'Manual task', color: '#64748b' },
  { type: 'Task', category: 'tasks', label: 'Generic task', color: '#94a3b8' },
  { type: 'ExclusiveGateway', category: 'gateways', label: 'Exclusive gateway', color: '#f59e0b' },
  { type: 'InclusiveGateway', category: 'gateways', label: 'Inclusive gateway', color: '#eab308' },
  { type: 'ParallelGateway', category: 'gateways', label: 'Parallel gateway', color: '#d97706' },
  { type: 'EventBasedGateway', category: 'gateways', label: 'Event gateway', color: '#fbbf24' },
  { type: 'SubProcess', category: 'composite', label: 'Sub-process', color: '#10b981' },
  { type: 'CallActivity', category: 'composite', label: 'Call activity', color: '#059669' },
];

export const NODE_PALETTE_CATEGORIES: NodePaletteCategory[] = [
  'events',
  'tasks',
  'gateways',
  'composite',
];

export function paletteItem(type: WorkflowNodeType['type']): NodePaletteItem | undefined {
  return NODE_PALETTE.find((item) => item.type === type);
}

const DEFAULT_RETRY = {
  initial_interval_ms: 500,
  max_attempts: 3,
  max_interval_ms: 5000,
  multiplier: 2,
  retryable_codes: [429, 500, 502, 503, 504],
};

/** Default `WorkflowNodeType` payload for a palette entry. */
export function buildNodeType(type: WorkflowNodeType['type']): WorkflowNodeType {
  switch (type) {
    case 'StartEvent':
      return { type, config: { event_type: 'none' } };
    case 'EndEvent':
      return { type, config: { event_type: 'none' } };
    case 'UserTask':
      return {
        type,
        config: { assignee: null, candidate_groups: [], due_date: null, form_id: null },
      };
    case 'ServiceTask':
      return {
        type,
        config: {
          async: false,
          exclusive: false,
          implementation: {
            type: 'Http',
            config: {
              url: '',
              method: 'POST',
              headers: {},
              body_template: null,
              timeout_ms: 30_000,
              retry: { ...DEFAULT_RETRY },
            },
          },
        },
      };
    case 'ScriptTask':
      return { type, config: { script: '', script_format: 'javascript' } };
    case 'BusinessRuleTask':
      return { type, config: { decision_ref: '', decision_table: null } };
    case 'SendTask':
      return { type, config: { message_ref: '' } };
    case 'ReceiveTask':
      return { type, config: { message_ref: '' } };
    case 'ManualTask':
      return { type };
    case 'ExclusiveGateway':
      return { type, config: { default_flow: null } };
    case 'InclusiveGateway':
      return { type, config: { default_flow: null } };
    case 'ParallelGateway':
      return { type };
    case 'EventBasedGateway':
      return { type };
    case 'SubProcess':
      return { type, config: { is_event_subprocess: false, is_transaction: false, workflow_id: null } };
    case 'CallActivity':
      return { type, config: { called_workflow_id: '', input_mapping: {}, output_mapping: {} } };
    case 'Task':
      return { type, config: { task_type: 'user' } };
    default:
      return { type: 'ManualTask' };
  }
}

export function emptyNodeConfig(): NodeConfig {
  return {
    boundary_events: [],
    execution_listeners: [],
    inputs: {},
    outputs: {},
    properties: {},
    multi_instance: null,
  };
}

export function newWorkflowNode(
  type: WorkflowNodeType['type'],
  position: NodePosition = { x: 0, y: 0 },
): WorkflowNode {
  return {
    id: generateId(),
    name: paletteItem(type)?.label ?? type,
    node_type: buildNodeType(type),
    position,
    config: emptyNodeConfig(),
    documentation: null,
  };
}

export function defaultEdgeStyle(): EdgeStyle {
  return { animated: false, stroke_color: null, stroke_width: null, stroke_dasharray: null, marker_end: null };
}

export function newWorkflowEdge(
  source: NodeId,
  target: NodeId,
  label: string | null = null,
  condition: string | null = null,
): WorkflowEdge {
  return {
    id: generateId(),
    source,
    target,
    label,
    condition,
    source_handle: null,
    target_handle: null,
    style: defaultEdgeStyle(),
  };
}

export function emptyWorkflowSettings(): WorkflowSettings {
  return {
    async_continuations: false,
    exclusive_gateway_default: true,
    history_level: 'full',
    is_executable: true,
    retry_config: null,
    timeout: null,
    variables_serialization: 'json',
  };
}

export function newWorkflow(name: string): Workflow {
  const now = new Date().toISOString();
  return {
    id: generateId(),
    name,
    version: '1.0.0',
    description: null,
    nodes: [],
    edges: [],
    variables: [],
    signals: [],
    timers: [],
    settings: emptyWorkflowSettings(),
    created_at: now,
    updated_at: now,
  };
}

/* ------------------------------------------------------------------ *
 * React Flow interop
 * ------------------------------------------------------------------ */

export interface FlowNodeData extends Record<string, unknown> {
  label: string;
  nodeType: WorkflowNodeType['type'];
  color: string;
}

export interface FlowNode {
  id: string;
  type: string;
  position: { x: number; y: number };
  data: FlowNodeData;
  draggable: boolean;
}

export interface FlowEdge {
  id: string;
  source: string;
  target: string;
  label?: string;
  animated?: boolean;
}

export function workflowToFlowNodes(nodes: WorkflowNode[]): FlowNode[] {
  return nodes.map((node) => {
    const item = paletteItem(node.node_type.type);
    return {
      id: node.id,
      type: 'process',
      position: { x: node.position.x, y: node.position.y },
      data: {
        label: node.name,
        nodeType: node.node_type.type,
        color: item?.color ?? '#64748b',
      },
      draggable: true,
    };
  });
}

export function workflowToFlowEdges(edges: WorkflowEdge[]): FlowEdge[] {
  return edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    label: edge.label ?? undefined,
    animated: edge.style.animated,
  }));
}

export interface NodePositionPatch {
  x: number;
  y: number;
}

/** Patch applied to one workflow node by the inspector. */
export interface WorkflowNodePatch {
  name?: string;
  documentation?: string | null;
  node_type?: WorkflowNodeType;
  config?: NodeConfig;
}

/** Patch applied to one workflow edge (gateway flows). */
export interface WorkflowEdgePatch {
  label?: string | null;
  condition?: string | null;
  style?: EdgeStyle;
}

/** Patch applied to workflow metadata. */
export interface WorkflowMetaPatch {
  name?: string;
  version?: string;
  description?: string | null;
  settings?: WorkflowSettings;
}

/** Round-trip guard: a stored record must look like a workflow before edits. */
export function toWorkflow(value: unknown): Workflow | null {
  if (!value || typeof value !== 'object') return null;
  const workflow = value as Workflow;
  return Array.isArray(workflow.nodes) && Array.isArray(workflow.edges) ? workflow : null;
}