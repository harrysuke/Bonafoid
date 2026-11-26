/**
 * Platform abstraction for the shared IDE.
 *
 * `ide-core` must never import `@tauri-apps/*` directly. Instead each host
 * application (desktop Tauri shell, browser web app) injects a concrete
 * `PlatformApi` implementation through `<PlatformProvider>`.
 *
 * All methods that touch the host are async so a future browser/server
 * implementation can perform network calls without an API change.
 */

import type { GenerationReport } from '../types/project';
import type {
  AgentChatHandle,
  AgentChatRequest,
  AgentConversation,
  AgentConversationDetail,
  AgentEvent,
  AgentKeyDraft,
  AgentKeyPatch,
  AgentPlatform,
  AgentProbeResult,
  AgentProvider,
  AgentProviderList,
} from '../types/agent';

/**
 * What the current host can do with the assistant.
 *
 * A probe, not a boolean: local mode implements a Tauri command rather than
 * `agentChat`, so `typeof agentChat === 'function'` would hide a working
 * feature. The drawer renders when `local || remote`, and Build & Preview only
 * when `preview` — the desktop host has no preview service.
 */
export interface AgentCapabilities {
  /** Desktop with a configured provider key. */
  local: boolean;
  /** Web, or a desktop connected to a server. */
  remote: boolean;
  /**
   * Platform ids this host can drive, for the settings screen.
   *
   * Platform ids, not wire formats: `groq` and `google` share the `openai`
   * format but have their own endpoint, default model and key rule.
   */
  providers: string[];
  /**
   * Display and key metadata for `providers`, when the host can supply it.
   *
   * Optional and additive: a host that predates it still lists `providers`,
   * and the settings form falls back to the raw id and to "the platform
   * decides" for the key hint rather than duplicating the catalog here.
   * Snake-case like the rest of this wire shape (`has_key`, `base_url`).
   */
  provider_details?: AgentPlatformOption[];
  /** Whether this host can build and preview a generated app. */
  preview: boolean;
}

/** One entry in `AgentCapabilities.provider_details`. */
export interface AgentPlatformOption {
  platform: string;
  label: string;
  /** True when the platform serves requests with no credential. */
  keyless: boolean;
}

/**
 * The assistant's provider settings, as the settings panel sees them.
 *
 * The key is never part of this shape. `has_key` is the only thing the UI may
 * know about it, because a host that returned the key would put it in a React
 * prop, a devtools panel and a crash report (plan §15.4.1).
 */
export interface AgentSettings {
  provider: string | null;
  model: string | null;
  base_url: string | null;
  /** Whether a key is stored. The key itself never leaves the host. */
  has_key: boolean;
}

/** A settings change. Omitting `api_key` keeps the stored key. */
export interface AgentSettingsInput {
  provider: string;
  model?: string | null;
  base_url?: string | null;
  api_key?: string | null;
}

/** One validation finding returned by the BPMN engine. */
export interface BpmnValidationIssue {
  severity: string;
  node_id?: string;
  message: string;
}

/** Outcome of validating a workflow definition. */
export interface BpmnValidationReport {
  valid: boolean;
  issues: BpmnValidationIssue[];
}

/**
 * A project as listed by a host that stores projects server-side.
 *
 * Desktop hosts keep working copy on disk and omit the optional project
 * methods below; the web host implements them so the IDE can show a project
 * picker instead of file dialogs.
 */
export interface PlatformProject {
  id: string;
  name: string;
  description?: string | null;
  revision?: number;
  updatedAt?: string;
}

/** Lifecycle of a hosted preview build (mirrors the server's `BuildState`). */
export type PreviewBuildState =
  | 'queued'
  | 'generating'
  | 'building_backend'
  | 'building_frontend'
  | 'ready'
  | 'cancelled'
  | 'failed';

/** Which artifacts a preview build should produce. */
export type PreviewBuildTarget = 'all' | 'backend' | 'frontend';

