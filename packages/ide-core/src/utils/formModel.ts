/**
 * Mapping helpers between the editor-facing form shapes used by the FormBuilder
 * UI and the Rust-shaped `nocode_model::form` documents the project store owns.
 *
 * The editor collapses the wide `FieldType` enum into a single `type` string and
 * represents choice options as plain labels; these helpers round-trip that view
 * while preserving every other field of the canonical model.
 */

import { generateId } from './helpers';
import type {
  FieldType,
  Form,
  FormField,
  FieldUiProps,
  SelectOption,
} from '../types/project';

export type EditorFormFieldType =
  | 'text'
  | 'textarea'
  | 'number'
  | 'integer'
  | 'boolean'
  | 'select'
  | 'radio'
  | 'checkbox'
  | 'date'
  | 'datetime'
  | 'time'
  | 'email'
  | 'url'
  | 'phone'
  | 'file'
  | 'image'
  | 'signature'
  | 'richtext'
  | 'json'
  | 'relation';

export interface EditorFormField {
  id: string;
  name: string;
  label: string;
  type: EditorFormFieldType;
  required: boolean;
  placeholder: string;
  options: string[];
}

export const EDITOR_FORM_FIELD_TYPES: { value: EditorFormFieldType; label: string; icon: string }[] = [
  { value: 'text', label: 'Text Input', icon: '📝' },
  { value: 'textarea', label: 'Text Area', icon: '📄' },
  { value: 'email', label: 'Email', icon: '📧' },
  { value: 'url', label: 'URL', icon: '🔗' },
  { value: 'phone', label: 'Phone', icon: '📞' },
  { value: 'number', label: 'Number', icon: '🔢' },
  { value: 'integer', label: 'Integer', icon: '🔢' },
  { value: 'boolean', label: 'Boolean', icon: '✅' },
  { value: 'select', label: 'Dropdown', icon: '📋' },
  { value: 'radio', label: 'Radio Group', icon: '🔘' },
  { value: 'checkbox', label: 'Checkbox Group', icon: '☑️' },
  { value: 'date', label: 'Date', icon: '📅' },
  { value: 'datetime', label: 'Date & Time', icon: '🕐' },
  { value: 'time', label: 'Time', icon: '⏰' },
  { value: 'file', label: 'File Upload', icon: '📎' },
  { value: 'image', label: 'Image', icon: '🖼️' },
  { value: 'signature', label: 'Signature', icon: '✍️' },
  { value: 'richtext', label: 'Rich Text', icon: '📰' },
  { value: 'json', label: 'JSON', icon: '🧾' },
  { value: 'relation', label: 'Relation', icon: '🔗' },
];

const optionConfigTypes = new Set(['Select', 'Radio', 'Checkbox']);

function defaultUiProps(): FieldUiProps {
  return {
    width: null,
    label_position: 'top',
    help_text: null,
    placeholder: null,
    prefix: null,
    suffix: null,
    readonly: false,
    disabled: false,
    hidden: false,
    css_classes: [],
    custom_attrs: {},
  };
}

function toSelectOptions(values: string[]): SelectOption[] {
  return values.map((value) => ({ value, label: value, disabled: false }));
}

/** Rust `FieldType` -> editor field type. */
export function formFieldTypeToEditor(fieldType: FieldType): EditorFormFieldType {
  switch (fieldType.type) {
    case 'Text':
      return 'text';
    case 'TextArea':
      return 'textarea';
    case 'Number':
      return 'number';
    case 'Integer':
      return 'integer';
    case 'Boolean':
      return 'boolean';
    case 'Select':
      return 'select';
    case 'Radio':
      return 'radio';
    case 'Checkbox':
      return 'checkbox';
    case 'Date':
      return 'date';
    case 'DateTime':
      return 'datetime';
    case 'Time':
      return 'time';
    case 'Email':
      return 'email';
    case 'Url':
      return 'url';
    case 'Phone':
      return 'phone';
    case 'File':
      return 'file';
    case 'Image':
      return 'image';
    case 'Signature':
      return 'signature';
    case 'RichText':
      return 'richtext';
    case 'Json':
      return 'json';
    case 'Relation':
      return 'relation';
    default:
      return 'text';
  }
}

/**
 * Editor field type -> Rust `FieldType`. When the current type already matches
 * the target variant it is returned unchanged so editor-collapsed distinctions
 * (lengths, min/max, accept lists, relation config) are preserved.
 */
