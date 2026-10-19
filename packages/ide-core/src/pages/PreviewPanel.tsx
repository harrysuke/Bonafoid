import { useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertTriangle,
  ExternalLink,
  Hammer,
  Loader2,
  Play,
  RefreshCw,
  Square,
  Terminal,
} from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Select } from '../components/ui/Select';
import { getPlatform } from '../platform/registry';
import { useProjectStore } from '../stores/projectStore';
import type { PreviewBuildState, PreviewBuildTarget } from '../platform/types';

/** How often the panel polls a build while it is running. */
const POLL_INTERVAL_MS = 2500;

const STATE_LABELS: Record<PreviewBuildState, string> = {
  queued: 'Queued',
  generating: 'Generating code',
  building_backend: 'Building backend',
  building_frontend: 'Building frontend',
  ready: 'Ready',
  cancelled: 'Cancelled',
  failed: 'Failed',
};

const STATE_COLORS: Record<PreviewBuildState, string> = {
  queued: 'text-gray-500',
  generating: 'text-blue-600 dark:text-blue-400',
  building_backend: 'text-blue-600 dark:text-blue-400',
  building_frontend: 'text-blue-600 dark:text-blue-400',
  ready: 'text-emerald-600 dark:text-emerald-400',
  cancelled: 'text-gray-500',
  failed: 'text-red-600 dark:text-red-400',
};

function isBusy(state: PreviewBuildState | undefined): boolean {
  return (
    state === undefined ||
    state === 'queued' ||
    state === 'generating' ||
    state === 'building_backend' ||
    state === 'building_frontend'
  );
}

