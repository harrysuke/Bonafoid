import { useMemo, useState } from 'react';
import { Plus, Trash2, Database, Link } from 'lucide-react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { useProjectStore } from '../stores/projectStore';
import { entityToEditor } from '../utils/dataModel';
import type { EditorFieldType, EditorRelationType } from '../utils/dataModel';

const fieldTypes = [
  { value: 'string', label: 'String' },
  { value: 'text', label: 'Text' },
  { value: 'integer', label: 'Integer' },
  { value: 'bigint', label: 'Big Integer' },
  { value: 'float', label: 'Float' },
  { value: 'decimal', label: 'Decimal' },
  { value: 'boolean', label: 'Boolean' },
  { value: 'date', label: 'Date' },
  { value: 'datetime', label: 'DateTime' },
  { value: 'json', label: 'JSON' },
  { value: 'uuid', label: 'UUID' },
  { value: 'enum', label: 'Enum' },
  { value: 'relation', label: 'Relation' },
];

const relationTypes = [
  { value: 'belongsTo', label: 'Belongs To' },
  { value: 'hasOne', label: 'Has One' },
  { value: 'hasMany', label: 'Has Many' },
  { value: 'manyToMany', label: 'Many to Many' },
];

export function DataModeler() {
  const project = useProjectStore((s) => s.project);
  const addEntity = useProjectStore((s) => s.addEntity);
  const removeEntity = useProjectStore((s) => s.removeEntity);
  const renameEntity = useProjectStore((s) => s.renameEntity);
  const addField = useProjectStore((s) => s.addField);
  const updateField = useProjectStore((s) => s.updateField);
  const removeField = useProjectStore((s) => s.removeField);
  const addRelation = useProjectStore((s) => s.addRelation);
  const updateRelation = useProjectStore((s) => s.updateRelation);
  const removeRelation = useProjectStore((s) => s.removeRelation);
  const setPrimaryKey = useProjectStore((s) => s.setPrimaryKey);

  const [selectedEntityId, setSelectedEntityId] = useState('');
  const [showAddEntity, setShowAddEntity] = useState(false);
  const [newEntityName, setNewEntityName] = useState('');
  const [searchTerm, setSearchTerm] = useState('');

  const entities = useMemo(
    () =>
      project ? Object.values(project.entities).map((e) => entityToEditor(e, project.entities)) : [],
    [project],
  );

  const selectedEntity =
    entities.find((entity) => entity.id === selectedEntityId) ?? entities[0] ?? null;

  const handleAddEntity = () => {
    if (!newEntityName.trim()) return;
    const id = addEntity(newEntityName.trim());
    if (id) setSelectedEntityId(id);
    setShowAddEntity(false);
    setNewEntityName('');
  };

  const handleDeleteEntity = () => {
    if (!selectedEntity) return;
    removeEntity(selectedEntity.id);
    setSelectedEntityId('');
  };

  const filteredEntities = entities.filter((e) =>
    e.name.toLowerCase().includes(searchTerm.toLowerCase()),
  );

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-4">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Data Modeler</h2>
          <Input
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
            placeholder="Search entities..."
            className="w-64"
          />
        </div>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" onClick={() => setShowAddEntity(true)}>
            <Plus className="w-4 h-4 mr-1" /> Add Entity
          </Button>
          <Button variant="secondary" size="sm">Export SQL</Button>
          <Button variant="primary" size="sm">Generate Migrations</Button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Left Panel - Entity List */}
        <div className="w-64 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <div className="flex items-center justify-between mb-4">
            <h3 className="font-semibold text-gray-900 dark:text-white">Entities ({entities.length})</h3>
            <Button variant="ghost" size="sm" onClick={() => setShowAddEntity(true)}>
              <Plus className="w-4 h-4" />
            </Button>
          </div>

          {showAddEntity ? (
            <div className="space-y-2 mb-4">
              <Input
                value={newEntityName}
                onChange={(e) => setNewEntityName(e.target.value)}
                placeholder="Entity name (PascalCase)"
                autoFocus
              />
              <div className="flex gap-2">
                <Button variant="primary" size="sm" onClick={handleAddEntity}>Create</Button>
                <Button variant="ghost" size="sm" onClick={() => setShowAddEntity(false)}>Cancel</Button>
              </div>
            </div>
          ) : null}

          <div className="space-y-1">
            {filteredEntities.map((entity) => (
              <div
                key={entity.id}
                className={`flex items-center gap-2 p-2 rounded-lg cursor-pointer transition-colors ${
                  selectedEntity?.id === entity.id
                    ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300'
                    : 'hover:bg-gray-100 dark:hover:bg-slate-700/50 text-gray-700 dark:text-gray-300'
                }`}
                onClick={() => setSelectedEntityId(entity.id)}
              >
                <Database className="w-5 h-5" />
                <span className="font-medium truncate">{entity.name}</span>
                <span className="text-xs text-gray-500 dark:text-gray-400 ml-auto">{entity.fields.length} fields</span>
              </div>
            ))}
          </div>
        </div>

        {/* Center Panel - Entity Designer */}
        <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
          {selectedEntity ? (
            <div className="max-w-3xl mx-auto space-y-6">
              {/* Entity Header */}
              <div className="flex items-center justify-between mb-6">
                <div>
                  <input
                    value={selectedEntity.name}
                    onChange={(e) => renameEntity(selectedEntity.id, e.target.value)}
                    className="text-2xl font-bold text-gray-900 dark:text-white bg-transparent border-none focus:ring-0 w-64"
                  />
                  <span className="text-sm text-gray-500 dark:text-gray-400 ml-2">{selectedEntity.fields.length} fields, {selectedEntity.relations.length} relations</span>
                </div>
                <div className="flex gap-2">
                  <Button variant="secondary" size="sm" onClick={() => addField(selectedEntity.id)}>
                    <Plus className="w-4 h-4 mr-1" /> Add Field
                  </Button>
                  <Button variant="secondary" size="sm" onClick={() => addRelation(selectedEntity.id)}>
                    <Link className="w-4 h-4 mr-1" /> Add Relation
                  </Button>
                  <Button variant="danger" size="sm" onClick={handleDeleteEntity}>
                    <Trash2 className="w-4 h-4 mr-1" /> Delete Entity
                  </Button>
                </div>
              </div>

              {/* Fields Table */}
              <Card>
                <div className="p-4 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between">
                  <h3 className="font-semibold text-gray-900 dark:text-white">Fields</h3>
                </div>
                <div className="overflow-x-auto">
                  <table className="w-full">
                    <thead>
                      <tr className="border-b border-gray-200 dark:border-slate-700">
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">#</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Name</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Type</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">PK</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Required</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Unique</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Default</th>
                        <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Actions</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                      {selectedEntity.fields.map((field, index) => (
                        <tr key={field.id} className="hover:bg-gray-50 dark:hover:bg-slate-800/50">
                          <td className="px-4 py-3 text-sm text-gray-500 dark:text-gray-400">{index + 1}</td>
                          <td className="px-4 py-3">
                            <input
                              value={field.name}
                              onChange={(e) => updateField(selectedEntity.id, field.id, { name: e.target.value })}
                              className="w-full px-2 py-1 border border-gray-300 dark:border-slate-600 rounded bg-white dark:bg-slate-800 text-sm"
                            />
                          </td>
                          <td className="px-4 py-3">
                            <Select
                              value={field.type}
                              onChange={(e) => updateField(selectedEntity.id, field.id, { type: e.target.value as EditorFieldType })}
                              options={fieldTypes.map(t => ({ value: t.value, label: t.label }))}
                              className="w-40 text-sm"
                            />
                          </td>
                          <td className="px-4 py-3">
                            <input type="checkbox" checked={field.primaryKey} onChange={(e) => setPrimaryKey(selectedEntity.id, field.id, e.target.checked)} className="rounded" />
                          </td>
                          <td className="px-4 py-3">
                            <input type="checkbox" checked={field.required} onChange={(e) => updateField(selectedEntity.id, field.id, { required: e.target.checked })} className="rounded" />
                          </td>
                          <td className="px-4 py-3">
                            <input type="checkbox" checked={field.unique} onChange={(e) => updateField(selectedEntity.id, field.id, { unique: e.target.checked })} className="rounded" />
                          </td>
                          <td className="px-4 py-3">
                            <Input value={field.default || ''} onChange={(e) => updateField(selectedEntity.id, field.id, { default: e.target.value })} placeholder="Default" className="text-sm" />
                          </td>
                          <td className="px-4 py-3 text-right">
                            <Button variant="ghost" size="sm" onClick={() => removeField(selectedEntity.id, field.id)}>
                              <Trash2 className="w-4 h-4" />
                            </Button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </Card>

              {/* Relations Table */}
              <Card className="mt-6">
                <div className="p-4 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between">
                  <h3 className="font-semibold text-gray-900 dark:text-white">Relations</h3>
                  <Button variant="secondary" size="sm" onClick={() => addRelation(selectedEntity.id)}>
                    <Plus className="w-4 h-4 mr-1" /> Add Relation
                  </Button>
                </div>
                <div className="overflow-x-auto">
                  <table className="w-full">
                    <thead>
                      <tr className="border-b border-gray-200 dark:border-slate-700">
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Name</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Type</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Target Entity</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Target Field</th>
                        <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Actions</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                      {selectedEntity.relations.map((relation) => (
                        <tr key={relation.id} className="hover:bg-gray-50 dark:hover:bg-slate-800/50">
                          <td className="px-4 py-3">
                            <Input value={relation.name} onChange={(e) => updateRelation(selectedEntity.id, relation.id, { name: e.target.value })} className="w-full text-sm" placeholder="Relation name" />
                          </td>
                          <td className="px-4 py-3">
                            <Select value={relation.type} onChange={(e) => updateRelation(selectedEntity.id, relation.id, { type: e.target.value as EditorRelationType })} options={relationTypes.map(t => ({ value: t.value, label: t.label }))} className="w-40 text-sm" />
                          </td>
                          <td className="px-4 py-3">
                            <Input value={relation.targetEntity} onChange={(e) => updateRelation(selectedEntity.id, relation.id, { targetEntity: e.target.value })} className="w-full text-sm" placeholder="Target entity" />
                          </td>
                          <td className="px-4 py-3">
                            <Input value={relation.targetField} onChange={(e) => updateRelation(selectedEntity.id, relation.id, { targetField: e.target.value })} className="w-full text-sm" placeholder="Target field" />
                          </td>
                          <td className="px-4 py-3 text-right">
                            <Button variant="ghost" size="sm" onClick={() => removeRelation(selectedEntity.id, relation.id)}>
                              <Trash2 className="w-4 h-4" />
                            </Button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </Card>
            </div>
          ) : (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              <Database className="w-12 h-12 mx-auto mb-4 text-gray-300 dark:text-gray-600" />
              <p className="text-lg">Select an entity or create a new one</p>
              <Button variant="primary" className="mt-4" onClick={() => setShowAddEntity(true)}>
                <Plus className="w-4 h-4 mr-1" /> Create Entity
              </Button>
            </div>
          )}
        </div>

        {/* Right Panel - SQL Preview */}
        <div className="w-80 border-l border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">SQL Preview</h3>
          <div className="bg-gray-900 dark:bg-gray-950 rounded-lg p-4 font-mono text-xs text-green-400 overflow-x-auto max-h-96 overflow-y-auto">
            {selectedEntity ? (
              `CREATE TABLE ${selectedEntity.name.toLowerCase()} (\n${selectedEntity.fields.map(f =>
                `  ${f.name} ${f.type.toUpperCase()}${f.primaryKey ? ' PRIMARY KEY' : ''}${f.required ? ' NOT NULL' : ''}${f.unique ? ' UNIQUE' : ''}`
              ).join(',\n')}\n);`
            ) : (
              'Select an entity to see SQL preview'
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