/** One build as the IDE sees it. */
export interface PreviewBuild {
  id: string;
  project_id: string;
  state: PreviewBuildState;
  target: PreviewBuildTarget;
  error: string | null;
  workspace: string;
  logs: string[];
  created_at: string;
  updated_at: string;
}

/** A running preview backend. */
export interface PreviewRun {
  build_id: string;
  port: number;
  pid: number | null;
  status: 'starting' | 'running' | 'stopped' | 'exited' | 'failed';
  requests: number;
}

/** Host toolchain availability for previews. */
export interface PreviewCapabilities {
  cargo: boolean;
  trunk: boolean;
  root: string;
  max_parallel: number;
  timeout_secs: number;
}

/** One node visited during a workflow run. */
export interface WorkflowRunStep {
  node_id: string;
  node_name: string;
  node_type: string;
  outcome: string;
}

/** A user task a run created and has not completed. */
export interface WorkflowOpenTask {
  id: string;
  node_name: string;
  status: string;
}

/** One engine history entry. */
export interface WorkflowRunEvent {
  event_type: string;
  node_name?: string;
  detail?: string;
}

/** Result of executing a workflow with the token engine. */
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

/** The signed-in account, when the host has one. */
export interface PlatformUser {
  id: string;
  email: string;
}

/**
 * An installed plugin.
 *
 * `permissions` are the wire names from the manifest ("read_forms"), not
 * the Rust variant names.
 */
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

/* ------------------------------------------------------------------ *
 * Deploy surface (Phase 15)
 * ------------------------------------------------------------------ */

/**
 * Lifecycle of a deployment. Mirrors the server's `DeploymentStatus`,
 * serialized snake_case on the wire. A deploy is a long pipeline: a successful
 * push is `pushed`, never `ci_succeeded`.
 */
export type DeploymentStatus =
  | 'generating'
  | 'committing'
  | 'pushing'
  | 'pushed'
  | 'ci_queued'
  | 'ci_running'
  | 'ci_succeeded'
  | 'ci_failed'
  | 'failed';

/** Redacted GitHub integration config. The PAT is never returned. */
export interface GitHubConfigResponse {
  configured: boolean;
  owner: string;
  repo: string;
  branch: string;
  verified: boolean;
}

/** A GitHub PAT submission. The token travels only in this request body. */
export interface GitHubConfigRequest {
  owner: string;
  repo: string;
  branch: string;
  token: string;
}

/** Redacted Docker Hub / registry config. The token is never returned. */
export interface RegistryConfigResponse {
  configured: boolean;
  namespace: string;
  registry: string;
  backend_image: string;
  frontend_image: string;
  verified: boolean;
}

/**
 * A Docker Hub configuration submission. Omit `token` to keep the already
 * sealed credential (an update that only changes the namespace, say).
 */
export interface DockerHubConfigRequest {
  namespace: string;
  registry: string;
  token?: string;
}

