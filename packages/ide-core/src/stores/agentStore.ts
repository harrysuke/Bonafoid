/**
 * The assistant drawer's state.
 *
 * One run at a time, one stream of events. The store is a reducer over
 * `AgentEvent`: `applyEvent` is the only place that interprets a frame, which
 * is what makes the wire contract testable without a browser or a server.
 *
 * Two rules the store enforces:
 *
 * 1. **Patches go to the project store, not into this store.** The drawer shows
 *    that `entities` changed; the canvas is the thing that must change.
 * 2. **A run never marks the project dirty.** The server saves before it
 *    reports success, so marking dirty would queue an autosave that fails the
 *    revision check.
 *
 * And two states that are deliberately *not* failures:
 *
 * - `lock` — another run holds the project (409, before any stream exists).
 * - `conflicts` — the save was stale and the merge found overlapping edits, so
 *   nothing was written and the server's copy is intact. The user resolves it,
 *   the store does not guess.
 */

import { create } from 'zustand';
import { getPlatform } from '../platform/registry';
import { notifyError } from '../utils/errors';
import { generateId } from '../utils/helpers';
import type {
  AgentChatHandle,
  AgentConflict,
  AgentDomain,
  AgentEvent,
  AgentMode,
  AgentRunStatus,
  AgentUsage,
  AgentValidationIssue,
} from '../types/agent';
import { useProjectStore } from './projectStore';
import type { Project } from '../types/project';

/** The handle of the run in flight, kept outside the store: it is not state. */
let activeRun: AgentChatHandle | null = null;

/** Cap on the timeline. A long run can emit hundreds of frames. */
const MAX_ENTRIES = 200;

/**
 * Cap on the in-progress assistant bubble.
 *
 * Deltas are appended to one string rather than pushed onto `entries` — an
 * array push per token re-renders the whole timeline, which drops frames on a
 * laptop. The cap stops a runaway generation from growing the store without
 * bound; the ellipsis makes the truncation visible instead of silent.
 */
const MAX_STREAMING = 8 * 1024;

/**
 * A held write lease, as the drawer needs it.
 *
 * `expiresAt` is an absolute timestamp, not a countdown: the store must not own
 * a timer, and a component that re-renders on an interval would keep resetting
 * a relative number anyway.
 */
export interface AgentLock {
  /** The run id holding the project, when the server named one. */
  holder: string | null;
  /** Epoch milliseconds at which the lease lapses by itself. */
  expiresAt: number;
}

/** One row of the drawer's timeline. */
export interface AgentEntry {
  id: string;
  kind: 'step' | 'thought' | 'tool' | 'patch' | 'validation' | 'conflict' | 'error';
  /** `step` */
  step?: string;
  title?: string;
  /** `thought` */
  text?: string;
  /** `tool` */
  tool?: string;
  args?: unknown;
  ok?: boolean;
  summary?: string;
  durationMs?: number;
  /** `patch` */
  domains?: AgentDomain[];
  /** `validation` */
  valid?: boolean;
  issues?: AgentValidationIssue[];
  /** `conflict` */
  conflicts?: AgentConflict[];
  /** `error` */
  message?: string;
  recoverable?: boolean;
}

export interface AgentState {
  /** Whether the drawer is visible. */
  open: boolean;
  /** The composer's contents. */
  prompt: string;
  /** `idle` before the first run, then the last run's status. */
  status: AgentRunStatus | 'idle';
  entries: AgentEntry[];
  issues: AgentValidationIssue[];
  usage: AgentUsage | null;
  error: string | null;
  runId: string | null;
  /** `provider/model`, for the drawer's subtitle. */
  model: string | null;
  /** The database revision the canvas now reflects. */
  revision: number | null;
  /**
   * Text the model is writing right now.
   *
   * Kept apart from `entries` on purpose: it changes many times per second and
   * must not re-render the timeline. It is frozen into a `thought` row when the
   * turn ends.
   */
  streaming: string;
  /** True when the last run saved — the Build & Preview call to action. */
  readyToPreview: boolean;
  /** True when the last run's save was merged onto another writer's revision. */
  rebased: boolean;
  /** Objects the merge could not resolve. Non-empty means nothing was saved. */
  conflicts: AgentConflict[];
  /** Set when the server refused the run because another run holds the project. */
  lock: AgentLock | null;

