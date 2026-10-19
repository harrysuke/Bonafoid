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
  GenerationReport,
  PlatformApi,
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
};
