/**
 * Browser `PlatformApi` scaffold for the web IDE.
 *
 * M1 keeps this deliberately thin: project JSON persists in `localStorage`
 * so a page reload keeps the working copy, while every operation that needs
 * the server-side backend (new/seed project, driver switches, code generation,
 * file pickers) throws an explicit "not implemented" until milestone M3/M5
 * wires the Axum REST API.
 *
 * `ide-core` must never import `@tauri-apps/*`; this file imports nothing
 * host-specific so the same shared package runs unchanged in the browser.
 */

import type {
  BpmnValidationReport,
  GenerationReport,
  PlatformApi,
} from '@nocode/ide-core';

const PROJECT_KEY = 'nocode-ide:project';
const PSEUDO_PATH = 'browser://local';
const API_BASE =
  (import.meta.env.VITE_API_BASE as string | undefined)?.replace(/\/$/, '') ?? '';

function storage(): Storage | null {
  try {
    return typeof window !== 'undefined' ? window.localStorage : null;
  } catch {
    return null;
  }
}

function notImplemented(action: string): never {
  throw new Error(
    `Web platform: ${action} is not available yet (the server backend arrives in milestone M3).`,
  );
}

/** POST JSON to the IDE server, surfacing server-side error messages. */
async function postJson<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    method: 'POST',
    credentials: 'include',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${path} failed (${response.status}): ${text}`);
  }
  return (await response.json()) as T;
}

export const webPlatform: PlatformApi = {
  kind: 'web',
  isDesktop: false,
  autoSeedSample: false,

  async getCurrentProject(): Promise<string | null> {
    return storage()?.getItem(PROJECT_KEY) ?? null;
  },

  async getProjectPath(): Promise<string | null> {
    return storage()?.getItem(PROJECT_KEY) ? PSEUDO_PATH : null;
  },

  async newProject(_name: string): Promise<string> {
    notImplemented('creating a project');
  },

  async seedSampleProject(): Promise<string> {
    notImplemented('seeding the sample project');
  },

  async setDbDriver(_driver: string): Promise<string> {
    notImplemented('switching the database driver');
  },

  async setFrontendFramework(_framework: string): Promise<string> {
    notImplemented('switching the frontend framework');
  },

  async updateProject(projectJson: string): Promise<void> {
    storage()?.setItem(PROJECT_KEY, projectJson);
  },

  async loadProject(_path: string): Promise<string> {
    notImplemented('loading a project');
  },

  async saveProject(_path: string): Promise<void> {
    notImplemented('saving a project');
  },

  async generateCode(_outputDir: string): Promise<GenerationReport> {
    notImplemented('code generation');
  },

  async pickOpenPath(): Promise<string | null> {
    notImplemented('opening a project file');
  },

  async pickSavePath(_defaultName: string): Promise<string | null> {
    notImplemented('choosing a save location');
  },

  async pickDirectory(): Promise<string | null> {
    notImplemented('choosing a directory');
  },

  async bpmnImport(xml: string, name: string): Promise<string> {
    const result = await postJson<{ workflow: unknown }>('/api/v1/bpmn/import', {
      xml,
      name,
    });
    return JSON.stringify(result.workflow);
  },

  async bpmnExport(workflowJson: string): Promise<string> {
    const workflow = JSON.parse(workflowJson) as unknown;
    const result = await postJson<{ xml: string }>('/api/v1/bpmn/export', { workflow });
    return result.xml;
  },

  async bpmnValidate(workflowJson: string): Promise<BpmnValidationReport> {
    const workflow = JSON.parse(workflowJson) as unknown;
    return postJson<BpmnValidationReport>('/api/v1/bpmn/validate', { workflow });
  },
};
