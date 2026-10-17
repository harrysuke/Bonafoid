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

/** JSON API error carrying the server's stable code and message. */
export class ApiRequestError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = 'ApiRequestError';
    this.status = status;
    this.code = code;
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
  try {
    const payload = (await response.json()) as { error?: string; message?: string };
    if (payload.error) code = payload.error;
    if (payload.message) message = payload.message;
  } catch {
    // Non-JSON error body: keep the status text.
  }
  return new ApiRequestError(response.status, code, message);
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
};

export const serverApi = {
  health: (): Promise<{ status: string; service: string }> => request('/health'),
  ready: (): Promise<{ status: string; database: string }> => request('/ready'),
};