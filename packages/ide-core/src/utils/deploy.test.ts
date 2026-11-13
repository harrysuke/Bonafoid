/**
 * Tests for the Deploy screen's status rules.
 *
 * These rules are the whole point of status honesty: a push is not a deploy.
 * A refactor that starts labelling `pushed` as success must fail here first.
 */

import { describe, expect, it } from 'vitest';
import type { DeploymentResponse, DeploymentStatus } from '../platform/types';
import {
  DEFAULT_DEPLOY_API_BASE,
  deploySteps,
  deploymentLabel,
  deploymentTone,
  formatDeployTime,
  isDeployed,
  isPending,
  shortSha,
  validDeployApiBase,
} from './deploy';

function deployment(overrides: Partial<DeploymentResponse> = {}): DeploymentResponse {
  return {
    id: 'd1',
    project_id: 'p1',
    commit_sha: '0123456789abcdef0123456789abcdef01234567',
    status: 'pushed',
    deployed: false,
    ci_state: 'not_started',
    github_run_id: null,
    github_run_url: null,
    error_message: null,
    created_at: '2026-11-13T00:00:00Z',
    updated_at: '2026-11-13T00:00:00Z',
    ...overrides,
  };
}

describe('deployment labels and tones', () => {
  it('never labels a push as deployed', () => {
    expect(deploymentLabel('pushed')).toBe('Pushed — CI pending');
    expect(deploymentTone('pushed')).toBe('warning');
    expect(deploymentLabel('ci_succeeded')).toBe('Deployed');
    expect(deploymentTone('ci_succeeded')).toBe('success');
  });

  it('labels every status distinctly', () => {
    const statuses: DeploymentStatus[] = [
      'generating', 'committing', 'pushing', 'pushed',
      'ci_queued', 'ci_running', 'ci_succeeded', 'ci_failed', 'failed',
    ];
    expect(new Set(statuses.map(deploymentLabel)).size).toBe(statuses.length);
    expect(deploymentTone('ci_failed')).toBe('danger');
    expect(deploymentTone('ci_running')).toBe('info');
  });
});

describe('isDeployed / isPending', () => {
  it('requires both a succeeded CI status and the server flag', () => {
    expect(isDeployed(deployment())).toBe(false);
    expect(isDeployed(deployment({ status: 'ci_succeeded' }))).toBe(false);
    expect(isDeployed(deployment({ deployed: true }))).toBe(false);
    expect(isDeployed(deployment({ status: 'ci_succeeded', deployed: true }))).toBe(true);
  });

  it('keeps pushed and CI-running deployments pending', () => {
    expect(isPending('pushed')).toBe(true);
    expect(isPending('ci_running')).toBe(true);
    expect(isPending('ci_succeeded')).toBe(false);
    expect(isPending('ci_failed')).toBe(false);
    expect(isPending('failed')).toBe(false);
  });
});

describe('deploySteps', () => {
  it('reports nothing as done before the server answers', () => {
    const steps = deploySteps(null);
    expect(steps.map((s) => s.state)).toEqual(['pending', 'pending', 'pending', 'pending']);
  });

  it('stops at CI pending after a push', () => {
    const steps = deploySteps('pushed');
    expect(steps.map((s) => s.state)).toEqual(['done', 'done', 'done', 'pending']);
    expect(steps[3].detail).toMatch(/no workflow run/i);
  });

  it('marks CI active while queued or running', () => {
    for (const status of ['ci_queued', 'ci_running'] as const) {
      expect(deploySteps(status)[3].state).toBe('active');
    }
  });

  it('completes all four steps only on CI success', () => {
    expect(deploySteps('ci_succeeded').every((s) => s.state === 'done')).toBe(true);
  });

  it('fails the CI step with the server error', () => {
    const steps = deploySteps('ci_failed', 'workflow exited 1');
    expect(steps[3].state).toBe('failed');
    expect(steps[3].detail).toBe('workflow exited 1');
  });

  it('does not pretend to know which pipeline stage failed', () => {
    const steps = deploySteps('failed', 'git push rejected');
    expect(steps[0].state).toBe('failed');
    expect(steps.slice(1).every((s) => s.state === 'pending')).toBe(true);
  });

  it('advances generate -> commit -> push in order', () => {
    expect(deploySteps('generating').map((s) => s.state)).toEqual(['active', 'pending', 'pending', 'pending']);
    expect(deploySteps('committing').map((s) => s.state)).toEqual(['done', 'active', 'pending', 'pending']);
    expect(deploySteps('pushing').map((s) => s.state)).toEqual(['done', 'done', 'active', 'pending']);
  });
});

describe('validDeployApiBase', () => {
  it('accepts plain absolute http(s) URLs including localhost', () => {
    expect(validDeployApiBase(DEFAULT_DEPLOY_API_BASE)).toBe(true);
    expect(validDeployApiBase('https://api.example.com')).toBe(true);
    expect(validDeployApiBase('https://api.example.com/v1/')).toBe(true);
  });

  it('rejects anything that could inject into a generated file', () => {
    for (const bad of [
      '',
      'api.example.com',
      'ftp://api.example.com',
      'javascript:alert(1)',
      'https://user:secret@api.example.com',
      'https://api.example.com/${INJECT}',
      'https://api.example.com #comment',
      'https://api.example.com?q=1',
      'https://api.example.com\nRUN evil',
      'https://',
      'https://:8081',
    ]) {
      expect(validDeployApiBase(bad), `should reject ${JSON.stringify(bad)}`).toBe(false);
    }
  });
});

describe('display helpers', () => {
  it('shortens a commit sha and tolerates an empty one', () => {
    expect(shortSha('0123456789abcdef')).toBe('01234567');
    expect(shortSha('')).toBe('—');
  });

  it('falls back to the raw timestamp when unparseable', () => {
    expect(formatDeployTime('not-a-date')).toBe('not-a-date');
    expect(formatDeployTime('2026-11-13T00:00:00Z')).not.toBe('');
  });
});
