/**
 * The assistant drawer.
 *
 * A floating panel rather than a route: the whole point is to watch the canvas
 * change while the agent works, so the panel must not navigate away from it.
 *
 * Rendering rules worth knowing:
 *
 * - a tool row appears on `tool_call` and settles on `tool_result`, so the user
 *   sees what is running instead of a spinner;
 * - `model_patch` rows name the domain that changed, because the canvas is
 *   where the change actually appears;
 * - a `conflict` is a choice, not an error: nothing was written, so the panel
 *   offers "try again" and "use the server's version" instead of a red box;
 * - a 409 lock is a rule, not a failure: it says who holds the project and for
 *   how much longer, so Retry is a decision rather than a guess;
 * - Build & Preview appears only after a *saved* run, because previewing a
 *   revision the server never accepted would show the previous model.
 */

import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  AlertTriangle,
  Check,
  ChevronRight,
  Database,
  GitMerge,
  Loader2,
  Lock,
  Play,
  RefreshCw,
  Send,
  Sparkles,
  Square,
  X,
} from 'lucide-react';
import { useAgentStore, agentSupported } from '../../stores/agentStore';
import { useProjectStore } from '../../stores/projectStore';
import type { AgentEntry } from '../../stores/agentStore';

/** Human labels for the domain names the server sends. */
const DOMAIN_LABELS: Record<string, string> = {
  entities: 'data model',
  forms: 'forms',
  pages: 'pages',
  themes: 'theme',
  workflows: 'workflows',
};

/** Short, stable label for an object id in a one-line list. */
function shortId(id: string): string {
  return id.length > 10 ? `${id.slice(0, 8)}…` : id;
}

/**
 * A ticking clock, but only while something needs counting down.
 *
 * The store keeps an absolute expiry so it owns no timer; the one component
 * that displays a countdown owns the interval instead.
 */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}

