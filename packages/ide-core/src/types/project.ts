/**
 * TypeScript view of the Rust `nocode_model::Project` and its nested documents.
 *
 * The IDE keeps the full project as an opaque JSON document owned by the Rust
 * side; these types mirror the `serde` wire shapes so the UI can read and edit
 * it without guessing:
 *
 * - Newtype ids (`ProjectId`, `EntityId`, `ThemeId`, ...) serialize as plain
 *   UUID strings.
 * - `IndexMap<K, V>` serializes as a JSON object keyed by the id string.
 * - `DateTime<Utc>` serializes as an RFC 3339 string.
 * - Adjacently tagged enums (`#[serde(tag = "type", content = "config")]`) use
 *   the PascalCase Rust variant name as the `type` tag; unit variants omit
 *   `config` entirely.
 * - `#[serde(rename_all = "snake_case")]` enums use snake_case values (note
 *   `HttpMethod` is `rename_all = "UPPERCASE"`).
 *
 * The Rust model derives `schemars::JsonSchema` across `crates/model/src`; the
 * generated types from `scripts/gen-model-types.mjs` are checked against the
 * `model.schema.json` artifact in CI so the two cannot drift silently.
 *
 * Names match the Rust `schemars` names to keep parity. `nocode_model::entity`
 * and `nocode_model::form` used to both define a `FieldId`, which made the
 * crate-root re-export ambiguous; they are now `EntityFieldId` and
 * `FormFieldId`, and the aliases below mirror those names.
 */

/** Any `*Id` newtype wrapping `Uuid` serializes as a string. */
export type Uuid = string;

/* -------------------------------------------------------------------------- */
/* Project                                                                     */
/* -------------------------------------------------------------------------- */

/**
 * The database drivers a project can use.
 *
 * SQLite is first because it is the default: a project needs no database
 * server to run. The order must match `DB_DRIVERS` below.
 */
export type DatabaseDriver =
  | 'sqlite'
  | 'postgres'
  | 'mysql'
  | 'sql_server'
  | 'oracle';

export interface DatabaseConfig {
  driver: DatabaseDriver;
  url: string;
  max_connections: number;
  min_connections: number;
  connect_timeout_seconds: number;
  idle_timeout_seconds: number;
  max_lifetime_seconds: number;
  [key: string]: unknown;
}

export interface FrontendConfig {
  framework: string;
  bundler: string;
  [key: string]: unknown;
}

/**
 * `ProjectSettings` mirrors the Rust struct field-by-field for the sections the
 * IDE inspects today; the nested config structs stay permissive until M7.
 */
export interface ProjectSettings {
  default_locale: string;
  supported_locales: string[];
  timezone: string;
  date_format: string;
  time_format: string;
  number_format: Record<string, unknown>;
  currency: Record<string, unknown>;
  authentication: Record<string, unknown>;
  api: Record<string, unknown>;
  database: DatabaseConfig;
  frontend: FrontendConfig;
  storage: Record<string, unknown>;
  email: Record<string, unknown>;
  features: Record<string, unknown>;
  custom: Record<string, unknown>;
  [key: string]: unknown;
}

export type EntityId = Uuid;

export interface Entity {
  id: EntityId;
  name: string;
  plural_name: string;
  description: string | null;
  fields: EntityField[];
  relations: EntityRelation[];
  indexes: EntityIndex[];
  constraints: EntityConstraint[];
  options: EntityOptions;
  created_at: string;
  updated_at: string;
}

/** `nocode_model::entity::EntityFieldId`. */
export type EntityFieldId = Uuid;

export type AutoGenerate =
  | 'uuid'
  | 'uuid_v7'
  | 'nanoid'
  | 'increment'
  | 'timestamp'
  | { custom: string };

export type OnDelete =
  | 'cascade'
  | 'set_null'
  | 'restrict'
  | 'no_action'
  | 'set_default';

export type OnUpdate = OnDelete;

export interface BelongsToConfig {
  entity_id: EntityId;
  field: string;
  on_delete: OnDelete;
  on_update: OnUpdate;
}

export interface HasOneConfig {
  entity_id: EntityId;
  field: string;
}

export interface HasManyConfig {
  entity_id: EntityId;
  field: string;
}

export interface ManyToManyConfig {
  entity_id: EntityId;
  through: string | null;
  field: string;
  related_field: string;
}

/** `RelationType` is adjacently tagged with the PascalCase variant name. */
export type RelationType =
  | { type: 'BelongsTo'; config: BelongsToConfig }
  | { type: 'HasOne'; config: HasOneConfig }
  | { type: 'HasMany'; config: HasManyConfig }
  | { type: 'ManyToMany'; config: ManyToManyConfig };

