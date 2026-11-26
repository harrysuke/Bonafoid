/**
 * Typed REST client for the No-Code IDE server.
 *
 * The server speaks cookie authentication (`nocode_access` HttpOnly +
 * `nocode_refresh` + a double-submit `nocode_csrf` cookie) and requires the
 * CSRF token echoed in `x-csrf-token` on every state-changing request that is
 * not bearer-authenticated (see `crates/ide-server/src/auth/mod.rs`).
 *
 * All requests send `credentials: 'include'` so the browser attaches the
 * session cookies, and a 401 triggers exactly one refresh + replay.
 */

import type {
  AdminAnalytics,
  AdminUsage,
  AdminUser,
  AdminUserUpdate,
  AgentChatHandle,
  AgentChatRequest,
  AgentConversation,
  AgentConversationDetail,
  AgentEvent,
  AgentProviderList,
  AuditEntry,
  BillingSession,
  Branch,
  CancelResult,
  ConnectorCreateRequest,
  ConnectorInfo,
  ConnectorInstance,
  ConnectorInvokeRequest,
  ConnectorInvokeResult,
  ConnectorTestResult,
  ConnectorUpdateRequest,
  DeploymentResponse,
  DiffResult,
  DockerHubConfigRequest,
  Entitlement,
  Environment,
  FlagCreateRequest,
  FlagOverride,
  FlagUpdateRequest,
  GitHubConfigRequest,
  GitHubConfigResponse,
  ImpersonationResult,
  Incident,
  Invoice,
  Plan,
  PlanCreateRequest,
  PlatformFlag,
  RegistryConfigResponse,
  Revision,
  SecretKey,
  StatusReport,
  Subscription,
  Tenant,
  TenantMember,
  UsageMeter,
} from '@nocode/ide-core';
import { createSseDecoder } from '@nocode/ide-core';

export interface ApiUser {
  id: string;
  email: string;
  is_verified: boolean;
  created_at: string;
}

export interface AuthResponse {
  user: ApiUser;
  expires_in: number;
  csrf_token: string;
}

export interface ProjectSummary {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  version: string;
  revision: number;
  created_at: string;
  updated_at: string;
}

/**
 * A project with its model. The server flattens the summary next to the data
 * (`ProjectDetail { #[serde(flatten)] summary, data }`), so the two are one
 * object on the wire.
 */
export interface ProjectDetail extends ProjectSummary {
  data: unknown;
}

export interface GeneratedFiles {
  project_id: string;
  file_count: number;
  total_bytes: number;
  files: { path: string; size: number }[];
}

export interface ValidationIssue {
  severity: string;
  node_id?: string;
  message: string;
}

export interface ValidationReport {
  valid: boolean;
  issues: ValidationIssue[];
}

export interface WorkflowRunStep {
  node_id: string;
  node_name: string;
  node_type: string;
  outcome: string;
}

export interface WorkflowOpenTask {
  id: string;
  node_name: string;
  status: string;
}

export interface WorkflowRunEvent {
  event_type: string;
  node_name?: string;
  detail?: string;
}

export interface WorkflowRunSummary {
  instance_id: string;
  workflow_id: string;
  workflow_name: string;
  status: string;
  completed: boolean;
  auto_completed_tasks: boolean;
  steps: WorkflowRunStep[];
  open_tasks: WorkflowOpenTask[];
  events: WorkflowRunEvent[];
  tokens_waiting: string[];
  variables: Record<string, unknown>;
}

/** JSON API error carrying the server's stable code and message. */
export class ApiRequestError extends Error {
  readonly status: number;
  readonly code: string;
  /**
   * The whole error body, when it had one.
   *
   * Some codes carry fields a caller must act on rather than just display — a
   * 409 `project_locked` names the holder and the remaining lease TTL, which is
   * what the drawer's countdown is built from. Flattening those into a message
   * string would leave the client parsing prose.
   */
  readonly details?: Record<string, unknown>;

  constructor(status: number, code: string, message: string, details?: Record<string, unknown>) {
    super(message);
    this.name = 'ApiRequestError';
    this.status = status;
    this.code = code;
    this.details = details;
  }
}

const RAW_BASE = (import.meta.env.VITE_API_BASE as string | undefined) ?? '';
export const API_BASE = RAW_BASE.replace(/\/$/, '');