  toggle: () => void;
  setOpen: (open: boolean) => void;
  setPrompt: (prompt: string) => void;
  send: (mode?: AgentMode) => Promise<void>;
  cancel: () => void;
  reset: () => void;
  /** Clear a lock notice so the composer is usable again. */
  clearLock: () => void;
  /** Clear an unresolved conflict once the user has picked a side. */
  clearConflicts: () => void;
  /**
   * Re-send the last prompt after a lock or a conflict.
   *
   * The agent re-reads the project, so a retry starts from whatever the other
   * writer committed — which is the only thing that can make the retry succeed
   * where the first attempt collided.
   */
  retry: () => Promise<void>;
  /** Apply one SSE frame. Public so tests can drive the reducer directly. */
  applyEvent: (event: AgentEvent) => void;
}

/** The prompt of the last send, so `retry` does not need the composer. */
let lastPrompt: string | null = null;
/** The mode of the last send, for the same reason. */
let lastMode: AgentMode = 'pipeline';

/** Keep the timeline bounded without dropping the newest entry. */
function cap(entries: AgentEntry[]): AgentEntry[] {
  return entries.length > MAX_ENTRIES ? entries.slice(entries.length - MAX_ENTRIES) : entries;
}

/** Append streamed text, keeping the buffer bounded. */
function grow(current: string, delta: string): string {
  const next = current + delta;
  if (next.length <= MAX_STREAMING) return next;
  return `${next.slice(0, MAX_STREAMING)}…`;
}

/**
 * Freeze the in-progress text into a timeline row.
 *
 * Normally `agent_thought` does this, because the server sends the complete
 * text right after the turn's deltas. This is the safety net for the paths
 * where no thought follows — a provider error mid-stream, or a cancel — so the
 * user does not watch the text vanish.
 */
function freeze(
  entries: AgentEntry[],
  streaming: string,
): { entries: AgentEntry[]; streaming: string } {
  if (!streaming.trim()) return { entries, streaming: '' };
  return { entries: appendEntry(entries, { kind: 'thought', text: streaming }), streaming: '' };
}

/**
 * Fold one event into the timeline.
 *
 * Pure apart from `generateId`, which only mints row keys.
 */
function appendEntry(entries: AgentEntry[], entry: Omit<AgentEntry, 'id'>): AgentEntry[] {
  return cap([...entries, { ...entry, id: generateId() }]);
}

/** Mark the pending call for `tool` as finished. */
function resolveTool(
  entries: AgentEntry[],
  tool: string,
  ok: boolean,
  summary: string,
  durationMs: number,
): AgentEntry[] {
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (entry.kind === 'tool' && entry.tool === tool && entry.ok === undefined) {
      const next = entries.slice();
      next[index] = { ...entry, ok, summary, durationMs };
      return next;
    }
  }
  // A result without a call should not happen, but losing it would hide a real
  // event, so show it as its own row.
  return appendEntry(entries, { kind: 'tool', tool, ok, summary, durationMs });
}

/**
 * Recognise the 409 the chat endpoint returns when another run holds the
 * project.
 *
 * Structural, not `instanceof`: `ide-core` must not import the web host's error
 * class, and a desktop host would reject with its own. Only the code matters,
 * and a rejection that is not this one falls through to the generic message.
 */
function readLock(error: unknown): AgentLock | null {
  if (typeof error !== 'object' || error === null) return null;
  const { code, details } = error as { code?: unknown; details?: unknown };
  if (code !== 'project_locked') return null;

  const body = (details ?? {}) as {
    holder?: { run_id?: unknown };
    expires_in_seconds?: unknown;
  };
  const seconds = typeof body.expires_in_seconds === 'number' ? body.expires_in_seconds : 0;
  const holder = typeof body.holder?.run_id === 'string' ? body.holder.run_id : null;
  return { holder, expiresAt: Date.now() + Math.max(0, seconds) * 1_000 };
}