/** One deployment, as both the server and the desktop report it identically. */
export interface DeploymentResponse {
  id: string;
  project_id: string;
  commit_sha: string;
  status: DeploymentStatus;
  /** True ONLY when CI has succeeded. A mere push is not deployed. */
  deployed: boolean;
  ci_state: 'queued' | 'running' | 'succeeded' | 'failed' | 'not_started';
  github_run_id: number | null;
  github_run_url: string | null;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

/* ------------------------------------------------------------------ *
 * SaaS surfaces (Phases 7-11)
 * ------------------------------------------------------------------ */

/** A workspace the signed-in user belongs to. */
export interface Tenant {
  id: string;
  name: string;
  slug: string;
  plan_id: string | null;
  status: string;
  /** Membership role, present on `GET /tenants` and absent on admin listings. */
  role?: string | null;
  created_at: string;
  updated_at: string;
}

/** A tenant membership row. Never carries credentials. */
export interface TenantMember {
  tenant_id: string;
  user_id: string;
  email: string;
  role: string;
  created_at: string;
}

/** A sellable billing plan. Money is integer minor units (cents). */
export interface Plan {
  id: string;
  name: string;
  description: string | null;
  currency: string;
  interval: string;
  amount_cents: number;
  is_active: boolean;
  external_id: string | null;
  created_at: string;
  updated_at: string;
}

/** Body of `POST /billing/plans`. */
export interface PlanCreateRequest {
  name: string;
  description?: string;
  currency: string;
  interval: string;
  amount_cents: number;
}

/** The active subscription of the current tenant, when it has one. */
export interface Subscription {
  id: string;
  tenant_id: string;
  plan_id: string;
  price_id: string;
  status: string;
  seat_count: number;
  period_start: string;
  period_end: string;
  cancel_at_period_end: boolean;
  external_customer_id: string | null;
  created_at: string;
  updated_at: string;
}

/** One invoice issued to the current tenant. */
export interface Invoice {
  id: string;
  tenant_id: string;
  subscription_id: string;
  amount_cents: number;
  currency: string;
  status: string;
  issued_at: string;
  paid_at: string | null;
  period_start: string | null;
  period_end: string | null;
  description: string | null;
}

/** A feature grant. A missing key fails closed. */
export interface Entitlement {
  feature_key: string;
  enabled: boolean;
  limit_value: number | null;
}

/** One usage counter for the current period. */
export interface UsageMeter {
  tenant_id: string;
  meter_key: string;
  quantity: number;
  period: string;
  updated_at: string;
}

/** Provider checkout/portal session: only its id and redirect URL are public. */
export interface BillingSession {
  id: string;
  url: string;
}

/** Outcome of cancelling a subscription. */
export interface CancelResult {
  subscription_id: string;
  cancel_at_period_end: boolean;
}

/** One month of platform revenue. */
export interface MonthlyRevenue {
  month: string;
  revenue_cents: number;
}

/** Platform-wide billing analytics (super-admin only). */
export interface AdminAnalytics {
  mrr_cents: number;
  arr_cents: number;
  churn_rate: number;
  revenue_by_plan: Record<string, number>;
  monthly_series: MonthlyRevenue[];
  active_subscriptions: number;
  churned_subscriptions: number;
}

/** Platform-wide object counts (super-admin only). */
export interface AdminUsage {
  projects: number;
  integrations: number;
  deployments: number;
  usage_events: number;
  tenant_members: number;
  audit_log: number;
}

/** A user as listed by the platform admin panel. */
export interface AdminUser {
  id: string;
  email: string;
  is_active: boolean;
  is_verified: boolean;
  platform_role: string | null;
  tenant_id: string | null;
  last_login_at: string | null;
  created_at: string;
}

/** Partial update accepted by `PATCH /admin/users/:id`. */
export interface AdminUserUpdate {
  is_active?: boolean;
  is_verified?: boolean;
  /** Only `super_admin` is a valid role; `null` clears it. Omit to leave it. */
  platform_role?: string | null;
}

/** Audit-only impersonation record. No session token is swapped. */
export interface ImpersonationResult {
  impersonation_id: string;
  note: string;
}

/** A platform-wide feature flag. Not the per-project feature-flags model. */
export interface PlatformFlag {
  key: string;
  description: string | null;
  enabled: boolean;
  rollout_percentage: number;
  kill_switch: boolean;
  created_at: string;
  updated_at: string;
}

/** Body of `POST /admin/flags`. */
export interface FlagCreateRequest {
  key: string;
  description?: string;
  enabled?: boolean;
  rollout_percentage?: number;
  kill_switch?: boolean;
}

/** Partial update accepted by `PATCH /admin/flags/:key`. */
export interface FlagUpdateRequest {
  description?: string | null;
  enabled?: boolean;
  rollout_percentage?: number;
  kill_switch?: boolean;
}

/** Result of setting a tenant flag override. */
export interface FlagOverride {
  flag_key: string;
  tenant_id: string;
  enabled: boolean;
}

/** One immutable admin audit-log row. */
export interface AuditEntry {
  id: string;
  actor_user_id: string;
  actor_email: string | null;
  tenant_id: string | null;
  action: string;
  target_type: string;
  target_id: string | null;
  before_json: string | null;
  after_json: string | null;
  ip_address: string | null;
  user_agent: string | null;
  created_at: string;
}

/** A connector kind advertised by the catalog. */
export interface ConnectorInfo {
  kind: string;
  description: string;
}

/**
 * A tenant's connector instance. `config` is redacted by the server and
 * `secret_set` reports only whether a sealed credential exists — no secret
 * value is ever present.
 */
export interface ConnectorInstance {
  id: string;
  tenant_id: string;
  kind: string;
  name: string;
  config: Record<string, unknown>;
  enabled: boolean;
  secret_set: boolean;
  created_at: string;
  updated_at: string;
}

/** Body of `POST /connectors`. `secret` is write-only. */
export interface ConnectorCreateRequest {
  kind: string;
  name: string;
  config: Record<string, unknown>;
  secret?: string;
}

/** Partial update accepted by `PATCH /connectors/:id`. */
export interface ConnectorUpdateRequest {
  name?: string;
  config?: Record<string, unknown>;
  secret?: string;
}

/** One JSON-proxy request through an http_rest/webhook connector. */
export interface ConnectorInvokeRequest {
  method: string;
  path: string;
  body?: unknown;
  headers?: Record<string, string>;
}

/** Response of a connector invoke proxy call. */
export interface ConnectorInvokeResult {
  status: number;
  body: unknown;
}

/** Outcome of `POST /connectors/:id/test`. */
export interface ConnectorTestResult {
  ok: boolean;
  message: string;
  latency_ms: number;
}

/** An append-only project revision. `data` is the full project JSON value. */
export interface Revision {
  id: string;
  project_id: string;
  parent_id: string | null;
  branch: string;
  message: string;
  author_id: string;
  data: unknown;
  created_at: string;
}

/** A named pointer at a revision head. */
export interface Branch {
  name: string;
  head_revision_id: string;
  created_at: string;
}

/** A deployment environment. `config` is redacted; secrets are never returned. */
export interface Environment {
  name: string;
  config: Record<string, unknown>;
  revision_id: string | null;
  status: string;
  updated_at: string;
}

/** One line of a deterministic project-JSON diff. */
export interface DiffLine {
  kind: string;
  text: string;
}

/** Response of `GET /versioning/diff`. */
export interface DiffResult {
  from: string;
  to: string;
  lines: DiffLine[];
}

/** A sealed environment secret, listed by key only. */
export interface SecretKey {
  key: string;
}

/** Public component-health report. No tenant data is exposed. */
export interface StatusReport {
  status: string;
  components: {
    database: string;
    plugin_host: string;
    preview_toolchain: string;
    storage: string;
    queue_depth: number;
  };
}

/** Public, allowlisted status incident. */
export interface Incident {
  status: string;
  component: string | null;
  title: string;
  opened_at: string;
  resolved_at: string | null;
}

export interface PlatformApi {
  /** Which host is running the IDE. */
  readonly kind: 'tauri' | 'web';

