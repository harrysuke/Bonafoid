/**
 * Browser `PlatformApi` implementation for the web IDE.
 *
 * The web host has no local file system: the Axum server owns project storage,
 * generation and BPMN conversion. This adapter therefore maps the file-shaped
 * `PlatformApi` calls onto the server REST API:
 *
 * - `getCurrentProject` / `updateProject` are an in-memory working copy so the
 *   shared store keeps working unchanged.
 * - `saveProject(path)` persists the working copy; `path` is the synthetic
 *   `api://projects/{id}` handle returned by `getProjectPath`.
 * - The pickers become server operations (`listProjects` / `openProjectById`)
 *   plus browser downloads.
 */

import type {
  BpmnValidationReport,
  GenerationReport,
  PlatformApi,
  PlatformProject,
  PlatformUser,
  PreviewBuildTarget,
} from '@nocode/ide-core';
import { currentUser as sessionUser, logout as endSession } from '../auth/session';
import { bpmnApi, generationApi, previewApi, previewUrl, projectsApi } from '../api/client';
import type { ProjectDetail } from '../api/client';

/** Working copy of the project currently open in the browser. */
let workingCopy: string | null = null;
let workingCopyId: string | null = null;
let workingCopyRevision = 0;

function parseProject(json: string): ProjectDetail['data'] {
  return JSON.parse(json) as ProjectDetail['data'];
}

/** The synthetic handle the shared store uses as `projectPath`. */
function projectHandle(id: string): string {
  return `api://projects/${id}`;
}

function idFromHandle(path: string | null): string | null {
  if (!path || !path.startsWith('api://projects/')) return null;
  const id = path.slice('api://projects/'.length);
  return id.length > 0 ? id : null;
}

function requireProjectId(): string {
  if (!workingCopyId) throw new Error('Web platform: no project is open.');
  return workingCopyId;
}

/** Adopt a server project as the working copy. */
function adopt(detail: ProjectDetail): string {
  workingCopyId = detail.id;
  workingCopyRevision = detail.revision;
  workingCopy = JSON.stringify(detail.data);
  return workingCopy;
}

