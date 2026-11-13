import { useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Check,
  ExternalLink,
  Github,
  Globe,
  Package,
  RefreshCw,
  Rocket,
  Shield,
  X,
} from 'lucide-react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { deploySupported, useDeployStore } from '../stores/deployStore';
import { useProjectStore } from '../stores/projectStore';
import { useUIStore } from '../stores/uiStore';
import type { DeploymentResponse, DockerHubConfigRequest } from '../platform/types';
import {
  DEFAULT_DEPLOY_API_BASE,
  STATUS_TONE_CLASSES,
  deploySteps,
  deploymentLabel,
  deploymentTone,
  formatDeployTime,
  isDeployed,
  isPending,
  shortSha,
  validDeployApiBase,
} from '../utils/deploy';
import type { DeployStep, DeployStepState } from '../utils/deploy';

const STEP_STATE_STYLES: Record<DeployStepState, string> = {
  done: 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300',
  active: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
  failed: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
  pending: 'bg-gray-100 text-gray-500 dark:bg-slate-700 dark:text-gray-400',
};

function StepMarker({ state }: { state: DeployStepState }) {
  return (
    <span
      className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${STEP_STATE_STYLES[state]}`}
      aria-hidden="true"
    >
      {state === 'done' ? <Check className="h-4 w-4" /> :
       state === 'failed' ? <X className="h-4 w-4" /> :
       state === 'active' ? <RefreshCw className="h-3.5 w-3.5 animate-spin" /> : '·'}
    </span>
  );
}

function StatusBadge({ status }: { status: DeploymentResponse['status'] }) {
  return (
    <span className={`rounded px-2 py-0.5 text-xs font-medium ${STATUS_TONE_CLASSES[deploymentTone(status)]}`}>
      {deploymentLabel(status)}
    </span>
  );
}

/**
 * The Deploy screen (Phase 15).
 *
 * Two hard rules shape this page:
 *  - The GitHub PAT is write-only. It lives in a local input until the save
 *    request, is cleared afterwards, and is never rendered from a response.
 *  - A successful push is `pushed`, NOT deployed. The page says so explicitly
 *    and only shows "Deployed" when CI actually succeeded.
 */
export function DeployPanel() {
  const {
    github,
    registry,
    apiBase,
    history,
    current,
    busy,
    deploying,
    error,
    load,
    setApiBase,
    connectGithub,
    connectDockerhub,
    deploy,
    refresh,
  } = useDeployStore();
  const addNotification = useUIStore((state) => state.addNotification);
  const project = useProjectStore((state) => state.project);
  const projectPath = useProjectStore((state) => state.projectPath);

  const supported = deploySupported();
  const projectId = useMemo(() => {
    if (projectPath?.startsWith('api://projects/')) {
      return projectPath.slice('api://projects/'.length) || null;
    }
    // The desktop host keeps a filesystem path; its deploy command works on
    // whichever project is open and filters history by the model id.
    return project?.id ? String(project.id) : null;
  }, [projectPath, project]);

  const [ghOwner, setGhOwner] = useState('');
  const [ghRepo, setGhRepo] = useState('');
  const [ghBranch, setGhBranch] = useState('main');
  const [ghToken, setGhToken] = useState('');
  const [dhNamespace, setDhNamespace] = useState('');
  const [dhRegistry, setDhRegistry] = useState('docker.io');
  const [dhToken, setDhToken] = useState('');

  useEffect(() => {
    if (supported) void load();
  }, [load, supported]);

  useEffect(() => {
    if (supported && projectId) void refresh(projectId);
  }, [projectId, refresh, supported]);

  // Seed the non-secret fields from the redacted GET responses. Secrets are
  // never returned, so the PAT inputs always start empty.
  useEffect(() => {
    if (!github) return;
    setGhOwner(github.owner);
    setGhRepo(github.repo);
    setGhBranch(github.branch || 'main');
  }, [github]);
  useEffect(() => {
    if (!registry) return;
    setDhNamespace(registry.namespace);
    setDhRegistry(registry.registry || 'docker.io');
  }, [registry]);

  const apiBaseValid = validDeployApiBase(apiBase);
  const githubReady = Boolean(github?.configured && github.verified);
  const registryReady = Boolean(registry?.configured && registry.verified);
  const ready =
    supported && Boolean(projectId) && apiBaseValid && githubReady && registryReady && !deploying;

  const saveGithub = async () => {
    const ok = await connectGithub({
      owner: ghOwner.trim(),
      repo: ghRepo.trim(),
      branch: ghBranch.trim() || 'main',
      token: ghToken,
    });
    // Write-only: drop the PAT from memory as soon as the request settles.
    setGhToken('');
    if (ok) {
      addNotification({ type: 'success', title: 'GitHub connected', message: `${ghOwner}/${ghRepo}` });
    }
  };

  const saveDockerhub = async () => {
    const request: DockerHubConfigRequest = {
      namespace: dhNamespace.trim(),
      registry: dhRegistry.trim() || 'docker.io',
    };
    if (dhToken.trim()) request.token = dhToken;
    const ok = await connectDockerhub(request);
    setDhToken('');
    if (ok) {
      addNotification({ type: 'success', title: 'Docker Hub configured', message: request.namespace });
    }
  };

  const steps: DeployStep[] = deploySteps(current?.status ?? null, current?.error_message);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-gray-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-800">
        <div className="flex items-center gap-3">
          <Rocket className="h-5 w-5 text-primary-600" />
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Deploy</h2>
          {supported && githubReady && registryReady && (
            <span className="rounded bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700 dark:bg-green-900/40 dark:text-green-300">
              Integrations ready
            </span>
          )}
        </div>
        {supported && (
          <Button
            variant="secondary"
            size="sm"
            disabled={busy || !projectId}
            onClick={() => void refresh(projectId)}
          >
            <RefreshCw className="mr-1 h-4 w-4" />
            Refresh history
          </Button>
        )}
      </div>

      {error && (
        <div className="mx-4 mt-4 rounded-lg bg-red-50 p-3 text-sm text-red-700 dark:bg-red-900/30 dark:text-red-300" role="alert">
          <div className="flex items-start gap-2">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            <span className="flex-1">{error}</span>
            <button type="button" aria-label="Dismiss error" onClick={() => useDeployStore.getState().clearError()}>
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>
      )}

      {!supported && (
        <div className="flex flex-1 items-center justify-center p-6">
          <Card className="max-w-md text-center">
            <Rocket className="mx-auto mb-3 h-8 w-8 text-gray-400" />
            <h3 className="mb-2 font-semibold text-gray-900 dark:text-white">
              Deployments are not available on this host
            </h3>
            <p className="text-sm text-gray-600 dark:text-gray-400">
              The web IDE needs the integration service (set <code>IDE_SECRET_KEY</code> on the
              server). If the service is disabled the API answers 503 and this screen stays read-only.
            </p>
          </Card>
        </div>
      )}

      {supported && (
        <div className="flex flex-1 overflow-hidden">
          {/* Left: connections ------------------------------------------- */}
          <aside className="w-full max-w-sm shrink-0 space-y-4 overflow-y-auto border-r border-gray-200 bg-gray-50 p-4 dark:border-slate-700 dark:bg-slate-800">
            <Card>
              <div className="mb-3 flex items-center justify-between">
                <h3 className="flex items-center gap-2 font-semibold text-gray-900 dark:text-white">
                  <Github className="h-4 w-4" /> GitHub
                </h3>
                <StatusPill ready={githubReady} />
              </div>
              <div className="space-y-3">
                <Input name="gh-owner" label="Owner" value={ghOwner} onChange={(e) => setGhOwner(e.target.value)} placeholder="acme" />
                <Input name="gh-repo" label="Repository" value={ghRepo} onChange={(e) => setGhRepo(e.target.value)} placeholder="my-app" />
                <Input name="gh-branch" label="Branch" value={ghBranch} onChange={(e) => setGhBranch(e.target.value)} placeholder="main" />
                <Input
                  name="gh-pat"
                  type="password"
                  label="Personal access token"
                  value={ghToken}
                  onChange={(e) => setGhToken(e.target.value)}
                  placeholder={github?.configured ? 'Leave blank to keep the saved token' : 'ghp_…'}
                  autoComplete="new-password"
                  spellCheck={false}
                  helperText="Write-only: encrypted at rest, never shown again, never returned by the API."
                />
                <Button size="sm" className="w-full" loading={busy} onClick={() => void saveGithub()}>
                  {github?.configured ? 'Update GitHub' : 'Connect GitHub'}
                </Button>
              </div>
            </Card>

            <Card>
              <div className="mb-3 flex items-center justify-between">
                <h3 className="flex items-center gap-2 font-semibold text-gray-900 dark:text-white">
                  <Package className="h-4 w-4" /> Docker Hub
                </h3>
                <StatusPill ready={registryReady} />
              </div>
              <div className="space-y-3">
                <Input name="dh-namespace" label="Namespace" value={dhNamespace} onChange={(e) => setDhNamespace(e.target.value)} placeholder="acme" />
                <Input name="dh-registry" label="Registry" value={dhRegistry} onChange={(e) => setDhRegistry(e.target.value)} placeholder="docker.io" />
                <Input
                  name="dh-token"
                  type="password"
                  label="Registry token"
                  value={dhToken}
                  onChange={(e) => setDhToken(e.target.value)}
                  placeholder={registry?.configured ? 'Leave blank to keep the saved token' : 'required on first configuration'}
                  autoComplete="new-password"
                  spellCheck={false}
                  helperText="Write-only. Omit on update to keep the already sealed credential."
                />
                <Button size="sm" className="w-full" loading={busy} onClick={() => void saveDockerhub()}>
                  {registry?.configured ? 'Update Docker Hub' : 'Connect Docker Hub'}
                </Button>
                {(registry?.backend_image || registry?.frontend_image) && (
                  <div className="rounded-lg border border-gray-200 bg-white p-2 text-xs text-gray-500 dark:border-slate-700 dark:bg-slate-900 dark:text-gray-400">
                    <p className="mb-1 font-medium">Image names (derived from the project slug)</p>
                    <p className="truncate">backend: {registry.backend_image || '—'}</p>
                    <p className="truncate">frontend: {registry.frontend_image || '—'}</p>
                  </div>
                )}
              </div>
            </Card>

            <Card>
              <h3 className="mb-2 flex items-center gap-2 font-semibold text-gray-900 dark:text-white">
                <Globe className="h-4 w-4" /> Public API base URL
              </h3>
              <Input
                name="deploy-api-base"
                label="API base URL"
                value={apiBase}
                onChange={(e) => setApiBase(e.target.value)}
                placeholder={DEFAULT_DEPLOY_API_BASE}
                error={apiBaseValid ? undefined : 'Use a plain absolute http:// or https:// URL without credentials or query.'}
                helperText="Baked into the generated frontend at build time. Set it BEFORE deploying — changing it later requires a new deploy."
              />
            </Card>

            <div className="flex gap-2 rounded-lg border border-gray-200 bg-white p-3 text-xs text-gray-500 dark:border-slate-700 dark:bg-slate-900 dark:text-gray-400">
              <Shield className="h-4 w-4 shrink-0" />
              <p>
                Credentials are sealed with <code>IDE_SECRET_KEY</code> and never returned by the
                API. A missing key makes every deploy route answer 503.
              </p>
            </div>
          </aside>

          {/* Centre: action, progress, history ---------------------------- */}
          <main className="flex-1 overflow-y-auto bg-gray-100 p-6 dark:bg-slate-900">
            <Card className="mb-6">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div>
                  <h3 className="text-lg font-semibold text-gray-900 dark:text-white">Deploy this project</h3>
                  <p className="mt-1 max-w-xl text-sm text-gray-600 dark:text-gray-400">
                    Generates the application, commits the tree and pushes it to GitHub. The image
                    build then runs on GitHub Actions — a successful push is <strong>not</strong> a
                    deployment.
                  </p>
                </div>
                <Button
                  variant="primary"
                  disabled={!ready}
                  loading={deploying}
                  onClick={() => void deploy(projectId)}
                >
                  <Rocket className="mr-1 h-4 w-4" />
                  Deploy
                </Button>
              </div>

              <ul className="mt-4 grid gap-2 text-sm sm:grid-cols-2">
                <ReadinessRow ok={Boolean(projectId)} label="A project is open" />
                <ReadinessRow ok={githubReady} label="GitHub connected and verified" />
                <ReadinessRow ok={registryReady} label="Docker Hub connected and verified" />
                <ReadinessRow ok={apiBaseValid} label="Public API base URL is valid" />
              </ul>
            </Card>

            {deploying && (
              <div className="mb-6 rounded-lg border border-blue-200 bg-blue-50 p-4 text-sm text-blue-800 dark:border-blue-800 dark:bg-blue-900/30 dark:text-blue-200" role="status">
                <div className="flex items-center gap-2">
                  <RefreshCw className="h-4 w-4 animate-spin" />
                  The server is generating, committing and pushing. Step results appear once it answers.
                </div>
              </div>
            )}

            {current && !deploying && (
              <Card className="mb-6">
                <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
                  <h3 className="font-semibold text-gray-900 dark:text-white">Latest deploy</h3>
                  <StatusBadge status={current.status} />
                </div>

                <ol className="space-y-3">
                  {steps.map((step) => (
                    <li key={step.key} className="flex items-start gap-3">
                      <StepMarker state={step.state} />
                      <div className="min-w-0 pt-0.5">
                        <p className="text-sm font-medium text-gray-800 dark:text-gray-200">{step.label}</p>
                        {step.detail && (
                          <p className="text-xs text-gray-500 dark:text-gray-400">{step.detail}</p>
                        )}
                      </div>
                    </li>
                  ))}
                </ol>

                {current.status === 'pushed' && (
                  <p className="mt-4 rounded bg-amber-50 p-3 text-sm text-amber-800 dark:bg-amber-900/30 dark:text-amber-200" role="status">
                    Pushed, not deployed. CI has not reported a successful run for this commit yet —
                    use “Refresh history” to poll again.
                  </p>
                )}
                {isDeployed(current) && (
                  <p className="mt-4 rounded bg-green-50 p-3 text-sm text-green-800 dark:bg-green-900/30 dark:text-green-200" role="status">
                    Deployed: CI succeeded for this exact commit and the images were published.
                  </p>
                )}
                {current.error_message && (
                  <p className="mt-4 rounded bg-red-50 p-3 text-sm text-red-700 dark:bg-red-900/30 dark:text-red-300">
                    {current.error_message}
                  </p>
                )}

                <div className="mt-4 flex flex-wrap items-center gap-4 text-xs text-gray-500 dark:text-gray-400">
                  <span>commit <code>{shortSha(current.commit_sha)}</code></span>
                  <span>updated {formatDeployTime(current.updated_at)}</span>
                  {current.github_run_url && (
                    <a
                      href={current.github_run_url}
                      target="_blank"
                      rel="noreferrer noopener"
                      className="inline-flex items-center gap-1 text-primary-600 hover:underline dark:text-primary-400"
                    >
                      View workflow run <ExternalLink className="h-3 w-3" />
                    </a>
                  )}
                </div>
              </Card>
            )}

            <div className="mb-3 flex items-center justify-between">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-white">Deployment history</h3>
              <span className="text-sm text-gray-500 dark:text-gray-400">{history.length} total</span>
            </div>

            {history.length === 0 ? (
              <Card className="text-center">
                <p className="text-sm text-gray-600 dark:text-gray-400">
                  No deployments for this project yet.
                </p>
              </Card>
            ) : (
              <Card padding="none" className="divide-y divide-gray-200 overflow-hidden dark:divide-slate-700">
                {history.map((row) => (
                  <div key={row.id} className="flex flex-wrap items-center gap-3 p-4">
                    <div className="min-w-[9rem]">
                      <StatusBadge status={row.status} />
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="text-sm font-medium text-gray-800 dark:text-gray-200">
                        <code>{shortSha(row.commit_sha)}</code>
                        {isDeployed(row) && (
                          <span className="ml-2 text-xs font-normal text-green-700 dark:text-green-400">images published</span>
                        )}
                      </p>
                      <p className="text-xs text-gray-500 dark:text-gray-400">
                        {formatDeployTime(row.created_at)} · CI: {row.ci_state}
                        {isPending(row.status) && ' · refresh to poll'}
                      </p>
                    </div>
                    {row.github_run_url && (
                      <a
                        href={row.github_run_url}
                        target="_blank"
                        rel="noreferrer noopener"
                        className="inline-flex items-center gap-1 text-xs text-primary-600 hover:underline dark:text-primary-400"
                      >
                        Run <ExternalLink className="h-3 w-3" />
                      </a>
                    )}
                  </div>
                ))}
              </Card>
            )}
          </main>
        </div>
      )}
    </div>
  );
}

function StatusPill({ ready }: { ready: boolean }) {
  return (
    <span
      className={`rounded px-2 py-0.5 text-xs font-medium ${
        ready
          ? 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300'
          : 'bg-gray-100 text-gray-500 dark:bg-slate-700 dark:text-gray-400'
      }`}
    >
      {ready ? 'Connected' : 'Not connected'}
    </span>
  );
}

function ReadinessRow({ ok, label }: { ok: boolean; label: string }) {
  return (
    <li className={`flex items-center gap-2 ${ok ? 'text-green-700 dark:text-green-400' : 'text-gray-500 dark:text-gray-400'}`}>
      {ok ? <Check className="h-4 w-4" /> : <X className="h-4 w-4" />}
      {label}
    </li>
  );
}

export default DeployPanel;
