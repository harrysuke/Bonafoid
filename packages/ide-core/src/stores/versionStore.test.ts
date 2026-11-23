import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { Branch, Environment, PlatformApi, Revision } from '../platform/types';
import { useVersionStore } from './versionStore';

const branch: Branch = {
  name: 'main',
  head_revision_id: 'r1',
  created_at: '2026-11-01T00:00:00Z',
};

const revision: Revision = {
  id: 'r1',
  project_id: 'proj1',
  parent_id: null,
  branch: 'main',
  message: 'initial',
  author_id: 'u1',
  data: { name: 'App' },
  created_at: '2026-11-01T00:00:00Z',
};

const environment: Environment = {
  name: 'staging',
  config: { region: 'eu' },
  revision_id: 'r1',
  status: 'deployed',
  updated_at: '2026-11-01T00:00:00Z',
};

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    versionBranches: vi.fn().mockResolvedValue([branch]),
    versionEnvironments: vi.fn().mockResolvedValue([environment]),
    versionHistory: vi.fn().mockResolvedValue([revision]),
    versionDiff: vi.fn().mockResolvedValue({ from: 'r0', to: 'r1', lines: [] }),
    versionCreateBranch: vi.fn().mockResolvedValue({ ...branch, name: 'dev' }),
    versionCommit: vi.fn().mockResolvedValue({ ...revision, id: 'r2', message: 'second' }),
    versionMerge: vi.fn().mockResolvedValue(revision),
    versionRestore: vi.fn().mockResolvedValue(revision),
    versionCreateEnvironment: vi.fn().mockResolvedValue(environment),
    versionUpdateEnvironment: vi.fn().mockResolvedValue(environment),
    versionPromote: vi.fn().mockResolvedValue(environment),
    versionRollback: vi.fn().mockResolvedValue(environment),
    versionSecrets: vi.fn().mockResolvedValue([{ key: 'API_KEY' }]),
    versionSetSecret: vi.fn().mockResolvedValue({ key: 'API_KEY' }),
    versionDeleteSecret: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  } as PlatformApi;
}

describe('versionStore', () => {
  beforeEach(() => {
    configurePlatform(stubPlatform());
    useVersionStore.getState().reset();
    useVersionStore.getState().setProject('proj1');
  });

  it('loads branches and environments for the open project', async () => {
    const branches = vi.fn().mockResolvedValue([branch]);
    configurePlatform(stubPlatform({ versionBranches: branches }));
    await useVersionStore.getState().load();
    const state = useVersionStore.getState();
    expect(branches).toHaveBeenCalledWith('proj1');
    expect(state.branches).toEqual([branch]);
    expect(state.environments).toEqual([environment]);
    expect(state.busy).toBe(false);
    expect(state.error).toBeNull();
  });

  it('reports an unsupported host without throwing', async () => {
    configurePlatform({ kind: 'tauri', isDesktop: true, autoSeedSample: true } as PlatformApi);
    await useVersionStore.getState().load();
    expect(useVersionStore.getState().error).toMatch(/does not expose/i);
    expect(await useVersionStore.getState().createBranch('dev')).toBe(false);
  });

  it('refuses to call the host without an open project', async () => {
    const branches = vi.fn().mockResolvedValue([branch]);
    configurePlatform(stubPlatform({ versionBranches: branches }));
    useVersionStore.getState().setProject(null);
    await useVersionStore.getState().load();
    expect(branches).not.toHaveBeenCalled();
    expect(useVersionStore.getState().error).toMatch(/open a project/i);
  });

  it('surfaces a load failure', async () => {
    configurePlatform(
      stubPlatform({ versionBranches: vi.fn().mockRejectedValue(new Error('git offline')) }),
    );
    await useVersionStore.getState().load();
    expect(useVersionStore.getState().error).toBe('git offline');
    expect(useVersionStore.getState().busy).toBe(false);
  });

  it('adds a new branch to the list', async () => {
    const create = vi.fn().mockResolvedValue({ ...branch, name: 'dev' });
    configurePlatform(stubPlatform({ versionCreateBranch: create }));
    expect(await useVersionStore.getState().createBranch('dev')).toBe(true);
    expect(create).toHaveBeenCalledWith('proj1', 'dev', undefined);
    expect(useVersionStore.getState().branches.map((row) => row.name)).toEqual(['dev']);
  });

  it('prepends a commit and advances the branch head', async () => {
    configurePlatform(stubPlatform());
    await useVersionStore.getState().load();
    await useVersionStore.getState().loadHistory('main');
    expect(await useVersionStore.getState().commit('main', 'second', { name: 'App 2' })).toBe(true);
    const state = useVersionStore.getState();
    expect(state.history[0].id).toBe('r2');
    expect(state.branches[0].head_revision_id).toBe('r2');
  });

  it('never stores or echoes a submitted secret value', async () => {
    configurePlatform(
      stubPlatform({
        versionSetSecret: vi.fn().mockRejectedValue(new Error('rejected top-secret-value')),
      }),
    );
    const ok = await useVersionStore.getState().setSecret('staging', 'API_KEY', 'top-secret-value');
    expect(ok).toBe(false);
    expect(useVersionStore.getState().error).toContain('REDACTED');
    expect(JSON.stringify(useVersionStore.getState())).not.toContain('top-secret-value');
  });
});