function EntryRow({ entry }: { entry: AgentEntry }) {
  switch (entry.kind) {
    case 'step':
      return (
        <div className="flex items-center gap-2 pt-2 first:pt-0">
          <ChevronRight className="w-3.5 h-3.5 text-primary-500" />
          <span className="text-xs font-semibold uppercase tracking-wide text-primary-600 dark:text-primary-400">
            {entry.title}
          </span>
        </div>
      );

    case 'thought':
      return <p className="text-sm italic text-gray-500 dark:text-gray-400">{entry.text}</p>;

    case 'tool': {
      const pending = entry.ok === undefined;
      return (
        <div className="flex items-start gap-2 text-sm">
          <span className="mt-0.5 flex-shrink-0">
            {pending ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin text-primary-500" />
            ) : entry.ok ? (
              <Check className="w-3.5 h-3.5 text-green-600 dark:text-green-400" />
            ) : (
              <X className="w-3.5 h-3.5 text-red-600 dark:text-red-400" />
            )}
          </span>
          <div className="min-w-0">
            <code className="text-xs font-medium text-gray-800 dark:text-gray-200">
              {entry.tool}
            </code>
            {entry.summary && (
              <p
                className="mt-0.5 text-xs text-gray-500 dark:text-gray-400 line-clamp-2 break-words"
                title={entry.summary}
              >
                {entry.summary}
              </p>
            )}
          </div>
          {entry.durationMs !== undefined && (
            <span className="ml-auto flex-shrink-0 text-[10px] tabular-nums text-gray-400">
              {entry.durationMs} ms
            </span>
          )}
        </div>
      );
    }

    case 'patch':
      return (
        <div className="flex flex-wrap items-center gap-1">
          <Database className="w-3 h-3 text-gray-400" />
          {(entry.domains ?? []).map((domain) => (
            <span
              key={domain}
              className="rounded bg-gray-100 px-1.5 py-0.5 text-[10px] font-medium text-gray-600 dark:bg-slate-700 dark:text-gray-300"
            >
              {DOMAIN_LABELS[domain] ?? domain}
            </span>
          ))}
        </div>
      );

    case 'validation': {
      const errors = (entry.issues ?? []).filter((issue) => issue.severity === 'error');
      if (entry.valid) {
        return (
          <p className="flex items-center gap-1.5 text-xs text-green-700 dark:text-green-400">
            <Check className="w-3.5 h-3.5" /> All integrity checks passed
          </p>
        );
      }
      return (
        <div className="rounded border border-amber-300 bg-amber-50 p-2 dark:border-amber-700 dark:bg-amber-900/20">
          <p className="flex items-center gap-1.5 text-xs font-medium text-amber-800 dark:text-amber-300">
            <AlertTriangle className="w-3.5 h-3.5" />
            {errors.length} problem{errors.length === 1 ? '' : 's'} found
          </p>
          <ul className="mt-1 space-y-1">
            {(entry.issues ?? []).slice(0, 8).map((issue, index) => (
              <li
                key={`${issue.domain}-${index}`}
                className="text-[11px] text-amber-800 dark:text-amber-200"
              >
                <span className="font-medium">{issue.domain}</span>
                {issue.object_id ? ` (${issue.object_id.slice(0, 8)})` : ''}: {issue.message}
              </li>
            ))}
          </ul>
        </div>
      );
    }

    case 'conflict': {
      const objects = entry.conflicts ?? [];
      return (
        <div className="rounded border border-amber-300 bg-amber-50 p-2 dark:border-amber-700 dark:bg-amber-900/20">
          <p className="flex items-center gap-1.5 text-xs font-medium text-amber-800 dark:text-amber-300">
            <AlertTriangle className="w-3.5 h-3.5" />
            Changed elsewhere too — nothing was saved
          </p>
          <ul className="mt-1 space-y-0.5">
            {objects.slice(0, 8).map((conflict) => (
              <li
                key={`${conflict.domain}-${conflict.object_id}`}
                className="text-[11px] text-amber-800 dark:text-amber-200"
              >
                <span className="font-medium">
                  {DOMAIN_LABELS[conflict.domain] ?? conflict.domain}
                </span>{' '}
                <code className="font-mono">{shortId(conflict.object_id)}</code>
              </li>
            ))}
            {objects.length > 8 && (
              <li className="text-[11px] text-amber-700 dark:text-amber-300">
                and {objects.length - 8} more
              </li>
            )}
          </ul>
        </div>
      );
    }

    case 'error':
      return (
        <p className="rounded bg-red-50 px-2 py-1 text-xs text-red-700 dark:bg-red-900/20 dark:text-red-300">
          {entry.message}
          {entry.recoverable ? ' — you can try again.' : ''}
        </p>
      );

    default:
      return null;
  }
}

