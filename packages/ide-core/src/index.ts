/**
 * Public surface of the shared IDE core.
 *
 * Host applications (the Tauri desktop shell and the browser web app) import
 * the router-level `IdeApp`, install a `PlatformApi` via `<PlatformProvider>`,
 * and re-use the stores, types and utilities exported here.
 */

export { default as IdeApp, IdeApp as App } from './App';

export {
  PlatformProvider,
  usePlatform,
  type PlatformProviderProps,
} from './platform/PlatformContext';
export { configurePlatform, getPlatform, hasPlatform } from './platform/registry';
export type {
  PlatformApi,
  PlatformProject,
  PlatformUser,
  BpmnValidationReport,
  BpmnValidationIssue,
  PreviewBuild,
  PreviewBuildState,
  PreviewBuildTarget,
  PreviewRun,
  PreviewCapabilities,
  WorkflowRunSummary,
  WorkflowRunStep,
  WorkflowOpenTask,
  WorkflowRunEvent,
} from './platform/types';

export { useProjectStore, type FieldPatch, type RelationPatch } from './stores/projectStore';
export { useUIStore } from './stores/uiStore';

export * from './pages';
export * from './types/project';
export * from './utils/helpers';
export * from './utils/dataModel';
export * from './utils/formModel';
export * from './utils/listModel';
export * from './utils/pageModel';
export * from './utils/workflowModel';

export { Button, Input, Select, Card, CardHeader, CardContent, CardFooter } from './components/ui';