/** `EntityFieldType` is adjacently tagged with the PascalCase variant name. */
export type EntityFieldType =
  | { type: 'String'; config: { max_length: number | null } }
  | { type: 'Text' }
  | { type: 'Integer' }
  | { type: 'BigInteger' }
  | { type: 'Float' }
  | { type: 'Decimal'; config: { precision: number; scale: number } }
  | { type: 'Boolean' }
  | { type: 'Date' }
  | { type: 'DateTime' }
  | { type: 'Time' }
  | { type: 'Timestamp' }
  | { type: 'Json' }
  | { type: 'Jsonb' }
  | { type: 'Uuid' }
  | { type: 'Enum'; config: { values: string[] } }
  | { type: 'Bytes' }
  | { type: 'Relation'; config: RelationType };

export interface EntityField {
  id: EntityFieldId;
  name: string;
  field_type: EntityFieldType;
  nullable: boolean;
  unique: boolean;
  default: unknown | null;
  auto_generate: AutoGenerate | null;
  validation: string | null;
  comment: string | null;
  order: number;
}

export type RelationId = Uuid;

export interface EntityRelation {
  id: RelationId;
  name: string;
  relation_type: RelationType;
  from_field: EntityFieldId;
  to_entity: EntityId;
  to_field: EntityFieldId;
  on_delete: OnDelete;
  on_update: OnUpdate;
}

export type IndexType = 'b_tree' | 'hash' | 'gin' | 'gist' | 'sp_gist' | 'brin';

export type IndexId = Uuid;

export interface EntityIndex {
  id: IndexId;
  name: string;
  fields: EntityFieldId[];
  unique: boolean;
  index_type: IndexType;
  condition: string | null;
}

export type ConstraintType =
  | 'primary_key'
  | 'unique'
  | 'check'
  | 'exclusion'
  | 'foreign_key';

export type ConstraintId = Uuid;

export interface EntityConstraint {
  id: ConstraintId;
  name: string;
  constraint_type: ConstraintType;
  fields: EntityFieldId[];
  expression: string | null;
}

export type RlsCommand = 'all' | 'select' | 'insert' | 'update' | 'delete';

export interface RlsPolicy {
  name: string;
  command: RlsCommand;
  roles: string[];
  using_expression: string;
  with_check_expression: string | null;
}

export interface EntityOptions {
  table_name: string | null;
  schema: string | null;
  comment: string | null;
  enable_rls: boolean;
  rls_policies: RlsPolicy[];
  audit_log: boolean;
  soft_delete: boolean;
  timestamps: boolean;
}

/* -------------------------------------------------------------------------- */
/* Nested documents (modelled permissively until the M7 builders)             */
/* -------------------------------------------------------------------------- */

export type FormId = Uuid;

/** `nocode_model::form::FormFieldId`. */
export type FormFieldId = Uuid;

export interface SelectOption {
  value: unknown;
  label: string;
  disabled: boolean;
}

export type VisibilityOperator =
  | 'equals'
  | 'not_equals'
  | 'contains'
  | 'not_contains'
  | 'greater_than'
  | 'less_than'
  | 'in'
  | 'not_in'
  | 'is_empty'
  | 'is_not_empty';

export type LabelPosition = 'top' | 'left' | 'right' | 'inline' | 'floating';

export interface ConditionalVisibility {
  field_id: FormFieldId;
  operator: VisibilityOperator;
  value: unknown;
}

/** `ValidationRule` is externally tagged (no `serde` rename). */
export type ValidationRule =
  | { Required: { message: string } }
  | { MinLength: { min: number; message: string } }
  | { MaxLength: { max: number; message: string } }
  | { Pattern: { regex: string; message: string } }
  | { Min: { value: number; message: string } }
  | { Max: { value: number; message: string } }
  | { Email: { message: string } }
  | { Url: { message: string } }
  | { Custom: { expression: string; message: string } };

export interface FieldUiProps {
  width: string | null;
  label_position: LabelPosition;
  help_text: string | null;
  placeholder: string | null;
  prefix: string | null;
  suffix: string | null;
  readonly: boolean;
  disabled: boolean;
  hidden: boolean;
  css_classes: string[];
  custom_attrs: Record<string, string>;
}

