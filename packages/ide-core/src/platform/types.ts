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
}