/** Cookie name that carries the double-submit CSRF token. */
const CSRF_COOKIE = 'nocode_csrf';

let cachedCsrfToken: string | null = null;
let refreshInFlight: Promise<boolean> | null = null;
const unauthorizedListeners = new Set<() => void>();

/** Read a non-HttpOnly cookie value (the CSRF token). */
function readCookie(name: string): string | null {
  if (typeof document === 'undefined') return null;
  for (const pair of document.cookie.split(';')) {
    const [key, ...rest] = pair.trim().split('=');
    if (key === name) return decodeURIComponent(rest.join('='));
  }
  return null;
}

export function csrfToken(): string | null {
  return cachedCsrfToken ?? readCookie(CSRF_COOKIE);
}

export function rememberCsrfToken(token: string | null): void {
  cachedCsrfToken = token;
}

/** Notified when the session is definitively gone (refresh failed). */
export function onUnauthorized(listener: () => void): () => void {
  unauthorizedListeners.add(listener);
  return () => unauthorizedListeners.delete(listener);
}

function notifyUnauthorized(): void {
  cachedCsrfToken = null;
  unauthorizedListeners.forEach((listener) => listener());
}

const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);

interface RequestOptions {
  method?: string;
  body?: unknown;
  /** Skip the automatic refresh+replay (used by the refresh call itself). */
  noRetry?: boolean;
  /** Return the raw Response instead of parsed JSON (downloads). */
  raw?: boolean;
}

async function toError(response: Response): Promise<ApiRequestError> {
  let code = 'error';
  let message = response.statusText || `HTTP ${response.status}`;
  let details: Record<string, unknown> | undefined;
  try {
    const payload = (await response.json()) as Record<string, unknown> & {
      error?: string;
      message?: string;
    };
    if (payload.error) code = payload.error;
    if (payload.message) message = payload.message;
    details = payload;
  } catch {
    // Non-JSON error body: keep the status text.
  }
  return new ApiRequestError(response.status, code, message, details);
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const method = (options.method ?? 'GET').toUpperCase();
  const headers: Record<string, string> = {};
  if (options.body !== undefined) headers['Content-Type'] = 'application/json';
  if (!SAFE_METHODS.has(method)) {
    const token = csrfToken();
    if (token) headers['x-csrf-token'] = token;
  }

  const response = await fetch(`${API_BASE}${path}`, {
    method,
    headers,
    credentials: 'include',
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });

  if (response.status === 401 && !options.noRetry) {
    const refreshed = await tryRefresh();
    if (refreshed) return request<T>(path, options);
    notifyUnauthorized();
  }

  if (!response.ok) throw await toError(response);
  if (options.raw) return response as unknown as T;
  if (response.status === 204) return undefined as T;

  const text = await response.text();
  return (text ? (JSON.parse(text) as T) : undefined) as T;
}

/** Refresh the access token once, coalescing concurrent attempts. */
async function tryRefresh(): Promise<boolean> {
  if (!refreshInFlight) {
    refreshInFlight = (async () => {
      try {
        const result = await request<AuthResponse>('/api/v1/auth/refresh', {
          method: 'POST',
          noRetry: true,
        });
        rememberCsrfToken(result.csrf_token);
        return true;
      } catch {
        return false;
      } finally {
        refreshInFlight = null;
      }
    })();
  }
  return refreshInFlight;
}

/* ------------------------------------------------------------------ *
 * Auth
 * ------------------------------------------------------------------ */

export const authApi = {
  async register(email: string, password: string): Promise<AuthResponse> {
    const result = await request<AuthResponse>('/api/v1/auth/register', {
      method: 'POST',
      body: { email, password },
      noRetry: true,
    });
    rememberCsrfToken(result.csrf_token);
    return result;
  },

  async login(email: string, password: string): Promise<AuthResponse> {
    const result = await request<AuthResponse>('/api/v1/auth/login', {
      method: 'POST',
      body: { email, password },
      noRetry: true,
    });
    rememberCsrfToken(result.csrf_token);
    return result;
  },

  async logout(): Promise<void> {
    try {
      await request<void>('/api/v1/auth/logout', { method: 'POST' });
    } finally {
      cachedCsrfToken = null;
    }
  },

  /** Current user, or null when the session is absent/expired. */
  async me(): Promise<ApiUser | null> {
    try {
      return await request<ApiUser>('/api/v1/auth/me');
    } catch {
      return null;
    }
  },
};

