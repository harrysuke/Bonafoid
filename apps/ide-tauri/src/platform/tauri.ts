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

import { Channel, invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
  AgentCapabilities,
  AgentChatHandle,
  AgentChatRequest,
  AgentEvent,
  AgentSettings,
  AgentSettingsInput,
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

  // Assistant surface (Phase 32). The desktop runs the same `nocode_agent`
  // loop the server runs, in-process, over the file the canvas has open. There
  // is no port, no token and no CSRF: the only network call is to the model.
  //
  // `projectId` is accepted for signature parity with the web host and
  // deliberately ignored — locally the project *is* the open file.
  agentCapabilities: (): Promise<AgentCapabilities> => {
    assertTauri();
    return invoke<AgentCapabilities>('agent_capabilities');
  },

  agentChat: async (
    _projectId: string,
    request: AgentChatRequest,
    onEvent: (event: AgentEvent) => void,
  ): Promise<AgentChatHandle> => {
    assertTauri();

    // One ordered channel per run; Tauri closes it with the window, so there is
    // no stream to leak if the drawer is unmounted mid-run.
    const channel = new Channel<AgentEvent>();
    let aborted = false;
    channel.onmessage = (event) => {
      // A late frame after Stop must not resurrect the timeline the store just
      // froze. The Rust side drops its receiver too, but that is one round trip
      // behind this flag.
      if (!aborted) onEvent(event);
    };

    // Read the working copy *before* starting. The command prefers the saved
    // file and falls back to this JSON for a project that was never saved, and
    // awaiting it here means `abort()` can only ever arrive after the run
    // exists — otherwise a cancel could be swallowed by the run's own reset.
    const projectJson = await invoke<string | null>('get_current_project');

    // Not awaited: the store needs the handle while the run is still going.
    // `done` is what it awaits, and it rejects with the command's message.
    const done = invoke<unknown>('agent_chat', {
      projectJson,
      request: { prompt: request.prompt, mode: request.mode ?? null },
      onEvent: channel,
    }).then(() => undefined);

    return {
      abort: () => {
        aborted = true;
        // Best effort: the run's next event emission is what actually stops it,
        // and the flag already keeps the drawer quiet either way.
        void invoke('agent_cancel').catch(() => undefined);
      },
      done,
    };
  },

  // Settings for the local provider. The key never comes back out: the command
  // returns `has_key` and nothing else, so this value can safely sit in React
  // state.
  agentSettings: (): Promise<AgentSettings> => {
    assertTauri();
    return invoke<AgentSettings>('agent_settings_get');
  },

  agentSettingsSave: (input: AgentSettingsInput): Promise<AgentSettings> => {
    assertTauri();
    return invoke<AgentSettings>('agent_settings_set', {
      provider: input.provider,
      model: input.model ?? null,
      baseUrl: input.base_url ?? null,
      // Absent, not empty: the command keeps the stored key when this is null,
      // so changing the model does not force the key to be retyped.
      apiKey: input.api_key ?? null,
    });
  },

  agentKeyClear: (): Promise<AgentSettings> => {
    assertTauri();
    return invoke<AgentSettings>('agent_key_clear');
  },

  agentTestConnection: (): Promise<string> => {
    assertTauri();
    return invoke<string>('agent_test_connection');
  },
};