  /** True for the desktop shell. Used to gate desktop-only affordances. */
  readonly isDesktop: boolean;

  /**
   * When true the store seeds a sample project the first time it starts
   * with no project loaded. Desktop seeds; the web app starts empty.
   */
  readonly autoSeedSample: boolean;

  /** Current in-memory project JSON, or null when nothing is loaded. */
  getCurrentProject(): Promise<string | null>;

  /** Filesystem path of the loaded project, or null when unsaved. */
  getProjectPath(): Promise<string | null>;

  /** Create a new empty project and return its JSON. */
  newProject(name: string): Promise<string>;

  /** Create the bundled sample project and return its JSON. */
  seedSampleProject(): Promise<string>;

  /** Switch the project database driver; returns updated project JSON. */
  setDbDriver(driver: string): Promise<string>;

  /** Switch the frontend framework; returns updated project JSON. */
  setFrontendFramework(framework: string): Promise<string>;

  /** Replace the in-memory project with the supplied JSON. */
  updateProject(projectJson: string): Promise<void>;

  /** Load a project from a path and return its JSON. */
  loadProject(path: string): Promise<string>;

  /** Persist the in-memory project to a path. */
  saveProject(path: string): Promise<void>;

  /** Generate code for the current project into `outputDir`. */
  generateCode(outputDir: string): Promise<GenerationReport>;