/* ------------------------------------------------------------------ *
 * Projects
 * ------------------------------------------------------------------ */

export const projectsApi = {
  list: (): Promise<ProjectSummary[]> => request<ProjectSummary[]>('/api/v1/projects'),

  get: (id: string): Promise<ProjectDetail> => request<ProjectDetail>(`/api/v1/projects/${id}`),

  create: (name: string, data?: unknown): Promise<ProjectDetail> =>
    request<ProjectDetail>('/api/v1/projects', { method: 'POST', body: { name, data } }),

  /** Optimistic-concurrency write: the server rejects a stale revision. */
  update: (id: string, revision: number, data: unknown): Promise<ProjectDetail> =>
    request<ProjectDetail>(`/api/v1/projects/${id}`, {
      method: 'PUT',
      body: { revision, data },
    }),

  remove: (id: string): Promise<void> =>
    request<void>(`/api/v1/projects/${id}`, { method: 'DELETE' }),

  duplicate: (id: string): Promise<ProjectDetail> =>
    request<ProjectDetail>(`/api/v1/projects/${id}/duplicate`, { method: 'POST' }),

  import: (projectJson: unknown): Promise<ProjectDetail> =>
    request<ProjectDetail>('/api/v1/projects/import', { method: 'POST', body: projectJson }),

  exportUrl: (id: string): string => `${API_BASE}/api/v1/projects/${id}/export`,
};

/* ------------------------------------------------------------------ *
 * Generation & BPMN
 * ------------------------------------------------------------------ */

export const generationApi = {
  generate: (id: string): Promise<GeneratedFiles> =>
    request<GeneratedFiles>(`/api/v1/projects/${id}/generate`, { method: 'POST' }),

  files: (id: string): Promise<GeneratedFiles> =>
    request<GeneratedFiles>(`/api/v1/projects/${id}/files`),

  archiveUrl: (id: string): string => `${API_BASE}/api/v1/projects/${id}/archive`,

  async downloadArchive(id: string, filename: string): Promise<void> {
    const response = await request<Response>(`/api/v1/projects/${id}/archive`, { raw: true });
    const blob = await response.blob();
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = filename;
    document.body.appendChild(anchor);
    anchor.click();
    document.body.removeChild(anchor);
    URL.revokeObjectURL(url);
  },
};

/* ------------------------------------------------------------------ *
 * Assistant (agentic app generation)
 * ------------------------------------------------------------------ */

export const agentApi = {
  /**
   * Start an assistant run and stream its events.
   *
   * `request()` cannot do this: it parses JSON. This opens a POST with `fetch`,
   * decodes the `text/event-stream` body with the shared decoder, and calls
   * `onEvent` per frame.
   *
   * The promise resolves once the response headers arrive, so a 503
   * (unconfigured), 403 (scope) or 404 (project) rejects *before* the drawer
   * commits to a running state. Aborting the controller closes the connection,
   * which is how the server learns to cancel the run.
   */
  async chat(
    projectId: string,
    body: AgentChatRequest,
    onEvent: (event: AgentEvent) => void,
  ): Promise<AgentChatHandle> {
    const controller = new AbortController();
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      Accept: 'text/event-stream',
    };
    const token = csrfToken();
    if (token) headers['x-csrf-token'] = token;

    const response = await fetch(`${API_BASE}/api/v1/projects/${projectId}/agent/chat`, {
      method: 'POST',
      headers,
      credentials: 'include',
      body: JSON.stringify(body),
      signal: controller.signal,
    });

    if (!response.ok) throw await toError(response);
    if (!response.body) {
      throw new ApiRequestError(0, 'stream', 'This browser cannot stream the assistant.');
    }

    const decoder = createSseDecoder(onEvent);
    const text = new TextDecoder();

    const done = (async () => {
      const reader = response.body!.getReader();
      try {
        for (;;) {
          const { done: finished, value } = await reader.read();
          if (finished) break;
          decoder.push(text.decode(value, { stream: true }));
        }
        decoder.flush();
      } catch (error) {
        // An abort is a user action, not a failure.
        if (!controller.signal.aborted) throw error;
      } finally {
        reader.releaseLock();
      }
    })();

    return { abort: () => controller.abort(), done };
  },

  /** The provider catalog the picker shows. Never contains a key. */
  providers: (): Promise<AgentProviderList> =>
    request<AgentProviderList>('/api/v1/agent/providers'),

  /** The project's conversations, newest first. */
  conversations: (projectId: string): Promise<AgentConversation[]> =>
    request<AgentConversation[]>(`/api/v1/projects/${projectId}/agent/conversations`),

  /** One conversation with its messages, for the restore after a reload. */
  conversation: (projectId: string, conversationId: string): Promise<AgentConversationDetail> =>
    request<AgentConversationDetail>(
      `/api/v1/projects/${projectId}/agent/conversations/${conversationId}`,
    ),

  renameConversation: (
    projectId: string,
    conversationId: string,
    title: string,
  ): Promise<AgentConversation> =>
    request<AgentConversation>(
      `/api/v1/projects/${projectId}/agent/conversations/${conversationId}`,
      { method: 'PATCH', body: { title } },
    ),

  deleteConversation: (projectId: string, conversationId: string): Promise<void> =>
    request<void>(`/api/v1/projects/${projectId}/agent/conversations/${conversationId}`, {
      method: 'DELETE',
    }),
};

