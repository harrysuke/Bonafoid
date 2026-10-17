import { useEffect, useMemo, useState } from 'react';
import {
  Copy,
  FileText,
  Layers,
  Monitor,
  Plus,
  Smartphone,
  Tablet,
  Trash2,
  ArrowUp,
  ArrowDown,
} from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { useProjectStore } from '../stores/projectStore';
import {
  PALETTE,
  PALETTE_CATEGORIES,
  newDataBinding,
  newDataSourceBinding,
  newPropTarget,
} from '../utils/pageModel';
import type { PaletteCategory } from '../utils/pageModel';
import type {
  ActionConfig,
  ActionType,
  ComponentType,
  Page as GeneratedPage,
  PageComponent,
} from '../types/model.generated';

type ViewMode = 'desktop' | 'tablet' | 'mobile';

type JsonDraft = { value: string; error: string | null } | null;

/** Action presets limited to the closed `ActionType` snake_case vocabulary. */
const ACTION_PRESETS: {
  key: string;
  label: string;
  type: ActionType;
  config: ActionConfig;
}[] = [
  {
    key: 'navigate',
    label: 'Navigate',
    type: 'navigate',
    config: { type: 'Navigate', config: { path: '/', replace: false } },
  },
  {
    key: 'open_modal',
    label: 'Open modal',
    type: 'open_modal',
    config: { type: 'OpenModal', config: { modal_id: '', props: {} } },
  },
  {
    key: 'close_modal',
    label: 'Close modal',
    type: 'close_modal',
    config: { type: 'CloseModal', config: { modal_id: '' } },
  },
  {
    key: 'submit_form',
    label: 'Submit form',
    type: 'submit_form',
    config: { type: 'SubmitForm', config: { form_id: '', on_success: [], on_error: [], validate: true } },
  },
  {
    key: 'reset_form',
    label: 'Reset form',
    type: 'reset_form',
    config: { type: 'ResetForm', config: { form_id: '' } },
  },
  {
    key: 'call_api',
    label: 'Call API',
    type: 'call_api',
    config: { type: 'CallApi', config: { data_source_id: '', params: {}, on_success: [], on_error: [] } },
  },
  {
    key: 'execute_workflow',
    label: 'Execute workflow',
    type: 'execute_workflow',
    config: {
      type: 'ExecuteWorkflow',
      config: { workflow_id: '', variables: {}, on_complete: [], on_error: [] },
    },
  },
  {
    key: 'set_state',
    label: 'Set state',
    type: 'set_state',
    config: { type: 'SetState', config: { key: '', scope: 'page', value: null } },
  },
  {
    key: 'emit_event',
    label: 'Emit event',
    type: 'emit_event',
    config: { type: 'EmitEvent', config: { event_name: '', payload: null, target: 'window' } },
  },
  {
    key: 'download',
    label: 'Download',
    type: 'download',
    config: { type: 'Download', config: { url: '', filename: '' } },
  },
  {
    key: 'copy_to_clipboard',
    label: 'Copy to clipboard',
    type: 'copy_to_clipboard',
    config: { type: 'CopyToClipboard', config: { text: '' } },
  },
  {
    key: 'print',
    label: 'Print',
    type: 'print',
    config: { type: 'Print' },
  },
];

function formatActionType(type: ActionType): string {
  return typeof type === 'string' ? type : type.custom;
}

function toEditorPage(value: unknown): GeneratedPage | null {
  if (!value || typeof value !== 'object') return null;
  const page = value as GeneratedPage;
  return Array.isArray(page.components) ? page : null;
}