export function AssistantDrawer() {
  const navigate = useNavigate();
  const open = useAgentStore((s) => s.open);
  const status = useAgentStore((s) => s.status);
  const entries = useAgentStore((s) => s.entries);
  const streaming = useAgentStore((s) => s.streaming);
  const prompt = useAgentStore((s) => s.prompt);
  const usage = useAgentStore((s) => s.usage);
  const error = useAgentStore((s) => s.error);
  const model = useAgentStore((s) => s.model);
  const revision = useAgentStore((s) => s.revision);
  const readyToPreview = useAgentStore((s) => s.readyToPreview);
  const rebased = useAgentStore((s) => s.rebased);
  const conflicts = useAgentStore((s) => s.conflicts);
  const lock = useAgentStore((s) => s.lock);
  const toggle = useAgentStore((s) => s.toggle);
  const setOpen = useAgentStore((s) => s.setOpen);
  const setPrompt = useAgentStore((s) => s.setPrompt);
  const send = useAgentStore((s) => s.send);
  const cancel = useAgentStore((s) => s.cancel);
  const retry = useAgentStore((s) => s.retry);
  const clearLock = useAgentStore((s) => s.clearLock);
  const clearConflicts = useAgentStore((s) => s.clearConflicts);
  const openProject = useProjectStore((s) => s.openProject);
  const startPreviewBuild = useProjectStore((s) => s.startPreviewBuild);

  const [mode, setMode] = useState<'pipeline' | 'auto'>('pipeline');
  const timelineRef = useRef<HTMLDivElement | null>(null);

  const now = useNow(lock !== null);

  // Follow the newest entry. `scrollTo` on every render would fight the user
  // scrolling up to read an error, so only pin when already near the bottom.
  // `streaming` is in the deps so the view follows text as it is written.
  useEffect(() => {
    const node = timelineRef.current;
    if (!node) return;
    const nearBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 120;
    if (nearBottom) node.scrollTop = node.scrollHeight;
  }, [entries, streaming]);

  // The desktop host has no server-side agent, so there is nothing to show.
  if (!agentSupported()) return null;

  const running = status === 'running';

  const submit = () => {
    if (!running) void send(mode);
  };

  const buildAndPreview = () => {
    void startPreviewBuild('all');
    navigate('/preview');
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={toggle}
        className="fixed bottom-6 right-6 z-40 flex items-center gap-2 rounded-full bg-primary-600 px-4 py-3 text-sm font-medium text-white shadow-lg transition-colors hover:bg-primary-700"
        aria-label="Open the assistant"
      >
        <Sparkles className="w-4 h-4" />
        Build with AI
      </button>
    );
  }

  return (
    <aside
      className="fixed bottom-0 right-0 z-40 flex h-[min(70vh,640px)] w-full flex-col border-l border-gray-200 bg-white shadow-2xl dark:border-slate-700 dark:bg-slate-800 sm:bottom-6 sm:right-6 sm:w-[420px] sm:rounded-xl sm:border"
      aria-label="Assistant"
    >
      <header className="flex items-center justify-between border-b border-gray-200 px-4 py-3 dark:border-slate-700">
        <div className="min-w-0">
          <h3 className="flex items-center gap-2 text-sm font-semibold text-gray-900 dark:text-white">
            <Sparkles className="w-4 h-4 text-primary-600" />
            Assistant
          </h3>
          <p className="truncate text-xs text-gray-500 dark:text-gray-400">
            {model ?? 'Describe the app you want'}
            {revision !== null && <span className="ml-2">rev {revision}</span>}
          </p>
        </div>
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-slate-700"
          aria-label="Close the assistant"
        >
          <X className="w-4 h-4" />
        </button>
      </header>

      <div ref={timelineRef} className="flex-1 space-y-2 overflow-y-auto px-4 py-3">
        {entries.length === 0 && !error && !streaming && (
          <div className="space-y-2 text-sm text-gray-500 dark:text-gray-400">
            <p>Tell me what to build, for example:</p>
            <button
              type="button"
              className="w-full rounded border border-dashed border-gray-300 px-3 py-2 text-left text-xs hover:border-primary-400 hover:text-primary-600 dark:border-slate-600"
              onClick={() =>
                setPrompt(
                  'An expense tracker: employees submit expenses with an amount and a receipt, managers approve or reject them, and there is a list page per employee.',
                )
              }
            >
              “An expense tracker: employees submit expenses with an amount and a receipt, managers
              approve or reject them…”
            </button>
          </div>
        )}

        {entries.map((entry) => (
          <EntryRow key={entry.id} entry={entry} />
        ))}

        {streaming && (
          <p className="whitespace-pre-wrap text-sm text-gray-700 dark:text-gray-300">
            {streaming}
            <span className="ml-0.5 inline-block h-3.5 w-1.5 animate-pulse bg-primary-500 align-text-bottom" />
          </p>
        )}

        {conflicts.length > 0 && !running && (
          <div className="rounded border border-amber-300 bg-amber-50 p-2 dark:border-amber-700 dark:bg-amber-900/20">
            <p className="text-xs font-medium text-amber-800 dark:text-amber-300">
              Someone else saved this project while the assistant was working.
            </p>
            <p className="mt-0.5 text-[11px] text-amber-700 dark:text-amber-300">
              The assistant's version was not written. The project on the server is unchanged.
            </p>
            <div className="mt-2 flex gap-2">
              <button
                type="button"
                onClick={() => void retry()}
                className="flex items-center gap-1 rounded bg-amber-600 px-2 py-1 text-[11px] font-medium text-white hover:bg-amber-700"
              >
                <RefreshCw className="w-3 h-3" /> Try again
              </button>
              <button
                type="button"
                onClick={() => {
                  void openProject();
                  clearConflicts();
                }}
                className="rounded border border-amber-400 px-2 py-1 text-[11px] font-medium text-amber-800 hover:bg-amber-100 dark:text-amber-200 dark:hover:bg-amber-900/40"
              >
                Use the server's version
              </button>
            </div>
          </div>
        )}

        {lock && !running && (
          <div className="rounded border border-gray-300 bg-gray-50 p-2 dark:border-slate-600 dark:bg-slate-900/40">
            <p className="flex items-center gap-1.5 text-xs font-medium text-gray-800 dark:text-gray-200">
              <Lock className="w-3.5 h-3.5" />
              Another tab is generating this project
            </p>
            <p className="mt-0.5 text-[11px] text-gray-500 dark:text-gray-400">
              {(() => {
                const left = Math.max(0, Math.ceil((lock.expiresAt - now) / 1_000));
                return left > 0
                  ? `It releases in ${left}s.`
                  : 'The hold has lapsed — you can try again.';
              })()}
            </p>
            <div className="mt-2 flex items-center gap-2">
              <button
                type="button"
                onClick={() => void retry()}
                className="flex items-center gap-1 rounded bg-primary-600 px-2 py-1 text-[11px] font-medium text-white hover:bg-primary-700"
              >
                <RefreshCw className="w-3 h-3" /> Retry
              </button>
              <button
                type="button"
                onClick={clearLock}
                className="text-[11px] text-gray-500 underline hover:text-gray-700 dark:text-gray-400"
              >
                Dismiss
              </button>
            </div>
          </div>
        )}

        {error && (
          <p className="rounded bg-red-50 px-2 py-1.5 text-xs text-red-700 dark:bg-red-900/20 dark:text-red-300">
            {error}
          </p>
        )}

        {rebased && (
          <p className="flex items-center gap-1 text-[11px] text-gray-500 dark:text-gray-400">
            <GitMerge className="w-3 h-3" />
            Merged onto a newer revision — the other edits were kept.
          </p>
        )}

        {readyToPreview && (
          <button
            type="button"
            onClick={buildAndPreview}
            className="mt-2 flex w-full items-center justify-center gap-2 rounded-lg bg-primary-600 px-3 py-2 text-sm font-medium text-white hover:bg-primary-700"
          >
            <Play className="w-4 h-4" /> Build &amp; Preview
          </button>
        )}
      </div>

      <footer className="border-t border-gray-200 px-3 py-2 dark:border-slate-700">
        {usage && (
          <p className="mb-1 text-[10px] tabular-nums text-gray-400">
            {usage.turns} turns · {usage.tool_calls} tool calls ·{' '}
            {usage.tokens_in + usage.tokens_out} tokens
          </p>
        )}
        <div className="flex items-end gap-2">
          <textarea
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault();
                submit();
              }
            }}
            rows={2}
            placeholder="Describe the app, or ask for a change…"
            className="flex-1 resize-none rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-sm text-gray-900 focus:border-primary-500 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-white"
            disabled={running}
          />
          {running ? (
            <button
              type="button"
              onClick={cancel}
              className="flex items-center gap-1 rounded-lg bg-gray-200 px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-300 dark:bg-slate-700 dark:text-gray-200"
            >
              <Square className="w-3.5 h-3.5" /> Stop
            </button>
          ) : (
            <button
              type="button"
              onClick={submit}
              disabled={prompt.trim().length === 0}
              className="flex items-center gap-1 rounded-lg bg-primary-600 px-3 py-2 text-sm font-medium text-white hover:bg-primary-700 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Send className="w-3.5 h-3.5" /> Send
            </button>
          )}
        </div>
        <div className="mt-1 flex items-center gap-3 text-[10px] text-gray-400">
          <label className="flex items-center gap-1">
            <input
              type="radio"
              name="agent-mode"
              checked={mode === 'pipeline'}
              onChange={() => setMode('pipeline')}
              disabled={running}
            />
            Build (data → process → UI)
          </label>
          <label className="flex items-center gap-1">
            <input
              type="radio"
              name="agent-mode"
              checked={mode === 'auto'}
              onChange={() => setMode('auto')}
              disabled={running}
            />
            Quick edit
          </label>
        </div>
      </footer>
    </aside>
  );
}