  /** Prompt the user for a project file to open. */
  pickOpenPath(): Promise<string | null>;

  /** Prompt the user for a save destination. */
  pickSavePath(defaultName: string): Promise<string | null>;

  /** Prompt the user for a directory. */
  pickDirectory(): Promise<string | null>;

  /** Parse BPMN 2.0 XML into a workflow; returns workflow JSON. */
  bpmnImport(xml: string, name: string): Promise<string>;

  /** Serialize workflow JSON into BPMN 2.0 XML. */
  bpmnExport(workflowJson: string): Promise<string>;

  /** Structurally validate workflow JSON with the BPMN engine. */
  bpmnValidate(workflowJson: string): Promise<BpmnValidationReport>;

  /**
   * Projects stored by the host, when it has a project store (web/server).
   * Absent on file-based hosts, which use `pickOpenPath` instead.
   */
  listProjects?(): Promise<PlatformProject[]>;

  /** Open a project from `listProjects` by id; returns project JSON. */
  openProjectById?(id: string): Promise<string>;

  /** Create a project in the host store; returns project JSON. */
  createProject?(name: string): Promise<string>;

  /** Delete a project from the host store. */
  deleteProject?(id: string): Promise<void>;

  /**
   * Plugins the user has installed. Absent on hosts with no plugin store.
   */
  pluginList?(): Promise<PlatformPlugin[]>;

  /**
   * Install a plugin from its manifest and WebAssembly module.
   *
   * The module is uploaded as bytes; the manifest is the plugin's own
   * `plugin.json`, verbatim.
   */
  pluginInstall?(manifest: string, module: Uint8Array): Promise<PlatformPlugin>;

  /** Turn a plugin on or off. */
  pluginSetEnabled?(id: string, enabled: boolean): Promise<PlatformPlugin>;

  /** Uninstall a plugin. */
  pluginUninstall?(id: string): Promise<void>;

  /**
   * Start a hosted preview build. Absent when the host cannot build previews
   * (the desktop shell generates locally instead).
   */
  previewBuild?(
    projectId: string,
    target?: PreviewBuildTarget,
  ): Promise<PreviewBuild>;

  /** Poll a build for state + logs. */
  previewStatus?(buildId: string): Promise<PreviewBuild>;

  /** Cancel a build and delete its workspace. */
  previewCancel?(buildId: string): Promise<void>;

  /** Builds the host knows about (newest first). */
  previewBuilds?(): Promise<PreviewBuild[]>;

  /** Host toolchain probe, so the UI can explain a missing `trunk`. */
  previewCapabilities?(): Promise<PreviewCapabilities>;

  /** Start the generated backend of a ready build. */
  previewRun?(buildId: string): Promise<PreviewRun>;

  /** Stop a running preview backend. */
  previewStop?(buildId: string): Promise<void>;

  /** Public URL of a ready build's frontend (iframe source). */
  previewUrl?(buildId: string): string;

  /**
   * Execute a workflow with the token engine. `autoCompleteTasks` completes
   * user tasks so the instance runs to the end (a simulation, not real work).
   */
  workflowRun?(
    workflowJson: string,
    variables?: Record<string, unknown>,
    autoCompleteTasks?: boolean,
  ): Promise<WorkflowRunSummary>;

  /**
   * The signed-in account. Optional: the desktop host opens local projects
   * without an account. Used for the header identity and the logout action.
   */
  currentUser?(): PlatformUser | null;

