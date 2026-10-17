import { useMemo, useState } from 'react';
import { Plus, Trash2, Settings, FileText } from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { DraggableFormField } from '../components/forms/FormField';
import { useProjectStore } from '../stores/projectStore';
import type { FormFieldPatch } from '../stores/projectStore';
import {
  EDITOR_FORM_FIELD_TYPES,
  formFieldToEditor,
} from '../utils/formModel';
import type { EditorFormFieldType } from '../utils/formModel';

export function FormBuilder() {
  const project = useProjectStore((s) => s.project);
  const addForm = useProjectStore((s) => s.addForm);
  const renameForm = useProjectStore((s) => s.renameForm);
  const addFormField = useProjectStore((s) => s.addFormField);
  const updateFormField = useProjectStore((s) => s.updateFormField);
  const removeFormField = useProjectStore((s) => s.removeFormField);
  const moveFormField = useProjectStore((s) => s.moveFormField);

  const [selectedFormId, setSelectedFormId] = useState('');
  const [selectedFieldId, setSelectedFieldId] = useState('');
  const [previewMode, setPreviewMode] = useState(false);

  const form = useMemo(() => {
    if (!project) return null;
    const forms = Object.values(project.forms);
    return forms.find((candidate) => candidate.id === selectedFormId) ?? forms[0] ?? null;
  }, [project, selectedFormId]);

  const fields = useMemo(() => (form ? form.fields.map(formFieldToEditor) : []), [form]);

  const selectedField = fields.find((field) => field.id === selectedFieldId) ?? null;

  const applyPatch = (fieldId: string, patch: Partial<FormFieldPatch>) => {
    if (!form) return;
    updateFormField(form.id, fieldId, patch);
  };

  const handleAddField = (type: EditorFormFieldType) => {
    if (!form) return;
    const id = addFormField(form.id, type);
    if (id) setSelectedFieldId(id);
  };

  const handleDuplicateField = (id: string) => {
    if (!form) return;
    const field = fields.find((candidate) => candidate.id === id);
    if (!field) return;
    const newId = addFormField(form.id, field.type);
    applyPatch(newId, {
      name: field.name,
      label: field.label,
      required: field.required,
      placeholder: field.placeholder,
      options: field.options,
    });
    setSelectedFieldId(newId);
  };

  const handleRemoveField = (id: string) => {
    if (!form) return;
    removeFormField(form.id, id);
    if (selectedFieldId === id) setSelectedFieldId('');
  };

  if (!form) {
    return (
      <div className="h-full flex items-center justify-center bg-gray-100 dark:bg-slate-900">
        <div className="text-center text-gray-500 dark:text-gray-400">
          <FileText className="w-12 h-12 mx-auto mb-4 text-gray-300 dark:text-gray-600" />
          <p className="text-lg mb-4">No forms yet</p>
          <Button variant="primary" onClick={() => setSelectedFormId(addForm('Untitled Form'))}>
            <Plus className="w-4 h-4 mr-1" /> Create Form
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-4">
          <Input
            value={form.name}
            onChange={(e) => renameForm(form.id, e.target.value)}
            placeholder="Form name"
            className="w-64 text-lg font-semibold bg-transparent border-none focus:ring-0"
          />
          <span className="text-sm text-gray-500 dark:text-gray-400">{fields.length} fields</span>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={() => setPreviewMode(!previewMode)}>
            {previewMode ? 'Edit' : 'Preview'}
          </Button>
          <Button variant="secondary" size="sm">Save Draft</Button>
          <Button variant="primary" size="sm">Publish</Button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Left Panel - Field Palette */}
        <div className="w-64 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Field Types</h3>
          <div className="space-y-2">
            {EDITOR_FORM_FIELD_TYPES.map((type) => (
              <button
                key={type.value}
                onClick={() => handleAddField(type.value)}
                className="w-full flex items-center gap-3 p-3 text-left rounded-lg border border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-gray-50 dark:hover:bg-slate-700/50 transition-colors group"
              >
                <span className="text-2xl">{type.icon}</span>
                <span className="font-medium text-gray-700 dark:text-gray-300">{type.label}</span>
              </button>
            ))}
          </div>
        </div>

        {/* Center Panel - Form Canvas */}
        <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
          <div className="max-w-2xl mx-auto bg-white dark:bg-slate-800 rounded-xl shadow-lg p-6 min-h-[500px]">
            {!previewMode ? (
              <div className="space-y-4">
                <h2 className="text-2xl font-bold text-gray-900 dark:text-white mb-2">{form.name}</h2>
                <p className="text-gray-600 dark:text-gray-400 mb-6">Add fields from the left panel to build your form</p>

                {fields.length === 0 ? (
                  <div className="border-2 border-dashed border-gray-300 dark:border-slate-600 rounded-lg p-12 text-center">
                    <p className="text-gray-500 dark:text-gray-400 mb-4">No fields added yet</p>
                    <p className="text-sm text-gray-400 dark:text-gray-500">Pick a field type from the left panel to get started</p>
                  </div>
                ) : (
                  <div className="space-y-3">
                    {fields.map((field, index) => (
                      <DraggableFormField
                        key={field.id}
                        field={field}
                        index={index}
                        isSelected={selectedFieldId === field.id}
                        onSelect={() => setSelectedFieldId(field.id)}
                        onUpdate={(updates) =>
                          applyPatch(field.id, {
                            label: updates.label,
                            type: updates.type as EditorFormFieldType | undefined,
                            required: updates.required,
                            placeholder: updates.placeholder,
                            options: updates.options,
                          })
                        }
                        onRemove={() => handleRemoveField(field.id)}
                        onDuplicate={() => handleDuplicateField(field.id)}
                        onMove={(from, to) => moveFormField(form.id, from, to)}
                      />
                    ))}
                  </div>
                )}
              </div>
            ) : (
              <div className="space-y-6">
                <h2 className="text-2xl font-bold text-gray-900 dark:text-white mb-2">{form.name}</h2>
                <form className="space-y-6">
                  {fields.map((field) => (
                    <div key={field.id} className="space-y-1">
                      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                        {field.label} {field.required && <span className="text-red-500 ml-1">*</span>}
                      </label>
                      {field.type === 'textarea' ? (
                        <textarea
                          className="w-full px-3 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800"
                          placeholder={field.placeholder}
                          rows={4}
                        />
                      ) : field.type === 'select' ? (
                        <select className="w-full px-3 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800">
                          <option value="">Select...</option>
                          {field.options.map((opt, i) => (
                            <option key={i} value={opt}>{opt}</option>
                          ))}
                        </select>
                      ) : field.type === 'checkbox' ? (
                        <div className="space-y-2">
                          {field.options.map((opt, i) => (
                            <label key={i} className="flex items-center gap-2 cursor-pointer">
                              <input type="checkbox" className="rounded border-gray-300" />
                              <span className="text-gray-700 dark:text-gray-300">{opt}</span>
                            </label>
                          ))}
                        </div>
                      ) : field.type === 'radio' ? (
                        <div className="space-y-2">
                          {field.options.map((opt, i) => (
                            <label key={i} className="flex items-center gap-2 cursor-pointer">
                              <input type="radio" name={field.id} className="rounded border-gray-300" />
                              <span className="text-gray-700 dark:text-gray-300">{opt}</span>
                            </label>
                          ))}
                        </div>
                      ) : (
                        <input
                          type={field.type === 'number' || field.type === 'integer' ? 'number' : 'text'}
                          className="w-full px-3 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800"
                          placeholder={field.placeholder}
                        />
                      )}
                    </div>
                  ))}
                  <div className="flex gap-4 pt-4 border-t border-gray-200 dark:border-slate-700">
                    <Button variant="secondary">Cancel</Button>
                    <Button variant="primary">Submit</Button>
                  </div>
                </form>
              </div>
            )}
          </div>
        </div>

        {/* Right Panel - Field Properties */}
        <div className="w-80 border-l border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          {selectedField ? (
            <div className="space-y-4">
              <h3 className="font-semibold text-gray-900 dark:text-white">Field Properties</h3>
              <div className="space-y-3">
                <div>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Label</label>
                  <Input
                    value={selectedField.label}
                    onChange={(e) => applyPatch(selectedField.id, { label: e.target.value })}
                    placeholder="Field label"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Name</label>
                  <Input
                    value={selectedField.name}
                    onChange={(e) => applyPatch(selectedField.id, { name: e.target.value })}
                    placeholder="Field name"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Placeholder</label>
                  <Input
                    value={selectedField.placeholder}
                    onChange={(e) => applyPatch(selectedField.id, { placeholder: e.target.value })}
                    placeholder="Placeholder text"
                  />
                </div>
                <div className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    id="required"
                    checked={selectedField.required}
                    onChange={(e) => applyPatch(selectedField.id, { required: e.target.checked })}
                    className="rounded border-gray-300"
                  />
                  <label htmlFor="required" className="text-sm text-gray-700 dark:text-gray-300">Required</label>
                </div>

                {['select', 'radio', 'checkbox'].includes(selectedField.type) && (
                  <div className="space-y-2">
                    <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Options</label>
                    {selectedField.options.map((opt, i) => (
                      <div key={i} className="flex gap-2">
                        <Input
                          value={opt}
                          onChange={(e) => {
                            const options = [...selectedField.options];
                            options[i] = e.target.value;
                            applyPatch(selectedField.id, { options });
                          }}
                        />
                        <Button variant="ghost" size="sm" onClick={() => {
                          applyPatch(selectedField.id, { options: selectedField.options.filter((_, idx) => idx !== i) });
                        }}>
                          <Trash2 className="w-4 h-4" />
                        </Button>
                      </div>
                    ))}
                    <Button variant="secondary" size="sm" onClick={() => {
                      applyPatch(selectedField.id, { options: [...selectedField.options, ''] });
                    }}>
                      <Plus className="w-4 h-4 mr-1" /> Add Option
                    </Button>
                  </div>
                )}

                <div className="pt-4 border-t border-gray-200 dark:border-slate-700">
                  <Button variant="danger" className="w-full mt-2" onClick={() => handleRemoveField(selectedField.id)}>
                    <Trash2 className="w-4 h-4 mr-1" /> Delete
                  </Button>
                </div>
              </div>
            </div>
          ) : (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              <Settings className="w-12 h-12 mx-auto mb-4 text-gray-300 dark:text-gray-600" />
              <p>Select a field to edit its properties</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
