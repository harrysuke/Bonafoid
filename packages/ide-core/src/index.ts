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
  PlatformPlugin,
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
  DeploymentStatus,
  GitHubConfigResponse,
  GitHubConfigRequest,
  RegistryConfigResponse,
  DockerHubConfigRequest,
  DeploymentResponse,
  Tenant,
  TenantMember,
  Plan,
  PlanCreateRequest,
  Subscription,
  Invoice,
  Entitlement,
  UsageMeter,
  BillingSession,
  CancelResult,
  MonthlyRevenue,
  AdminAnalytics,
  AdminUsage,
  AdminUser,
  AdminUserUpdate,
  ImpersonationResult,
  PlatformFlag,
  FlagCreateRequest,
  FlagUpdateRequest,
  FlagOverride,
  AuditEntry,
  ConnectorInfo,
  ConnectorInstance,
  ConnectorCreateRequest,
  ConnectorUpdateRequest,
  ConnectorInvokeRequest,
  ConnectorInvokeResult,
  ConnectorTestResult,
  Revision,
  Branch,
  Environment,
  DiffLine,
  DiffResult,
  SecretKey,
  StatusReport,
  Incident,
} from './platform/types';
export type {
  AgentMode,
  AgentRunStatus,
  AgentUsage,
  AgentValidationIssue,
  AgentDomain,
  AgentConflict,
  AgentEvent,
  AgentChatRequest,
  AgentChatHandle,
  AgentProjectLockedBody,
  AgentConversation,
  AgentConversationDetail,
  AgentConversationMessage,
  AgentProvider,
  AgentProviderList,
  AgentPlatform,
  AgentKeyDraft,
  AgentKeyPatch,
  AgentProbeResult,
} from './types/agent';
export type { AgentCapabilities, AgentSettings, AgentSettingsInput } from './platform/types';

export { useProjectStore, type FieldPatch, type RelationPatch } from './stores/projectStore';
export { useUIStore } from './stores/uiStore';
export { usePluginStore, type PluginState } from './stores/pluginStore';
export { useDeployStore, deploySupported, type DeployState } from './stores/deployStore';
export { useTenantStore, tenantSupported, type TenantState } from './stores/tenantStore';
export { useBillingStore, billingSupported, type BillingState } from './stores/billingStore';
export { useAdminStore, adminSupported, type AdminState } from './stores/adminStore';
export { useConnectorStore, connectorSupported, type ConnectorState } from './stores/connectorStore';
export { useVersionStore, versionSupported, type VersionState } from './stores/versionStore';
export {
  useAgentStore,
  agentSupported,
  hasAgent,
  resetAgentStore,
  type AgentState,
  type AgentEntry,
} from './stores/agentStore';

export { confirmLeave, useUnsavedChangesGuard } from './hooks/useUnsavedChangesGuard';
export {
  ApiRequestError,
  describeError,
  notifyError,
  notifyInfo,
  notifySuccess,
} from './utils/errors';

export * from './pages';
export * from './types/project';
export * from './utils/helpers';
export * from './utils/dataModel';
export * from './utils/databaseModel';
export * from './utils/formModel';
export * from './utils/listModel';
export * from './utils/pageModel';
export * from './utils/workflowModel';
export * from './utils/deploy';
export * from './utils/saas';
export { createSseDecoder, parseSseFrame, type SseDecoder } from './utils/sse';

export { Button, Input, Select, Card, CardHeader, CardContent, CardFooter } from './components/ui';