/** `FieldType` is adjacently tagged with the PascalCase variant name. */
export type FieldType =
  | { type: 'Text'; config: { max_length: number | null; placeholder: string | null } }
  | { type: 'TextArea'; config: { max_length: number | null; rows: number | null } }
  | { type: 'Number'; config: { min: number | null; max: number | null; step: number | null } }
  | { type: 'Integer'; config: { min: number | null; max: number | null } }
  | { type: 'Boolean' }
  | { type: 'Select'; config: { options: SelectOption[]; multiple: boolean; searchable: boolean } }
  | { type: 'Radio'; config: { options: SelectOption[] } }
  | { type: 'Checkbox'; config: { options: SelectOption[] } }
  | { type: 'Date' }
  | { type: 'DateTime' }
  | { type: 'Time' }
  | { type: 'Email' }
  | { type: 'Url' }
  | { type: 'Phone' }
  | { type: 'File'; config: { accept: string | null; max_size_mb: number | null; multiple: boolean } }
  | { type: 'Image'; config: { max_width: number | null; max_height: number | null } }
  | { type: 'Signature' }
  | { type: 'RichText' }
  | { type: 'Json' }
  | { type: 'Relation'; config: { entity_id: EntityId; display_field: string; multiple: boolean } }
  | { type: 'Computed'; config: { expression: string; return_type: FieldType } }
  | { type: 'Section'; config: { fields: FormField[] } }
  | { type: 'Repeater'; config: { fields: FormField[]; min: number; max: number | null } };

export interface FormField {
  id: FormFieldId;
  name: string;
  label: string;
  field_type: FieldType;
  required: boolean;
  default_value: unknown | null;
  validation_rules: ValidationRule[];
  conditional_visibility: ConditionalVisibility | null;
  ui_props: FieldUiProps;
  order: number;
}

export interface FieldLayout {
  column_start: number;
  column_span: number;
  row_start: number | null;
  row_span: number | null;
}

export interface FormLayout {
  columns: number;
  column_gap: string;
  row_gap: string;
  fields: Record<FormFieldId, FieldLayout>;
}

export interface CustomValidator {
  name: string;
  expression: string;
  message: string;
  fields: FormFieldId[];
}

export interface FormValidation {
  validate_on_change: boolean;
  validate_on_blur: boolean;
  validate_on_submit: boolean;
  custom_validators: CustomValidator[];
}

export interface FieldPermissions {
  read_roles: string[];
  write_roles: string[];
}

export interface FormPermissions {
  create_roles: string[];
  read_roles: string[];
  update_roles: string[];
  delete_roles: string[];
  field_permissions: Record<FormFieldId, FieldPermissions>;
}

export interface Form {
  id: FormId;
  name: string;
  description: string | null;
  fields: FormField[];
  layout: FormLayout;
  validation: FormValidation;
  permissions: FormPermissions;
  created_at: string;
  updated_at: string;
}

export type WorkflowId = Uuid;

export interface Workflow {
  id: WorkflowId;
  name: string;
  [key: string]: unknown;
}

export type PageId = Uuid;

export interface Page {
  id: PageId;
  name: string;
  path: string;
  [key: string]: unknown;
}

export type ThemeId = Uuid;

export interface Theme {
  id: ThemeId;
  name: string;
  [key: string]: unknown;
}

export interface PluginConfig {
  id: string;
  name: string;
  [key: string]: unknown;
}

export interface EnvironmentConfig {
  name: string;
  [key: string]: unknown;
}

export interface Project {
  id: Uuid;
  name: string;
  slug: string;
  description: string | null;
  version: string;
  icon: string | null;
  settings: ProjectSettings;
  forms: Record<FormId, Form>;
  entities: Record<EntityId, Entity>;
  workflows: Record<WorkflowId, Workflow>;
  pages: Record<PageId, Page>;
  themes: Record<ThemeId, Theme>;
  active_theme_id: ThemeId | null;
  plugins: PluginConfig[];
  environments: Record<string, EnvironmentConfig>;
  created_at: string;
  updated_at: string;
  published_at: string | null;
}

export interface GenerationReport {
  output_dir: string;
  backend_files: number;
  frontend_files: number;
  migrations: number;
}

export interface Option {
  value: string;
  label: string;
}

/**
 * The database drivers a project can choose, in the order they are offered.
 *
 * Values must match the `snake_case` serde representation of
 * `DatabaseDriver`. SQLite is first because it is the default: a project
 * needs no database server to run.
 */
export const DB_DRIVERS: Option[] = [
  { value: 'sqlite', label: 'SQLite' },
  { value: 'postgres', label: 'PostgreSQL' },
  { value: 'mysql', label: 'MySQL' },
  { value: 'sql_server', label: 'SQL Server' },
  { value: 'oracle', label: 'Oracle' },
];

/** Values must match the `snake_case` serde representation of `FrontendFramework`. */
export const FRONTEND_FRAMEWORKS: Option[] = [
  { value: 'leptos', label: 'Leptos (Rust / WASM)' },
];

/** Values must match the `snake_case` serde representation of `Bundler`. */
export const BUNDLERS: Option[] = [
  { value: 'trunk', label: 'Trunk' },
  { value: 'vite', label: 'Vite' },
  { value: 'webpack', label: 'Webpack' },
  { value: 'rollup', label: 'Rollup' },
  { value: 'parcel', label: 'Parcel' },
];
