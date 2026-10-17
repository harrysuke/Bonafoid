/**
 * Mapping helpers between the editor-facing data-model shapes used by the
 * DataModeler UI and the Rust-shaped `nocode_model::entity` documents that the
 * project store owns.
 *
 * The editor intentionally exposes a smaller, friendlier vocabulary than the
 * canonical model:
 *
 * - a single `type` string instead of the adjacently tagged `EntityFieldType`;
 * - a `primaryKey` boolean instead of an `EntityConstraint` entry;
 * - `required` (inverse of `nullable`);
 * - relation `targetEntity` / `targetField` as entity and field *names*.
 *
 * These helpers are pure and lossless for the fields the editor round-trips;
 * they preserve `auto_generate`, `validation`, `comment`, `options`, `indexes`
 * and any timestamps they do not own.
 */

import { generateId } from './helpers';
import type {
  Entity,
  EntityConstraint,
  EntityField,
  EntityFieldId,
  EntityFieldType,
  EntityId,
  EntityRelation,
  RelationType,
} from '../types/project';

/* -------------------------------------------------------------------------- */
/* Editor shapes                                                              */
/* -------------------------------------------------------------------------- */

export type EditorFieldType =
  | 'string'
  | 'text'
  | 'integer'
  | 'bigint'
  | 'float'
  | 'decimal'
  | 'boolean'
  | 'date'
  | 'datetime'
  | 'json'
  | 'uuid'
  | 'enum'
  | 'relation';

export interface EditorField {
  id: string;
  name: string;
  type: EditorFieldType;
  primaryKey?: boolean;
  required?: boolean;
  unique?: boolean;
  /** Raw, user-editable text; parsed by {@link parseDefaultValue}. */
  default?: string;
}

export type EditorRelationType = 'hasOne' | 'hasMany' | 'belongsTo' | 'manyToMany';

export interface EditorRelation {
  id: string;
  name: string;
  type: EditorRelationType;
  /** Target entity name (falls back to id when the target is unresolved). */
  targetEntity: string;
  /** Target field name (falls back to id when the target is unresolved). */
  targetField: string;
}

export interface EditorEntity {
  id: string;
  name: string;
  fields: EditorField[];
  relations: EditorRelation[];
}

/* -------------------------------------------------------------------------- */
/* Field type mapping                                                         */
/* -------------------------------------------------------------------------- */

/** Rust `EntityFieldType` -> editor field type. */
export function fieldTypeToEditor(fieldType: EntityFieldType): EditorFieldType {
  switch (fieldType.type) {
    case 'String':
      return 'string';
    case 'Text':
      return 'text';
    case 'Integer':
      return 'integer';
    case 'BigInteger':
      return 'bigint';
    case 'Float':
      return 'float';
    case 'Decimal':
      return 'decimal';
    case 'Boolean':
      return 'boolean';
    case 'Date':
      return 'date';
    case 'DateTime':
    case 'Time':
    case 'Timestamp':
      return 'datetime';
    case 'Json':
    case 'Jsonb':
      return 'json';
    case 'Uuid':
      return 'uuid';
    case 'Enum':
      return 'enum';
    case 'Bytes':
      return 'string';
    case 'Relation':
      return 'relation';
    default:
      return 'string';
  }
}

/**
 * Editor field type -> Rust `EntityFieldType`. When the current type already
 * matches, it is returned unchanged so that editor-only distinctions that the
 * editor collapses (String length, Enum values, Json/Jsonb, Time/Timestamp,
 * Bytes, Relation config) are preserved.
 */