export function PreviewPanel() {
  const project = useProjectStore((s) => s.project);
  const previewBuild = useProjectStore((s) => s.previewBuild);
  const previewBuilds = useProjectStore((s) => s.previewBuilds);
  const previewRun = useProjectStore((s) => s.previewRun);
  const capabilities = useProjectStore((s) => s.previewCapabilities);
  const polling = useProjectStore((s) => s.previewPolling);
  const error = useProjectStore((s) => s.previewError);
  const startPreviewBuild = useProjectStore((s) => s.startPreviewBuild);
  const refreshPreviewBuild = useProjectStore((s) => s.refreshPreviewBuild);
  const cancelPreviewBuild = useProjectStore((s) => s.cancelPreviewBuild);
  const runPreview = useProjectStore((s) => s.runPreview);
  const stopPreview = useProjectStore((s) => s.stopPreview);
  const loadPreviewCapabilities = useProjectStore((s) => s.loadPreviewCapabilities);
  const refreshPreviewBuilds = useProjectStore((s) => s.refreshPreviewBuilds);

  const [target, setTarget] = useState<PreviewBuildTarget>('all');
  const [showLogs, setShowLogs] = useState(true);
  const logRef = useRef<HTMLPreElement>(null);

  const platform = getPlatform();
  const supported = typeof platform.previewBuild === 'function';

  useEffect(() => {
    void loadPreviewCapabilities();
    void refreshPreviewBuilds();
  }, [loadPreviewCapabilities, refreshPreviewBuilds]);

  // Poll while a build is in flight; the store stops when state is terminal.
  useEffect(() => {
    if (!polling) return undefined;
    const timer = window.setInterval(() => {
      void refreshPreviewBuild();
    }, POLL_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [polling, refreshPreviewBuild]);

  const build = previewBuild;
  const running = previewRun?.status === 'running';
  const previewSrc = useMemo(() => {
    if (!build || build.state !== 'ready' || !platform.previewUrl) return null;
    // Cache-bust so a rebuild reloads the iframe instead of showing the old app.
    return `${platform.previewUrl(build.id)}?v=${encodeURIComponent(build.updated_at)}`;
  }, [build, platform]);

  useEffect(() => {
    if (showLogs && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [build?.logs.length, showLogs]);

  if (!supported) {
    return (
      <div className="h-full flex items-center justify-center bg-gray-100 dark:bg-slate-900">
        <Card className="max-w-md p-6 text-center">
          <AlertTriangle className="w-8 h-8 mx-auto mb-3 text-amber-500" />
          <p className="font-medium text-gray-900 dark:text-white">Hosted preview unavailable</p>
          <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
            This host builds locally. Use <span className="font-mono">Generate</span> to produce
            the project sources, then build and run them yourself.
          </p>
        </Card>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-gray-100 dark:bg-slate-900">
      <div className="flex flex-wrap items-center gap-3 p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <h1 className="text-lg font-semibold text-gray-900 dark:text-white">Preview</h1>

        <div className="w-40">
          <Select
            label="Target"
            value={target}
            onChange={(event) => setTarget(event.target.value as PreviewBuildTarget)}
            options={[
              { value: 'all', label: 'App + API' },
              { value: 'backend', label: 'Backend only' },
              { value: 'frontend', label: 'Frontend only' },
            ]}
          />
        </div>

        <Button
          variant="primary"
          onClick={() => void startPreviewBuild(target)}
          disabled={!project || polling}
        >
          {polling ? (
            <Loader2 className="w-4 h-4 mr-1 animate-spin" />
          ) : (
            <Hammer className="w-4 h-4 mr-1" />
          )}
          {build ? 'Rebuild' : 'Build & Run'}
        </Button>

        {build && isBusy(build.state) && (
          <Button variant="secondary" onClick={() => void cancelPreviewBuild()}>
            <Square className="w-4 h-4 mr-1" /> Cancel
          </Button>
        )}
        {build && build.state === 'ready' && !running && (
          <Button variant="secondary" onClick={() => void runPreview()}>
            <Play className="w-4 h-4 mr-1" /> Start app
          </Button>
        )}
        {running && (
          <Button variant="secondary" onClick={() => void stopPreview()}>
            <Square className="w-4 h-4 mr-1" /> Stop app
          </Button>
        )}
        {build && (
          <Button variant="ghost" onClick={() => void refreshPreviewBuild()}>
            <RefreshCw className="w-4 h-4" />
          </Button>
        )}

        <div className="flex-1" />
        {build && (
          <span
            className={`text-sm font-medium ${STATE_COLORS[build.state]}`}
            aria-live="polite"
          >
            {STATE_LABELS[build.state]}
            {previewRun ? ` · app :${previewRun.port}` : ''}
          </span>
        )}
      </div>

      {error && (
        <p role="alert" className="px-4 py-2 text-sm bg-red-50 text-red-700 dark:bg-red-900/30 dark:text-red-200">
          {error}
        </p>
      )}

      {capabilities && (!capabilities.cargo || !capabilities.trunk) && (
        <p className="px-4 py-2 text-xs bg-amber-50 text-amber-800 dark:bg-amber-900/30 dark:text-amber-200">
          Host toolchain: cargo {capabilities.cargo ? 'ok' : 'missing'}, trunk{' '}
          {capabilities.trunk ? 'ok' : 'missing (cargo install trunk)'}. Workspace root:{' '}
          <span className="font-mono">{capabilities.root}</span>
        </p>
      )}

      <div className="flex-1 flex overflow-hidden">
        <div className="flex-1 p-4 overflow-auto">
          {previewSrc ? (
            <iframe
              title="Application preview"
              src={previewSrc}
              className="w-full h-full min-h-[480px] rounded-xl border border-gray-200 dark:border-slate-700 bg-white shadow-sm"
            />
          ) : (
            <div className="h-full min-h-[480px] flex items-center justify-center rounded-xl border-2 border-dashed border-gray-300 dark:border-slate-600">
              <p className="text-sm text-gray-500 dark:text-gray-400">
                {build
                  ? build.state === 'ready'
                    ? 'The app is built. Start it to see it here.'
                    : `Build in progress: ${STATE_LABELS[build.state].toLowerCase()}…`
                  : 'Build the project to see a live preview.'}
              </p>
            </div>
          )}
        </div>

        <div className="w-[28rem] border-l border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 flex flex-col">
          <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 dark:border-slate-700">
            <h2 className="text-sm font-semibold text-gray-900 dark:text-white flex items-center gap-2">
              <Terminal className="w-4 h-4" /> Build log
            </h2>
            <button
              type="button"
              onClick={() => setShowLogs((value) => !value)}
              className="text-xs text-gray-500 hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-200"
            >
              {showLogs ? 'Hide' : 'Show'}
            </button>
          </div>

          {showLogs && (
            <pre
              ref={logRef}
              className="flex-1 overflow-auto p-3 text-[11px] leading-relaxed font-mono text-gray-700 dark:text-gray-300 bg-gray-50 dark:bg-slate-900"
            >
              {build?.logs.length ? build.logs.join('\n') : 'No build output yet.'}
            </pre>
          )}

          {previewBuilds.length > 1 && (
            <div className="border-t border-gray-200 dark:border-slate-700 p-3">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400 mb-2">
                Earlier builds
              </h3>
              <ul className="space-y-1 max-h-32 overflow-auto">
                {previewBuilds.slice(1, 8).map((candidate) => (
                  <li key={candidate.id} className="flex items-center justify-between text-[11px]">
                    <span className="truncate font-mono text-gray-500 dark:text-gray-400">
                      {candidate.id.slice(0, 8)}
                    </span>
                    <span className={STATE_COLORS[candidate.state]}>{STATE_LABELS[candidate.state]}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {previewSrc && (
            <div className="border-t border-gray-200 dark:border-slate-700 p-3">
              <a
                href={previewSrc}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1 text-xs text-primary-600 hover:underline dark:text-primary-400"
              >
                <ExternalLink className="w-3 h-3" /> Open preview in a new tab
              </a>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}