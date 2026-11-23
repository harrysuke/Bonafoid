/**
 * State for the version control and environments panel.
 *
 * Every operation is scoped to an open project id held only in memory. Secret
 * values are submitted once, never stored, and redacted from error text.
 */

import { create } from 'zustand';
import { getPlatform, hasPlatform } from '../platform/registry';
import type {
  Branch,
  DiffResult,
  Environment,
  PlatformApi,
  Revision,
  SecretKey,
} from '../platform/types';
import { describeError } from '../utils/errors';
import { redactSecret } from '../utils/saas';

const UNSUPPORTED = 'This host does not expose versioning.';
const NO_PROJECT = 'Open a project before using version control.';

/** True when the active host implements the versioning service. */
export function versionSupported(): boolean {
  return hasPlatform() && typeof getPlatform().versionBranches === 'function';
}

export interface VersionState {
  /** Open project all operations are scoped to. */
  projectId: string | null;
  branches: Branch[];
  history: Revision[];
  environments: Environment[];
  secrets: SecretKey[];
  diff: DiffResult | null;
  busy: boolean;
  error: string | null;
  /** Select the project to operate on. */
  setProject: (id: string | null) => void;
  /** Refresh branches and environments for the open project. */
  load: () => Promise<void>;
  loadHistory: (branch: string) => Promise<void>;
  loadDiff: (from: string, to: string) => Promise<void>;
  createBranch: (name: string, fromRevisionId?: string) => Promise<boolean>;
  commit: (branch: string, message: string, data: unknown) => Promise<boolean>;
  merge: (targetBranch: string, sourceBranch: string, strategy: string) => Promise<boolean>;
  restore: (revisionId: string) => Promise<boolean>;
  createEnvironment: (name: string, config?: Record<string, unknown>) => Promise<boolean>;
  updateEnvironment: (name: string, config: Record<string, unknown>) => Promise<boolean>;
  promote: (name: string, revisionId: string) => Promise<boolean>;
  rollback: (name: string, revisionId: string) => Promise<boolean>;
  loadSecrets: (name: string) => Promise<void>;
  setSecret: (name: string, key: string, value: string) => Promise<boolean>;
  deleteSecret: (name: string, key: string) => Promise<boolean>;
  clearError: () => void;
  /** Test seam: forget all loaded state. */
  reset: () => void;
}

function initialState() {
  return {
    projectId: null as string | null,
    branches: [] as Branch[],
    history: [] as Revision[],
    environments: [] as Environment[],
    secrets: [] as SecretKey[],
    diff: null as DiffResult | null,
    busy: false,
    error: null,
  };
}

export const useVersionStore = create<VersionState>()((set, get) => {
  const requirePlatform = (): PlatformApi | null => {
    if (!hasPlatform()) {
      set({ error: UNSUPPORTED });
      return null;
    }
    return getPlatform();
  };

  const requireProjectId = (): string | null => {
    const id = get().projectId;
    if (!id) {
      set({ error: NO_PROJECT });
      return null;
    }
    return id;
  };

  return {
    ...initialState(),

    setProject: (id) => set({ ...initialState(), projectId: id }),

    load: async () => {
      if (!versionSupported()) {
        set({ ...initialState(), projectId: get().projectId, error: UNSUPPORTED });
        return;
      }
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform || !projectId) return;
      set({ busy: true, error: null });
      try {
        const [branches, environments] = await Promise.all([
          platform.versionBranches!(projectId),
          platform.versionEnvironments!(projectId),
        ]);
        set({ branches, environments, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    loadHistory: async (branch) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionHistory || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return;
      }
      set({ busy: true, error: null });
      try {
        const history = await platform.versionHistory(projectId, branch);
        set({ history, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    loadDiff: async (from, to) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionDiff || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return;
      }
      set({ busy: true, error: null });
      try {
        const diff = await platform.versionDiff(projectId, from, to);
        set({ diff, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    createBranch: async (name, fromRevisionId) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionCreateBranch || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const branch = await platform.versionCreateBranch(projectId, name, fromRevisionId);
        set({ branches: [...get().branches, branch], busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    commit: async (branch, message, data) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionCommit || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const revision = await platform.versionCommit(projectId, branch, message, data);
        set({
          history: [revision, ...get().history.filter((row) => row.id !== revision.id)],
          branches: get().branches.map((row) =>
            row.name === branch ? { ...row, head_revision_id: revision.id } : row,
          ),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    merge: async (targetBranch, sourceBranch, strategy) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionMerge || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const revision = await platform.versionMerge(
          targetBranch,
          projectId,
          sourceBranch,
          strategy,
        );
        set({
          history: [revision, ...get().history.filter((row) => row.id !== revision.id)],
          branches: get().branches.map((row) =>
            row.name === targetBranch ? { ...row, head_revision_id: revision.id } : row,
          ),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    restore: async (revisionId) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionRestore || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const revision = await platform.versionRestore(projectId, revisionId);
        set({
          history: [revision, ...get().history.filter((row) => row.id !== revision.id)],
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    createEnvironment: async (name, config) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionCreateEnvironment || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const environment = await platform.versionCreateEnvironment(projectId, name, config);
        set({
          environments: [
            ...get().environments.filter((row) => row.name !== environment.name),
            environment,
          ],
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    updateEnvironment: async (name, config) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionUpdateEnvironment || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const environment = await platform.versionUpdateEnvironment(name, projectId, config);
        set({
          environments: get().environments.map((row) =>
            row.name === environment.name ? environment : row,
          ),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    promote: async (name, revisionId) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionPromote || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const environment = await platform.versionPromote(name, projectId, revisionId);
        set({
          environments: get().environments.map((row) =>
            row.name === environment.name ? environment : row,
          ),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    rollback: async (name, revisionId) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionRollback || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const environment = await platform.versionRollback(name, projectId, revisionId);
        set({
          environments: get().environments.map((row) =>
            row.name === environment.name ? environment : row,
          ),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    loadSecrets: async (name) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionSecrets || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return;
      }
      set({ busy: true, error: null });
      try {
        const secrets = await platform.versionSecrets(name, projectId);
        set({ secrets, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    setSecret: async (name, key, value) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionSetSecret || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const secret = await platform.versionSetSecret(name, projectId, key, value);
        set({
          secrets: [...get().secrets.filter((row) => row.key !== secret.key), secret],
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: redactSecret(describeError(error), value) });
        return false;
      }
    },

    deleteSecret: async (name, key) => {
      const platform = requirePlatform();
      const projectId = requireProjectId();
      if (!platform?.versionDeleteSecret || !projectId) {
        if (platform && projectId) set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        await platform.versionDeleteSecret(name, projectId, key);
        set({ secrets: get().secrets.filter((row) => row.key !== key), busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    clearError: () => set({ error: null }),

    reset: () => set(initialState()),
  };
});
