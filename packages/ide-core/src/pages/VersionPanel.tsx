import { useEffect, useState } from 'react';
import {
  GitCommitHorizontal, RefreshCw, Plus, GitMerge, RotateCcw, KeyRound, Trash2, Server,
} from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { Modal } from '../components/ui/Modal';
import { versionSupported, useVersionStore } from '../stores/versionStore';
import { useProjectStore } from '../stores/projectStore';
import { useUIStore } from '../stores/uiStore';

/** Commit graph, branches, diffs and deployment environments. */
export function VersionPanel() {
  const supported = versionSupported();
  const {
    projectId, branches, history, environments, secrets, diff, busy, error,
    setProject, load, loadHistory, loadDiff, createBranch, commit, merge, restore,
    createEnvironment, promote, rollback, loadSecrets, setSecret, deleteSecret, clearError,
  } = useVersionStore();
  const currentProjectId = useProjectStore((state) => state.project?.id ?? null);
  const currentProject = useProjectStore((state) => state.project);
  const notify = useUIStore((state) => state.addNotification);

  const [projectInput, setProjectInput] = useState('');
  const [branchInput, setBranchInput] = useState('');
  const [commitMessage, setCommitMessage] = useState('');
  const [mergeInto, setMergeInto] = useState<string | null>(null);
  const [sourceBranch, setSourceBranch] = useState('');
  const [strategy, setStrategy] = useState('fast_forward');
  const [revisionId, setRevisionId] = useState('');
  const [diffFrom, setDiffFrom] = useState('');
  const [diffTo, setDiffTo] = useState('');
  const [envName, setEnvName] = useState('');
  const [secretEnv, setSecretEnv] = useState<string | null>(null);
  const [secretKey, setSecretKey] = useState('');
  const [secretValue, setSecretValue] = useState('');

  const effectiveId = projectInput.trim() || currentProjectId;

  useEffect(() => {
    if (!supported || !effectiveId || effectiveId === projectId) return;
    setProject(effectiveId);
  }, [supported, effectiveId, projectId, setProject]);

  useEffect(() => {
    if (supported && projectId) void load();
  }, [supported, projectId, load]);

  if (!supported) {
    return (
      <Card className="mx-auto max-w-lg text-center">
        <GitCommitHorizontal className="mx-auto mb-3 h-8 w-8 text-gray-400" />
        <h2 className="mb-2 font-semibold text-gray-900 dark:text-white">
          Versioning needs the hosted IDE
        </h2>
        <p className="text-sm text-gray-600 dark:text-gray-400">
          This host stores projects locally and has no commit graph or environments.
        </p>
      </Card>
    );
  }

  const doCommit = async () => {
    if (!commitMessage.trim() || !currentProject) return;
    if (await commit(branchInput.trim() || 'main', commitMessage.trim(), currentProject)) {
      notify({ type: 'success', title: 'Commit created' });
      setCommitMessage('');
      await loadHistory(branchInput.trim() || 'main');
    }
  };

  const doSetSecret = async () => {
    if (!secretEnv || !secretKey.trim() || !secretValue) return;
    if (await setSecret(secretEnv, secretKey.trim(), secretValue)) {
      notify({ type: 'success', title: 'Secret stored' });
      setSecretKey('');
      setSecretValue(''); // Write-only: never keep the submitted value.
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <GitCommitHorizontal className="h-6 w-6 text-primary-600" />
          <h1 className="text-xl font-semibold text-gray-900 dark:text-white">Version control</h1>
        </div>
        <div className="flex gap-2">
          <Input value={projectInput} onChange={(e) => setProjectInput(e.target.value)} placeholder={currentProjectId ?? 'Project id'} className="w-56" />
          <Button variant="secondary" size="sm" onClick={() => void load()} disabled={busy || !projectId}>
            <RefreshCw className="mr-1 h-4 w-4" /> Refresh
          </Button>
        </div>
      </div>

      {error && (
        <div role="alert" className="flex items-center justify-between rounded bg-red-50 p-3 text-sm text-red-700">
          <span>{error}</span>
          <Button variant="link" size="sm" onClick={clearError}>Dismiss</Button>
        </div>
      )}

      {!projectId && (
        <Card className="text-center text-sm text-gray-500">Open a project or enter its id to use versioning.</Card>
      )}

      {projectId && (
        <div className="grid gap-6 lg:grid-cols-2">
          <Card>
            <h2 className="mb-3 font-semibold">Branches</h2>
            <div className="flex gap-2">
              <Input value={branchInput} onChange={(e) => setBranchInput(e.target.value)} placeholder="new-branch" />
              <Button size="sm" disabled={!branchInput.trim()} onClick={async () => {
                if (await createBranch(branchInput.trim())) { setBranchInput(''); notify({ type: 'success', title: 'Branch created' }); }
              }}><Plus className="mr-1 h-4 w-4" /> Branch</Button>
            </div>
            {branches.map((branch) => (
              <div key={branch.name} className="flex items-center justify-between border-t border-gray-100 py-2 text-sm dark:border-slate-700">
                <span>{branch.name} <span className="text-xs text-gray-400">{branch.head_revision_id.slice(0, 8)}</span></span>
                <div className="flex gap-1">
                  <Button size="sm" variant="ghost" onClick={() => void loadHistory(branch.name)}>History</Button>
                  <Button size="sm" variant="secondary" onClick={() => setMergeInto(branch.name)}><GitMerge className="mr-1 h-4 w-4" /> Merge</Button>
                </div>
              </div>
            ))}
            <div className="mt-4 border-t border-gray-200 pt-3 dark:border-slate-700">
              <h3 className="mb-2 text-sm font-medium">Commit current project</h3>
              <div className="flex gap-2">
                <Input value={commitMessage} onChange={(e) => setCommitMessage(e.target.value)} placeholder="Commit message" />
                <Button size="sm" disabled={!commitMessage.trim()} onClick={() => void doCommit()}>
                  <GitCommitHorizontal className="mr-1 h-4 w-4" /> Commit
                </Button>
              </div>
            </div>
          </Card>

          <Card>
            <h2 className="mb-3 font-semibold">Revision history</h2>
            <div className="flex flex-wrap gap-2">
              <Input value={diffFrom} onChange={(e) => setDiffFrom(e.target.value)} placeholder="from revision" className="w-32" />
              <Input value={diffTo} onChange={(e) => setDiffTo(e.target.value)} placeholder="to revision" className="w-32" />
              <Button size="sm" disabled={!diffFrom.trim() || !diffTo.trim()} onClick={() => void loadDiff(diffFrom.trim(), diffTo.trim())}>Diff</Button>
            </div>
            {diff && (
              <pre className="mt-3 max-h-40 overflow-auto rounded bg-gray-50 p-2 text-xs dark:bg-slate-900">
                {diff.lines.map((line, index) => (
                  <div key={index} className={line.kind === 'add' ? 'text-green-600' : line.kind === 'remove' ? 'text-red-600' : ''}>
                    {line.kind === 'add' ? '+' : line.kind === 'remove' ? '-' : ' '} {line.text}
                  </div>
                ))}
              </pre>
            )}
            <div className="mt-3 max-h-64 overflow-y-auto">
              {history.length === 0 && <p className="text-sm text-gray-500">Load a branch history to see revisions.</p>}
              {history.map((revision) => (
                <div key={revision.id} className="flex items-center justify-between border-t border-gray-100 py-2 text-xs dark:border-slate-700">
                  <span className="truncate">{revision.message} · {revision.id.slice(0, 8)}</span>
                  <Button size="sm" variant="ghost" onClick={() => void restore(revision.id)}>
                    <RotateCcw className="mr-1 h-3 w-3" /> Restore
                  </Button>
                </div>
              ))}
            </div>
          </Card>
        </div>
      )}

      {projectId && (
        <Card>
          <h2 className="mb-3 flex items-center gap-2 font-semibold"><Server className="h-4 w-4" /> Environments</h2>
          <div className="flex gap-2">
            <Input value={envName} onChange={(e) => setEnvName(e.target.value)} placeholder="staging" />
            <Button size="sm" disabled={!envName.trim()} onClick={async () => {
              if (await createEnvironment(envName.trim())) { setEnvName(''); notify({ type: 'success', title: 'Environment created' }); }
            }}><Plus className="mr-1 h-4 w-4" /> Environment</Button>
          </div>
          {environments.map((environment) => (
            <div key={environment.name} className="mt-3 rounded border border-gray-200 p-3 dark:border-slate-700">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="font-medium">{environment.name} <span className="text-xs text-gray-500">{environment.status}</span></p>
                <div className="flex gap-1">
                  <Button size="sm" variant="secondary" onClick={() => void loadSecrets(environment.name)}>
                    <KeyRound className="mr-1 h-4 w-4" /> Secrets
                  </Button>
                  <Button size="sm" variant="danger" onClick={() => setSecretEnv(environment.name)}>Set secret</Button>
                </div>
              </div>
              <div className="mt-2 flex gap-2">
                <Input value={revisionId} onChange={(e) => setRevisionId(e.target.value)} placeholder="revision id" className="w-56" />
                <Button size="sm" disabled={!revisionId.trim()} onClick={() => void promote(environment.name, revisionId.trim())}>Promote</Button>
                <Button size="sm" variant="ghost" disabled={!revisionId.trim()} onClick={() => void rollback(environment.name, revisionId.trim())}>Rollback</Button>
              </div>
              {secrets.length > 0 && (
                <div className="mt-2 flex flex-wrap gap-2">
                  {secrets.map((secret) => (
                    <span key={secret.key} className="flex items-center gap-1 rounded bg-gray-100 px-2 py-1 text-xs dark:bg-slate-700">
                      {secret.key}
                      <button type="button" aria-label={`Delete ${secret.key}`} onClick={() => void deleteSecret(environment.name, secret.key)}>
                        <Trash2 className="h-3 w-3" />
                      </button>
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </Card>
      )}

      <Modal open={mergeInto !== null} title={`Merge into ${mergeInto ?? ''}`} onClose={() => setMergeInto(null)}>
        <div className="space-y-3">
          <Select
            label="Source branch"
            value={sourceBranch}
            onChange={(e) => setSourceBranch(e.target.value)}
            options={branches.map((branch) => ({ value: branch.name, label: branch.name }))}
            placeholder="Select a branch"
          />
          <Select
            label="Strategy"
            value={strategy}
            onChange={(e) => setStrategy(e.target.value)}
            options={[
              { value: 'fast_forward', label: 'Fast forward' },
              { value: 'theirs', label: 'Prefer theirs' },
              { value: 'ours', label: 'Prefer ours' },
            ]}
          />
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setMergeInto(null)}>Cancel</Button>
            <Button disabled={!sourceBranch || !mergeInto} onClick={async () => {
              if (!mergeInto || !sourceBranch) return;
              if (await merge(mergeInto, sourceBranch, strategy)) { setMergeInto(null); notify({ type: 'success', title: 'Merge complete' }); }
            }}>Merge</Button>
          </div>
        </div>
      </Modal>

      <Modal open={secretEnv !== null} title={`Set secret in ${secretEnv ?? ''}`} onClose={() => setSecretEnv(null)}>
        <div className="space-y-3">
          <Input label="Key" value={secretKey} onChange={(e) => setSecretKey(e.target.value)} />
          <Input label="Value (write-only)" type="password" value={secretValue} onChange={(e) => setSecretValue(e.target.value)} />
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setSecretEnv(null)}>Cancel</Button>
            <Button disabled={!secretKey.trim() || !secretValue} onClick={() => void doSetSecret()}>Store secret</Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
