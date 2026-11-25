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
  AdminAnalytics,
  AdminUsage,
  AdminUser,
  AdminUserUpdate,
  AgentChatHandle,
  AgentChatRequest,
  AgentEvent,
  AuditEntry,
  BillingSession,
  BpmnValidationReport,
  Branch,
  CancelResult,
  ConnectorCreateRequest,
  ConnectorInfo,
  ConnectorInstance,
  ConnectorInvokeRequest,
  ConnectorInvokeResult,
  ConnectorTestResult,
  ConnectorUpdateRequest,
  DatabaseDriver,
  DatabaseSettings,
  DeploymentResponse,
  DiffResult,
  DockerHubConfigRequest,
  Entitlement,
  Environment,
  FlagCreateRequest,
  FlagOverride,
  FlagUpdateRequest,
  GenerationReport,
  GitHubConfigRequest,
  GitHubConfigResponse,
  ImpersonationResult,
  Incident,
  Invoice,
  Plan,
  PlanCreateRequest,
  PlatformApi,
  PlatformFlag,
  PlatformPlugin,
  PlatformProject,
  PlatformUser,
  PreviewBuildTarget,
  RegistryConfigResponse,
  Revision,
  SecretKey,
  StatusReport,
  Subscription,
  Tenant,
  TenantMember,
  UsageMeter,
} from '@nocode/ide-core';
import { applyDatabaseDriver } from '@nocode/ide-core';
import { currentUser as sessionUser, logout as endSession } from '../auth/session';
import {
  adminApi,
  agentApi,
  billingApi,
  bpmnApi,
  connectorApi,
  deployApi,
  generationApi,
  pluginApi,
  previewApi,
  previewUrl,
  projectsApi,
  rememberCsrfToken,
  statusApi,
  tenantApi,
  versionApi,
} from '../api/client';
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
        settings: { database: { driver: string; url: string } };
      };
      settings.settings.database = applyDatabaseDriver(
        settings.settings.database as DatabaseSettings,
        driver as DatabaseDriver,
      );
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

  async pluginList(): Promise<PlatformPlugin[]> {
    return pluginApi.list();
  },

  async pluginInstall(manifest: string, module: Uint8Array): Promise<PlatformPlugin> {
    return pluginApi.install(manifest, module);
  },

  async pluginSetEnabled(id: string, enabled: boolean): Promise<PlatformPlugin> {
    return pluginApi.setEnabled(id, enabled);
  },

  async pluginUninstall(id: string): Promise<void> {
    await pluginApi.uninstall(id);
  },

  // Deploy surface (Phase 15). `deployApi` sends the CSRF header on every
  // mutating request and never receives a token back: the config responses
  // are redacted server-side.
  deployGithubGet: (): Promise<GitHubConfigResponse> => deployApi.githubConfig(),

  deployGithubSet: (request: GitHubConfigRequest): Promise<GitHubConfigResponse> =>
    deployApi.setGithubConfig(request),

  deployDockerhubGet: (): Promise<RegistryConfigResponse> => deployApi.dockerhubConfig(),

  deployDockerhubSet: (request: DockerHubConfigRequest): Promise<RegistryConfigResponse> =>
    deployApi.setDockerhubConfig(request),

  deployProject: (projectId: string, apiBase?: string): Promise<DeploymentResponse> =>
    deployApi.deploy(projectId, apiBase),

  deployList: (projectId: string): Promise<DeploymentResponse[]> =>
    deployApi.list(projectId),

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

  /* SaaS surfaces (Phases 7-11). All delegates go through the typed REST
   * client, which attaches cookies and the CSRF header on mutations. No
   * connector or environment secret is ever cached in this module. */

  tenantList: (): Promise<Tenant[]> => tenantApi.list(),
  tenantCreate: (name: string, slug: string): Promise<Tenant> => tenantApi.create(name, slug),
  tenantUpdate: (id: string, name: string, status: string): Promise<Tenant> =>
    tenantApi.update(id, name, status),
  tenantSwitch: async (id: string): Promise<{ tenant_id: string; csrf_token: string }> => {
    const result = await tenantApi.switch(id);
    // The server issues the same CSRF token back; refresh the client cache so
    // subsequent mutations keep working after the tenant switch.
    rememberCsrfToken(result.csrf_token);
    return result;
  },
  tenantMembers: (id: string): Promise<TenantMember[]> => tenantApi.members(id),
  tenantAddMember: (id: string, userId: string, role: string): Promise<TenantMember> =>
    tenantApi.addMember(id, userId, role),
  tenantRemoveMember: (id: string, userId: string): Promise<void> =>
    tenantApi.removeMember(id, userId),

  billingPlans: (): Promise<Plan[]> => billingApi.plans(),
  billingCreatePlan: (body: PlanCreateRequest): Promise<Plan> => billingApi.createPlan(body),
  billingSubscription: (): Promise<Subscription | null> => billingApi.subscription(),
  billingSubscribe: (planId: string, seatCount?: number): Promise<Subscription> =>
    billingApi.subscribe(planId, seatCount),
  billingCancel: (): Promise<CancelResult> => billingApi.cancel(),
  billingInvoices: (): Promise<Invoice[]> => billingApi.invoices(),
  billingEntitlements: (): Promise<Entitlement[]> => billingApi.entitlements(),
  billingUsage: (): Promise<UsageMeter[]> => billingApi.usage(),
  billingRecordUsage: (meterKey: string, quantity: number): Promise<UsageMeter> =>
    billingApi.recordUsage(meterKey, quantity),
  billingCheckout: (
    planId: string,
    successUrl: string,
    cancelUrl: string,
  ): Promise<BillingSession> => billingApi.checkout(planId, successUrl, cancelUrl),
  billingPortal: (returnUrl: string): Promise<BillingSession> => billingApi.portal(returnUrl),

  adminTenants: (search?: string, limit?: number, offset?: number): Promise<Tenant[]> =>
    adminApi.tenants(search, limit, offset),
  adminUpdateTenant: (id: string, status: string): Promise<Tenant> =>
    adminApi.updateTenant(id, status),
  adminUsers: (search?: string, limit?: number, offset?: number): Promise<AdminUser[]> =>
    adminApi.users(search, limit, offset),
  adminUpdateUser: (id: string, body: AdminUserUpdate): Promise<AdminUser> =>
    adminApi.updateUser(id, body),
  adminImpersonate: (id: string, reason?: string): Promise<ImpersonationResult> =>
    adminApi.impersonate(id, reason),
  adminAnalytics: (): Promise<AdminAnalytics> => adminApi.analytics(),
  adminUsage: (): Promise<AdminUsage> => adminApi.usage(),
  adminFlags: (): Promise<PlatformFlag[]> => adminApi.flags(),
  adminCreateFlag: (body: FlagCreateRequest): Promise<PlatformFlag> => adminApi.createFlag(body),
  adminUpdateFlag: (key: string, body: FlagUpdateRequest): Promise<PlatformFlag> =>
    adminApi.updateFlag(key, body),
  adminDeleteFlag: (key: string): Promise<void> => adminApi.deleteFlag(key),
  adminSetFlagOverride: (
    flagKey: string,
    tenantId: string,
    enabled: boolean,
  ): Promise<FlagOverride> => adminApi.setOverride(flagKey, tenantId, enabled),
  adminEvaluateFlag: (key: string): Promise<{ key: string; enabled: boolean }> =>
    adminApi.evaluateFlag(key),
  adminAudit: (limit?: number, offset?: number): Promise<AuditEntry[]> =>
    adminApi.audit(limit, offset),

  statusGet: (): Promise<StatusReport> => statusApi.get(),
  statusIncidents: (): Promise<Incident[]> => statusApi.incidents(),

  connectorCatalog: (): Promise<ConnectorInfo[]> => connectorApi.catalog(),
  connectorList: (): Promise<ConnectorInstance[]> => connectorApi.list(),
  connectorCreate: (body: ConnectorCreateRequest): Promise<ConnectorInstance> =>
    connectorApi.create(body),
  connectorUpdate: (id: string, body: ConnectorUpdateRequest): Promise<ConnectorInstance> =>
    connectorApi.update(id, body),
  connectorDelete: (id: string): Promise<void> => connectorApi.remove(id),
  connectorTest: (id: string): Promise<ConnectorTestResult> => connectorApi.test(id),
  connectorInvoke: (id: string, body: ConnectorInvokeRequest): Promise<ConnectorInvokeResult> =>
    connectorApi.invoke(id, body),

  versionBranches: (projectId: string): Promise<Branch[]> => versionApi.branches(projectId),
  versionCreateBranch: (
    projectId: string,
    name: string,
    fromRevisionId?: string,
  ): Promise<Branch> => versionApi.createBranch(projectId, name, fromRevisionId),
  agentChat: (
    projectId: string,
    request: AgentChatRequest,
    onEvent: (event: AgentEvent) => void,
  ): Promise<AgentChatHandle> => agentApi.chat(projectId, request, onEvent),

  versionHistory: (projectId: string, branch: string): Promise<Revision[]> =>
    versionApi.history(projectId, branch),
  versionDiff: (projectId: string, from: string, to: string): Promise<DiffResult> =>
    versionApi.diff(projectId, from, to),
  versionCommit: (
    projectId: string,
    branch: string,
    message: string,
    data: unknown,
  ): Promise<Revision> => versionApi.commit(projectId, branch, message, data),
  versionMerge: (
    targetBranch: string,
    projectId: string,
    sourceBranch: string,
    strategy: string,
  ): Promise<Revision> => versionApi.merge(targetBranch, projectId, sourceBranch, strategy),
  versionRestore: (projectId: string, revisionId: string): Promise<Revision> =>
    versionApi.restore(projectId, revisionId),
  versionEnvironments: (projectId: string): Promise<Environment[]> =>
    versionApi.environments(projectId),
  versionCreateEnvironment: (
    projectId: string,
    name: string,
    config?: Record<string, unknown>,
  ): Promise<Environment> => versionApi.createEnvironment(projectId, name, config),
  versionUpdateEnvironment: (
    name: string,
    projectId: string,
    config: Record<string, unknown>,
  ): Promise<Environment> => versionApi.updateEnvironment(name, projectId, config),
  versionPromote: (name: string, projectId: string, revisionId: string): Promise<Environment> =>
    versionApi.promote(name, projectId, revisionId),
  versionRollback: (name: string, projectId: string, revisionId: string): Promise<Environment> =>
    versionApi.rollback(name, projectId, revisionId),
  versionSecrets: (name: string, projectId: string): Promise<SecretKey[]> =>
    versionApi.secrets(name, projectId),
  versionSetSecret: (
    name: string,
    projectId: string,
    key: string,
    value: string,
  ): Promise<SecretKey> => versionApi.setSecret(name, projectId, key, value),
  versionDeleteSecret: (name: string, projectId: string, key: string): Promise<void> =>
    versionApi.deleteSecret(name, projectId, key),
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