export function editorTypeToFieldType(
  type: EditorFormFieldType,
  current?: FieldType,
): FieldType {
  if (current?.type === editorTypeToTag(type)) return current;
  switch (type) {
    case 'text':
      return { type: 'Text', config: { max_length: null, placeholder: null } };
    case 'textarea':
      return { type: 'TextArea', config: { max_length: null, rows: null } };
    case 'number':
      return { type: 'Number', config: { min: null, max: null, step: null } };
    case 'integer':
      return { type: 'Integer', config: { min: null, max: null } };
    case 'boolean':
      return { type: 'Boolean' };
    case 'select':
      return { type: 'Select', config: { options: [], multiple: false, searchable: false } };
    case 'radio':
      return { type: 'Radio', config: { options: [] } };
    case 'checkbox':
      return { type: 'Checkbox', config: { options: [] } };
    case 'date':
      return { type: 'Date' };
    case 'datetime':
      return { type: 'DateTime' };
    case 'time':
      return { type: 'Time' };
    case 'email':
      return { type: 'Email' };
    case 'url':
      return { type: 'Url' };
    case 'phone':
      return { type: 'Phone' };
    case 'file':
      return { type: 'File', config: { accept: null, max_size_mb: null, multiple: false } };
    case 'image':
      return { type: 'Image', config: { max_width: null, max_height: null } };
    case 'signature':
      return { type: 'Signature' };
    case 'richtext':
      return { type: 'RichText' };
    case 'json':
      return { type: 'Json' };
    case 'relation':
      return { type: 'Relation', config: { entity_id: '', display_field: '', multiple: false } };
    default:
      return { type: 'Text', config: { max_length: null, placeholder: null } };
  }
}

function editorTypeToTag(type: EditorFormFieldType): FieldType['type'] {
  const map: Record<EditorFormFieldType, FieldType['type']> = {
    text: 'Text',
    textarea: 'TextArea',
    number: 'Number',
    integer: 'Integer',
    boolean: 'Boolean',
    select: 'Select',
    radio: 'Radio',
    checkbox: 'Checkbox',
    date: 'Date',
    datetime: 'DateTime',
    time: 'Time',
    email: 'Email',
    url: 'Url',
    phone: 'Phone',
    file: 'File',
    image: 'Image',
    signature: 'Signature',
    richtext: 'RichText',
    json: 'Json',
    relation: 'Relation',
  };
  return map[type];
}

export function formFieldToEditor(field: FormField): EditorFormField {
  const fieldType = field.field_type;
  const options = optionConfigTypes.has(fieldType.type)
    ? (fieldType as { config: { options: SelectOption[] } }).config.options.map(
        (option) => option.label || String(option.value ?? ''),
      )
    : [];
  const configPlaceholder =
    fieldType.type === 'Text' || fieldType.type === 'TextArea'
      ? (fieldType as { config: { placeholder?: string | null } }).config.placeholder ?? null
      : null;
  return {
    id: field.id,
    name: field.name,
    label: field.label,
    type: formFieldTypeToEditor(fieldType),
    required: field.required,
    placeholder: field.ui_props.placeholder ?? configPlaceholder ?? '',
    options,
  };
}

export function editorToFormField(
  editor: EditorFormField,
  previous?: FormField,
): FormField {
  let fieldType = editorTypeToFieldType(editor.type, previous?.field_type);
  if (optionConfigTypes.has(fieldType.type)) {
    fieldType = {
      ...fieldType,
      config: { ...(fieldType as { config: object }).config, options: toSelectOptions(editor.options) },
    } as FieldType;
  } else if (fieldType.type === 'Text') {
    fieldType = { type: 'Text', config: { ...fieldType.config, placeholder: editor.placeholder || null } };
  }

  const uiProps: FieldUiProps = {
    ...(previous?.ui_props ?? defaultUiProps()),
    placeholder: editor.placeholder || null,
  };

  return {
    id: editor.id,
    name: editor.name,
    label: editor.label,
    field_type: fieldType,
    required: editor.required,
    default_value: previous?.default_value ?? null,
    validation_rules: previous?.validation_rules ?? [],
    conditional_visibility: previous?.conditional_visibility ?? null,
    ui_props: uiProps,
    order: previous?.order ?? 0,
  };
}

/** Create a new empty Rust-shaped form field for the given editor type. */
export function newFormField(type: EditorFormFieldType): FormField {
  return editorToFormField({
    id: generateId(),
    name: `field_${Date.now().toString(36)}`,
    label: '',
    type,
    required: false,
    placeholder: '',
    options: type === 'select' || type === 'radio' || type === 'checkbox' ? ['Option 1', 'Option 2'] : [],
  });
}

/** Create a new empty form with the canonical default nested documents. */
export function newForm(name: string): Form {
  const now = new Date().toISOString();
  return {
    id: generateId(),
    name,
    description: null,
    fields: [],
    layout: { columns: 1, column_gap: '1rem', row_gap: '1rem', fields: {} },
    validation: {
      validate_on_change: false,
      validate_on_blur: true,
      validate_on_submit: true,
      custom_validators: [],
    },
    permissions: {
      create_roles: [],
      read_roles: [],
      update_roles: [],
      delete_roles: [],
      field_permissions: {},
    },
    created_at: now,
    updated_at: now,
  };
}
