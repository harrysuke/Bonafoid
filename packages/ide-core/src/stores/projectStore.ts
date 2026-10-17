import { create } from 'zustand';
import { getPlatform } from '../platform/registry';
import { generateId } from '../utils/helpers';
import {
  editorTypeToEntityFieldType,
  editorTypeToRelationType,
  parseDefaultValue,
  relationTypeToEditor,
} from '../utils/dataModel';
import type { EditorFieldType, EditorRelationType } from '../utils/dataModel';
import {
  editorToFormField,
  formFieldToEditor,
  newForm,
  newFormField,
} from '../utils/formModel';
import type { EditorFormFieldType } from '../utils/formModel';
import type {
  Entity,
  EntityConstraint,
  EntityField,
  EntityId,
  EntityOptions,
  EntityRelation,
  Form,
  FormFieldId,
  FormId,
  GenerationReport,
  PageId,
  Project,
} from '../types/project';
import { newListPage } from '../utils/listModel';
import type { ListPageKind } from '../utils/listModel';
import { newPage, newPageAction, newPageComponent, pagePathFromName } from '../utils/pageModel';
import type { PageMetaPatch, PageComponentPatch } from '../utils/pageModel';
import {
  newWorkflow,
  newWorkflowEdge,
  newWorkflowNode,
} from '../utils/workflowModel';
import type {
  WorkflowEdgePatch,
  WorkflowMetaPatch,
  WorkflowNodePatch,
} from '../utils/workflowModel';
import type {
  ActionConfig,
  ActionType,
  ComponentType,
  DataBinding,
  NodeId,
  Page as GeneratedPage,
  PageComponent as GeneratedPageComponent,
  Workflow as GeneratedWorkflow,
  WorkflowNodeType,
} from '../types/model.generated';

const AUTOSAVE_DELAY = 1500;

let autosaveTimer: ReturnType<typeof setTimeout> | null = null;

export interface FieldPatch {
  name?: string;
  type?: EditorFieldType;
  required?: boolean;
  unique?: boolean;
  default?: string;
}

export interface RelationPatch {
  name?: string;
  type?: EditorRelationType;
  targetEntity?: string;
  targetField?: string;
}

export interface FormFieldPatch {
  name?: string;
  label?: string;
  type?: EditorFormFieldType;
  required?: boolean;
  placeholder?: string;
  options?: string[];
}

interface ProjectState {
  project: Project | null;
  projectPath: string | null;
  isDirty: boolean;
  loading: boolean;
  error: string | null;

  init: () => Promise<void>;
  newProject: (name: string) => Promise<void>;
  seedSample: () => Promise<void>;
  openProject: () => Promise<void>;
  saveProject: () => Promise<void>;
  saveProjectAs: () => Promise<void>;
  setDbDriver: (driver: string) => Promise<void>;
  setFrontendFramework: (framework: string) => Promise<void>;
  generate: (outputDir: string) => Promise<GenerationReport>;

  addEntity: (name: string) => string;
  removeEntity: (id: string) => void;
  renameEntity: (id: string, name: string) => void;
  addField: (entityId: string) => string;
  updateField: (entityId: string, fieldId: string, patch: FieldPatch) => void;
  removeField: (entityId: string, fieldId: string) => void;
  addRelation: (entityId: string) => string;
  updateRelation: (entityId: string, relationId: string, patch: RelationPatch) => void;
  removeRelation: (entityId: string, relationId: string) => void;
  setPrimaryKey: (entityId: string, fieldId: string, on: boolean) => void;

  addForm: (name: string) => string;
  removeForm: (id: string) => void;
  renameForm: (id: string, name: string) => void;
  addFormField: (formId: string, type: EditorFormFieldType) => string;
  updateFormField: (formId: string, fieldId: string, patch: FormFieldPatch) => void;
  removeFormField: (formId: string, fieldId: string) => void;
  moveFormField: (formId: string, fromIndex: number, toIndex: number) => void;

  addListPage: (entityId: string, kind: ListPageKind) => string;
  removePage: (id: string) => void;