export const bpmnApi = {
  async import(xml: string, name: string): Promise<string> {
    const result = await request<{ workflow: unknown }>('/api/v1/bpmn/import', {
      method: 'POST',
      body: { xml, name },
    });
    return JSON.stringify(result.workflow);
  },

  async export(workflowJson: string): Promise<string> {
    const result = await request<{ xml: string }>('/api/v1/bpmn/export', {
      method: 'POST',
      body: { workflow: JSON.parse(workflowJson) as unknown },
    });
    return result.xml;
  },

  validate: (workflowJson: string): Promise<ValidationReport> =>
    request<ValidationReport>('/api/v1/bpmn/validate', {
      method: 'POST',
      body: { workflow: JSON.parse(workflowJson) as unknown },
    }),

  run: (
    workflowJson: string,
    variables: Record<string, unknown> = {},
    autoCompleteTasks = false,
  ): Promise<WorkflowRunSummary> =>
    request<WorkflowRunSummary>('/api/v1/bpmn/run', {
      method: 'POST',
      body: {
        workflow: JSON.parse(workflowJson) as unknown,
        variables,
        auto_complete_tasks: autoCompleteTasks,
      },
    }),
};

export const serverApi = {
  health: (): Promise<{ status: string; service: string }> => request('/health'),
  ready: (): Promise<{ status: string; database: string }> => request('/ready'),
};

/* ------------------------------------------------------------------ *
 * Hosted preview builds
 * ------------------------------------------------------------------ */

export interface PreviewBuild {
  id: string;
  project_id: string;
  state:
    | 'queued'
    | 'generating'
    | 'building_backend'
    | 'building_frontend'
    | 'ready'
    | 'cancelled'
    | 'failed';
  target: 'all' | 'backend' | 'frontend';
  error: string | null;
  workspace: string;
  dist_dir: string;
  logs: string[];
  created_at: string;
  updated_at: string;
}

export interface PreviewRun {
  build_id: string;
  port: number;
  pid: number | null;
  status: 'starting' | 'running' | 'stopped' | 'exited' | 'failed';
  requests: number;
}

export interface PreviewCapabilities {
  cargo: boolean;
  trunk: boolean;
  root: string;
  max_parallel: number;
  timeout_secs: number;
}

/** Public URL of a preview build's frontend (relative to the API base). */
export function previewUrl(buildId: string): string {
  return `${API_BASE}/preview/${buildId}/`;
}

