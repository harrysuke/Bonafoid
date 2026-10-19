/**
 * Tests for the project store's entity, form and theme mutations.
 *
 * The store is the single writer of project.json, so these tests exist to
 * catch a mutation that silently drops or corrupts part of the project.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { PlatformApi } from '../platform/types';
import { useProjectStore } from './projectStore';
import type { Project } from '../types/project';

const emptyProject = {
  id: 'p1',
  name: 'Test',
  slug: 'test',
  description: null,
  version: '0.1.0',
  icon: null,
  settings: {
    default_locale: 'en',
    supported_locales: ['en'],
    timezone: 'UTC',
    date_format: 'YYYY-MM-DD',
    time_format: 'HH:mm:ss',
    number_format: {},
    currency: {},
    authentication: {},
    api: {},
    database: {
      driver: 'sqlite',
      url: '',
      max_connections: 20,
      min_connections: 5,
      migrations_path: 'migrations',
      ssl: false,
    },
    frontend: { framework: 'leptos', bundler: 'trunk' },
    storage: {},
    email: {},
    features: {},
    custom: {},
  },
  forms: {},
  entities: {},
  workflows: {},
  pages: {},
  themes: {},
  active_theme_id: null,
  plugins: [],
  environments: {},
  created_at: '2024-01-01T00:00:00.000Z',
  updated_at: '2024-01-01T00:00:00.000Z',
  published_at: null,
} as unknown as Project;

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    ...overrides,
  } as PlatformApi;
}

/** Install a fresh project and return the store's state. */
function loadProject(project: Project = emptyProject) {
  const platform = stubPlatform({
    getCurrentProject: vi.fn().mockResolvedValue(JSON.stringify(project)),
    getProjectPath: vi.fn().mockResolvedValue('api://projects/p1'),
    saveProject: vi.fn().mockResolvedValue(undefined),
  });
  configurePlatform(platform);
  return useProjectStore;
}

describe('projectStore', () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it('marks the project dirty and restamps updated_at on a mutation', async () => {
    const store = loadProject();
    await store.getState().init();

    const before = store.getState().project?.updated_at;
    vi.setSystemTime(new Date('2025-06-01T12:00:00.000Z'));

    store.getState().addEntity('Customer');

    const state = store.getState();
    expect(state.isDirty).toBe(true);
    expect(state.project?.updated_at).not.toBe(before);
    expect(Object.keys(state.project?.entities ?? {})).toHaveLength(1);
  });

  it('seeds a new entity with a uuid primary key', async () => {
    const store = loadProject();
    await store.getState().init();
    store.getState().addEntity('Customer');

    const entity = Object.values(store.getState().project?.entities ?? {})[0];
    expect(entity?.name).toBe('Customer');
    expect(entity?.plural_name).toBe('Customers');
    expect(entity?.fields).toHaveLength(1);
    expect(entity?.fields[0]?.name).toBe('id');
    expect(entity?.constraints.some((c) => c.constraint_type === 'primary_key')).toBe(true);
  });

  it('removes an entity without touching the others', async () => {
    const store = loadProject();
    await store.getState().init();
    store.getState().addEntity('Customer');
    store.getState().addEntity('Order');
    const ids = Object.keys(store.getState().project?.entities ?? {});
    expect(ids).toHaveLength(2);

    store.getState().removeEntity(ids[0]);

    expect(Object.keys(store.getState().project?.entities ?? {})).toEqual([ids[1]]);
  });

  it('ignores removal of an entity that does not exist', async () => {
    const store = loadProject();
    await store.getState().init();
    store.getState().addEntity('Customer');
    const before = JSON.stringify(store.getState().project);

    store.getState().removeEntity('not-a-real-id');

    expect(JSON.stringify(store.getState().project)).toBe(before);
  });

  it('reorders form fields by rewriting their order', async () => {
    const store = loadProject();
    await store.getState().init();
    const formId = store.getState().addForm('Signup');
    const first = store.getState().addFormField(formId, 'text');
    store.getState().addFormField(formId, 'text');
    store.getState().addFormField(formId, 'text');

    const formBefore = store.getState().project?.forms[formId];
    expect(formBefore?.fields.map((f) => f.order)).toEqual([0, 1, 2]);
    expect(formBefore?.fields[0]?.id).toBe(first);

    store.getState().moveFormField(formId, 0, 2);

    const formAfter = store.getState().project?.forms[formId];
    expect(formAfter?.fields.map((f) => f.order)).toEqual([0, 1, 2]);
    expect(formAfter?.fields[2]?.id).toBe(first);
  });

  it('leaves field order untouched for an out-of-range move', async () => {
    const store = loadProject();
    await store.getState().init();
    const formId = store.getState().addForm('Signup');
    store.getState().addFormField(formId, 'text');
    store.getState().addFormField(formId, 'text');

    store.getState().moveFormField(formId, 5, 0);

    expect(store.getState().project?.forms[formId]?.fields.map((f) => f.order)).toEqual([0, 1]);
  });

  it('makes the first theme active so codegen always has tokens', async () => {
    const store = loadProject();
    await store.getState().init();

    const themeId = store.getState().addTheme('Brand');

    expect(store.getState().project?.active_theme_id).toBe(themeId);
  });

  it('keeps the active theme when a second theme is added', async () => {
    const store = loadProject();
    await store.getState().init();
    const first = store.getState().addTheme('Brand');
    store.getState().addTheme('Contrast');

    expect(store.getState().project?.active_theme_id).toBe(first);
  });

  it('promotes a surviving theme when the active one is deleted', async () => {
    const store = loadProject();
    await store.getState().init();
    const first = store.getState().addTheme('Brand');
    const second = store.getState().addTheme('Contrast');
    store.getState().setActiveTheme(first);

    store.getState().removeTheme(first);

    expect(store.getState().project?.active_theme_id).toBe(second);
  });

  it('leaves active_theme_id null when the last theme is deleted', async () => {
    const store = loadProject();
    await store.getState().init();
    const themeId = store.getState().addTheme('Brand');

    store.getState().removeTheme(themeId);

    expect(store.getState().project?.active_theme_id).toBeNull();
    expect(Object.keys(store.getState().project?.themes ?? {})).toHaveLength(0);
  });

  it('applies a theme color edit through to the stored tokens', async () => {
    const store = loadProject();
    await store.getState().init();
    const themeId = store.getState().addTheme('Brand');

    store.getState().setThemeColor(themeId, 'primary', '#ff00aa');

    const theme = store.getState().project?.themes[themeId] as unknown as {
      tokens: { colors: { primary: { base: string; scale: Record<string, string> } } };
    };
    expect(theme.tokens.colors.primary.base).toBe('#ff00aa');
    expect(theme.tokens.colors.primary.scale['500']).toBe('#ff00aa');
  });

  it('does not duplicate a theme into the source theme', async () => {
    const store = loadProject();
    await store.getState().init();
    const themeId = store.getState().addTheme('Brand');

    const copyId = store.getState().duplicateTheme(themeId);

    expect(copyId).not.toBe(themeId);
    expect(Object.keys(store.getState().project?.themes ?? {})).toHaveLength(2);
    expect((store.getState().project?.themes[themeId] as { name: string }).name).toBe('Brand');
  });

  it('returns an empty id when there is no project to mutate', () => {
    const store = useProjectStore;
    store.setState({ project: null });
    expect(store.getState().addEntity('Nope')).toBe('');
    expect(store.getState().addForm('Nope')).toBe('');
    expect(store.getState().addTheme('Nope')).toBe('');
  });
});