  addPage: (name: string) => string;
  updatePageMeta: (pageId: string, patch: PageMetaPatch) => void;
  addPageComponent: (pageId: string, type: ComponentType['type']) => string;
  updatePageComponent: (pageId: string, componentId: string, patch: PageComponentPatch) => void;
  removePageComponent: (pageId: string, componentId: string) => void;
  movePageComponent: (pageId: string, fromIndex: number, toIndex: number) => void;
  duplicatePageComponent: (pageId: string, componentId: string) => string;
  addPageComponentBinding: (pageId: string, componentId: string, binding: DataBinding) => void;
  removePageComponentBinding: (pageId: string, componentId: string, index: number) => void;
  addPageAction: (
    pageId: string,
    name: string,
    actionType: ActionType,
    config: ActionConfig,
  ) => string;
  removePageAction: (pageId: string, actionId: string) => void;

  addWorkflow: (name: string) => string;
  removeWorkflow: (id: string) => void;
  updateWorkflowMeta: (workflowId: string, patch: WorkflowMetaPatch) => void;
  addWorkflowNode: (workflowId: string, nodeType: WorkflowNodeType['type'], x: number, y: number) => string;
  updateWorkflowNode: (workflowId: string, nodeId: string, patch: WorkflowNodePatch) => void;
  moveWorkflowNode: (workflowId: string, nodeId: string, x: number, y: number) => void;
  removeWorkflowNode: (workflowId: string, nodeId: string) => void;
  connectWorkflowNodes: (workflowId: string, source: NodeId, target: NodeId) => string;
  updateWorkflowEdge: (workflowId: string, edgeId: string, patch: WorkflowEdgePatch) => void;
  removeWorkflowEdge: (workflowId: string, edgeId: string) => void;
  importWorkflow: (workflowJson: string) => string;

  markDirty: () => void;
  autosave: () => Promise<void>;
}

function parse(json: string): Project {
  return JSON.parse(json) as Project;
}

function defaultEntityOptions(): EntityOptions {
  return {
    table_name: null,
    schema: null,
    comment: null,
    enable_rls: false,
    rls_policies: [],
    audit_log: false,
    soft_delete: false,
    timestamps: true,
  };
}

function primaryKeyFieldId(entity: Entity): string {
  const constraint = entity.constraints.find((c) => c.constraint_type === 'primary_key');
  return constraint?.fields[0] ?? entity.fields[0]?.id ?? '';
}