export const previewApi = {
  capabilities: (): Promise<PreviewCapabilities> =>
    request<PreviewCapabilities>('/api/v1/preview/capabilities'),

  start: (projectId: string, target: 'all' | 'backend' | 'frontend' = 'all'): Promise<PreviewBuild> =>
    request<PreviewBuild>(`/api/v1/projects/${projectId}/preview/build`, {
      method: 'POST',
      body: { target },
    }),

  status: (buildId: string): Promise<PreviewBuild> =>
    request<PreviewBuild>(`/api/v1/builds/${buildId}`),

  list: (): Promise<PreviewBuild[]> => request<PreviewBuild[]>('/api/v1/builds'),

  cancel: (buildId: string): Promise<void> =>
    request<void>(`/api/v1/builds/${buildId}`, { method: 'DELETE' }),

  run: (buildId: string): Promise<PreviewRun> =>
    request<PreviewRun>(`/api/v1/builds/${buildId}/run`, { method: 'POST' }),

  stop: (buildId: string): Promise<void> =>
    request<PreviewRun>(`/api/v1/builds/${buildId}/stop`, { method: 'POST' }).then(() => undefined),

  runs: (): Promise<PreviewRun[]> => request<PreviewRun[]>('/api/v1/runs'),
};

/** An installed plugin, as the server reports it. */
export interface PlatformPlugin {
  id: string;
  name: string;
  version: string;
  api_version: string;
  enabled: boolean;
  permissions: string[];
  description?: string;
  author?: string;
  entry_points: string[];
}

/**
 * Upload a plugin as multipart form data.
 *
 * The manifest and the module are separate parts: the manifest is the
 * plugin's own `plugin.json` text, and the module is its WebAssembly
 * bytes.
 */
async function uploadPlugin(manifest: string, module: Uint8Array): Promise<PlatformPlugin> {
  const form = new FormData();
  form.append('manifest', new Blob([manifest], { type: 'application/json' }), 'plugin.json');
  form.append(
    'module',
    new Blob([module.slice().buffer as ArrayBuffer], { type: 'application/wasm' }),
    'plugin.wasm',
  );

  const headers: Record<string, string> = {};
  const token = csrfToken();
  if (token) headers['x-csrf-token'] = token;

  const response = await fetch(`${API_BASE}/api/v1/plugins`, {
    method: 'POST',
    headers,
    credentials: 'include',
    body: form,
  });

  if (!response.ok) throw await toError(response);
  return (await response.json()) as PlatformPlugin;
}

