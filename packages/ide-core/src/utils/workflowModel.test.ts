/**
 * Tests for the process builder model.
 *
 * The React Flow canvas is the editor's only view of a workflow, so the
 * conversions in both directions and the palette contract are what keep a
 * dragged node from becoming a corrupt definition.
 */

import { describe, expect, it } from 'vitest';
import {
  NODE_PALETTE,
  NODE_PALETTE_CATEGORIES,
  buildNodeType,
  newWorkflow,
  newWorkflowEdge,
  newWorkflowNode,
  paletteItem,
  toWorkflow,
  workflowToFlowEdges,
  workflowToFlowNodes,
} from './workflowModel';

describe('node palette', () => {
  it('gives every entry a known category', () => {
    for (const item of NODE_PALETTE) {
      expect(NODE_PALETTE_CATEGORIES, item.type).toContain(item.category);
    }
  });

  it('has no duplicate node types', () => {
    const types = NODE_PALETTE.map((item) => item.type);
    expect(new Set(types).size).toBe(types.length);
  });

  it('builds a valid node type for every palette entry', () => {
    for (const item of NODE_PALETTE) {
      const built = buildNodeType(item.type);
      expect(built.type, item.type).toBe(item.type);
    }
  });

  it('supplies the config the model requires for configured node types', () => {
    // Not every variant has a config: ManualTask is a bare `{ type }` in the
    // Rust model. These ones do, and the engine reads them.
    for (const type of ['StartEvent', 'EndEvent', 'UserTask', 'ServiceTask', 'ExclusiveGateway'] as const) {
      const built = buildNodeType(type) as { config?: unknown };
      expect(built.config, type).toBeTruthy();
    }
  });

  it('resolves a palette item by type', () => {
    expect(paletteItem('StartEvent')).toBeTruthy();
    expect(paletteItem('NotARealNode' as never)).toBeUndefined();
  });
});

describe('newWorkflow', () => {
  it('creates an executable, empty workflow', () => {
    const workflow = newWorkflow('Approval');
    expect(workflow.name).toBe('Approval');
    expect(workflow.nodes).toEqual([]);
    expect(workflow.edges).toEqual([]);
    expect(workflow.settings.is_executable).toBe(true);
  });
});

describe('newWorkflowNode', () => {
  it('places the node and names it after the palette entry', () => {
    const node = newWorkflowNode('UserTask', { x: 120, y: 40 });
    expect(node.position).toEqual({ x: 120, y: 40 });
    expect(node.node_type.type).toBe('UserTask');
    expect(node.name).toBeTruthy();
  });

  it('defaults to the origin', () => {
    expect(newWorkflowNode('UserTask').position).toEqual({ x: 0, y: 0 });
  });

  it('gives each node its own id', () => {
    expect(newWorkflowNode('UserTask').id).not.toBe(newWorkflowNode('UserTask').id);
  });
});

describe('newWorkflowEdge', () => {
  it('connects two nodes', () => {
    const edge = newWorkflowEdge('a', 'b');
    expect(edge.source).toBe('a');
    expect(edge.target).toBe('b');
    expect(edge.label).toBeNull();
    expect(edge.style.animated).toBe(false);
  });

  it('carries a label and condition when given', () => {
    const edge = newWorkflowEdge('a', 'b', 'approved', 'x == 1');
    expect(edge.label).toBe('approved');
    expect(edge.condition).toBe('x == 1');
  });

  it('gives each edge its own id', () => {
    expect(newWorkflowEdge('a', 'b').id).not.toBe(newWorkflowEdge('a', 'b').id);
  });
});

describe('flow conversion', () => {
  it('maps workflow nodes onto canvas nodes', () => {
    const nodes = [
      newWorkflowNode('StartEvent', { x: 10, y: 20 }),
      newWorkflowNode('UserTask', { x: 200, y: 20 }),
    ];
    const flow = workflowToFlowNodes(nodes);

    expect(flow).toHaveLength(2);
    expect(flow[0]).toMatchObject({
      id: nodes[0].id,
      position: { x: 10, y: 20 },
    });
    expect(flow[0].data.label).toBe(nodes[0].name);
    expect(flow[0].data.nodeType).toBe('StartEvent');
    expect(flow[0].data.color).toMatch(/^#/);
  });

  it('falls back to a neutral colour for an unknown node type', () => {
    const node = newWorkflowNode('UserTask');
    const flow = workflowToFlowNodes([{ ...node, node_type: { type: 'ManualTask' } }]);
    expect(flow[0].data.color).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it('maps workflow edges onto canvas edges', () => {
    const edge = newWorkflowEdge('a', 'b', 'yes');
    const flow = workflowToFlowEdges([edge]);
    expect(flow[0]).toMatchObject({ id: edge.id, source: 'a', target: 'b', label: 'yes' });
  });

  it('drops a null label rather than emitting undefined keys', () => {
    const flow = workflowToFlowEdges([newWorkflowEdge('a', 'b')]);
    expect(flow[0].label).toBeUndefined();
  });
});

describe('toWorkflow', () => {
  it('accepts a stored workflow', () => {
    const workflow = newWorkflow('A');
    expect(toWorkflow(workflow)).toBe(workflow);
  });

  it('rejects records that are not workflows', () => {
    expect(toWorkflow(null)).toBeNull();
    expect(toWorkflow(undefined)).toBeNull();
    expect(toWorkflow('a string')).toBeNull();
    expect(toWorkflow({})).toBeNull();
    expect(toWorkflow({ nodes: [], edges: 'nope' })).toBeNull();
  });
});