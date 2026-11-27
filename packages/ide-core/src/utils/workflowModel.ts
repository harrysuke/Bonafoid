/**
 * Process builder model (M7 #4).
 *
 * Maps the generated Rust `Workflow` contract onto React Flow's node/edge
 * graph and back. Positions live on `WorkflowNode.position`, so canvas state
 * round-trips through project JSON without a separate layout file.
 */

import type {
  AgentNodeConfig,
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

/**
 * `events`/`tasks`/`gateways`/`composite` are the BPMN 2.0 groups. `agent`
 * collects the nine agent kinds, which all create the same `AgentTask` node
 * type and differ only in their payload — one group, because splitting it
 * further is a canvas presentation choice (Phase 40), not a model one.
 */
export type NodePaletteCategory = 'events' | 'tasks' | 'gateways' | 'composite' | 'agent';

/** Which executor an `AgentTask` runs. Mirrors `AgentNodeKind` in Rust. */
export type AgentNodeKind = AgentNodeConfig['kind'];

export interface NodePaletteItem {
  type: WorkflowNodeType['type'];
  category: NodePaletteCategory;
  label: string;
  /** Tailwind color hints used by the canvas nodes and the minimap. */
  color: string;
  /**
   * Present only on the agent items. Nine palette entries share one node type,
   * so this is what distinguishes them and what a stored node is matched back
   * against.
   */
  agentKind?: AgentNodeKind;
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
  { type: 'AgentTask', category: 'agent', label: 'LLM call', color: '#7c3aed', agentKind: 'llm' },
  { type: 'AgentTask', category: 'agent', label: 'Shell command', color: '#0f766e', agentKind: 'shell' },
  { type: 'AgentTask', category: 'agent', label: 'File operation', color: '#2563eb', agentKind: 'file' },
  { type: 'AgentTask', category: 'agent', label: 'Git operation', color: '#c2410c', agentKind: 'git' },
  { type: 'AgentTask', category: 'agent', label: 'HTTP request', color: '#0891b2', agentKind: 'http' },
  { type: 'AgentTask', category: 'agent', label: 'SQLite query', color: '#4338ca', agentKind: 'sqlite' },
  { type: 'AgentTask', category: 'agent', label: 'Memory', color: '#be123c', agentKind: 'memory' },
  { type: 'AgentTask', category: 'agent', label: 'Approval', color: '#ca8a04', agentKind: 'approval' },
  { type: 'AgentTask', category: 'agent', label: 'Sub-workflow', color: '#15803d', agentKind: 'subflow' },
];

export const NODE_PALETTE_CATEGORIES: NodePaletteCategory[] = [
  'events',
  'tasks',
  'gateways',
  'composite',
  'agent',
];

export function paletteItem(type: WorkflowNodeType['type']): NodePaletteItem | undefined {
  return NODE_PALETTE.find((item) => item.type === type);
}

/** The palette entry an existing node came from, agent kinds included. */
export function paletteItemForNode(node: WorkflowNode): NodePaletteItem | undefined {
  const kind = node.config?.agent?.kind;
  if (kind) {
    return NODE_PALETTE.find((item) => item.type === 'AgentTask' && item.agentKind === kind);
  }
  return paletteItem(node.node_type.type);
}

export function paletteItemForAgentKind(kind: AgentNodeKind): NodePaletteItem | undefined {
  return NODE_PALETTE.find((item) => item.agentKind === kind);
}

/**
 * A starter payload for each agent kind.
 *
 * These mirror the serde defaults in `crates/model/src/agent.rs`. They are
 * duplicated here because the canvas must build a complete `AgentNodeConfig`
 * before it has ever spoken to a server, and a partial one would not typecheck.
 * If a Rust default changes, this table has to change with it — the palette
 * test pins the shape so a missing field is a compile error, not a blank form.
 */
export function buildAgentConfig(kind: AgentNodeKind): AgentNodeConfig {
  switch (kind) {
    case 'llm':
      return {
        kind,
        config: {
          provider: null,
          model: null,
          system: null,
          prompt: '',
          tools: [],
          output_schema: null,
          temperature: null,
          max_tokens: null,
          max_turns: 8,
          stream: false,
        },
      };
    case 'shell':
      return {
        kind,
        config: {
          program: '',
          args: [],
          cwd: null,
          env: {},
          timeout_ms: 600_000,
          mode: 'argv',
          stdin: null,
        },
      };
    case 'file':
      return { kind, config: { op: 'read', path: '', content: null, glob: null } };
    case 'git':
      return { kind, config: { op: 'status', args: [], message: null } };
    case 'http':
      return {
        kind,
        config: {
          url: '',
          method: 'GET',
          headers: {},
          body: null,
          timeout_ms: 30_000,
          expect: [],
        },
      };
    case 'sqlite':
      return { kind, config: { path: '', query: '', params: [], mode: 'read' } };
    case 'memory':
      return {
        kind,
        config: {
          store: null,
          op: 'get',
          namespace: 'global',
          key: null,
          query: null,
          top_k: null,
          ttl: null,
        },
      };
    case 'approval':
      return {
        kind,
        config: { title: 'Approval required', body: null, timeout: null, on_timeout: 'fail' },
      };
    case 'subflow':
      return {
        kind,
        config: {
          workflow_id: '00000000-0000-0000-0000-000000000000',
          input_mapping: {},
          output_mapping: {},
        },
      };
  }
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
    case 'AgentTask':
      // The node type carries no payload; the config is built by
      // `buildAgentConfig` and attached to `NodeConfig.agent`.
      return { type };
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
  agentKind?: AgentNodeKind,
): WorkflowNode {
  const item = agentKind ? paletteItemForAgentKind(agentKind) : paletteItem(type);
  const config = emptyNodeConfig();
  if (type === 'AgentTask') {
    // An AgentTask with no payload is invalid, so the palette always supplies
    // one; the default is the first agent entry (an LLM call).
    config.agent = buildAgentConfig(agentKind ?? 'llm');
  }
  return {
    id: generateId(),
    name: item?.label ?? type,
    node_type: buildNodeType(type),
    position,
    config,
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
    const item = paletteItemForNode(node);
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