export const pluginApi = {
  list: (): Promise<PlatformPlugin[]> => request<PlatformPlugin[]>('/api/v1/plugins'),

  install: (manifest: string, module: Uint8Array): Promise<PlatformPlugin> =>
    uploadPlugin(manifest, module),

  setEnabled: (id: string, enabled: boolean): Promise<PlatformPlugin> =>
    request<PlatformPlugin>(`/api/v1/plugins/${encodeURIComponent(id)}/enable`, {
      method: 'POST',
      body: { enabled },
    }),

  uninstall: (id: string): Promise<void> =>
    request<void>(`/api/v1/plugins/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};

/* ------------------------------------------------------------------ *
 * Deploy (Phase 15)
 * ------------------------------------------------------------------ */

export const deployApi = {
  /** Redacted GitHub config — never a token. */
  githubConfig: (): Promise<GitHubConfigResponse> =>
    request<GitHubConfigResponse>('/api/v1/integrations/github'),

  /** Set (and verify) the GitHub PAT. The token is write-only. */
  setGithubConfig: (body: GitHubConfigRequest): Promise<GitHubConfigResponse> =>
    request<GitHubConfigResponse>('/api/v1/integrations/github', {
      method: 'POST',
      body,
    }),

  /** Redacted Docker Hub config — never a token. */
  dockerhubConfig: (): Promise<RegistryConfigResponse> =>
    request<RegistryConfigResponse>('/api/v1/integrations/dockerhub'),

  /** Set (and verify) the Docker Hub config. Omit `token` to keep the sealed one. */
  setDockerhubConfig: (body: DockerHubConfigRequest): Promise<RegistryConfigResponse> =>
    request<RegistryConfigResponse>('/api/v1/integrations/dockerhub', {
      method: 'POST',
      body,
    }),

  /** Deploy a project: generate → commit → push → CI. */
  deploy: (projectId: string, apiBase?: string): Promise<DeploymentResponse> =>
    request<DeploymentResponse>(`/api/v1/projects/${projectId}/deploy`, {
      method: 'POST',
      body: apiBase && apiBase.trim() ? { api_base: apiBase.trim() } : {},
    }),

  /** Deployment history for a project (refreshes CI status server-side). */
  list: (projectId: string): Promise<DeploymentResponse[]> =>
    request<DeploymentResponse[]>(`/api/v1/projects/${projectId}/deployments`),
};

/* ------------------------------------------------------------------ *
 * SaaS surfaces (Phases 7-11)
 * ------------------------------------------------------------------ */

/** Build an encoded query string, omitting absent or empty parameters. */
function queryString(params: Record<string, string | number | undefined>): string {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== '') query.set(key, String(value));
  }
  const encoded = query.toString();
  return encoded ? `?${encoded}` : '';
}

/** Tenancy / workspaces (Phase 7). */
export const tenantApi = {
  list: (): Promise<Tenant[]> => request<Tenant[]>('/api/v1/tenants'),

  create: (name: string, slug: string): Promise<Tenant> =>
    request<Tenant>('/api/v1/tenants', { method: 'POST', body: { name, slug } }),

  update: (id: string, name: string, status: string): Promise<Tenant> =>
    request<Tenant>(`/api/v1/tenants/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: { name, status },
    }),

  /** Switch the session tenant; the response carries the refreshed CSRF token. */
  switch: (id: string): Promise<{ tenant_id: string; csrf_token: string }> =>
    request<{ tenant_id: string; csrf_token: string }>(
      `/api/v1/tenants/${encodeURIComponent(id)}/switch`,
      { method: 'POST' },
    ),

  members: (id: string): Promise<TenantMember[]> =>
    request<TenantMember[]>(`/api/v1/tenants/${encodeURIComponent(id)}/members`),

  addMember: (id: string, userId: string, role: string): Promise<TenantMember> =>
    request<TenantMember>(`/api/v1/tenants/${encodeURIComponent(id)}/members`, {
      method: 'POST',
      body: { user_id: userId, role },
    }),

  removeMember: (id: string, userId: string): Promise<void> =>
    request<void>(
      `/api/v1/tenants/${encodeURIComponent(id)}/members/${encodeURIComponent(userId)}`,
      { method: 'DELETE' },
    ),
};

/** Billing and monetization (Phase 8). */
export const billingApi = {
  plans: (): Promise<Plan[]> => request<Plan[]>('/api/v1/billing/plans'),

  createPlan: (body: PlanCreateRequest): Promise<Plan> =>
    request<Plan>('/api/v1/billing/plans', { method: 'POST', body }),

  subscription: (): Promise<Subscription | null> =>
    request<Subscription | null>('/api/v1/billing/subscription'),

  subscribe: (planId: string, seatCount?: number): Promise<Subscription> =>
    request<Subscription>('/api/v1/billing/subscribe', {
      method: 'POST',
      body: seatCount === undefined ? { plan_id: planId } : { plan_id: planId, seat_count: seatCount },
    }),

  cancel: (): Promise<CancelResult> =>
    request<CancelResult>('/api/v1/billing/cancel', { method: 'POST' }),

  invoices: (): Promise<Invoice[]> => request<Invoice[]>('/api/v1/billing/invoices'),

  entitlements: (): Promise<Entitlement[]> =>
    request<Entitlement[]>('/api/v1/billing/entitlements'),

  usage: (): Promise<UsageMeter[]> => request<UsageMeter[]>('/api/v1/billing/usage'),

  recordUsage: (meterKey: string, quantity: number): Promise<UsageMeter> =>
    request<UsageMeter>('/api/v1/billing/record-usage', {
      method: 'POST',
      body: { meter_key: meterKey, quantity },
    }),

  checkout: (planId: string, successUrl: string, cancelUrl: string): Promise<BillingSession> =>
    request<BillingSession>('/api/v1/billing/checkout', {
      method: 'POST',
      body: { plan_id: planId, success_url: successUrl, cancel_url: cancelUrl },
    }),

  portal: (returnUrl: string): Promise<BillingSession> =>
    request<BillingSession>('/api/v1/billing/portal', {
      method: 'POST',
      body: { return_url: returnUrl },
    }),
};