export const useAgentStore = create<AgentState>((set, get) => {
  /**
   * Start one run.
   *
   * `send` and `retry` differ only in where the prompt comes from, so they share
   * this. A retry that re-read the composer would send whatever the user had
   * typed since the failure, which is not what "retry" means.
   */
  const start = async (prompt: string, mode: AgentMode): Promise<void> => {
    const platform = getPlatform();
    if (typeof platform.agentChat !== 'function') {
      set({
        error: 'The assistant needs the server host; this build has none.',
      });
      return;
    }

    const projectState = useProjectStore.getState();
    const path = projectState.projectPath;
    const projectId =
      path && path.startsWith('api://projects/') ? path.slice('api://projects/'.length) : null;
    if (!projectId) {
      set({
        error: 'Open a project from the server before asking the assistant.',
      });
      return;
    }

    // Flush local edits first. The agent reads the server's copy, so anything
    // still in the browser would be silently reverted by the first patch.
    if (projectState.isDirty) await projectState.saveProject();

    lastPrompt = prompt;
    lastMode = mode;

    // The timeline is deliberately *not* cleared here — `run_started` replaces
    // it. A refused start (409 lock, 503 unconfigured) must leave the run the
    // user was reading on screen, with the reason shown next to it.
    set({
      status: 'running',
      issues: [],
      usage: null,
      error: null,
      lock: null,
      runId: null,
      revision: null,
      streaming: '',
      readyToPreview: false,
      rebased: false,
      conflicts: [],
    });

    try {
      const handle = await platform.agentChat(projectId, { prompt, mode }, (event) =>
        get().applyEvent(event),
      );
      activeRun = handle;
      await handle.done;
    } catch (error) {
      // One writer per project. A 409 arrives before the stream exists, so
      // there is nothing to clean up — and it is a rule, not a failure, so the
      // drawer shows a lock notice instead of an error.
      const lock = readLock(error);
      if (lock) {
        set({ status: 'idle', lock, error: null });
        return;
      }
      // Starting can fail with a real HTTP status (503 unconfigured, 403 scope,
      // 404 project). That is worth a toast as well as an inline message.
      set({
        status: 'failed',
        error: notifyError(error, 'The assistant could not start'),
      });
      return;
    } finally {
      activeRun = null;
    }

    // A stream that ends without `run_finished` means the connection dropped.
    if (get().status === 'running') {
      set({
        status: 'failed',
        error: 'The connection closed before the assistant finished.',
      });
    }
  };

  return {
    open: false,
    prompt: '',
    status: 'idle',
    entries: [],
    issues: [],
    usage: null,
    error: null,
    runId: null,
    model: null,
    revision: null,
    streaming: '',
    readyToPreview: false,
    rebased: false,
    conflicts: [],
    lock: null,

    toggle: () => set({ open: !get().open }),
    setOpen: (open) => set({ open }),
    setPrompt: (prompt) => set({ prompt }),

    reset: () =>
      set({
        status: 'idle',
        entries: [],
        issues: [],
        usage: null,
        error: null,
        runId: null,
        revision: null,
        streaming: '',
        readyToPreview: false,
        rebased: false,
        conflicts: [],
        lock: null,
      }),

    clearLock: () => set({ lock: null }),

    clearConflicts: () => set({ conflicts: [] }),

    cancel: () => {
      const handle = activeRun;
      activeRun = null;
      if (!handle) return;
      handle.abort();
      // The server sees the disconnect and records the run as cancelled. The
      // client must not sit waiting for a `run_finished` that will never
      // arrive, so it settles the status itself. Whatever the model had already
      // written is kept, because the user watched it arrive.
      const { entries, streaming } = get();
      const frozen = freeze(entries, streaming);
      set({ status: 'cancelled', entries: frozen.entries, streaming: frozen.streaming });
    },

    send: (mode = 'pipeline') => {
      const prompt = get().prompt.trim();
      return prompt ? start(prompt, mode) : Promise.resolve();
    },

    retry: () => (lastPrompt ? start(lastPrompt, lastMode) : Promise.resolve()),

    applyEvent: (event) => {
      const { entries, streaming } = get();

      switch (event.type) {
        case 'run_started':
          set({
            status: 'running',
            runId: event.run_id,
            model: `${event.provider}/${event.model}`,
            streaming: '',
            entries: appendEntry([], {
              kind: 'step',
              step: event.mode,
              title: `Run started (${event.mode})`,
            }),
          });
          return;

        case 'agent_delta':
          // Deliberately not an entry: this fires many times a second and the
          // timeline must not re-render for each one.
          set({ streaming: grow(streaming, event.text) });
          return;

        case 'step_started': {
          const frozen = freeze(entries, streaming);
          set({
            streaming: frozen.streaming,
            entries: appendEntry(frozen.entries, {
              kind: 'step',
              step: event.step,
              title: event.title,
            }),
          });
          return;
        }

        case 'agent_thought':
          // The complete text arrives here right after its deltas, so it
          // replaces the buffer instead of adding a second row for the same
          // words.
          set({
            streaming: '',
            entries: appendEntry(entries, { kind: 'thought', text: event.text }),
          });
          return;

        case 'tool_call': {
          const frozen = freeze(entries, streaming);
          set({
            streaming: frozen.streaming,
            entries: appendEntry(frozen.entries, {
              kind: 'tool',
              tool: event.tool,
              args: event.args,
              step: event.step,
            }),
          });
          return;
        }

        case 'tool_result':
          set({
            entries: resolveTool(entries, event.tool, event.ok, event.summary, event.duration_ms),
          });
          return;

        case 'model_patch': {
          // The canvas is the destination; the timeline only records that it
          // happened. Applying here keeps the two in step even if a frame is
          // rendered before the drawer is opened.
          useProjectStore.getState().applyModelPatch(event.domain, event.value);
          set({
            entries: appendEntry(entries, {
              kind: 'patch',
              domains: [event.domain],
            }),
            revision: event.revision,
          });
          return;
        }

        case 'validation':
          set({
            issues: event.issues,
            entries: appendEntry(entries, {
              kind: 'validation',
              valid: event.valid,
              issues: event.issues,
            }),
          });
          return;

        case 'project_updated':
          useProjectStore.getState().applyServerProject(event.project as Project);
          set({ revision: event.revision });
          return;

        case 'conflict':
          // Nothing was written, so the canvas still holds the agent's patches
          // while the server holds someone else's. Say so, and let the user
          // decide; `run_finished` closes the run as `conflict`.
          set({
            conflicts: event.objects,
            revision: event.current_revision,
            streaming: '',
            entries: appendEntry(entries, {
              kind: 'conflict',
              conflicts: event.objects,
            }),
          });
          return;

        case 'run_finished': {
          const frozen = freeze(entries, streaming);
          set({
            status: event.status as AgentRunStatus,
            usage: event.usage,
            revision: event.revision,
            rebased: event.rebased === true,
            error: event.error ?? null,
            entries: frozen.entries,
            streaming: frozen.streaming,
            readyToPreview: event.status === 'succeeded',
          });
          return;
        }

        case 'error': {
          const frozen = freeze(entries, streaming);
          set({
            error: event.message,
            streaming: frozen.streaming,
            entries: appendEntry(frozen.entries, {
              kind: 'error',
              message: event.message,
              recoverable: event.recoverable,
            }),
          });
          return;
        }

        default:
          // An unknown frame from a newer server is ignored, not fatal: the
          // drawer keeps working and the run still ends with `run_finished`.
          return;
      }
    },
  };
});

/** Whether this host can run the assistant at all. */
export function agentSupported(): boolean {
  try {
    return typeof getPlatform().agentChat === 'function';
  } catch {
    return false;
  }
}

/** Reset the drawer's transient state (used by tests and on logout). */
export function resetAgentStore(): void {
  activeRun?.abort();
  activeRun = null;
  lastPrompt = null;
  lastMode = 'pipeline';
  useAgentStore.getState().reset();
}
