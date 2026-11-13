/**
 * Tests for the deploy store.
 *
 * The store is the shared UI's only view of the integration service. These
 * tests pin down the two things a refactor is most likely to break: a secret
 * leaking into state, and a push being celebrated as a deployment.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type {
  DeploymentResponse,
  DockerHubConfigRequest,
  GitHubConfigRequest,
  GitHubConfigResponse,
  PlatformApi,
  RegistryConfigResponse,
} from '../platform/types';
import { ApiRequestError } from '../utils/errors';
import { useDeployStore } from './deployStore';

const github: GitHubConfigResponse = {
  configured: true,
  owner: 'acme',
  repo: 'app',
  branch: 'main',
  verified: true,
};

const registry: RegistryConfigResponse = {
  configured: true,
  namespace: 'acme',
  registry: 'docker.io',
  backend_image: 'acme/app-backend',
  frontend_image: 'acme/app-frontend',
  verified: true,
};

const githubRequest: GitHubConfigRequest = {
  owner: 'acme',
  repo: 'app',
  branch: 'main',
  token: 'ghp_thisIsAFakeTokenForTestsOnly999',
};

const dockerhubRequest: DockerHubConfigRequest = {
  namespace: 'acme',
  registry: 'docker.io',
  token: 'dckr_pat_fakeTokenForTestsOnly',
};

function pushed(overrides: Partial<DeploymentResponse> = {}): DeploymentResponse {
  return {
    id: 'dep-1',
    project_id: 'p1',
    commit_sha: '0123456789abcdef0123456789abcdef01234567',
    status: 'pushed',
    deployed: false,
    ci_state: 'not_started',
    github_run_id: null,
    github_run_url: null,
    error_message: null,
    created_at: '2026-11-13T00:00:00Z',
    updated_at: '2026-11-13T00:00:00Z',
    ...overrides,
  };
}

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    deployGithubGet: vi.fn().mockResolvedValue(github),
    deployDockerhubGet: vi.fn().mockResolvedValue(registry),
    deployGithubSet: vi.fn().mockResolvedValue(github),
    deployDockerhubSet: vi.fn().mockResolvedValue(registry),
    deployProject: vi.fn().mockResolvedValue(pushed()),
    deployList: vi.fn().mockResolvedValue([pushed()]),
    ...overrides,
  } as PlatformApi;
}

function install(overrides: Partial<PlatformApi> = {}): PlatformApi {
  const platform = stubPlatform(overrides);
  configurePlatform(platform);
  return platform;
}

describe('deployStore', () => {
  beforeEach(() => {
    install();
    useDeployStore.getState().reset();
  });

  it('loads the redacted configs', async () => {
    await useDeployStore.getState().load();

    const state = useDeployStore.getState();
    expect(state.github).toEqual(github);
    expect(state.registry).toEqual(registry);
    expect(state.busy).toBe(false);
    expect(state.error).toBeNull();
    expect(JSON.stringify(state)).not.toContain('token');
  });

  it('reports a host without a deploy service instead of throwing', async () => {
    configurePlatform({ kind: 'tauri', isDesktop: true, autoSeedSample: true } as PlatformApi);

    await useDeployStore.getState().load();
    expect(useDeployStore.getState().error).toMatch(/does not expose/i);

    expect(await useDeployStore.getState().connectGithub(githubRequest)).toBe(false);
    expect(await useDeployStore.getState().connectDockerhub(dockerhubRequest)).toBe(false);
  });

  it('forwards the PAT once and keeps it out of state', async () => {
    const platform = install();
    expect(await useDeployStore.getState().connectGithub(githubRequest)).toBe(true);

    expect(platform.deployGithubSet).toHaveBeenCalledTimes(1);
    expect(platform.deployGithubSet).toHaveBeenCalledWith(githubRequest);
    const serialized = JSON.stringify(useDeployStore.getState());
    expect(serialized).not.toContain(githubRequest.token);
    expect(serialized).not.toContain('ghp_');
  });

  it('never echoes a submitted token in an error message', async () => {
    install({
      deployGithubSet: vi
        .fn()
        .mockRejectedValue(new Error(`rejected ${githubRequest.token}`)),
    });

    expect(await useDeployStore.getState().connectGithub(githubRequest)).toBe(false);
    const { error } = useDeployStore.getState();
    expect(error).toContain('REDACTED');
    expect(error).not.toContain(githubRequest.token);
  });

  it('surfaces the 503 setup message from a disabled service', async () => {
    install({
      deployGithubGet: vi.fn().mockRejectedValue(
        new ApiRequestError(503, 'service_unavailable', 'set IDE_SECRET_KEY and restart'),
      ),
      deployDockerhubGet: vi.fn().mockResolvedValue(registry),
    });

    await useDeployStore.getState().load();
    expect(useDeployStore.getState().error).toContain('IDE_SECRET_KEY');
  });

  it('refuses to deploy without a project', async () => {
    const platform = install();
    await useDeployStore.getState().load();

    await useDeployStore.getState().deploy(null);

    expect(platform.deployProject).not.toHaveBeenCalled();
    expect(useDeployStore.getState().error).toMatch(/open a project/i);
  });

  it('refuses to deploy with an invalid public API base URL', async () => {
    const platform = install();
    await useDeployStore.getState().load();
    useDeployStore.getState().setApiBase('javascript:alert(1)');

    await useDeployStore.getState().deploy('p1');

    expect(platform.deployProject).not.toHaveBeenCalled();
    expect(useDeployStore.getState().error).toMatch(/valid public API base URL/i);
  });

  it('refuses to deploy until both integrations are verified', async () => {
    const platform = install({
      deployGithubGet: vi.fn().mockResolvedValue({ ...github, verified: false }),
    });
    await useDeployStore.getState().load();

    await useDeployStore.getState().deploy('p1');

    expect(platform.deployProject).not.toHaveBeenCalled();
    expect(useDeployStore.getState().error).toMatch(/verify both/i);
  });

  it('passes the API base URL and keeps the honest pushed result', async () => {
    const platform = install();
    await useDeployStore.getState().load();
    useDeployStore.getState().setApiBase('https://api.example.com');

    await useDeployStore.getState().deploy('p1');

    expect(platform.deployProject).toHaveBeenCalledWith('p1', 'https://api.example.com');
    const state = useDeployStore.getState();
    expect(state.current?.status).toBe('pushed');
    expect(state.current?.deployed).toBe(false);
    expect(state.deploying).toBe(false);
    expect(state.history).toHaveLength(1);
  });

  it('does not hide a successful deploy when the history refresh fails', async () => {
    install({ deployList: vi.fn().mockRejectedValue(new Error('history unavailable')) });
    await useDeployStore.getState().load();

    await useDeployStore.getState().deploy('p1');

    const state = useDeployStore.getState();
    expect(state.error).toBeNull();
    expect(state.current?.status).toBe('pushed');
    expect(state.history).toHaveLength(1);
  });

  it('reports a deploy failure and clears the in-flight flag', async () => {
    install({ deployProject: vi.fn().mockRejectedValue(new Error('git push rejected')) });
    await useDeployStore.getState().load();

    await useDeployStore.getState().deploy('p1');

    const state = useDeployStore.getState();
    expect(state.deploying).toBe(false);
    expect(state.error).toBe('git push rejected');
    expect(state.current).toBeNull();
  });

  it('refresh replaces history and updates the current deployment', async () => {
    install({
      deployList: vi.fn().mockResolvedValue([
        pushed({ status: 'ci_succeeded', deployed: true, ci_state: 'succeeded' }),
      ]),
    });
    await useDeployStore.getState().load();
    useDeployStore.setState({ current: pushed(), history: [pushed()] });

    await useDeployStore.getState().refresh('p1');

    const state = useDeployStore.getState();
    expect(state.history[0].status).toBe('ci_succeeded');
    expect(state.current?.deployed).toBe(true);
    expect(state.busy).toBe(false);
  });

  it('clears history when no project is open', async () => {
    install();
    useDeployStore.setState({ history: [pushed()], current: pushed() });

    await useDeployStore.getState().refresh(null);

    expect(useDeployStore.getState().history).toHaveLength(0);
    expect(useDeployStore.getState().current).toBeNull();
  });
});
