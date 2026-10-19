import { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, CheckCircle2, Download, FileDown, GitBranch, Play, Square, Trash2, Upload } from 'lucide-react';
import {
  Background,
  ConnectionLineType,
  Controls,
  Handle,
  MiniMap,
  Position,
  ReactFlow,
  useEdgesState,
  useNodesState,
} from 'reactflow';
import type { Connection, Node, NodeProps } from 'reactflow';
import 'reactflow/dist/style.css';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { getPlatform } from '../platform/registry';
import { useProjectStore } from '../stores/projectStore';
import {
  NODE_PALETTE,
  NODE_PALETTE_CATEGORIES,
  toWorkflow,
  workflowToFlowEdges,
  workflowToFlowNodes,
} from '../utils/workflowModel';
import type { FlowNodeData, NodePaletteCategory } from '../utils/workflowModel';
import type { Workflow, WorkflowNodeType } from '../types/model.generated';
import type { BpmnValidationReport, WorkflowRunSummary } from '../platform/types';

function ProcessNode({ data }: NodeProps<FlowNodeData>) {
  return (
    <div
      className="min-w-[9rem] rounded-lg border-2 bg-white px-3 py-2 text-center shadow-sm dark:bg-slate-800"
      style={{ borderColor: data.color }}
    >
      <Handle type="target" position={Position.Top} />
      <p className="text-xs font-medium text-gray-800 dark:text-gray-100">{data.label}</p>
      <p className="text-[10px] uppercase tracking-wide" style={{ color: data.color }}>
        {data.nodeType}
      </p>
      <Handle type="source" position={Position.Bottom} />
    </div>
  );
}

const nodeTypes = { process: ProcessNode };

type JsonDraft = { value: string; error: string | null } | null;

