import { useMemo, useState } from 'react';
import { Plus, Trash2, Table2, List } from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Select } from '../components/ui/Select';
import { useProjectStore } from '../stores/projectStore';
import { newListPage, type ListPageKind } from '../utils/listModel';

export function ListBuilder() {
  const project = useProjectStore((s) => s.project);
  const addListPage = useProjectStore((s) => s.addListPage);
  const removePage = useProjectStore((s) => s.removePage);

  const entities = useMemo(
    () => (project ? Object.values(project.entities) : []),
    [project],
  );
  const [entityId, setEntityId] = useState('');
  const [kind, setKind] = useState<ListPageKind>('table');

  const selectedEntity = entities.find((candidate) => candidate.id === entityId) ?? entities[0] ?? null;
  const preview = useMemo(
    () => (selectedEntity ? newListPage(selectedEntity, kind) : null),
    [selectedEntity, kind],
  );

  const generatedListPages = useMemo(
    () =>
      project
        ? Object.values(project.pages).filter(
            (page) => page.path.endsWith('/list') && !Array.isArray((page as Record<string, unknown>).widgets),
          )
        : [],
    [project],
  );

  const handleGenerate = () => {
    if (!selectedEntity) return;
    addListPage(selectedEntity.id, kind);
  };

  return (
    <div className="h-full overflow-y-auto bg-gray-100 dark:bg-slate-900">
      <div className="mx-auto max-w-5xl p-6 space-y-6">
        <header>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">List Builder</h1>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            Generate a page with a data-bound Table or List component from an entity.
          </p>
        </header>

        {entities.length === 0 ? (
          <Card className="p-8 text-center text-gray-500 dark:text-gray-400">
            <p>No entities yet — create one in the Data Modeler first.</p>
          </Card>
        ) : (
          <Card className="p-6 space-y-4">
            <div className="flex flex-col sm:flex-row gap-4">
              <div className="flex-1">
                <Select
                  label="Entity"
                  value={selectedEntity?.id ?? ''}
                  onChange={(event) => setEntityId(event.target.value)}
                  options={entities.map((entity) => ({
                    value: entity.id,
                    label: entity.name,
                  }))}
                />
              </div>
              <div className="w-full sm:w-48">
                <Select
                  label="Component kind"
                  value={kind}
                  onChange={(event) => setKind(event.target.value as ListPageKind)}
                  options={[
                    { value: 'table', label: 'Table' },
                    { value: 'list', label: 'List' },
                  ]}
                />
              </div>
              <div className="flex items-end">
                <Button variant="primary" onClick={handleGenerate}>
                  <Plus className="w-4 h-4 mr-1" /> Generate Page
                </Button>
              </div>
            </div>
            <p className="text-xs text-gray-400 dark:text-gray-500">
              Generated pages ship read-only plus five CRUD page actions (create, read,
              update, delete, reload) wired to the list data source. Bind them to buttons
              in the Page Builder.
            </p>
          </Card>
        )}

        <section>
          <h2 className="mb-3 text-sm font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400">
            Generated list pages
          </h2>
          {generatedListPages.length === 0 ? (
            <Card className="p-6 text-center text-sm text-gray-500 dark:text-gray-400">
              None yet. Pick an entity above and generate.
            </Card>
          ) : (
            <ul className="space-y-2">
              {generatedListPages.map((page) => (
                <li
                  key={page.id}
                  className="flex items-center justify-between rounded-lg border border-gray-200 bg-white px-4 py-3 dark:border-slate-700 dark:bg-slate-800"
                >
                  <div className="flex items-center gap-3 min-w-0">
                    {kind === 'table' ? (
                      <Table2 className="w-4 h-4 text-primary-500 shrink-0" />
                    ) : (
                      <List className="w-4 h-4 text-primary-500 shrink-0" />
                    )}
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium text-gray-900 dark:text-white">
                        {page.name}
                      </p>
                      <p className="truncate text-xs text-gray-400 dark:text-gray-500">{page.path}</p>
                    </div>
                  </div>
                  <Button variant="ghost" onClick={() => removePage(page.id)}>
                    <Trash2 className="w-4 h-4" />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </section>

        {preview && (
          <section>
            <h2 className="mb-3 text-sm font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400">
              Preview
            </h2>
            <pre className="max-h-72 overflow-auto rounded-lg bg-slate-950 p-4 text-xs leading-relaxed text-slate-200">
              {JSON.stringify(preview, null, 2)}
            </pre>
          </section>
        )}
      </div>
    </div>
  );
}