export function editorTypeToEntityFieldType(
  type: EditorFieldType,
  current?: EntityFieldType,
): EntityFieldType {
  switch (type) {
    case 'string':
      if (current?.type === 'String') return current;
      return { type: 'String', config: { max_length: 255 } };
    case 'text':
      return { type: 'Text' };
    case 'integer':
      return { type: 'Integer' };
    case 'bigint':
      return { type: 'BigInteger' };
    case 'float':
      return { type: 'Float' };
    case 'decimal':
      if (current?.type === 'Decimal') return current;
      return { type: 'Decimal', config: { precision: 10, scale: 2 } };
    case 'boolean':
      return { type: 'Boolean' };
    case 'date':
      return { type: 'Date' };
    case 'datetime':
      if (
        current?.type === 'DateTime' ||
        current?.type === 'Timestamp' ||
        current?.type === 'Time'
      ) {
        return current;
      }
      return { type: 'DateTime' };
    case 'json':
      if (current?.type === 'Json' || current?.type === 'Jsonb') return current;
      return { type: 'Json' };
    case 'uuid':
      return { type: 'Uuid' };
    case 'enum':
      if (current?.type === 'Enum') return current;
      return { type: 'Enum', config: { values: [] } };
    case 'relation':
      if (current?.type === 'Relation') return current;
      return {
        type: 'Relation',
        config: {
          type: 'BelongsTo',
          config: {
            entity_id: '',
            field: '',
            on_delete: 'cascade',
            on_update: 'cascade',
          },
        },
      };
    default:
      return { type: 'String', config: { max_length: 255 } };
  }
}

/* -------------------------------------------------------------------------- */
/* Relation type mapping                                                      */
/* -------------------------------------------------------------------------- */

/** Rust `RelationType` -> editor relation type. */
export function relationTypeToEditor(relationType: RelationType): EditorRelationType {
  switch (relationType.type) {
    case 'BelongsTo':
      return 'belongsTo';
    case 'HasOne':
      return 'hasOne';
    case 'HasMany':
      return 'hasMany';
    case 'ManyToMany':
      return 'manyToMany';
    default:
      return 'belongsTo';
  }
}

/** Editor relation type -> Rust `RelationType`. */
export function editorTypeToRelationType(
  type: EditorRelationType,
  entityId: EntityId,
  field: string,
  targetField: string,
): RelationType {
  switch (type) {
    case 'hasOne':
      return { type: 'HasOne', config: { entity_id: entityId, field } };
    case 'hasMany':
      return { type: 'HasMany', config: { entity_id: entityId, field } };
    case 'manyToMany':
      return {
        type: 'ManyToMany',
        config: { entity_id: entityId, through: null, field, related_field: targetField },
      };
    default:
      return {
        type: 'BelongsTo',
        config: { entity_id: entityId, field, on_delete: 'cascade', on_update: 'cascade' },
      };
  }
}

/* -------------------------------------------------------------------------- */
/* Default value mapping                                                      */
/* -------------------------------------------------------------------------- */

/** Parse raw editor text into a JSON-compatible default (as Rust stores it). */
export function parseDefaultValue(raw?: string): unknown | null {
  if (raw === undefined) return null;
  const trimmed = raw.trim();
  if (trimmed === '') return null;
  try {
    return JSON.parse(trimmed) as unknown;
  } catch {
    return trimmed;
  }
}

/** Render a JSON-compatible default into editable text. */
export function formatDefaultValue(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return value;
  return JSON.stringify(value);
}

/* -------------------------------------------------------------------------- */
/* Entity round-tripping                                                      */
/* -------------------------------------------------------------------------- */

function primaryKeyConstraint(entity: Entity): EntityConstraint | undefined {
  return entity.constraints.find((constraint) => constraint.constraint_type === 'primary_key');
}

function isPrimaryKeyField(entity: Entity, fieldId: EntityFieldId): boolean {
  const primaryKey = primaryKeyConstraint(entity);
  return primaryKey ? primaryKey.fields.includes(fieldId) : false;
}

function entityByName(
  entities: Record<EntityId, Entity>,
  reference: string,
): Entity | undefined {
  return Object.values(entities).find(
    (entity) => entity.name === reference || entity.id === reference,
  );
}

