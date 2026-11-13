/**
 * Pure helpers for the Deploy screen (Phase 15).
 *
 * The server owns the truth: a successful `git push` is `pushed`, and only a
 * matching, successful GitHub Actions run promotes it to `ci_succeeded`.
 * Keeping every label, tone and step transition in one pure module means the
 * UI cannot accidentally congratulate the user for a deployment that has not
 * happened yet, and the rules are directly unit-testable.
 */

import type { DeploymentResponse, DeploymentStatus } from '../platform/types';

export type DeployStepKey = 'generate' | 'commit' | 'push' | 'ci';
export type DeployStepState = 'pending' | 'active' | 'done' | 'failed';

export interface DeployStep {
  key: DeployStepKey;
  label: string;
  state: DeployStepState;
  detail: string;
}

export type StatusTone = 'neutral' | 'info' | 'warning' | 'success' | 'danger';

export const DEPLOY_STEP_LABELS: Record<DeployStepKey, string> = {
  generate: 'Generate',
  commit: 'Commit',
  push: 'Push',
  ci: 'CI / deploy',
};

/** Human labels. `pushed` deliberately says the deploy is still pending. */
export const DEPLOY_STATUS_LABELS: Record<DeploymentStatus, string> = {
  generating: 'Generating',
  committing: 'Committing',
  pushing: 'Pushing',
  pushed: 'Pushed — CI pending',
  ci_queued: 'CI queued',
  ci_running: 'CI running',
  ci_succeeded: 'Deployed',
  ci_failed: 'CI failed',
  failed: 'Failed',
};

export const STATUS_TONE_CLASSES: Record<StatusTone, string> = {
  neutral: 'bg-gray-100 text-gray-600 dark:bg-slate-700 dark:text-gray-300',
  info: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
  warning: 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300',
  success: 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300',
  danger: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
};

export function deploymentLabel(status: DeploymentStatus): string {
  return DEPLOY_STATUS_LABELS[status] ?? status;
}

export function deploymentTone(status: DeploymentStatus): StatusTone {
  switch (status) {
    case 'ci_succeeded':
      return 'success';
    case 'ci_failed':
    case 'failed':
      return 'danger';
    case 'ci_queued':
    case 'ci_running':
      return 'info';
    case 'pushed':
      return 'warning';
    default:
      return 'neutral';
  }
}

/**
 * Only a succeeded CI run is a deployment. The server also sends a derived
 * `deployed` flag; requiring both keeps a bad payload from claiming success.
 */
export function isDeployed(
  deployment: Pick<DeploymentResponse, 'status' | 'deployed'>,
): boolean {
  return deployment.status === 'ci_succeeded' && deployment.deployed === true;
}

/** Non-terminal deployments can still change, so the UI offers a refresh. */
export function isPending(status: DeploymentStatus): boolean {
  return !['ci_succeeded', 'ci_failed', 'failed'].includes(status);
}

/**
 * Map a reported status onto the four progress steps.
 *
 * `null` means "no result yet". A single HTTP request performs generate,
 * commit and push on the server, so the UI never invents intermediate
 * completions while that request is in flight: it shows an in-flight banner
 * instead and only reports steps once the server has answered.
 */
export function deploySteps(
  status: DeploymentStatus | null,
  errorMessage?: string | null,
): DeployStep[] {
  const step = (key: DeployStepKey, state: DeployStepState, detail = ''): DeployStep => ({
    key,
    label: DEPLOY_STEP_LABELS[key],
    state,
    detail,
  });
  const keys: DeployStepKey[] = ['generate', 'commit', 'push', 'ci'];
  const all = (state: DeployStepState): DeployStep[] =>
    keys.map((key) => step(key, state));

  if (status === null) return all('pending');

  switch (status) {
    case 'generating':
      return [step('generate', 'active'), step('commit', 'pending'), step('push', 'pending'), step('ci', 'pending')];
    case 'committing':
      return [step('generate', 'done'), step('commit', 'active'), step('push', 'pending'), step('ci', 'pending')];
    case 'pushing':
      return [step('generate', 'done'), step('commit', 'done'), step('push', 'active'), step('ci', 'pending')];
    case 'pushed':
      return [
        step('generate', 'done'),
        step('commit', 'done'),
        step('push', 'done', 'Code is on GitHub'),
        step('ci', 'pending', 'No workflow run has been reported yet'),
      ];
    case 'ci_queued':
      return [
        step('generate', 'done'),
        step('commit', 'done'),
        step('push', 'done', 'Code is on GitHub'),
        step('ci', 'active', 'Workflow run queued'),
      ];
    case 'ci_running':
      return [
        step('generate', 'done'),
        step('commit', 'done'),
        step('push', 'done', 'Code is on GitHub'),
        step('ci', 'active', 'Building and publishing images'),
      ];
    case 'ci_succeeded':
      return [
        step('generate', 'done'),
        step('commit', 'done'),
        step('push', 'done', 'Code is on GitHub'),
        step('ci', 'done', 'CI passed — the images were published'),
      ];
    case 'ci_failed':
      return [
        step('generate', 'done'),
        step('commit', 'done'),
        step('push', 'done', 'Code is on GitHub'),
        step('ci', 'failed', errorMessage || 'The workflow run failed'),
      ];
    case 'failed':
      // The server does not report which pipeline stage failed. Say so
      // rather than pointing at a stage we cannot know.
      return [
        step('generate', 'failed', errorMessage || 'The deploy pipeline failed'),
        step('commit', 'pending', 'Stage not reported'),
        step('push', 'pending', 'Stage not reported'),
        step('ci', 'pending', 'Not reached'),
      ];
  }
}

/** The server's `IDE_PUBLIC_API_BASE` fallback, shown as a starting value. */
export const DEFAULT_DEPLOY_API_BASE = 'http://localhost:8081';

// Mirrors `nocode_deploy::valid_api_base`. Client-side validation is an
// affordance only; the server validates again before generating any file.
const FORBIDDEN_API_BASE_CHARS = new Set([
  '"', "'", '`', '\\', '$', '{', '}', '<', '>', '|', ';', '&', '@', '#', '?',
  '^', '!', '(', ')',
]);

/**
 * A public API base URL must be a plain absolute http(s) URL: no embedded
 * credentials, no whitespace and no template/YAML metacharacters, because it
 * is baked into generated files at build time (15.4).
 */
export function validDeployApiBase(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > 300) return false;
  const rest = trimmed.startsWith('https://')
    ? trimmed.slice('https://'.length)
    : trimmed.startsWith('http://')
      ? trimmed.slice('http://'.length)
      : null;
  if (rest === null) return false;
  for (const char of rest) {
    if (char <= '\x1f' || /\s/.test(char) || FORBIDDEN_API_BASE_CHARS.has(char)) {
      return false;
    }
  }
  const authority = rest.split('/')[0];
  return authority.length > 0 && !authority.startsWith(':');
}

/** Short commit hash for the history table. */
export function shortSha(sha: string): string {
  return sha ? sha.slice(0, 8) : '—';
}

/** Readable timestamp; an unparseable value is shown verbatim. */
export function formatDeployTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
