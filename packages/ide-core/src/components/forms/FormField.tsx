import { useState } from 'react';
import { GripVertical, Trash2, Copy, Settings, Plus } from 'lucide-react';
import { Input } from '../ui/Input';

import { Button } from '../ui/Button';
import { cn } from '../../utils/helpers';

export interface FormField {
  id: string;
  type: string;
  label: string;
  placeholder: string;
  required: boolean;
  options: string[];
}

interface DraggableFormFieldProps {
  field: FormField;
  index: number;
  isSelected: boolean;
  onSelect: () => void;
  onUpdate: (updates: Partial<FormField>) => void;
  onRemove: () => void;
  onDuplicate: () => void;
  onMove: (fromIndex: number, toIndex: number) => void;
}

export function DraggableFormField({
  field,
  index,
  isSelected,
  onSelect,
  onRemove,
  onDuplicate,
  onMove,
}: DraggableFormFieldProps) {
  const [dragging, setDragging] = useState(false);
  const [showOptions, setShowOptions] = useState(false);

  const handleDragStart = (e: React.DragEvent) => {
    e.dataTransfer.setData('text/plain', index.toString());
    e.dataTransfer.effectAllowed = 'move';
    setDragging(true);
  };

  const handleDragEnd = () => {
    setDragging(false);
  };

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    const fromIndex = parseInt(e.dataTransfer.getData('text/plain'), 10);
    if (fromIndex !== index) {
      onMove(fromIndex, index);
    }
  };

  const fieldTypeLabels: Record<string, string> = {
    text: 'Text Input',
    textarea: 'Text Area',
    email: 'Email',
    number: 'Number',
    select: 'Dropdown',
    checkbox: 'Checkbox Group',
    radio: 'Radio Group',
    date: 'Date Picker',
    file: 'File Upload',
  };

  return (
    <div
      draggable
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      onClick={onSelect}
      className={cn(
        'relative p-4 border rounded-lg bg-white dark:bg-slate-800 transition-all',
        'hover:shadow-md',
        isSelected
          ? 'border-primary-500 ring-2 ring-primary-500/20'
          : 'border-gray-200 dark:border-slate-700',
        dragging && 'opacity-50 rotate-1 scale-102'
      )}
    >
      {/* Drag handle */}
      <div className="absolute -left-10 top-1/2 -translate-y-1/2 flex flex-col items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
        <GripVertical className="w-5 h-5 text-gray-400 cursor-grab active:cursor-grabbing" />
      </div>

      {/* Field preview */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-gray-500 dark:text-gray-400">{index + 1}.</span>
            <span className="font-medium text-gray-900 dark:text-white">{field.label || 'Untitled'}</span>
            {field.required && <span className="text-red-500 ml-1">*</span>}
          </div>
          <span className="text-xs px-2 py-0.5 bg-gray-100 dark:bg-slate-800 text-gray-600 dark:text-gray-400 rounded">
            {fieldTypeLabels[field.type] || field.type}
          </span>
        </div>

        {/* Field input preview */}
        <div className="bg-gray-50 dark:bg-slate-900/50 rounded p-3 text-sm">
          {field.type === 'textarea' ? (
            <textarea
              className="w-full bg-transparent border-none focus:ring-0 resize-none text-gray-600 dark:text-gray-400"
              placeholder={field.placeholder || 'Enter text...'}
              rows={3}
              readOnly
            />
          ) : field.type === 'select' ? (
            <select className="w-full bg-transparent border-none focus:ring-0 text-gray-600 dark:text-gray-400" disabled>
              <option value="">{field.placeholder || 'Select...'}</option>
              {field.options.map((opt, i) => (
                <option key={i} value={opt}>{opt}</option>
              ))}
            </select>
          ) : field.type === 'checkbox' ? (
            <div className="space-y-1">
              {field.options.map((opt, i) => (
                <label key={i} className="flex items-center gap-2 cursor-pointer">
                  <input type="checkbox" className="rounded border-gray-300" disabled />
                  <span className="text-gray-600 dark:text-gray-400">{opt}</span>
                </label>
              ))}
            </div>
          ) : field.type === 'radio' ? (
            <div className="space-y-1">
              {field.options.map((opt, i) => (
                <label key={i} className="flex items-center gap-2 cursor-pointer">
                  <input type="radio" className="rounded border-gray-300" disabled />
                  <span className="text-gray-600 dark:text-gray-400">{opt}</span>
                </label>
              ))}
            </div>
          ) : (
            <input
              type={field.type === 'number' ? 'number' : field.type}
              className="w-full bg-transparent border-none focus:ring-0 text-gray-600 dark:text-gray-400"
              placeholder={field.placeholder || 'Enter value...'}
              readOnly
            />
          )}
        </div>
      </div>

      {/* Actions menu */}
      <div className="absolute top-2 right-2">
        <div className="relative">
          <button
            onClick={(e) => { e.stopPropagation(); setShowOptions(!showOptions); }}
            className="p-1 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
          >
            <Settings className="w-4 h-4" />
          </button>
          {showOptions && (
            <div className="absolute right-0 mt-1 w-32 bg-white dark:bg-slate-800 border border-gray-200 dark:border-slate-700 rounded-lg shadow-lg py-1 z-10">
              <button
                onClick={(e) => { e.stopPropagation(); onDuplicate(); setShowOptions(false); }}
                className="w-full px-3 py-1.5 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2"
              >
                <Copy className="w-4 h-4" /> Duplicate
              </button>
              <button
                onClick={(e) => { e.stopPropagation(); onRemove(); setShowOptions(false); }}
                className="w-full px-3 py-1.5 text-left text-sm text-red-600 dark:text-red-400 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2"
              >
                <Trash2 className="w-4 h-4" /> Delete
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Selection indicator */}
      {isSelected && (
        <div className="absolute -bottom-1 left-1/2 -translate-x-1/2 w-2 h-2 bg-primary-500 rounded-full" />
      )}
    </div>
  );
}

interface FormFieldProps {
  field: FormField;
  onUpdate: (updates: Partial<FormField>) => void;
  onRemove: () => void;
}

export function FormField({ field, onUpdate, onRemove }: FormFieldProps) {

  return (
    <div className="space-y-4 p-4 border border-gray-200 dark:border-slate-700 rounded-lg bg-white dark:bg-slate-800">
      <div className="flex items-center justify-between">
        <h3 className="font-medium text-gray-900 dark:text-white capitalize">{field.type} Field</h3>
        <button
          onClick={onRemove}
          className="p-1 rounded text-gray-400 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20"
        >
          <Trash2 className="w-4 h-4" />
        </button>
      </div>

      <div className="space-y-3">
        <Input
          label="Label"
          value={field.label}
          onChange={(e) => onUpdate({ label: e.target.value })}
          placeholder="Field label"
        />

        <Input
          label="Placeholder"
          value={field.placeholder}
          onChange={(e) => onUpdate({ placeholder: e.target.value })}
          placeholder="Placeholder text"
        />

        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            id={`required-${field.id}`}
            checked={field.required}
            onChange={(e) => onUpdate({ required: e.target.checked })}
            className="rounded border-gray-300"
          />
          <label htmlFor={`required-${field.id}`} className="text-sm text-gray-700 dark:text-gray-300">
            Required field
          </label>
        </div>

        {['select', 'checkbox', 'radio'].includes(field.type) && (
          <div className="space-y-2">
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">Options</label>
            {field.options.map((opt, i) => (
              <div key={i} className="flex gap-2">
                <Input
                  value={opt}
                  onChange={(e) => {
                    const newOptions = [...field.options];
                    newOptions[i] = e.target.value;
                    onUpdate({ options: newOptions });
                  }}
                />
                <Button variant="ghost" size="sm" onClick={() => {
                  const newOptions = field.options.filter((_, idx) => idx !== i);
                  onUpdate({ options: newOptions });
                }}>
                  <Trash2 className="w-4 h-4" />
                </Button>
              </div>
            ))}
            <Button variant="secondary" size="sm" onClick={() => {
              onUpdate({ options: [...field.options, ''] });
            }}>
              <Plus className="w-4 h-4 mr-1" /> Add Option
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}