function fieldByName(entity: Entity | undefined, reference: string): EntityField | undefined {
  if (!entity) return undefined;
  return entity.fields.find((field) => field.name === reference || field.id === reference);
}

/** Rust `Entity` -> editor entity (resolves relation targets to names). */
export function entityToEditor(
  entity: Entity,
  entities: Record<EntityId, Entity>,
): EditorEntity {
  return {
    id: entity.id,
    name: entity.name,
    fields: entity.fields.map((field) => ({
      id: field.id,
      name: field.name,
      type: fieldTypeToEditor(field.field_type),
      primaryKey: isPrimaryKeyField(entity, field.id),
      required: !field.nullable,
      unique: field.unique,
      default: formatDefaultValue(field.default),
    })),
    relations: entity.relations.map((relation) => {
      const target = entities[relation.to_entity];
      const targetField = target?.fields.find((field) => field.id === relation.to_field);
      return {
        id: relation.id,
        name: relation.name,
        type: relationTypeToEditor(relation.relation_type),
        targetEntity: target?.name ?? relation.to_entity,
        targetField: targetField?.name ?? relation.to_field,
      };
    }),
  };
}

/**
 * Editor entity -> Rust `Entity`, preserving everything the editor does not own
 * (`options`, `indexes`, non-primary-key constraints, `created_at`, unknown
 * fields) and folding `primaryKey` back into an `EntityConstraint`.
 */
export function editorToEntity(
  editor: EditorEntity,
  existing: Entity,
  entities: Record<EntityId, Entity>,
): Entity {
  const fields: EntityField[] = editor.fields.map((field, index) => {
    const previous = existing.fields.find((candidate) => candidate.id === field.id);
    return {
      id: field.id,
      name: field.name,
      field_type: editorTypeToEntityFieldType(field.type, previous?.field_type),
      nullable: !field.required,
      unique: field.unique ?? false,
      default: parseDefaultValue(field.default),
      auto_generate: previous?.auto_generate ?? null,
      validation: previous?.validation ?? null,
      comment: previous?.comment ?? null,
      order: index,
    };
  });

  const previousPrimaryKey = primaryKeyConstraint(existing);
  const primaryKeyField = editor.fields.find((field) => field.primaryKey);
  const constraints = existing.constraints.filter(
    (constraint) => constraint.constraint_type !== 'primary_key',
  );
  if (primaryKeyField) {
    constraints.push({
      id: previousPrimaryKey?.id ?? generateId(),
      name: `pk_${editor.name}`,
      constraint_type: 'primary_key',
      fields: [primaryKeyField.id],
      expression: null,
    });
  }

  const relations: EntityRelation[] = editor.relations.map((relation) => {
    const previous = existing.relations.find((candidate) => candidate.id === relation.id);
    const target = entityByName(entities, relation.targetEntity);
    const toEntityId = target?.id ?? relation.targetEntity;

    const fallbackFrom = fields.find((field) => field.id === primaryKeyField?.id) ?? fields[0];
    const fromFieldId =
      previous && fields.some((field) => field.id === previous.from_field)
        ? previous.from_field
        : fallbackFrom?.id ?? '';

    const targetField = fieldByName(target, relation.targetField);
    const toFieldId =
      previous && target?.fields.some((field) => field.id === previous.to_field)
        ? previous.to_field
        : targetField?.id ?? target?.fields[0]?.id ?? '';

    return {
      id: relation.id,
      name: relation.name,
      relation_type: editorTypeToRelationType(
        relation.type,
        toEntityId,
        fromFieldId,
        toFieldId,
      ),
      from_field: fromFieldId,
      to_entity: toEntityId,
      to_field: toFieldId,
      on_delete: previous?.on_delete ?? 'cascade',
      on_update: previous?.on_update ?? 'cascade',
    };
  });

  return {
    ...existing,
    id: editor.id,
    name: editor.name,
    fields,
    relations,
    constraints,
  };
}
