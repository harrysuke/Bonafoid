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
}