export function WorkflowBuilder() {
  const project = useProjectStore((s) => s.project);
  const addWorkflow = useProjectStore((s) => s.addWorkflow);
  const removeWorkflow = useProjectStore((s) => s.removeWorkflow);
  const updateWorkflowMeta = useProjectStore((s) => s.updateWorkflowMeta);
  const addWorkflowNode = useProjectStore((s) => s.addWorkflowNode);
  const updateWorkflowNode = useProjectStore((s) => s.updateWorkflowNode);
  const moveWorkflowNode = useProjectStore((s) => s.moveWorkflowNode);
  const removeWorkflowNode = useProjectStore((s) => s.removeWorkflowNode);
  const connectWorkflowNodes = useProjectStore((s) => s.connectWorkflowNodes);
  const updateWorkflowEdge = useProjectStore((s) => s.updateWorkflowEdge);
  const removeWorkflowEdge = useProjectStore((s) => s.removeWorkflowEdge);
  const importWorkflow = useProjectStore((s) => s.importWorkflow);

  const [selectedWorkflowId, setSelectedWorkflowId] = useState('');
  const [selectedNodeId, setSelectedNodeId] = useState('');
  const [selectedEdgeId, setSelectedEdgeId] = useState('');
  const [newWorkflowName, setNewWorkflowName] = useState('');
  const [nodeTypeDraft, setNodeTypeDraft] = useState<JsonDraft>(null);
  const [nodeConfigDraft, setNodeConfigDraft] = useState<JsonDraft>(null);
  const [bpmnMode, setBpmnMode] = useState<'idle' | 'import' | 'export'>('idle');
  const [bpmnXml, setBpmnXml] = useState('');
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<BpmnValidationReport | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [runSummary, setRunSummary] = useState<WorkflowRunSummary | null>(null);
  const [variablesDraft, setVariablesDraft] = useState('{}');

  const workflows = useMemo(() => (project ? Object.values(project.workflows) : []), [project]);
  const workflow = useMemo<Workflow | null>(() => {
    const found = workflows.find((candidate) => candidate.id === selectedWorkflowId) ?? workflows[0];
    return toWorkflow(found);
  }, [workflows, selectedWorkflowId]);

  useEffect(() => {
    if (workflow && workflow.id !== selectedWorkflowId) setSelectedWorkflowId(workflow.id);
  }, [workflow, selectedWorkflowId]);

  useEffect(() => {
    setNodeTypeDraft(null);
    setNodeConfigDraft(null);
  }, [selectedNodeId, selectedWorkflowId]);

  const nodesFromStore = useMemo(
    () => workflowToFlowNodes(workflow?.nodes ?? []),
    [workflow],
  );
  const edgesFromStore = useMemo(
    () => workflowToFlowEdges(workflow?.edges ?? []),
    [workflow],
  );

  const [nodes, setNodes, onNodesChange] = useNodesState<FlowNodeData>(nodesFromStore);
  const [edges, setEdges, onEdgesChange] = useEdgesState(edgesFromStore);

  // Push store graph into React Flow whenever the store changes (other tabs,
  // imports, node inspector edits). Drag positions are committed on drag stop.
  useEffect(() => {
    setNodes(nodesFromStore);
    setEdges(edgesFromStore);
  }, [nodesFromStore, edgesFromStore, setNodes, setEdges]);

  const onNodeDragStop = useCallback(
    (_event: React.MouseEvent, node: Node<FlowNodeData>) => {
      if (!workflow) return;
      moveWorkflowNode(workflow.id, node.id, Math.round(node.position.x), Math.round(node.position.y));
    },
    [moveWorkflowNode, workflow],
  );

  const handleConnect = useCallback(
    (connection: Connection) => {
      if (!workflow || !connection.source || !connection.target) return;
      connectWorkflowNodes(workflow.id, connection.source, connection.target);
    },
    [connectWorkflowNodes, workflow],
  );

  const selectedNode = workflow?.nodes.find((node) => node.id === selectedNodeId) ?? null;
  const selectedEdge = workflow?.edges.find((edge) => edge.id === selectedEdgeId) ?? null;

  const handleAddNode = (type: WorkflowNodeType['type']) => {
    if (!workflow) return;
    // Deterministic offset keeps repeated palette clicks readable.
    const offset = workflow.nodes.length * 40;
    const id = addWorkflowNode(workflow.id, type, 80 + offset, 60 + offset);
    if (id) setSelectedNodeId(id);
  };

  const commitNodeType = () => {
    if (!workflow || !selectedNode || !nodeTypeDraft) return;
    try {
      const parsed = JSON.parse(nodeTypeDraft.value) as WorkflowNodeType;
      updateWorkflowNode(workflow.id, selectedNode.id, { node_type: parsed });
      setNodeTypeDraft(null);
    } catch {
      setNodeTypeDraft({ value: nodeTypeDraft.value, error: 'Invalid JSON' });
    }
  };

  const commitNodeConfig = () => {
    if (!workflow || !selectedNode || !nodeConfigDraft) return;
    try {
      const parsed = JSON.parse(nodeConfigDraft.value) as Workflow['nodes'][number]['config'];
      updateWorkflowNode(workflow.id, selectedNode.id, { config: parsed });
      setNodeConfigDraft(null);
    } catch {
      setNodeConfigDraft({ value: nodeConfigDraft.value, error: 'Invalid JSON' });
    }
  };

  const runValidate = async () => {
    if (!workflow) return;
    setBusy(true);
    setMessage(null);
    try {
      const result = await getPlatform().bpmnValidate(JSON.stringify(workflow));
      setReport(result);
      setMessage(
        result.valid
          ? `Valid: ${workflow.nodes.length} node(s), ${workflow.edges.length} flow(s).`
          : `Invalid: ${result.issues.length} issue(s).`,
      );
    } catch (error) {
      setReport(null);
      setMessage(`Validation failed: ${describe(error)}`);
    } finally {
      setBusy(false);
    }
  };

  /** Execute the workflow with the token engine. */
  const runWorkflow = async (autoComplete: boolean) => {
    if (!workflow) return;
    const platform = getPlatform();
    if (!platform.workflowRun) {
      setMessage('This host cannot run workflows.');
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      // Variables come from the editor-shaped JSON field; a parse error is
      // reported instead of silently running with none.
      let variables: Record<string, unknown> = {};
      if (variablesDraft.trim()) {
        try {
          variables = JSON.parse(variablesDraft) as Record<string, unknown>;
        } catch {
          setMessage('Variables must be a JSON object.');
          setBusy(false);
          return;
        }
      }
      const summary = await platform.workflowRun(JSON.stringify(workflow), variables, autoComplete);
      setRunSummary(summary);
      setMessage(
        `${summary.completed ? 'Completed' : 'Running'} · ${summary.steps.length} node(s) visited, ${summary.open_tasks.length} open task(s).`,
      );
    } catch (error) {
      setRunSummary(null);
      setMessage(`Run failed: ${describe(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const runExport = async () => {
    if (!workflow) return;
    setBusy(true);
    setMessage(null);
    try {
      const xml = await getPlatform().bpmnExport(JSON.stringify(workflow));
      setBpmnXml(xml);
      setBpmnMode('export');
    } catch (error) {
      setMessage(`Export failed: ${describe(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const runImport = async () => {
    if (!bpmnXml.trim()) return;
    setBusy(true);
    setMessage(null);
    try {
      const json = await getPlatform().bpmnImport(bpmnXml, newWorkflowName || 'Imported Process');
      const id = importWorkflow(json);
      if (!id) {
        setMessage('Import produced an unusable workflow.');
        return;
      }
      setSelectedWorkflowId(id);
      setBpmnMode('idle');
      setBpmnXml('');
    } catch (error) {
      setMessage(`Import failed: ${describe(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full flex flex-col">
      <div className="flex flex-wrap items-center justify-between gap-2 p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-3 min-w-0">
          <GitBranch className="w-5 h-5 text-primary-600" />
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white truncate">
            {workflow?.name ?? 'Process Builder'}
          </h2>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="secondary" size="sm" onClick={() => { setReport(null); void runValidate(); }} disabled={!workflow || busy}>
            <CheckCircle2 className="w-4 h-4 mr-1" /> Validate
          </Button>
          <Button variant="primary" size="sm" onClick={() => void runWorkflow(false)} disabled={!workflow || busy}>
            <Play className="w-4 h-4 mr-1" /> Run
          </Button>
          <Button variant="secondary" size="sm" onClick={() => void runWorkflow(true)} disabled={!workflow || busy}>
            <Square className="w-4 h-4 mr-1" /> Run to end
          </Button>
          <Button variant="secondary" size="sm" onClick={() => { setBpmnXml(''); setBpmnMode('import'); }} disabled={!workflow || busy}>
            <Upload className="w-4 h-4 mr-1" /> Import BPMN
          </Button>
          <Button variant="secondary" size="sm" onClick={() => void runExport()} disabled={!workflow || busy}>
            <Download className="w-4 h-4 mr-1" /> Export BPMN
          </Button>
        </div>
      </div>

      <div className="px-4 py-2 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <label className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400">
          Run variables (JSON object)
          <input
            value={variablesDraft}
            onChange={(event) => setVariablesDraft(event.target.value)}
            spellCheck={false}
            className="flex-1 rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-900 px-2 py-1 font-mono text-xs text-gray-900 dark:text-white"
          />
        </label>
      </div>

      {message && (
        <div className="px-4 py-2 text-sm border-b border-gray-200 dark:border-slate-700 bg-amber-50 text-amber-800 dark:bg-amber-900/30 dark:text-amber-200">
          {message}
        </div>
      )}

      {report && !report.valid && (
        <ul className="px-4 py-2 space-y-1 text-xs border-b border-gray-200 dark:border-slate-700 bg-red-50 text-red-700 dark:bg-red-900/30 dark:text-red-200">
          {report.issues.map((issue, index) => (
            <li key={index}>
              [{issue.severity}] {issue.node_id ? `${issue.node_id}: ` : ''}
              {issue.message}
            </li>
          ))}
        </ul>
      )}
      {report?.valid && (
        <div className="px-4 py-2 flex items-center gap-2 text-sm border-b border-gray-200 dark:border-slate-700 bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-200">
          <CheckCircle2 className="w-4 h-4" /> Workflow is valid.
        </div>
      )}

      {runSummary && (
        <div className="px-4 py-3 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 space-y-2">
          <div className="flex flex-wrap items-center gap-3 text-sm">
            <span className="font-medium text-gray-900 dark:text-white">
              Run {runSummary.completed ? 'completed' : 'is running'}
            </span>
            <span className="text-xs font-mono text-gray-400 dark:text-gray-500">
              instance {runSummary.instance_id.slice(0, 8)} · status {runSummary.status}
            </span>
            {runSummary.auto_completed_tasks && (
              <span className="text-xs text-amber-700 dark:text-amber-300">
                user tasks were auto-completed (simulation)
              </span>
            )}
          </div>

          {runSummary.open_tasks.length > 0 && (
            <p className="text-xs text-gray-600 dark:text-gray-300">
              Waiting on:{' '}
              {runSummary.open_tasks.map((task) => task.node_name).join(', ')}
            </p>
          )}

          {runSummary.steps.length > 0 && (
            <ol className="flex flex-wrap items-center gap-1 text-xs">
              {runSummary.steps.map((step, index) => (
                <li key={step.node_id} className="flex items-center gap-1">
                  {index > 0 && <span className="text-gray-300 dark:text-gray-600">→</span>}
                  <span
                    className={`rounded px-1.5 py-0.5 ${
                      step.outcome === 'open'
                        ? 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-200'
                        : step.outcome === 'waiting'
                          ? 'bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-200'
                          : 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-200'
                    }`}
                    title={`${step.node_type} · ${step.outcome}`}
                  >
                    {step.node_name}
                  </span>
                </li>
              ))}
            </ol>
          )}

          <details>
            <summary className="cursor-pointer text-xs text-gray-500 dark:text-gray-400">
              Variables + history ({runSummary.events.length} events)
            </summary>
            <pre className="mt-2 max-h-40 overflow-auto rounded bg-gray-50 dark:bg-slate-900 p-2 text-[11px] font-mono text-gray-700 dark:text-gray-300">
              {JSON.stringify(
                { variables: runSummary.variables, events: runSummary.events },
                null,
                2,
              )}
            </pre>
          </details>
        </div>
      )}

      <div className="flex-1 flex overflow-hidden">
        {/* Palette + process list */}
        <div className="w-60 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-3 overflow-y-auto flex-shrink-0">
          <h3 className="font-semibold text-sm text-gray-900 dark:text-white mb-2">Processes</h3>
          <ul className="space-y-1 mb-4">
            {workflows.map((candidate) => (
              <li key={candidate.id} className="flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => setSelectedWorkflowId(candidate.id)}
                  className={`flex-1 text-left px-2 py-1 rounded text-sm truncate ${
                    workflow?.id === candidate.id
                      ? 'bg-primary-50 text-primary-700 dark:bg-primary-900/30 dark:text-primary-300'
                      : 'text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-slate-700/50'
                  }`}
                >
                  {candidate.name}
                </button>
                <button
                  type="button"
                  aria-label={`Delete ${candidate.name}`}
                  onClick={() => removeWorkflow(candidate.id)}
                  className="p-1 text-gray-400 hover:text-red-600"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </li>
            ))}
          </ul>
          <div className="flex gap-1 mb-5">
            <Input
              value={newWorkflowName}
              placeholder="New process"
              onChange={(event) => setNewWorkflowName(event.target.value)}
            />
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                const id = addWorkflow(newWorkflowName || 'Untitled Process');
                if (id) setSelectedWorkflowId(id);
                setNewWorkflowName('');
              }}
            >
              Add
            </Button>
          </div>

          <h3 className="font-semibold text-sm text-gray-900 dark:text-white mb-2">Elements</h3>
          {NODE_PALETTE_CATEGORIES.map((category: NodePaletteCategory) => (
            <div key={category} className="mb-3">
              <h4 className="text-[10px] font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1">
                {category}
              </h4>
              <div className="space-y-1">
                {NODE_PALETTE.filter((item) => item.category === category).map((item) => (
                  <button
                    key={item.type}
                    type="button"
                    disabled={!workflow}
                    onClick={() => handleAddNode(item.type)}
                    className="w-full flex items-center gap-2 px-2 py-1 rounded border border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 text-xs text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700/50 disabled:opacity-50"
                  >
                    <span className="w-2.5 h-2.5 rounded-full" style={{ backgroundColor: item.color }} />
                    {item.label}
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* Canvas */}
        <div className="flex-1 relative">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onNodeDragStop={onNodeDragStop}
            onConnect={handleConnect}
            onNodeClick={(_event, node) => {
              setSelectedNodeId(node.id);
              setSelectedEdgeId('');
            }}
            onEdgeClick={(_event, edge) => {
              setSelectedEdgeId(edge.id);
              setSelectedNodeId('');
            }}
            onPaneClick={() => {
              setSelectedNodeId('');
              setSelectedEdgeId('');
            }}
            connectionLineType={ConnectionLineType.SmoothStep}
            fitView
          >
            <Background color="#e5e7eb" gap={16} />
            <Controls />
            <MiniMap
              nodeColor={(node) => (node.data as FlowNodeData | undefined)?.color ?? '#64748b'}
            />
          </ReactFlow>
        </div>

        {/* Inspector */}
        <div className="w-80 border-l border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto flex-shrink-0">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Properties</h3>

          {workflow && (
            <div className="space-y-3 mb-6 pb-5 border-b border-gray-200 dark:border-slate-700">
              <Input
                label="Name"
                value={workflow.name}
                onChange={(event) => updateWorkflowMeta(workflow.id, { name: event.target.value })}
              />
              <Input
                label="Version"
                value={workflow.version}
                onChange={(event) => updateWorkflowMeta(workflow.id, { version: event.target.value })}
              />
              <div>
                <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                  Description
                </label>
                <textarea
                  className="w-full h-20 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-900 p-2 text-xs text-gray-800 dark:text-gray-200"
                  value={workflow.description ?? ''}
                  onChange={(event) => updateWorkflowMeta(workflow.id, { description: event.target.value })}
                />
              </div>
              <p className="text-[11px] text-gray-400 dark:text-gray-500">
                {workflow.nodes.length} node(s), {workflow.edges.length} flow(s), history{' '}
                {workflow.settings.history_level}
              </p>
            </div>
          )}

          {selectedNode && workflow && (
            <Card className="p-3 space-y-3">
              <h4 className="text-sm font-semibold text-gray-900 dark:text-white">
                {selectedNode.node_type.type}
              </h4>
              <Input
                label="Node name"
                value={selectedNode.name}
                onChange={(event) =>
                  updateWorkflowNode(workflow.id, selectedNode.id, { name: event.target.value })
                }
              />
              <div>
                <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                  Node type (JSON)
                </label>
                <textarea
                  className="w-full h-32 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-900 p-2 font-mono text-[11px] text-gray-800 dark:text-gray-200"
                  value={nodeTypeDraft?.value ?? JSON.stringify(selectedNode.node_type, null, 2)}
                  onChange={(event) => setNodeTypeDraft({ value: event.target.value, error: null })}
                  onBlur={commitNodeType}
                />
                {nodeTypeDraft?.error && (
                  <p className="mt-1 text-[11px] text-red-600">{nodeTypeDraft.error}</p>
                )}
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                  Node config (JSON)
                </label>
                <textarea
                  className="w-full h-32 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-900 p-2 font-mono text-[11px] text-gray-800 dark:text-gray-200"
                  value={nodeConfigDraft?.value ?? JSON.stringify(selectedNode.config, null, 2)}
                  onChange={(event) => setNodeConfigDraft({ value: event.target.value, error: null })}
                  onBlur={commitNodeConfig}
                />
                {nodeConfigDraft?.error && (
                  <p className="mt-1 text-[11px] text-red-600">{nodeConfigDraft.error}</p>
                )}
              </div>
              <Button
                variant="danger"
                size="sm"
                onClick={() => {
                  removeWorkflowNode(workflow.id, selectedNode.id);
                  setSelectedNodeId('');
                }}
              >
                <Trash2 className="w-4 h-4 mr-1" /> Delete node
              </Button>
            </Card>
          )}

          {selectedEdge && workflow && (
            <Card className="p-3 space-y-3">
              <h4 className="text-sm font-semibold text-gray-900 dark:text-white">Flow</h4>
              <Input
                label="Label"
                value={selectedEdge.label ?? ''}
                onChange={(event) =>
                  updateWorkflowEdge(workflow.id, selectedEdge.id, { label: event.target.value })
                }
              />
              <Input
                label="Condition"
                value={selectedEdge.condition ?? ''}
                placeholder="${approved == true}"
                onChange={(event) =>
                  updateWorkflowEdge(workflow.id, selectedEdge.id, { condition: event.target.value })
                }
              />
              <Button
                variant="danger"
                size="sm"
                onClick={() => {
                  removeWorkflowEdge(workflow.id, selectedEdge.id);
                  setSelectedEdgeId('');
                }}
              >
                <Trash2 className="w-4 h-4 mr-1" /> Delete flow
              </Button>
            </Card>
          )}

          {!selectedNode && !selectedEdge && (
            <p className="text-sm text-gray-500 dark:text-gray-400 py-4">
              Select a node or a flow on the canvas.
            </p>
          )}
        </div>
      </div>

      {bpmnMode !== 'idle' && (
        <div className="border-t border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 p-4 space-y-2">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-white">
              {bpmnMode === 'import' ? 'Paste BPMN 2.0 XML' : 'BPMN 2.0 export'}
            </h3>
            <Button variant="ghost" size="sm" onClick={() => setBpmnMode('idle')}>
              Close
            </Button>
          </div>
          {bpmnMode === 'import' && (
            <Input
              value={newWorkflowName}
              placeholder="Process name"
              onChange={(event) => setNewWorkflowName(event.target.value)}
            />
          )}
          <textarea
            className="w-full h-40 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-900 p-2 font-mono text-[11px] text-gray-800 dark:text-gray-200"
            value={bpmnXml}
            readOnly={bpmnMode === 'export'}
            placeholder="<definitions xmlns=...>"
            onChange={(event) => setBpmnXml(event.target.value)}
          />
          {bpmnMode === 'import' ? (
            <div className="flex gap-2">
              <Button variant="primary" size="sm" onClick={() => void runImport()} disabled={busy}>
                <Upload className="w-4 h-4 mr-1" /> Import process
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  const blob = new Blob([bpmnXml], { type: 'text/xml' });
                  const url = URL.createObjectURL(blob);
                  const anchor = document.createElement('a');
                  anchor.href = url;
                  anchor.download = `${workflow?.name ?? 'process'}.bpmn`;
                  anchor.click();
                  URL.revokeObjectURL(url);
                }}
                disabled={!bpmnXml}
              >
                <FileDown className="w-4 h-4 mr-1" /> Download .bpmn
              </Button>
            </div>
          ) : (
            <div className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400">
              <AlertTriangle className="w-4 h-4" /> Round-trips through nocode-bpmn on the host.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function describe(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}