  /** End the session. No-op where there is no session to end. */
  logout?(): Promise<void>;

  /**
   * Deployment integrations (Phase 15). Optional: only hosts wired to the
   * integration service expose them. The web server does; the desktop shell
   * can too, via its local deploy commands. The Deploy page gates itself on
   * `deployGithubGet` being a function, like the plugin panel does.
   */
  deployGithubGet?(): Promise<GitHubConfigResponse>;
  deployGithubSet?(request: GitHubConfigRequest): Promise<GitHubConfigResponse>;
  deployDockerhubGet?(): Promise<RegistryConfigResponse>;
  deployDockerhubSet?(request: DockerHubConfigRequest): Promise<RegistryConfigResponse>;
  deployProject?(projectId: string, apiBase?: string): Promise<DeploymentResponse>;
  deployList?(projectId: string): Promise<DeploymentResponse[]>;

  /*
   * SaaS surfaces (Phases 7-11). These are OPTIONAL: the desktop host has no
   * tenancy, billing, admin, connector or versioning service and omits them.
   * Every consuming store and page gates itself on the capability it needs
   * (usually `typeof platform.<method> === 'function'`) before calling through.
   */

  /** Tenants the signed-in user belongs to. */
  tenantList?(): Promise<Tenant[]>;
  /** Create a tenant and become its owner. */
  tenantCreate?(name: string, slug: string): Promise<Tenant>;
  /** Rename a tenant or change its status (`active` / `suspended`). */
  tenantUpdate?(id: string, name: string, status: string): Promise<Tenant>;
  /** Switch the session's active tenant; the response carries new cookies. */
  tenantSwitch?(id: string): Promise<{ tenant_id: string; csrf_token: string }>;
  /** Members of a tenant the caller administers. */
  tenantMembers?(id: string): Promise<TenantMember[]>;
  /** Add or update a membership. */
  tenantAddMember?(id: string, userId: string, role: string): Promise<TenantMember>;
  /** Remove a membership. The last owner is protected server-side. */
  tenantRemoveMember?(id: string, userId: string): Promise<void>;

  /** Active, sellable plans. */
  billingPlans?(): Promise<Plan[]>;
  /** Create a plan (platform super-admin). */
  billingCreatePlan?(body: PlanCreateRequest): Promise<Plan>;
  /** The current tenant's subscription, or null. */
  billingSubscription?(): Promise<Subscription | null>;
  /** Subscribe the current tenant to a plan. */
  billingSubscribe?(planId: string, seatCount?: number): Promise<Subscription>;
  /** Cancel at period end. */
  billingCancel?(): Promise<CancelResult>;
  /** Invoices issued to the current tenant. */
  billingInvoices?(): Promise<Invoice[]>;
  /** Feature grants of the current tenant. */
  billingEntitlements?(): Promise<Entitlement[]>;
  /** Usage meters of the current tenant. */
  billingUsage?(): Promise<UsageMeter[]>;
  /** Add to a usage meter for the current period. */
  billingRecordUsage?(meterKey: string, quantity: number): Promise<UsageMeter>;
  /** Start a provider checkout; redirect the browser to `url`. */
  billingCheckout?(planId: string, successUrl: string, cancelUrl: string): Promise<BillingSession>;
  /** Start a provider billing-portal session. */
  billingPortal?(returnUrl: string): Promise<BillingSession>;

