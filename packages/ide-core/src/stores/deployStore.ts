/**
 * State for the Deploy screen (Phase 15 / 15.7).
 *
 * The store talks only to the optional deploy surface of `PlatformApi`, so
 * the same UI works against the web server's REST routes and the desktop
 * Tauri commands. It deliberately holds no credential: a PAT is forwarded
 * once on save and then dropped, and nothing here is persisted except the
 * public (non-secret) API base URL.
 */

import { create } from 'zustand';
import { getPlatform, hasPlatform } from '../platform/registry';
import type {
  DeploymentResponse,
  DockerHubConfigRequest,
  GitHubConfigRequest,
  GitHubConfigResponse,
  PlatformApi,
  RegistryConfigResponse,
} from '../platform/types';
import { describeError } from '../utils/errors';
import { DEFAULT_DEPLOY_API_BASE, validDeployApiBase } from '../utils/deploy';

const API_BASE_KEY = 'nocode.deploy.apiBase';

function readApiBase(): string {
  try {
    if (typeof localStorage === 'undefined') return DEFAULT_DEPLOY_API_BASE;
    return localStorage.getItem(API_BASE_KEY) || DEFAULT_DEPLOY_API_BASE;
  } catch {
    return DEFAULT_DEPLOY_API_BASE;
  }
}

function writeApiBase(value: string): void {
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(API_BASE_KEY, value);
  } catch {
    // A blocked storage API must not break the deploy screen.
  }
}

/** Never let a submitted secret reach an error message or a toast. */
function withoutSecret(message: string, secret?: string): string {
  if (!secret) return message;
  return message.split(secret).join('***REDACTED***');
}

export interface DeployState {
  /** Redacted GitHub config; null until loaded. Never contains a token. */
  github: GitHubConfigResponse | null;
  /** Redacted registry config; null until loaded. Never contains a token. */
  registry: RegistryConfigResponse | null;
  /** Public API base URL baked into the generated frontend at build time. */
  apiBase: string;
  /** Newest-first deployment history for the open project. */
  history: DeploymentResponse[];
  /** Result of the most recent deploy action. */
  current: DeploymentResponse | null;
  /** Loading configs or refreshing history. */
  busy: boolean;
  /** A deploy request is in flight (generate -> commit -> push). */
  deploying: boolean;
  error: string | null;
  /** Load the redacted integration configs. */
  load: () => Promise<void>;
  setApiBase: (value: string) => void;
  /** Save a GitHub PAT. Returns true only when the host accepted it. */
  connectGithub: (request: GitHubConfigRequest) => Promise<boolean>;
  /** Save the Docker Hub config. Returns true only when the host accepted it. */
  connectDockerhub: (request: DockerHubConfigRequest) => Promise<boolean>;
  /** Run the deploy pipeline for a project. */
  deploy: (projectId: string | null) => Promise<void>;
  /** Re-read history (also refreshes CI status on the web host). */
  refresh: (projectId: string | null) => Promise<void>;
  clearError: () => void;
  /** Test seam: forget all loaded state. */
  reset: () => void;
}

/** True when the active host exposes the deployment service. */
export function deploySupported(): boolean {
  return hasPlatform() && typeof getPlatform().deployGithubGet === 'function';
}

const UNSUPPORTED = 'This host does not expose the deployment service.';

function initialState() {
  return {
    github: null,
    registry: null,
    apiBase: readApiBase(),
    history: [] as DeploymentResponse[],
    current: null,
    busy: false,
    deploying: false,
    error: null,
  };
}

export const useDeployStore = create<DeployState>()((set, get) => {
  /** A store action must never throw just because no host was installed. */
  const requirePlatform = (): PlatformApi | null => {
    if (!hasPlatform()) {
      set({ error: UNSUPPORTED });
      return null;
    }
    return getPlatform();
  };

  return {
    ...initialState(),

    load: async () => {
      if (!deploySupported()) {
        set({ ...initialState(), error: UNSUPPORTED, apiBase: get().apiBase });
        return;
      }
      const platform = requirePlatform();
      if (!platform) return;
      set({ busy: true, error: null });
      try {
        const [github, registry] = await Promise.all([
          platform.deployGithubGet!(),
          platform.deployDockerhubGet!(),
        ]);
        set({ github, registry, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    setApiBase: (value) => {
      writeApiBase(value);
      set({ apiBase: value });
    },

    connectGithub: async (request) => {
      const platform = requirePlatform();
      if (!platform?.deployGithubSet) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        // `request` is not stored. Only the redacted response is kept.
        const github = await platform.deployGithubSet(request);
        set({ github, busy: false });
        return true;
      } catch (error) {
        set({
          busy: false,
          error: withoutSecret(describeError(error), request.token),
        });
        return false;
      }
    },

    connectDockerhub: async (request) => {
      const platform = requirePlatform();
      if (!platform?.deployDockerhubSet) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const registry = await platform.deployDockerhubSet(request);
        set({ registry, busy: false });
        return true;
      } catch (error) {
        set({
          busy: false,
          error: withoutSecret(describeError(error), request.token),
        });
        return false;
      }
    },

    deploy: async (projectId) => {
      const platform = requirePlatform();
      if (!platform?.deployProject) {
        set({ error: UNSUPPORTED });
        return;
      }
      const { apiBase, github, registry } = get();
      if (!projectId) {
        set({ error: 'Open a project before deploying.' });
        return;
      }
      if (!validDeployApiBase(apiBase)) {
        set({
          error:
            'Enter a valid public API base URL (an absolute http:// or https:// URL) before deploying.',
        });
        return;
      }
      if (!github?.configured || !github.verified || !registry?.configured || !registry.verified) {
        set({ error: 'Connect and verify both GitHub and Docker Hub before deploying.' });
        return;
      }

      set({ deploying: true, error: null, current: null });
      try {
        const result = await platform.deployProject(projectId, apiBase.trim());
        // History comes from the host, not from the optimistic result. A failed
        // refresh must not hide a deploy that actually succeeded.
        let history = get().history;
        if (platform.deployList) {
          try {
            history = await platform.deployList(projectId);
          } catch {
            history = [result, ...history.filter((row) => row.id !== result.id)];
          }
        } else {
          history = [result, ...history.filter((row) => row.id !== result.id)];
        }
        set({ current: result, history, deploying: false });
      } catch (error) {
        set({ deploying: false, error: describeError(error) });
      }
    },

    refresh: async (projectId) => {
      const platform = requirePlatform();
      if (!platform?.deployList) {
        set({ error: UNSUPPORTED });
        return;
      }
      if (!projectId) {
        set({ history: [], current: null });
        return;
      }
      set({ busy: true, error: null });
      try {
        const history = await platform.deployList(projectId);
        const current = get().current;
        const updated = current
          ? history.find((row) => row.id === current.id) ?? current
          : null;
        set({ history, current: updated, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    clearError: () => set({ error: null }),

    reset: () => set(initialState()),
  };
});
