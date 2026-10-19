/**
 * Tests for the editor <-> Rust mapping used by the data modeler.
 *
 * These functions decide what gets written into project.json, so a silent
 * round-trip loss here is data loss in the user's project.
 */

import { describe, expect, it } from 'vitest';
import {
  editorTypeToEntityFieldType,
  editorTypeToRelationType,
  entityToEditor,
  fieldTypeToEditor,
  formatDefaultValue,
  parseDefaultValue,
  relationTypeToEditor,
} from './dataModel';
import type { EntityField, RelationType } from '../types/project';

function field(overrides: Partial<EntityField> = {}): EntityField {
  return {
    id: 'f-id',
    name: 'id',
    field_type: { type: 'Uuid' },
    nullable: false,
    unique: false,
    default: null,
    auto_generate: null,
    validation: null,
    comment: null,
    order: 0,
    options: null,
    ...overrides,
  } as EntityField;
}

describe('parseDefaultValue', () => {
  it('returns null for undefined and blank input', () => {
    expect(parseDefaultValue(undefined)).toBeNull();
    expect(parseDefaultValue('')).toBeNull();
    expect(parseDefaultValue('   ')).toBeNull();
  });

  it('parses JSON scalars', () => {
    expect(parseDefaultValue('42')).toBe(42);
    expect(parseDefaultValue('true')).toBe(true);
    expect(parseDefaultValue('null')).toBeNull();
    expect(parseDefaultValue('"quoted"')).toBe('quoted');
  });

  it('falls back to the raw text when it is not JSON', () => {
    // A bare word is not valid JSON, but it is a perfectly valid string default.
    expect(parseDefaultValue('pending')).toBe('pending');
  });
});

describe('formatDefaultValue', () => {
  it('renders nullish values as empty text', () => {
    expect(formatDefaultValue(null)).toBe('');
    expect(formatDefaultValue(undefined)).toBe('');
  });

  it('leaves strings unquoted so they stay editable', () => {
    expect(formatDefaultValue('pending')).toBe('pending');
  });

  it('round-trips structured defaults', () => {
    expect(parseDefaultValue(formatDefaultValue({ a: 1 }))).toEqual({ a: 1 });
    expect(parseDefaultValue(formatDefaultValue(42))).toBe(42);
  });
});

describe('fieldTypeToEditor', () => {
  it('maps the scalar Rust types', () => {
    expect(fieldTypeToEditor({ type: 'String', config: { max_length: null } })).toBe('string');
    expect(fieldTypeToEditor({ type: 'Integer' })).toBe('integer');
    expect(fieldTypeToEditor({ type: 'Boolean' })).toBe('boolean');
    expect(fieldTypeToEditor({ type: 'DateTime' })).toBe('datetime');
  });

  it('maps enum and relation types', () => {
    expect(fieldTypeToEditor({ type: 'Enum', config: { values: ['a', 'b'] } })).toBe('enum');
    expect(fieldTypeToEditor({ type: 'Text' })).toBe('text');
  });

  it('falls back to string for a type the editor does not model', () => {
    expect(fieldTypeToEditor({ type: 'NotARealType' } as never)).toBe('string');
  });
});

describe('editorTypeToEntityFieldType', () => {
  it('maps the plain editor types back to Rust', () => {
    expect(editorTypeToEntityFieldType('text')).toEqual({ type: 'Text' });
    expect(editorTypeToEntityFieldType('integer')).toEqual({ type: 'Integer' });
    expect(editorTypeToEntityFieldType('boolean')).toEqual({ type: 'Boolean' });
    expect(editorTypeToEntityFieldType('datetime')).toEqual({ type: 'DateTime' });
  });

  it('gives the sized types a default config when there is no current type', () => {
    expect(editorTypeToEntityFieldType('string')).toEqual({
      type: 'String',
      config: { max_length: 255 },
    });
    expect(editorTypeToEntityFieldType('decimal')).toEqual({
      type: 'Decimal',
      config: { precision: 10, scale: 2 },
    });
  });

  it('preserves the existing config of a sized type', () => {
    // This is the property that matters: editing a field in the builder must
    // not silently reset a max_length the user set.
    const current = { type: 'String', config: { max_length: 120 } } as never;
    expect(editorTypeToEntityFieldType('string', current)).toEqual(current);

    const decimal = { type: 'Decimal', config: { precision: 6, scale: 3 } } as never;
    expect(editorTypeToEntityFieldType('decimal', decimal)).toEqual(decimal);
  });

  it('does not carry a config across types', () => {
    const current = { type: 'String', config: { max_length: 120 } } as never;
    expect(editorTypeToEntityFieldType('integer', current)).toEqual({ type: 'Integer' });
  });

  it('round-trips the type tag of the scalar types', () => {
    for (const rustType of [
      { type: 'Text' },
      { type: 'Integer' },
      { type: 'Boolean' },
      { type: 'DateTime' },
    ] as const) {
      expect(editorTypeToEntityFieldType(fieldTypeToEditor(rustType as never))).toEqual(rustType);
    }
  });
});

describe('relation type mapping', () => {
  it('maps every Rust relation type to an editor type', () => {
    expect(relationTypeToEditor({ type: 'BelongsTo' } as never)).toBe('belongsTo');
    expect(relationTypeToEditor({ type: 'HasOne' } as never)).toBe('hasOne');
    expect(relationTypeToEditor({ type: 'HasMany' } as never)).toBe('hasMany');
    expect(relationTypeToEditor({ type: 'ManyToMany' } as never)).toBe('manyToMany');
  });

  it('defaults an unknown relation type to belongsTo', () => {
    expect(relationTypeToEditor({ type: 'Nonsense' } as never)).toBe('belongsTo');
  });

  it('builds the tagged Rust shape the editor expects', () => {
    const belongsTo = editorTypeToRelationType('belongsTo', 'e1', 'customer', 'id');
    expect(belongsTo.type).toBe('BelongsTo');
    expect(belongsTo).toMatchObject({ config: { entity_id: 'e1', field: 'customer' } });

    const manyToMany = editorTypeToRelationType('manyToMany', 'e1', 'tags', 'tag_id');
    expect(manyToMany.type).toBe('ManyToMany');
    expect(manyToMany).toMatchObject({ config: { related_field: 'tag_id', through: null } });
  });

  it('round-trips the relation type tag', () => {
    const types: RelationType['type'][] = ['BelongsTo', 'HasOne', 'HasMany', 'ManyToMany'];
    for (const type of types) {
      const editorType = relationTypeToEditor({ type } as never);
      expect(editorTypeToRelationType(editorType, 'e1', 'f', 'g').type).toBe(type);
    }
  });
});

describe('entityToEditor', () => {
  it('marks the field named by the primary key constraint', () => {
    const customer = {
      id: 'e1',
      name: 'Customer',
      plural_name: 'Customers',
      description: null,
      fields: [field()],
      relations: [],
      indexes: [],
      constraints: [{ name: 'pk', constraint_type: 'primary_key', fields: ['f-id'] }],
      options: null,
    };

    const editor = entityToEditor(customer as never, { e1: customer } as never);
    expect(editor.fields[0]?.primaryKey).toBe(true);
  });

  it('leaves every field unmarked when the entity has no primary key', () => {
    const entity = {
      id: 'e1',
      name: 'Customer',
      plural_name: 'Customers',
      description: null,
      fields: [field()],
      relations: [],
      indexes: [],
      constraints: [],
      options: null,
    };

    const editor = entityToEditor(entity as never, { e1: entity } as never);
    expect(editor.fields[0]?.primaryKey).toBe(false);
  });
});