/** Platform administration (Phase 9). */
export const adminApi = {
  tenants: (search?: string, limit?: number, offset?: number): Promise<Tenant[]> =>
    request<Tenant[]>(`/api/v1/admin/tenants${queryString({ search, limit, offset })}`),

  updateTenant: (id: string, status: string): Promise<Tenant> =>
    request<Tenant>(`/api/v1/admin/tenants/${encodeURIComponent(id)}/status`, {
      method: 'PATCH',
      body: { status },
    }),

  users: (search?: string, limit?: number, offset?: number): Promise<AdminUser[]> =>
    request<AdminUser[]>(`/api/v1/admin/users${queryString({ search, limit, offset })}`),

  updateUser: (id: string, body: AdminUserUpdate): Promise<AdminUser> =>
    request<AdminUser>(`/api/v1/admin/users/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body,
    }),

  impersonate: (id: string, reason?: string): Promise<ImpersonationResult> =>
    request<ImpersonationResult>(
      `/api/v1/admin/users/${encodeURIComponent(id)}/impersonate`,
      { method: 'POST', body: reason ? { reason } : {} },
    ),

  analytics: (): Promise<AdminAnalytics> =>
    request<AdminAnalytics>('/api/v1/admin/analytics'),

  usage: (): Promise<AdminUsage> => request<AdminUsage>('/api/v1/admin/usage'),

  flags: (): Promise<PlatformFlag[]> => request<PlatformFlag[]>('/api/v1/admin/flags'),

  createFlag: (body: FlagCreateRequest): Promise<PlatformFlag> =>
    request<PlatformFlag>('/api/v1/admin/flags', { method: 'POST', body }),

  updateFlag: (key: string, body: FlagUpdateRequest): Promise<PlatformFlag> =>
    request<PlatformFlag>(`/api/v1/admin/flags/${encodeURIComponent(key)}`, {
      method: 'PATCH',
      body,
    }),

  deleteFlag: (key: string): Promise<void> =>
    request<void>(`/api/v1/admin/flags/${encodeURIComponent(key)}`, { method: 'DELETE' }),

  setOverride: (flagKey: string, tenantId: string, enabled: boolean): Promise<FlagOverride> =>
    request<FlagOverride>('/api/v1/admin/flags/overrides', {
      method: 'POST',
      body: { flag_key: flagKey, tenant_id: tenantId, enabled },
    }),

  evaluateFlag: (key: string): Promise<{ key: string; enabled: boolean }> =>
    request<{ key: string; enabled: boolean }>(
      `/api/v1/admin/flags/evaluate${queryString({ key })}`,
    ),

  audit: (limit?: number, offset?: number): Promise<AuditEntry[]> =>
    request<AuditEntry[]>(`/api/v1/admin/audit${queryString({ limit, offset })}`),
};

/** Public service-status endpoints (Phase 9). */
export const statusApi = {
  get: (): Promise<StatusReport> => request<StatusReport>('/api/v1/status'),
  incidents: (): Promise<Incident[]> => request<Incident[]>('/api/v1/status/incidents'),
};

/** Connector registry (Phase 10). Secrets travel only in create/update bodies. */
export const connectorApi = {
  catalog: (): Promise<ConnectorInfo[]> =>
    request<ConnectorInfo[]>('/api/v1/connectors/catalog'),

  list: (): Promise<ConnectorInstance[]> =>
    request<ConnectorInstance[]>('/api/v1/connectors'),

  create: (body: ConnectorCreateRequest): Promise<ConnectorInstance> =>
    request<ConnectorInstance>('/api/v1/connectors', { method: 'POST', body }),

  update: (id: string, body: ConnectorUpdateRequest): Promise<ConnectorInstance> =>
    request<ConnectorInstance>(`/api/v1/connectors/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body,
    }),

  remove: (id: string): Promise<void> =>
    request<void>(`/api/v1/connectors/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  test: (id: string): Promise<ConnectorTestResult> =>
    request<ConnectorTestResult>(`/api/v1/connectors/${encodeURIComponent(id)}/test`, {
      method: 'POST',
    }),

  invoke: (id: string, body: ConnectorInvokeRequest): Promise<ConnectorInvokeResult> =>
    request<ConnectorInvokeResult>(`/api/v1/connectors/${encodeURIComponent(id)}/invoke`, {
      method: 'POST',
      body,
    }),
};

/** Version control and environments (Phase 11). */
export const versionApi = {
  branches: (projectId: string): Promise<Branch[]> =>
    request<Branch[]>(`/api/v1/versioning/branches${queryString({ project_id: projectId })}`),

  createBranch: (
    projectId: string,
    name: string,
    fromRevisionId?: string,
  ): Promise<Branch> =>
    request<Branch>('/api/v1/versioning/branches', {
      method: 'POST',
      body: {
        project_id: projectId,
        name,
        ...(fromRevisionId ? { from_revision_id: fromRevisionId } : {}),
      },
    }),

  history: (projectId: string, branch: string): Promise<Revision[]> =>
    request<Revision[]>(
      `/api/v1/versioning/history${queryString({ project_id: projectId, branch })}`,
    ),

  revision: (id: string): Promise<Revision> =>
    request<Revision>(`/api/v1/versioning/revisions/${encodeURIComponent(id)}`),

  diff: (projectId: string, from: string, to: string): Promise<DiffResult> =>
    request<DiffResult>(
      `/api/v1/versioning/diff${queryString({ project_id: projectId, from, to })}`,
    ),

  commit: (
    projectId: string,
    branch: string,
    message: string,
    data: unknown,
  ): Promise<Revision> =>
    request<Revision>('/api/v1/versioning/commits', {
      method: 'POST',
      body: { project_id: projectId, branch, message, data },
    }),

  merge: (
    targetBranch: string,
    projectId: string,
    sourceBranch: string,
    strategy: string,
  ): Promise<Revision> =>
    request<Revision>(
      `/api/v1/versioning/branches/${encodeURIComponent(targetBranch)}/merge`,
      { method: 'POST', body: { project_id: projectId, source_branch: sourceBranch, strategy } },
    ),

  restore: (projectId: string, revisionId: string): Promise<Revision> =>
    request<Revision>('/api/v1/versioning/restore', {
      method: 'POST',
      body: { project_id: projectId, revision_id: revisionId },
    }),

  environments: (projectId: string): Promise<Environment[]> =>
    request<Environment[]>(
      `/api/v1/versioning/environments${queryString({ project_id: projectId })}`,
    ),

  createEnvironment: (
    projectId: string,
    name: string,
    config: Record<string, unknown> = {},
  ): Promise<Environment> =>
    request<Environment>('/api/v1/versioning/environments', {
      method: 'POST',
      body: { project_id: projectId, name, config },
    }),

  updateEnvironment: (
    name: string,
    projectId: string,
    config: Record<string, unknown>,
  ): Promise<Environment> =>
    request<Environment>(
      `/api/v1/versioning/environments/${encodeURIComponent(name)}`,
      { method: 'PATCH', body: { project_id: projectId, config } },
    ),

  promote: (name: string, projectId: string, revisionId: string): Promise<Environment> =>
    request<Environment>(
      `/api/v1/versioning/environments/${encodeURIComponent(name)}/promote`,
      { method: 'POST', body: { project_id: projectId, revision_id: revisionId } },
    ),

  rollback: (name: string, projectId: string, revisionId: string): Promise<Environment> =>
    request<Environment>(
      `/api/v1/versioning/environments/${encodeURIComponent(name)}/rollback`,
      { method: 'POST', body: { project_id: projectId, revision_id: revisionId } },
    ),

  secrets: (name: string, projectId: string): Promise<SecretKey[]> =>
    request<SecretKey[]>(
      `/api/v1/versioning/environments/${encodeURIComponent(name)}/secrets${queryString({ project_id: projectId })}`,
    ),

  setSecret: (
    name: string,
    projectId: string,
    key: string,
    value: string,
  ): Promise<SecretKey> =>
    request<SecretKey>(
      `/api/v1/versioning/environments/${encodeURIComponent(name)}/secrets`,
      { method: 'POST', body: { project_id: projectId, key, value } },
    ),

  deleteSecret: (name: string, projectId: string, key: string): Promise<void> =>
    request<void>(
      `/api/v1/versioning/environments/${encodeURIComponent(name)}/secrets/${encodeURIComponent(key)}${queryString({ project_id: projectId })}`,
      { method: 'DELETE' },
    ),
};