export const useProjectStore = create<ProjectState>((set, get) => {
  const commit = (project: Project) => {
    set({ project, isDirty: true });
    if (autosaveTimer !== null) clearTimeout(autosaveTimer);
    autosaveTimer = setTimeout(() => {
      autosaveTimer = null;
      void get().autosave();
    }, AUTOSAVE_DELAY);
  };

  const applyEntity = (entityId: EntityId, entity: Entity) => {
    const current = get().project;
    if (!current) return;
    commit({
      ...current,
      entities: { ...current.entities, [entityId]: entity },
      updated_at: new Date().toISOString(),
    });
  };

  const applyForm = (formId: FormId, form: Form) => {
    const current = get().project;
    if (!current) return;
    commit({
      ...current,
      forms: { ...current.forms, [formId]: form },
      updated_at: new Date().toISOString(),
    });
  };

  /**
   * Page mutations work on the generated `Page` shape and cast back into the
   * permissive editor record. This is the single editor/Rust boundary for pages.
   */
  const applyPage = (pageId: string, page: GeneratedPage) => {
    const current = get().project;
    if (!current) return;
    commit({
      ...current,
      pages: {
        ...current.pages,
        [pageId as PageId]: page as unknown as Project['pages'][string],
      },
      updated_at: new Date().toISOString(),
    });
  };

  const readPage = (pageId: string): GeneratedPage | null => {
    const stored = get().project?.pages[pageId];
    return stored ? (stored as unknown as GeneratedPage) : null;
  };

  const applyComponent = (
    page: GeneratedPage,
    componentId: string,
    mutate: (component: GeneratedPageComponent) => GeneratedPageComponent,
  ): void => {
    let found = false;
    const components = page.components.map((component) => {
      if (component.id !== componentId) return component;
      found = true;
      return mutate(component);
    });
    if (!found) return;
    applyPage(page.id, { ...page, components, updated_at: new Date().toISOString() });
  };

  /** Workflow mutations mirror the page helpers; same single cast boundary. */
  const applyWorkflow = (workflowId: string, workflow: GeneratedWorkflow) => {
    const current = get().project;
    if (!current) return;
    commit({
      ...current,
      workflows: {
        ...current.workflows,
        [workflowId]: workflow as unknown as Project['workflows'][string],
      },
      updated_at: new Date().toISOString(),
    });
  };

  const readWorkflow = (workflowId: string): GeneratedWorkflow | null => {
    const stored = get().project?.workflows[workflowId];
    if (!stored || typeof stored !== 'object') return null;
    const workflow = stored as unknown as GeneratedWorkflow;
    return Array.isArray(workflow.nodes) && Array.isArray(workflow.edges) ? workflow : null;
  };

  return {
    project: null,
    projectPath: null,
    isDirty: false,
    loading: false,
    error: null,

    init: async () => {
      set({ loading: true, error: null });
      try {
        const platform = getPlatform();
        const existing = await platform.getCurrentProject();
        if (existing) {
          const path = await platform.getProjectPath();
          set({ project: parse(existing), projectPath: path, loading: false });
        } else if (platform.autoSeedSample) {
          const seeded = await platform.seedSampleProject();
          set({ project: parse(seeded), projectPath: null, loading: false });
        } else {
          set({ project: null, projectPath: null, loading: false });
        }
      } catch (e) {
        set({ error: (e as Error).message, loading: false });
      }
    },

    newProject: async (name) => {
      try {
        const json = await getPlatform().newProject(name);
        set({ project: parse(json), projectPath: null, isDirty: true, error: null });
      } catch (e) {
        set({ error: (e as Error).message });
      }
    },

    seedSample: async () => {
      try {
        const json = await getPlatform().seedSampleProject();
        set({ project: parse(json), projectPath: null, isDirty: false, error: null });
      } catch (e) {
        set({ error: (e as Error).message });
      }
    },

    openProject: async () => {
      try {
        const platform = getPlatform();
        const path = await platform.pickOpenPath();
        if (!path) return;
        const json = await platform.loadProject(path);
        set({ project: parse(json), projectPath: path, isDirty: false, error: null });
      } catch (e) {
        set({ error: (e as Error).message });
      }
    },

    saveProject: async () => {
      const { projectPath, project } = get();
      if (!project) {
        set({ error: 'No project loaded' });
        return;
      }
      if (!projectPath) {
        await get().saveProjectAs();
        return;
      }
      try {
        const platform = getPlatform();
        await platform.updateProject(JSON.stringify(project));
        await platform.saveProject(projectPath);
        set({ isDirty: false, error: null });
      } catch (e) {
        set({ error: (e as Error).message });
      }
    },

    saveProjectAs: async () => {
      const { project } = get();
      if (!project) {
        set({ error: 'No project loaded' });
        return;
      }
      try {
        const platform = getPlatform();
        const path = await platform.pickSavePath(`${project.slug}.ncproj.json`);
        if (!path) return;
        await platform.updateProject(JSON.stringify(project));
        await platform.saveProject(path);
        set({ projectPath: path, isDirty: false, error: null });
      } catch (e) {
        set({ error: (e as Error).message });
      }
    },

    setDbDriver: async (driver) => {
      try {
        const json = await getPlatform().setDbDriver(driver);
        commit(parse(json));
      } catch (e) {
        set({ error: (e as Error).message });
      }
    },

    setFrontendFramework: async (framework) => {
      try {
        const json = await getPlatform().setFrontendFramework(framework);
        commit(parse(json));
      } catch (e) {
        set({ error: (e as Error).message });
      }
    },

    generate: async (outputDir) => {
      const { project } = get();
      if (!project) throw new Error('No project loaded');
      const platform = getPlatform();
      await platform.updateProject(JSON.stringify(project));
      return platform.generateCode(outputDir);
    },

    addEntity: (name) => {
      const current = get().project;
      if (!current) return '';
      const now = new Date().toISOString();
      const id = generateId();
      const fieldId = generateId();
      const entity: Entity = {
        id,
        name,
        plural_name: `${name}s`,
        description: null,
        fields: [
          {
            id: fieldId,
            name: 'id',
            field_type: { type: 'Uuid' },
            nullable: false,
            unique: true,
            default: null,
            auto_generate: 'uuid',
            validation: null,
            comment: null,
            order: 0,
          },
        ],
        relations: [],
        indexes: [],
        constraints: [
          {
            id: generateId(),
            name: `pk_${name}`,
            constraint_type: 'primary_key',
            fields: [fieldId],
            expression: null,
          },
        ],
        options: defaultEntityOptions(),
        created_at: now,
        updated_at: now,
      };
      commit({
        ...current,
        entities: { ...current.entities, [id]: entity },
        updated_at: now,
      });
      return id;
    },

    removeEntity: (id) => {
      const current = get().project;
      if (!current) return;
      const entities: Record<EntityId, Entity> = {};
      for (const [key, entity] of Object.entries(current.entities)) {
        if (key === id) continue;
        entities[key] = {
          ...entity,
          relations: entity.relations.filter((relation) => relation.to_entity !== id),
        };
      }
      commit({ ...current, entities, updated_at: new Date().toISOString() });
    },

    renameEntity: (id, name) => {
      const current = get().project;
      const entity = current?.entities[id];
      if (!current || !entity) return;
      applyEntity(id, {
        ...entity,
        name,
        plural_name: `${name}s`,
        updated_at: new Date().toISOString(),
      });
    },

    addField: (entityId) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!entity) return '';
      const id = generateId();
      const field: EntityField = {
        id,
        name: `field_${entity.fields.length + 1}`,
        field_type: { type: 'String', config: { max_length: 255 } },
        nullable: true,
        unique: false,
        default: null,
        auto_generate: null,
        validation: null,
        comment: null,
        order: entity.fields.length,
      };
      applyEntity(entityId, { ...entity, fields: [...entity.fields, field] });
      return id;
    },

    updateField: (entityId, fieldId, patch) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!entity) return;
      const fields = entity.fields.map((field) => {
        if (field.id !== fieldId) return field;
        const next: EntityField = { ...field };
        if (patch.name !== undefined) next.name = patch.name;
        if (patch.type !== undefined) {
          next.field_type = editorTypeToEntityFieldType(patch.type, field.field_type);
        }
        if (patch.required !== undefined) next.nullable = !patch.required;
        if (patch.unique !== undefined) next.unique = patch.unique;
        if (patch.default !== undefined) next.default = parseDefaultValue(patch.default);
        return next;
      });
      applyEntity(entityId, { ...entity, fields });
    },

    removeField: (entityId, fieldId) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!entity) return;
      const fields = entity.fields
        .filter((field) => field.id !== fieldId)
        .map((field, index) => ({ ...field, order: index }));
      const constraints = entity.constraints
        .map((constraint) => ({
          ...constraint,
          fields: constraint.fields.filter((field) => field !== fieldId),
        }))
        .filter((constraint) => constraint.fields.length > 0);
      const indexes = entity.indexes
        .map((index) => ({
          ...index,
          fields: index.fields.filter((field) => field !== fieldId),
        }))
        .filter((index) => index.fields.length > 0);
      const relations = entity.relations.filter(
        (relation) => relation.from_field !== fieldId && relation.to_field !== fieldId,
      );
      applyEntity(entityId, { ...entity, fields, constraints, indexes, relations });
    },

    addRelation: (entityId) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!entity) return '';
      const target = Object.values(current.entities).find((candidate) => candidate.id !== entityId);
      const id = generateId();
      const fromField = primaryKeyFieldId(entity);
      const toField = target ? primaryKeyFieldId(target) : '';
      const relation: EntityRelation = {
        id,
        name: target ? `${entity.name}_${target.name}` : `relation_${id.slice(0, 8)}`,
        relation_type: editorTypeToRelationType('belongsTo', target?.id ?? '', fromField, toField),
        from_field: fromField,
        to_entity: target?.id ?? '',
        to_field: toField,
        on_delete: 'cascade',
        on_update: 'cascade',
      };
      applyEntity(entityId, { ...entity, relations: [...entity.relations, relation] });
      return id;
    },

    updateRelation: (entityId, relationId, patch) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!entity) return;
      const relations = entity.relations.map((relation) => {
        if (relation.id !== relationId) return relation;
        const target = patch.targetEntity
          ? Object.values(current.entities).find(
              (candidate) =>
                candidate.name === patch.targetEntity || candidate.id === patch.targetEntity,
            )
          : undefined;
        const toEntityId = target?.id ?? relation.to_entity;
        const matchedField = patch.targetField
          ? (target?.fields.find(
              (field) => field.name === patch.targetField || field.id === patch.targetField,
            )?.id ?? relation.to_field)
          : relation.to_field;
        const editorType = patch.type ?? relationTypeToEditor(relation.relation_type);
        return {
          ...relation,
          name: patch.name ?? relation.name,
          to_entity: toEntityId,
          to_field: matchedField,
          relation_type: editorTypeToRelationType(
            editorType,
            toEntityId,
            relation.from_field,
            matchedField,
          ),
        };
      });
      applyEntity(entityId, { ...entity, relations });
    },

    removeRelation: (entityId, relationId) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!entity) return;
      applyEntity(entityId, {
        ...entity,
        relations: entity.relations.filter((relation) => relation.id !== relationId),
      });
    },

    setPrimaryKey: (entityId, fieldId, on) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!entity) return;
      const withoutPrimaryKey = entity.constraints.filter(
        (constraint) => constraint.constraint_type !== 'primary_key',
      );
      let constraints: EntityConstraint[];
      if (on) {
        const previous = entity.constraints.find(
          (constraint) => constraint.constraint_type === 'primary_key',
        );
        const primaryKey: EntityConstraint = {
          id: previous?.id ?? generateId(),
          name: `pk_${entity.name}`,
          constraint_type: 'primary_key',
          fields: [fieldId],
          expression: null,
        };
        constraints = [...withoutPrimaryKey, primaryKey];
      } else {
        constraints = entity.constraints.filter(
          (constraint) =>
            !(constraint.constraint_type === 'primary_key' && constraint.fields.includes(fieldId)),
        );
      }
      applyEntity(entityId, { ...entity, constraints });
    },

    addForm: (name) => {
      const current = get().project;
      if (!current) return '';
      const form = newForm(name);
      commit({
        ...current,
        forms: { ...current.forms, [form.id]: form },
        updated_at: form.updated_at,
      });
      return form.id;
    },

    removeForm: (id) => {
      const current = get().project;
      if (!current) return;
      const forms: Record<FormId, Form> = {};
      for (const [key, form] of Object.entries(current.forms)) {
        if (key === id) continue;
        forms[key] = form;
      }
      commit({ ...current, forms, updated_at: new Date().toISOString() });
    },

    renameForm: (id, name) => {
      const current = get().project;
      const form = current?.forms[id];
      if (!current || !form) return;
      applyForm(id, { ...form, name, updated_at: new Date().toISOString() });
    },

    addFormField: (formId, type) => {
      const current = get().project;
      const form = current?.forms[formId];
      if (!form) return '';
      const field = newFormField(type);
      applyForm(formId, {
        ...form,
        fields: [...form.fields, { ...field, order: form.fields.length }],
      });
      return field.id;
    },

    updateFormField: (formId, fieldId, patch) => {
      const current = get().project;
      const form = current?.forms[formId];
      if (!form) return;
      const fields = form.fields.map((field) => {
        if (field.id !== fieldId) return field;
        const editor = formFieldToEditor(field);
        if (patch.name !== undefined) editor.name = patch.name;
        if (patch.label !== undefined) editor.label = patch.label;
        if (patch.type !== undefined) editor.type = patch.type;
        if (patch.required !== undefined) editor.required = patch.required;
        if (patch.placeholder !== undefined) editor.placeholder = patch.placeholder;
        if (patch.options !== undefined) editor.options = patch.options;
        return { ...editorToFormField(editor, field), order: field.order };
      });
      applyForm(formId, { ...form, fields });
    },

    removeFormField: (formId, fieldId: FormFieldId) => {
      const current = get().project;
      const form = current?.forms[formId];
      if (!form) return;
      const fields = form.fields
        .filter((field) => field.id !== fieldId)
        .map((field, index) => ({ ...field, order: index }));
      const layoutFields = { ...form.layout.fields };
      delete layoutFields[fieldId];
      applyForm(formId, { ...form, fields, layout: { ...form.layout, fields: layoutFields } });
    },

    moveFormField: (formId, fromIndex, toIndex) => {
      const current = get().project;
      const form = current?.forms[formId];
      if (!form) return;
      const fields = [...form.fields];
      if (fromIndex < 0 || fromIndex >= fields.length || toIndex < 0 || toIndex >= fields.length) {
        return;
      }
      const [moved] = fields.splice(fromIndex, 1);
      fields.splice(toIndex, 0, moved);
      applyForm(formId, {
        ...form,
        fields: fields.map((field, index) => ({ ...field, order: index })),
      });
    },

    addListPage: (entityId, kind) => {
      const current = get().project;
      const entity = current?.entities[entityId];
      if (!current || !entity) return '';
      const page = newListPage(entity, kind);
      commit({
        ...current,
        // Generated Page lacks the editor's string-index signature; the cast
        // keeps the single tolerated boundary between the two Page shapes.
        pages: {
          ...current.pages,
          [page.id as PageId]: page as unknown as Project['pages'][string],
        },
        updated_at: page.updated_at,
      });
      return page.id;
    },

    removePage: (id) => {
      const current = get().project;
      if (!current || !current.pages[id]) return;
      const { [id]: _removed, ...pages } = current.pages;
      commit({ ...current, pages, updated_at: new Date().toISOString() });
    },

    addPage: (name) => {
      const current = get().project;
      if (!current) return '';
      const trimmed = name.trim() || 'Untitled Page';
      const page = newPage(trimmed, pagePathFromName(trimmed));
      commit({
        ...current,
        pages: {
          ...current.pages,
          [page.id as PageId]: page as unknown as Project['pages'][string],
        },
        updated_at: page.updated_at,
      });
      return page.id;
    },

    updatePageMeta: (pageId, patch) => {
      const page = readPage(pageId);
      if (!page) return;
      applyPage(pageId, {
        ...page,
        name: patch.name ?? page.name,
        path: patch.path ?? page.path,
        title: patch.title ?? page.title,
        description: patch.description === undefined ? page.description : patch.description,
        updated_at: new Date().toISOString(),
      });
    },

    addPageComponent: (pageId, type) => {
      const page = readPage(pageId);
      if (!page) return '';
      const component = newPageComponent(type);
      applyPage(pageId, {
        ...page,
        components: [...page.components, component],
        updated_at: new Date().toISOString(),
      });
      return component.id;
    },

    updatePageComponent: (pageId, componentId, patch) => {
      const page = readPage(pageId);
      if (!page) return;
      applyComponent(page, componentId, (component) => ({
        ...component,
        component: patch.component ?? component.component,
        props: patch.props ?? component.props,
        style_overrides: patch.style_overrides ?? component.style_overrides,
        bindings: patch.bindings ?? component.bindings,
        conditions: patch.conditions ?? component.conditions,
      }));
    },

    removePageComponent: (pageId, componentId) => {
      const page = readPage(pageId);
      if (!page) return;
      const components = page.components.filter((component) => component.id !== componentId);
      if (components.length === page.components.length) return;
      applyPage(pageId, { ...page, components, updated_at: new Date().toISOString() });
    },

    movePageComponent: (pageId, fromIndex, toIndex) => {
      const page = readPage(pageId);
      if (!page) return;
      const components = [...page.components];
      if (
        fromIndex < 0 ||
        fromIndex >= components.length ||
        toIndex < 0 ||
        toIndex >= components.length ||
        fromIndex === toIndex
      ) {
        return;
      }
      const [moved] = components.splice(fromIndex, 1);
      components.splice(toIndex, 0, moved);
      applyPage(pageId, { ...page, components, updated_at: new Date().toISOString() });
    },

    duplicatePageComponent: (pageId, componentId) => {
      const page = readPage(pageId);
      if (!page) return '';
      const index = page.components.findIndex((component) => component.id === componentId);
      if (index < 0) return '';
      const source = page.components[index];
      const copy: GeneratedPageComponent = {
        ...source,
        id: generateId(),
        component: { ...source.component } as ComponentType,
        props: { ...source.props },
        style_overrides: { ...source.style_overrides },
        bindings: [...source.bindings],
        conditions: [...source.conditions],
        events: [...source.events],
        slots: { ...source.slots },
      };
      const components = [...page.components];
      components.splice(index + 1, 0, copy);
      applyPage(pageId, { ...page, components, updated_at: new Date().toISOString() });
      return copy.id;
    },

    addPageComponentBinding: (pageId, componentId, binding) => {
      const page = readPage(pageId);
      if (!page) return;
      applyComponent(page, componentId, (component) => ({
        ...component,
        bindings: [...component.bindings, binding],
      }));
    },

    removePageComponentBinding: (pageId, componentId, index) => {
      const page = readPage(pageId);
      if (!page) return;
      applyComponent(page, componentId, (component) => ({
        ...component,
        bindings: component.bindings.filter((_, position) => position !== index),
      }));
    },

    addPageAction: (pageId, name, actionType, config) => {
      const page = readPage(pageId);
      if (!page) return '';
      const action = newPageAction(name, actionType, config);
      applyPage(pageId, {
        ...page,
        actions: [...page.actions, action],
        updated_at: new Date().toISOString(),
      });
      return action.id;
    },

    removePageAction: (pageId, actionId) => {
      const page = readPage(pageId);
      if (!page) return;
      const actions = page.actions.filter((action) => action.id !== actionId);
      if (actions.length === page.actions.length) return;
      applyPage(pageId, { ...page, actions, updated_at: new Date().toISOString() });
    },

    addWorkflow: (name) => {
      const current = get().project;
      if (!current) return '';
      const workflow = newWorkflow(name.trim() || 'Untitled Process');
      commit({
        ...current,
        workflows: {
          ...current.workflows,
          [workflow.id]: workflow as unknown as Project['workflows'][string],
        },
        updated_at: workflow.updated_at,
      });
      return workflow.id;
    },

    removeWorkflow: (id) => {
      const current = get().project;
      if (!current || !current.workflows[id]) return;
      const { [id]: _removed, ...workflows } = current.workflows;
      commit({ ...current, workflows, updated_at: new Date().toISOString() });
    },

    updateWorkflowMeta: (workflowId, patch) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return;
      applyWorkflow(workflowId, {
        ...workflow,
        name: patch.name ?? workflow.name,
        version: patch.version ?? workflow.version,
        description: patch.description === undefined ? workflow.description : patch.description,
        settings: patch.settings ?? workflow.settings,
        updated_at: new Date().toISOString(),
      });
    },

    addWorkflowNode: (workflowId, nodeType, x, y) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return '';
      const node = newWorkflowNode(nodeType, { x, y });
      applyWorkflow(workflowId, {
        ...workflow,
        nodes: [...workflow.nodes, node],
        updated_at: new Date().toISOString(),
      });
      return node.id;
    },

    updateWorkflowNode: (workflowId, nodeId, patch) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return;
      let found = false;
      const nodes = workflow.nodes.map((node) => {
        if (node.id !== nodeId) return node;
        found = true;
        return {
          ...node,
          name: patch.name ?? node.name,
          documentation: patch.documentation === undefined ? node.documentation : patch.documentation,
          node_type: patch.node_type ?? node.node_type,
          config: patch.config ?? node.config,
        };
      });
      if (!found) return;
      applyWorkflow(workflowId, { ...workflow, nodes, updated_at: new Date().toISOString() });
    },

    moveWorkflowNode: (workflowId, nodeId, x, y) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return;
      let found = false;
      const nodes = workflow.nodes.map((node) => {
        if (node.id !== nodeId) return node;
        found = true;
        return { ...node, position: { x, y } };
      });
      if (!found) return;
      applyWorkflow(workflowId, { ...workflow, nodes, updated_at: new Date().toISOString() });
    },

    removeWorkflowNode: (workflowId, nodeId) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return;
      const nodes = workflow.nodes.filter((node) => node.id !== nodeId);
      const edges = workflow.edges.filter((edge) => edge.source !== nodeId && edge.target !== nodeId);
      if (nodes.length === workflow.nodes.length) return;
      applyWorkflow(workflowId, { ...workflow, nodes, edges, updated_at: new Date().toISOString() });
    },

    connectWorkflowNodes: (workflowId, source, target) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return '';
      const exists = workflow.edges.some((edge) => edge.source === source && edge.target === target);
      if (exists) return '';
      const edge = newWorkflowEdge(source, target);
      applyWorkflow(workflowId, {
        ...workflow,
        edges: [...workflow.edges, edge],
        updated_at: new Date().toISOString(),
      });
      return edge.id;
    },

    updateWorkflowEdge: (workflowId, edgeId, patch) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return;
      let found = false;
      const edges = workflow.edges.map((edge) => {
        if (edge.id !== edgeId) return edge;
        found = true;
        return {
          ...edge,
          label: patch.label === undefined ? edge.label : patch.label,
          condition: patch.condition === undefined ? edge.condition : patch.condition,
          style: patch.style ?? edge.style,
        };
      });
      if (!found) return;
      applyWorkflow(workflowId, { ...workflow, edges, updated_at: new Date().toISOString() });
    },

    removeWorkflowEdge: (workflowId, edgeId) => {
      const workflow = readWorkflow(workflowId);
      if (!workflow) return;
      const edges = workflow.edges.filter((edge) => edge.id !== edgeId);
      if (edges.length === workflow.edges.length) return;
      applyWorkflow(workflowId, { ...workflow, edges, updated_at: new Date().toISOString() });
    },

    importWorkflow: (workflowJson) => {
      const current = get().project;
      if (!current) return '';
      let parsed: GeneratedWorkflow;
      try {
        parsed = JSON.parse(workflowJson) as GeneratedWorkflow;
      } catch {
        return '';
      }
      if (!Array.isArray(parsed.nodes) || !Array.isArray(parsed.edges)) return '';
      // Imported definitions may reuse ids; re-key so graph references stay unique.
      const idMap = new Map<string, string>();
      parsed.nodes.forEach((node) => {
        const next = generateId();
        idMap.set(node.id, next);
        node.id = next;
      });
      parsed.edges.forEach((edge) => {
        const source = idMap.get(edge.source) ?? edge.source;
        const target = idMap.get(edge.target) ?? edge.target;
        edge.id = generateId();
        edge.source = source;
        edge.target = target;
      });
      const workflow: GeneratedWorkflow = { ...parsed, id: generateId() };
      commit({
        ...current,
        workflows: {
          ...current.workflows,
          [workflow.id]: workflow as unknown as Project['workflows'][string],
        },
        updated_at: workflow.updated_at,
      });
      return workflow.id;
    },

    markDirty: () => {
      const current = get().project;
      if (!current) return;
      commit(current);
    },

    autosave: async () => {
      const { project, projectPath } = get();
      if (!project || !projectPath) return;
      try {
        const platform = getPlatform();
        await platform.updateProject(JSON.stringify(project));
        await platform.saveProject(projectPath);
        set({ isDirty: false, error: null });
      } catch {
        return;
      }
    },
  };
});
