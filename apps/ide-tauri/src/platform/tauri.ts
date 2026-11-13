/**
 * Tauri `PlatformApi` implementation for the desktop shell.
 *
 * This is the only file in the desktop app that imports `@tauri-apps/*`.
 * The shared `@nocode/ide-core` package stays host-agnostic and consumes this
 * object through `<PlatformProvider platform={tauriPlatform}>`.
 *
 * Every method asserts the Tauri runtime first so accidental use in a plain
 * browser fails with a clear message instead of a cryptic invoke rejection.
 */

import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
  BpmnValidationReport,
  DeploymentResponse,
  DockerHubConfigRequest,
  GenerationReport,
  GitHubConfigRequest,
  GitHubConfigResponse,
  PlatformApi,
  RegistryConfigResponse,
  WorkflowRunSummary,
} from '@nocode/ide-core';

const isTauri =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function assertTauri() {
  if (!isTauri) {
    throw new Error(
      'This action requires the desktop app (run `npm run tauri:dev`).',
    );
  }
}

const PROJECT_FILTERS = [
  { name: 'No-Code Project', extensions: ['ncproj', 'json'] },
];

export const tauriPlatform: PlatformApi = {
  kind: 'tauri',
  isDesktop: true,
  autoSeedSample: true,

  async getCurrentProject(): Promise<string | null> {
    assertTauri();
    return invoke<string | null>('get_current_project');
  },

  async getProjectPath(): Promise<string | null> {
    assertTauri();
    return invoke<string | null>('get_project_path');
  },

  async newProject(name: string): Promise<string> {
    assertTauri();
    return invoke<string>('new_project', { name });
  },

  async seedSampleProject(): Promise<string> {
    assertTauri();
    return invoke<string>('seed_sample_project');
  },

  async setDbDriver(driver: string): Promise<string> {
    assertTauri();
    return invoke<string>('set_db_driver', { driver });
  },

  async setFrontendFramework(framework: string): Promise<string> {
    assertTauri();
    return invoke<string>('set_frontend_framework', { framework });
  },

  async updateProject(projectJson: string): Promise<void> {
    assertTauri();
    return invoke<void>('update_project', { projectJson });
  },

  async loadProject(path: string): Promise<string> {
    assertTauri();
    return invoke<string>('load_project', { path });
  },

  async saveProject(path: string): Promise<void> {
    assertTauri();
    return invoke<void>('save_project', { path });
  },

  async generateCode(outputDir: string): Promise<GenerationReport> {
    assertTauri();
    return invoke<GenerationReport>('generate_code', { outputDir });
  },

  async pickOpenPath(): Promise<string | null> {
    assertTauri();
    const result = await open({ multiple: false, filters: PROJECT_FILTERS });
    return typeof result === 'string' ? result : null;
  },

  async pickSavePath(defaultName: string): Promise<string | null> {
    assertTauri();
    const result = await save({
      defaultPath: defaultName,
      filters: PROJECT_FILTERS,
    });
    return typeof result === 'string' ? result : null;
  },

  async pickDirectory(): Promise<string | null> {
    assertTauri();
    const result = await open({ directory: true, multiple: false });
    return typeof result === 'string' ? result : null;
  },

  async bpmnImport(xml: string, name: string): Promise<string> {
    assertTauri();
    return invoke<string>('bpmn_import', { xml, name });
  },

  async bpmnExport(workflowJson: string): Promise<string> {
    assertTauri();
    return invoke<string>('bpmn_export', { workflowJson });
  },

  async bpmnValidate(workflowJson: string): Promise<BpmnValidationReport> {
    assertTauri();
    return invoke<BpmnValidationReport>('bpmn_validate', { workflowJson });
  },

  async workflowRun(
    workflowJson: string,
    variables: Record<string, unknown> = {},
    autoCompleteTasks = false,
  ): Promise<WorkflowRunSummary> {
    assertTauri();
    return invoke<WorkflowRunSummary>('workflow_run', {
      workflowJson,
      variables,
      autoCompleteTasks,
    });
  },

  // Deploy surface (Phase 15). The desktop owns its own encrypted vault and
  // talks to the same `nocode-deploy` crate through Tauri commands. Response
  // shapes are identical to the web REST API, so the shared UI is unchanged.
  async deployGithubGet(): Promise<GitHubConfigResponse> {
    assertTauri();
    return invoke<GitHubConfigResponse>('github_get_config');
  },

  async deployGithubSet(request: GitHubConfigRequest): Promise<GitHubConfigResponse> {
    assertTauri();
    return invoke<GitHubConfigResponse>('github_set_config', {
      owner: request.owner,
      repo: request.repo,
      branch: request.branch,
      token: request.token,
    });
  },

  async deployDockerhubGet(): Promise<RegistryConfigResponse> {
    assertTauri();
    return invoke<RegistryConfigResponse>('dockerhub_get_config');
  },

  async deployDockerhubSet(request: DockerHubConfigRequest): Promise<RegistryConfigResponse> {
    assertTauri();
    return invoke<RegistryConfigResponse>('dockerhub_set_config', {
      namespace: request.namespace,
      registry: request.registry,
      token: request.token ?? null,
    });
  },

  async deployProject(_projectId: string, apiBase?: string): Promise<DeploymentResponse> {
    assertTauri();
    // The desktop command deploys whichever project is currently open and
    // filters history by that project's id, so it needs no path argument.
    return invoke<DeploymentResponse>('deploy_project', { apiBase: apiBase ?? null });
  },

  async deployList(projectId: string): Promise<DeploymentResponse[]> {
    assertTauri();
    // The web host refreshes CI status inside its list route. The desktop
    // polls the most recent non-terminal deployment here instead, so history
    // does not stay stuck at `pushed` forever.
    const existing = await invoke<DeploymentResponse[]>('list_deployments');
    const pending = existing.find(
      (row) =>
        row.project_id === projectId &&
        !['ci_succeeded', 'ci_failed', 'failed'].includes(row.status),
    );
    if (pending) {
      try {
        await invoke<DeploymentResponse>('refresh_deployment', { id: pending.id });
      } catch {
        // Best effort: a GitHub outage must not hide stored history.
      }
    }
    const all = await invoke<DeploymentResponse[]>('list_deployments');
    return all.filter((row) => row.project_id === projectId);
  },
};
