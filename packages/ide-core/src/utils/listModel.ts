/**
 * List builder model (M7 #2).
 *
 * Maps IDE entities to server-shaped list pages (a Table or List component
 * bound to a Database data source). Types come straight from
 * `types/model.generated.ts` (ts-rust-bindings output for
 * `apps/ide-tauri/src-tauri/src/model/page.rs`), so payloads round-trip
 * through sea-orm increments without shape drift.
 */

import type {
  ActionConfig,
  DataSource,
  EntityId,
  Page,
  PageAction,
  PageComponent,
  TableColumn,
} from '../types/model.generated';
import type { Entity, EntityFieldType } from '../types/project';
import { generateId } from './helpers';

/** Component layout kind produced by the list builder. */
export type ListPageKind = 'table' | 'list';

/** Field types the server can sort on; the rest become non-sortable columns. */
const SORTABLE_FIELD_TYPES: ReadonlySet<EntityFieldType['type']> = new Set([
  'String',
  'Text',
  'Integer',
  'BigInteger',
  'Float',
  'Decimal',
  'Boolean',
  'Date',
  'DateTime',
  'Time',
  'Timestamp',
  'Uuid',
  'Enum',
]);

/** True when the field type can be sorted server-side. */
export function isSortableFieldType(fieldType: EntityFieldType['type']): boolean {
  return SORTABLE_FIELD_TYPES.has(fieldType);
}

/** Derive table columns from every entity field (one column each). */
export function entityToTableColumns(entity: Entity, sortEnabled: boolean): TableColumn[] {
  return entity.fields.map((field) => ({
    key: field.id,
    title: field.name,
    data_index: field.name,
    ellipsis: true,
    filterable: false,
    children: [],
    fixed: null,
    width: null,
    sortable: sortEnabled ? isSortableFieldType(field.field_type.type) : false,
  }));
}

/** Datasource that reads the entity's rows through the Database source type. */
function newListDataSource(entity: Entity): DataSource {
  const sortField =
    entity.fields.find(
      (field) =>
        field.field_type.type === 'DateTime' || field.field_type.type === 'Timestamp',
    ) ?? entity.fields[0];

  return {
    id: generateId(),
    name: `${entity.name} Data`,
    auto_load: true,
    polling_interval: null,
    transform: null,
    source_type: {
      type: 'Database',
      config: {
        entity_id: entity.id satisfies string as EntityId,
        query: {
          select: [],
          where: [],
          having: [],
          group_by: [],
          order_by: [{ field: sortField?.name ?? 'id', direction: 'asc' }],
          limit: null,
          offset: null,
        },
      },
    },
    config: {
      timeout_ms: 30_000,
      debounce_ms: null,
      transform_error: null,
      transform_response: null,
      cache: {
        enabled: false,
        ttl_ms: 30_000,
        key: null,
        stale_while_revalidate_ms: null,
      },
      retry: {
        initial_interval_ms: 500,
        max_attempts: 3,
        max_interval_ms: 5000,
        multiplier: 2,
        retryable_codes: [429, 500, 502, 503, 504],
      },
    },
  };
}

/**
 * CRUD page actions for a list page.
 *
 * The generated `ActionConfig` vocabulary has no dedicated record-CRUD
 * variants, so record operations ride on `CallApi` against the list's data
 * source: the HTTP verb and record id travel as request params
 * (`method`, `id: "{{selection.id}}"`), which the generated runtime resolves
 * against the selected Table row.
 */
export function listCrudActions(dataSourceId: string): PageAction[] {
  const callApi = (name: string, params: Record<string, string>): PageAction => {
    const config: ActionConfig = {
      type: 'CallApi',
      config: {
        data_source_id: dataSourceId,
        params,
        on_success: [],
        on_error: [],
      },
    };
    return {
      id: generateId(),
      name,
      action_type: 'call_api',
      config,
      permissions: [],
    };
  };

  return [
    callApi('Create Record', { method: 'POST' }),
    callApi('Read Record', { method: 'GET', id: '{{selection.id}}' }),
    callApi('Update Record', { method: 'PUT', id: '{{selection.id}}' }),
    callApi('Delete Record', { method: 'DELETE', id: '{{selection.id}}' }),
    callApi('Reload Rows', { method: 'GET' }),
  ];
}

/** Build a server-shaped list page (Table or List component) from an entity. */
export function newListPage(entity: Entity, kind: ListPageKind): Page {
  const dataSource = newListDataSource(entity);
  const now = new Date().toISOString();

  const component: PageComponent =
    kind === 'table'
      ? {
          id: generateId(),
          component: {
            type: 'Table',
            config: {
              columns: entityToTableColumns(entity, true),
              data_source: dataSource.id,
              sorting: true,
              filtering: false,
              selection: {
                enabled: true,
                type: 'checkbox',
                fixed: false,
                column_title: null,
                column_width: null,
              },
              pagination: {
                enabled: true,
                page_size: 20,
                show_size_changer: true,
                show_quick_jumper: false,
                show_total: true,
              },
            },
          },
          props: {},
          bindings: [],
          events: [],
          conditions: [],
          slots: {},
          style_overrides: {},
        }
      : {
          id: generateId(),
          component: {
            type: 'List',
            config: {
              data_source: dataSource.id,
              item_template: generateId(),
              empty_state: null,
            },
          },
          props: {},
          bindings: [],
          events: [],
          conditions: [],
          slots: {},
          style_overrides: {},
        };

  return {
    id: generateId(),
    name: `${entity.plural_name} List`,
    path: `/${entity.plural_name}/list`,
    title: entity.plural_name,
    layout: {
      type: 'fluid',
      breakpoints: {},
      containers: [],
      global_styles: {},
    },
    permissions: {
      public_access: true,
      view_roles: [],
      edit_roles: [],
      admin_roles: [],
    },
    seo: {
      keywords: [],
    },
    description: null,
    data_sources: [dataSource],
    components: [component],
    actions: listCrudActions(dataSource.id),
    created_at: now,
    updated_at: now,
  };
}