export const webPlatform: PlatformApi = {
  kind: 'web',
  isDesktop: false,
  autoSeedSample: false,

  async getCurrentProject(): Promise<string | null> {
    return workingCopy;
  },

  async getProjectPath(): Promise<string | null> {
    return workingCopyId ? projectHandle(workingCopyId) : null;
  },

  async newProject(name: string): Promise<string> {
    const detail = await projectsApi.create(name);
    return adopt(detail);
  },

  async seedSampleProject(): Promise<string> {
    // The server has no sample endpoint yet; an empty project is equivalent
    // for the web host (the desktop shell is where the demo lives).
    const detail = await projectsApi.create('My App');
    return adopt(detail);
  },

  async setDbDriver(driver: string): Promise<string> {
    return mutateWorkingCopy((project) => {
      const settings = project as {
        settings: { database: { driver: string } };
      };
      settings.settings.database.driver = driver;
    });
  },

  async setFrontendFramework(framework: string): Promise<string> {
    return mutateWorkingCopy((project) => {
      const settings = project as {
        settings: { frontend: { framework: string } };
      };
      settings.settings.frontend.framework = framework;
    });
  },

  async updateProject(projectJson: string): Promise<void> {
    workingCopy = projectJson;
  },

  async loadProject(path: string): Promise<string> {
    const id = idFromHandle(path);
    if (!id) throw new Error(`Web platform: "${path}" is not a project handle.`);
    return adopt(await projectsApi.get(id));
  },

  async saveProject(path: string): Promise<void> {
    const id = idFromHandle(path) ?? requireProjectId();
    if (!workingCopy) throw new Error('Web platform: nothing to save.');
    const detail = await projectsApi.update(id, workingCopyRevision, JSON.parse(workingCopy));
    workingCopyRevision = detail.revision;
    workingCopy = JSON.stringify(detail.data);
  },

  async generateCode(_outputDir: string): Promise<GenerationReport> {
    const id = requireProjectId();
    const files = await generationApi.generate(id);
    // The server owns the workspace, so the caller-supplied directory is
    // ignored: the artifacts come back as a ZIP download.
    await generationApi.downloadArchive(id, `${files.project_id}.zip`);
    return {
      output_dir: "api://projects/" + id,
      backend_files: files.files.filter((file) => file.path.startsWith('backend/')).length,
      frontend_files: files.files.filter((file) => file.path.startsWith('frontend/')).length,
      migrations: files.files.filter((file) => file.path.includes('migrations/')).length,
    };
  },

  async pickOpenPath(): Promise<string | null> {
    // The shell routes to the project picker when `listProjects` exists; this
    // fallback only fires if the IDE asks for a path directly.
    const projects = await listProjectsOnServer();
    return projects.length > 0 ? projectHandle(projects[0].id) : null;
  },

  async pickSavePath(defaultName: string): Promise<string | null> {
    if (workingCopyId) return projectHandle(workingCopyId);
    // Nothing open yet: the caller should have created a project first.
    void defaultName;
    return null;
  },

  async pickDirectory(): Promise<string | null> {
    // No local directories in the browser; the server picks the workspace.
    return null;
  },

  async bpmnImport(xml: string, name: string): Promise<string> {
    return bpmnApi.import(xml, name);
  },

  async bpmnExport(workflowJson: string): Promise<string> {
    return bpmnApi.export(workflowJson);
  },

  async bpmnValidate(workflowJson: string): Promise<BpmnValidationReport> {
    return bpmnApi.validate(workflowJson);
  },

  async listProjects(): Promise<PlatformProject[]> {
    return listProjectsOnServer();
  },

  async openProjectById(id: string): Promise<string> {
    return adopt(await projectsApi.get(id));
  },

  async createProject(name: string): Promise<string> {
    return adopt(await projectsApi.create(name));
  },

  async deleteProject(id: string): Promise<void> {
    await projectsApi.remove(id);
    if (workingCopyId === id) {
      workingCopyId = null;
      workingCopy = null;
      workingCopyRevision = 0;
    }
  },

  async previewBuild(projectId: string, target: PreviewBuildTarget = 'all') {
    return previewApi.start(projectId, target);
  },

  async previewStatus(buildId: string) {
    return previewApi.status(buildId);
  },

  async previewCancel(buildId: string): Promise<void> {
    await previewApi.cancel(buildId);
  },

  async previewBuilds() {
    return previewApi.list();
  },

  async previewCapabilities() {
    return previewApi.capabilities();
  },

  async previewRun(buildId: string) {
    return previewApi.run(buildId);
  },

  async previewStop(buildId: string): Promise<void> {
    await previewApi.stop(buildId);
  },

  previewUrl(buildId: string): string {
    return previewUrl(buildId);
  },

  async workflowRun(
    workflowJson: string,
    variables: Record<string, unknown> = {},
    autoCompleteTasks = false,
  ) {
    return bpmnApi.run(workflowJson, variables, autoCompleteTasks);
  },

  currentUser(): PlatformUser | null {
    const user = sessionUser();
    return user ? { id: user.id, email: user.email } : null;
  },

  async logout(): Promise<void> {
    await endSession();
  },
};

/** Apply an in-place edit to the working copy and return its JSON. */
async function mutateWorkingCopy(mutate: (project: unknown) => void): Promise<string> {
  if (!workingCopy) throw new Error('Web platform: no project is open.');
  const project = parseProject(workingCopy);
  mutate(project);
  workingCopy = JSON.stringify(project);
  return workingCopy;
}

/** Server-side project listing shared by `listProjects` and `pickOpenPath`. */
async function listProjectsOnServer(): Promise<PlatformProject[]> {
  const projects = await projectsApi.list();
  return projects.map((project) => ({
    id: project.id,
    name: project.name,
    description: project.description,
    revision: project.revision,
    updatedAt: project.updated_at,
  }));
}

/** Test seam: reset the module-level working copy. */
export function __resetWebPlatformState(): void {
  workingCopy = null;
  workingCopyId = null;
  workingCopyRevision = 0;
}