  /** All tenants (platform super-admin). */
  adminTenants?(search?: string, limit?: number, offset?: number): Promise<Tenant[]>;
  /** Set a tenant's status (platform super-admin). */
  adminUpdateTenant?(id: string, status: string): Promise<Tenant>;
  /** All users (platform super-admin). */
  adminUsers?(search?: string, limit?: number, offset?: number): Promise<AdminUser[]>;
  /** Update a user's active/verified flags or platform role. */
  adminUpdateUser?(id: string, body: AdminUserUpdate): Promise<AdminUser>;
  /** Record an audit-only impersonation. No token is swapped. */
  adminImpersonate?(id: string, reason?: string): Promise<ImpersonationResult>;
  /** Platform revenue and churn analytics. */
  adminAnalytics?(): Promise<AdminAnalytics>;
  /** Platform object counts. */
  adminUsage?(): Promise<AdminUsage>;
  /** Platform-wide feature flags. */
  adminFlags?(): Promise<PlatformFlag[]>;
  /** Create a platform flag. */
  adminCreateFlag?(body: FlagCreateRequest): Promise<PlatformFlag>;
  /** Update a platform flag. */
  adminUpdateFlag?(key: string, body: FlagUpdateRequest): Promise<PlatformFlag>;
  /** Delete a platform flag. */
  adminDeleteFlag?(key: string): Promise<void>;
  /** Override a flag for one tenant. */
  adminSetFlagOverride?(
    flagKey: string,
    tenantId: string,
    enabled: boolean,
  ): Promise<FlagOverride>;
  /** Evaluate a flag for the current tenant. */
  adminEvaluateFlag?(key: string): Promise<{ key: string; enabled: boolean }>;
  /** Most recent admin audit entries. */
  adminAudit?(limit?: number, offset?: number): Promise<AuditEntry[]>;

  /** Public component-health report. */
  statusGet?(): Promise<StatusReport>;
  /** Public status incidents. */
  statusIncidents?(): Promise<Incident[]>;

  /** Supported connector kinds. */
  connectorCatalog?(): Promise<ConnectorInfo[]>;
  /** This tenant's redacted connector instances. */
  connectorList?(): Promise<ConnectorInstance[]>;
  /** Create a connector. The secret travels only in this request. */
  connectorCreate?(body: ConnectorCreateRequest): Promise<ConnectorInstance>;
  /** Update a connector; omit `secret` to keep the sealed credential. */
  connectorUpdate?(id: string, body: ConnectorUpdateRequest): Promise<ConnectorInstance>;
  /** Delete a connector. */
  connectorDelete?(id: string): Promise<void>;
  /** Probe a connector. */
  connectorTest?(id: string): Promise<ConnectorTestResult>;
  /** Proxy one JSON request through an http_rest/webhook connector. */
  connectorInvoke?(id: string, body: ConnectorInvokeRequest): Promise<ConnectorInvokeResult>;

  /**
   * Start an assistant run and stream its progress.
   *
   * Optional: the desktop host has no server-side agent, so the drawer hides
   * itself when this is absent. The promise resolves once the stream is open
   * (so an authentication or configuration failure arrives as a rejection
   * before any UI state is committed); events then arrive through `onEvent`.
   */
  agentChat?(
    projectId: string,
    request: AgentChatRequest,
    onEvent: (event: AgentEvent) => void,
  ): Promise<AgentChatHandle>;

  /**
   * What this host can do with the assistant.
   *
   * Optional so a host that predates it still type-checks; `hasAgent()` falls
   * back to probing `agentChat` when it is absent.
   */
  agentCapabilities?(): Promise<AgentCapabilities>;

  /**
   * The assistant's provider catalog.
   *
   * Optional: a host that owns a single provider (the desktop shell, whose key
   * lives in its own settings panel) omits it, and the drawer shows no picker.
   */
  agentProviders?(): Promise<AgentProviderList>;

  /**
   * Every provider the server supports, configured or not.
   *
   * Optional for the same reason as `agentProviders`: a host with one fixed
   * provider has nothing to offer here. The drawer uses it to show a provider
   * that has no key row yet, with the model ids it accepts.
   */
  agentPlatforms?(): Promise<AgentPlatform[]>;

  /**
   * Add a provider key on the host.
   *
   * Optional, and separate from `agentSettingsSave` on purpose: the desktop
   * shell owns exactly one provider and stores it in its own app-data file,
   * while a server deployment stores many rows in `agent_api_keys`. A host
   * implements the shape it actually has.
   */
  agentAddProvider?(draft: AgentKeyDraft): Promise<AgentProvider>;