export function PageBuilder() {
  const project = useProjectStore((s) => s.project);
  const addPage = useProjectStore((s) => s.addPage);
  const removePage = useProjectStore((s) => s.removePage);
  const updatePageMeta = useProjectStore((s) => s.updatePageMeta);
  const addPageComponent = useProjectStore((s) => s.addPageComponent);
  const updatePageComponent = useProjectStore((s) => s.updatePageComponent);
  const removePageComponent = useProjectStore((s) => s.removePageComponent);
  const movePageComponent = useProjectStore((s) => s.movePageComponent);
  const duplicatePageComponent = useProjectStore((s) => s.duplicatePageComponent);
  const addPageComponentBinding = useProjectStore((s) => s.addPageComponentBinding);
  const removePageComponentBinding = useProjectStore((s) => s.removePageComponentBinding);
  const addPageAction = useProjectStore((s) => s.addPageAction);
  const removePageAction = useProjectStore((s) => s.removePageAction);

  const [selectedPageId, setSelectedPageId] = useState('');
  const [selectedComponentId, setSelectedComponentId] = useState('');
  const [viewMode, setViewMode] = useState<ViewMode>('desktop');
  const [showAddPage, setShowAddPage] = useState(false);
  const [newPageName, setNewPageName] = useState('');
  const [configDraft, setConfigDraft] = useState<JsonDraft>(null);
  const [propsDraft, setPropsDraft] = useState<JsonDraft>(null);
  const [bindingSourceId, setBindingSourceId] = useState('');
  const [bindingPath, setBindingPath] = useState('');
  const [bindingProp, setBindingProp] = useState('');
  const [actionPreset, setActionPreset] = useState('call_api');

  const pages = useMemo(() => (project ? Object.values(project.pages) : []), [project]);
  const page = useMemo(() => {
    const found = pages.find((candidate) => candidate.id === selectedPageId) ?? pages[0];
    return toEditorPage(found);
  }, [pages, selectedPageId]);

  useEffect(() => {
    if (page && page.id !== selectedPageId) setSelectedPageId(page.id);
  }, [page, selectedPageId]);

  const components: PageComponent[] = page?.components ?? [];
  const selectedComponent = components.find((c) => c.id === selectedComponentId) ?? null;

  useEffect(() => {
    setConfigDraft(null);
    setPropsDraft(null);
    setBindingSourceId('');
    setBindingPath('');
    setBindingProp('');
  }, [selectedComponentId, selectedPageId]);

  const handleCreatePage = () => {
    const name = newPageName.trim();
    if (!name) return;
    const id = addPage(name);
    if (id) {
      setSelectedPageId(id);
      setSelectedComponentId('');
    }
    setNewPageName('');
    setShowAddPage(false);
  };

  const handleAddComponent = (type: ComponentType['type']) => {
    if (!page) return;
    const id = addPageComponent(page.id, type);
    if (id) setSelectedComponentId(id);
  };

  const commitConfig = (draft: JsonDraft, next: string) => {
    if (!page || !selectedComponent) return;
    if (draft) return;
    try {
      const parsed = JSON.parse(next) as ComponentType;
      updatePageComponent(page.id, selectedComponent.id, { component: parsed });
      setConfigDraft(null);
    } catch {
      setConfigDraft({ value: next, error: 'Invalid JSON' });
    }
  };

  const commitProps = (draft: JsonDraft, next: string) => {
    if (!page || !selectedComponent) return;
    if (draft) return;
    try {
      const parsed = JSON.parse(next) as Record<string, unknown>;
      updatePageComponent(page.id, selectedComponent.id, { props: parsed });
      setPropsDraft(null);
    } catch {
      setPropsDraft({ value: next, error: 'Invalid JSON object' });
    }
  };

  const handleAddBinding = () => {
    if (!page || !selectedComponent) return;
    if (!bindingSourceId || !bindingProp) return;
    addPageComponentBinding(
      page.id,
      selectedComponent.id,
      newDataBinding(newDataSourceBinding(bindingSourceId, bindingPath), newPropTarget(bindingProp), true),
    );
    setBindingPath('');
    setBindingProp('');
  };

  const handleAddAction = () => {
    if (!page) return;
    const preset = ACTION_PRESETS.find((candidate) => candidate.key === actionPreset);
    if (!preset) return;
    addPageAction(page.id, preset.label, preset.type, preset.config);
  };

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Page Builder</h2>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" onClick={() => setShowAddPage(true)}>
            <Plus className="w-4 h-4 mr-1" /> New Page
          </Button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Pages */}
        <div className="w-64 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Pages</h3>
          {showAddPage ? (
            <div className="space-y-2 mb-4">
              <Input
                value={newPageName}
                onChange={(event) => setNewPageName(event.target.value)}
                placeholder="Page name"
                autoFocus
              />
              <div className="flex gap-2">
                <Button variant="primary" size="sm" onClick={handleCreatePage}>
                  Create
                </Button>
                <Button variant="ghost" size="sm" onClick={() => setShowAddPage(false)}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : null}

          <ul className="space-y-1">
            {pages.map((candidate) => {
              const active = page?.id === candidate.id;
              return (
                <li key={candidate.id}>
                  <button
                    type="button"
                    onClick={() => setSelectedPageId(candidate.id)}
                    className={`w-full flex items-center gap-2 p-2 rounded-lg text-left text-sm transition-colors ${
                      active
                        ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300'
                        : 'text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-slate-700/50'
                    }`}
                  >
                    <FileText className="w-4 h-4 shrink-0" />
                    <span className="truncate">{candidate.name}</span>
                  </button>
                </li>
              );
            })}
          </ul>

          {page && (
            <div className="mt-6 space-y-2">
              <Input
                label="Name"
                value={page.name}
                onChange={(event) => updatePageMeta(page.id, { name: event.target.value })}
              />
              <Input
                label="Path"
                value={page.path}
                onChange={(event) => updatePageMeta(page.id, { path: event.target.value })}
              />
              <Button
                variant="danger"
                size="sm"
                onClick={() => {
                  removePage(page.id);
                  setSelectedComponentId('');
                }}
              >
                <Trash2 className="w-4 h-4 mr-1" /> Delete page
              </Button>
            </div>
          )}
        </div>

        {/* Canvas */}
        <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
          <div className="max-w-4xl mx-auto">
            <div className="flex items-center justify-between mb-4 p-3 bg-white dark:bg-slate-800 rounded-lg shadow-sm">
              <div className="flex items-center gap-3 min-w-0">
                <h3 className="font-semibold text-gray-900 dark:text-white truncate">
                  {page?.name ?? 'Select a page'}
                </h3>
                {page && <span className="text-xs text-gray-400 truncate">{page.path}</span>}
              </div>
              <div className="flex bg-gray-100 dark:bg-slate-900 rounded-lg p-1">
                <Button variant={viewMode === 'desktop' ? 'secondary' : 'ghost'} size="sm" onClick={() => setViewMode('desktop')}>
                  <Monitor className="w-4 h-4" />
                </Button>
                <Button variant={viewMode === 'tablet' ? 'secondary' : 'ghost'} size="sm" onClick={() => setViewMode('tablet')}>
                  <Tablet className="w-4 h-4" />
                </Button>
                <Button variant={viewMode === 'mobile' ? 'secondary' : 'ghost'} size="sm" onClick={() => setViewMode('mobile')}>
                  <Smartphone className="w-4 h-4" />
                </Button>
              </div>
            </div>

            <div
              className={`bg-white dark:bg-slate-800 rounded-xl shadow-xl overflow-hidden transition-all mx-auto ${
                viewMode === 'mobile' ? 'max-w-sm' : viewMode === 'tablet' ? 'max-w-md' : 'max-w-4xl'
              }`}
            >
              <div className="p-4 min-h-[400px] space-y-2">
                {components.length === 0 ? (
                  <div className="border-2 border-dashed border-gray-300 dark:border-slate-600 rounded-lg h-64 flex flex-col items-center justify-center">
                    <Layers className="w-12 h-12 text-gray-300 dark:text-gray-600 mb-3" />
                    <p className="text-gray-500 dark:text-gray-400 text-sm">
                      Add components from the palette on the right
                    </p>
                  </div>
                ) : (
                  components.map((component, index) => {
                    const active = component.id === selectedComponentId;
                    return (
                      <div
                        key={component.id}
                        onClick={() => setSelectedComponentId(component.id)}
                        className={`flex items-center justify-between p-3 border rounded-lg cursor-pointer transition-colors ${
                          active
                            ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20'
                            : 'border-gray-200 dark:border-slate-700 hover:bg-gray-50 dark:hover:bg-slate-800/60'
                        }`}
                      >
                        <div className="min-w-0">
                          <p className="text-sm font-medium text-gray-800 dark:text-gray-200">
                            {component.component.type}
                          </p>
                          {component.bindings.length > 0 && (
                            <p className="text-xs text-primary-600 dark:text-primary-400">
                              {component.bindings.length} binding(s)
                            </p>
                          )}
                        </div>
                        <div className="flex items-center gap-1" onClick={(event) => event.stopPropagation()}>
                          <Button variant="ghost" size="sm" disabled={index === 0} onClick={() => movePageComponent(page!.id, index, index - 1)}>
                            <ArrowUp className="w-4 h-4" />
                          </Button>
                          <Button variant="ghost" size="sm" disabled={index === components.length - 1} onClick={() => movePageComponent(page!.id, index, index + 1)}>
                            <ArrowDown className="w-4 h-4" />
                          </Button>
                          <Button variant="ghost" size="sm" onClick={() => setSelectedComponentId(duplicatePageComponent(page!.id, component.id))}>
                            <Copy className="w-4 h-4" />
                          </Button>
                          <Button variant="ghost" size="sm" onClick={() => removePageComponent(page!.id, component.id)}>
                            <Trash2 className="w-4 h-4" />
                          </Button>
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </div>

            {selectedComponent && page && (
              <Card className="mt-4 p-4 space-y-4">
                <h4 className="text-sm font-semibold text-gray-900 dark:text-white">
                  {selectedComponent.component.type} configuration
                </h4>
                <div>
                  <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                    Component config (JSON)
                  </label>
                  <textarea
                    className="w-full h-48 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-900 p-2 font-mono text-xs text-gray-800 dark:text-gray-200"
                    value={configDraft?.value ?? JSON.stringify(selectedComponent.component.config, null, 2)}
                    onChange={(event) => setConfigDraft({ value: event.target.value, error: null })}
                    onBlur={(event) => commitConfig(configDraft, event.target.value)}
                  />
                  {configDraft?.error && (
                    <p className="mt-1 text-xs text-red-600 dark:text-red-400">{configDraft.error}</p>
                  )}
                </div>
                <div>
                  <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                    Props (JSON)
                  </label>
                  <textarea
                    className="w-full h-24 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-900 p-2 font-mono text-xs text-gray-800 dark:text-gray-200"
                    value={propsDraft?.value ?? JSON.stringify(selectedComponent.props, null, 2)}
                    onChange={(event) => setPropsDraft({ value: event.target.value, error: null })}
                    onBlur={(event) => commitProps(propsDraft, event.target.value)}
                  />
                  {propsDraft?.error && (
                    <p className="mt-1 text-xs text-red-600 dark:text-red-400">{propsDraft.error}</p>
                  )}
                </div>

                <div className="space-y-2">
                  <h5 className="text-xs font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400">
                    Data bindings
                  </h5>
                  {selectedComponent.bindings.length === 0 ? (
                    <p className="text-xs text-gray-400 dark:text-gray-500">No bindings yet.</p>
                  ) : (
                    <ul className="space-y-1">
                      {selectedComponent.bindings.map((binding, index) => (
                        <li
                          key={index}
                          className="flex items-center justify-between rounded border border-gray-200 dark:border-slate-700 px-2 py-1 text-xs font-mono text-gray-700 dark:text-gray-300"
                        >
                          <span className="truncate">
                            {JSON.stringify(binding.source)} → {JSON.stringify(binding.target)}
                          </span>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => removePageComponentBinding(page.id, selectedComponent.id, index)}
                          >
                            <Trash2 className="w-3 h-3" />
                          </Button>
                        </li>
                      ))}
                    </ul>
                  )}
                  <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
                    <Select
                      label="Data source"
                      value={bindingSourceId}
                      onChange={(event) => setBindingSourceId(event.target.value)}
                      options={[
                        { value: '', label: 'None' },
                        ...(page.data_sources ?? []).map((source) => ({
                          value: source.id,
                          label: source.name,
                        })),
                      ]}
                    />
                    <Input
                      label="Path"
                      value={bindingPath}
                      placeholder="items.name"
                      onChange={(event) => setBindingPath(event.target.value)}
                    />
                    <Input
                      label="Prop"
                      value={bindingProp}
                      placeholder="title"
                      onChange={(event) => setBindingProp(event.target.value)}
                    />
                  </div>
                  <Button variant="secondary" size="sm" onClick={handleAddBinding}>
                    <Plus className="w-4 h-4 mr-1" /> Add binding
                  </Button>
                </div>
              </Card>
            )}

            {page && (
              <Card className="mt-4 p-4 space-y-3">
                <h4 className="text-sm font-semibold text-gray-900 dark:text-white">Page actions</h4>
                {page.actions.length === 0 ? (
                  <p className="text-xs text-gray-400 dark:text-gray-500">
                    No actions yet — buttons and events can invoke these.
                  </p>
                ) : (
                  <ul className="space-y-1">
                    {page.actions.map((action) => (
                      <li
                        key={action.id}
                        className="flex items-center justify-between rounded border border-gray-200 dark:border-slate-700 px-2 py-1 text-xs text-gray-700 dark:text-gray-300"
                      >
                        <span className="truncate">
                          {action.name} <span className="text-gray-400">({formatActionType(action.action_type)})</span>
                        </span>
                        <Button variant="ghost" size="sm" onClick={() => removePageAction(page.id, action.id)}>
                          <Trash2 className="w-3 h-3" />
                        </Button>
                      </li>
                    ))}
                  </ul>
                )}
                <div className="flex items-end gap-2">
                  <div className="flex-1">
                    <Select
                      label="Action"
                      value={actionPreset}
                      onChange={(event) => setActionPreset(event.target.value)}
                      options={ACTION_PRESETS.map((preset) => ({ value: preset.key, label: preset.label }))}
                    />
                  </div>
                  <Button variant="secondary" size="sm" onClick={handleAddAction}>
                    <Plus className="w-4 h-4 mr-1" /> Add
                  </Button>
                </div>
              </Card>
            )}
          </div>
        </div>

        {/* Palette */}
        <div className="w-72 border-l border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Palette</h3>
          <div className="space-y-4">
            {PALETTE_CATEGORIES.map((category: PaletteCategory) => (
              <div key={category}>
                <h4 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-2">
                  {category}
                </h4>
                <div className="space-y-1">
                  {PALETTE.filter((item) => item.category === category).map((item) => (
                    <button
                      key={item.type}
                      type="button"
                      onClick={() => handleAddComponent(item.type)}
                      className="w-full text-left p-2 border border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 rounded-lg text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700/50 disabled:opacity-50"
                      disabled={!page}
                    >
                      {item.label}
                      {item.needsBinding && (
                        <span className="ml-1 text-[10px] uppercase text-primary-600 dark:text-primary-400">
                          bind
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