  /** Change one provider row. Omitted fields are left alone; `''` clears. */
  agentUpdateProvider?(id: string, patch: AgentKeyPatch): Promise<AgentProvider>;

  /** Remove a provider row and its sealed credential. */
  agentRemoveProvider?(id: string): Promise<void>;

  /**
   * Spend one tiny completion to find out whether a stored key works.
   *
   * `ok: false` is a normal result, not a rejected promise.
   */
  agentTestProvider?(id: string): Promise<AgentProbeResult>;

  /** This project's conversations, newest first (the history menu). */
  agentConversations?(projectId: string): Promise<AgentConversation[]>;

  /** One conversation with its messages (the restore after a reload). */
  agentConversation?(projectId: string, conversationId: string): Promise<AgentConversationDetail>;

  /** Rename a conversation. */
  agentRenameConversation?(
    projectId: string,
    conversationId: string,
    title: string,
  ): Promise<AgentConversation>;

  /** Delete a conversation and its messages. */
  agentDeleteConversation?(projectId: string, conversationId: string): Promise<void>;

  /**
   * The assistant's provider settings.
   *
   * Only a host that owns its own provider has these — the desktop shell. The
   * web host's key is server-side, so it implements none of the four and the
   * settings panel stays hidden.
   */
  agentSettings?(): Promise<AgentSettings>;

  /** Save provider/model/base URL, and optionally a new key. */
  agentSettingsSave?(input: AgentSettingsInput): Promise<AgentSettings>;

  /** Forget the stored key. */
  agentKeyClear?(): Promise<AgentSettings>;

  /** Send one tiny completion to prove the key works. Returns the reply. */
  agentTestConnection?(): Promise<string>;

  /** Branches of a project. */
  versionBranches?(projectId: string): Promise<Branch[]>;
  /** Create a branch at a revision (or the current head). */
  versionCreateBranch?(projectId: string, name: string, fromRevisionId?: string): Promise<Branch>;
  /** Newest-first revision history of a branch. */
  versionHistory?(projectId: string, branch: string): Promise<Revision[]>;
  /** Line-based diff between two revisions. */
  versionDiff?(projectId: string, from: string, to: string): Promise<DiffResult>;
  /** Commit the supplied project JSON to a branch. */
  versionCommit?(
    projectId: string,
    branch: string,
    message: string,
    data: unknown,
  ): Promise<Revision>;
  /** Merge a source branch into a named target branch. */
  versionMerge?(
    targetBranch: string,
    projectId: string,
    sourceBranch: string,
    strategy: string,
  ): Promise<Revision>;
  /** Append a revision that restores an earlier snapshot. */
  versionRestore?(projectId: string, revisionId: string): Promise<Revision>;
  /** Environments of a project. */
  versionEnvironments?(projectId: string): Promise<Environment[]>;
  /** Create an environment. */
  versionCreateEnvironment?(
    projectId: string,
    name: string,
    config?: Record<string, unknown>,
  ): Promise<Environment>;
  /** Replace an environment's redacted config. */
  versionUpdateEnvironment?(
    name: string,
    projectId: string,
    config: Record<string, unknown>,
  ): Promise<Environment>;
  /** Point an environment at a revision and record a deployment. */
  versionPromote?(name: string, projectId: string, revisionId: string): Promise<Environment>;
  /** Roll an environment back to a revision. */
  versionRollback?(name: string, projectId: string, revisionId: string): Promise<Environment>;
  /** List an environment's secret keys (values never returned). */
  versionSecrets?(name: string, projectId: string): Promise<SecretKey[]>;
  /** Store a write-only environment secret. */
  versionSetSecret?(
    name: string,
    projectId: string,
    key: string,
    value: string,
  ): Promise<SecretKey>;
  /** Delete an environment secret by key. */
  versionDeleteSecret?(name: string, projectId: string, key: string): Promise<void>;
}
