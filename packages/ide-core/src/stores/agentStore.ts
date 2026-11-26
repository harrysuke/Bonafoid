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
  AgentConversation,
  AgentDomain,
  AgentEvent,
  AgentMode,
  AgentProvider,
  AgentRunStatus,
  AgentUsage,
  AgentValidationIssue,
} from '../types/agent';
import type { AgentCapabilities } from '../platform/types';
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
  kind: 'step' | 'user' | 'thought' | 'tool' | 'patch' | 'validation' | 'conflict' | 'error';
  /** `step` */
  step?: string;
  title?: string;
  /** `thought`, `user` */
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
  /**
   * The conversation this drawer is showing.
   *
   * Kept so a follow-up continues the same thread and a reload can restore it.
   * `null` before the first run of a fresh session.
   */
  conversationId: string | null;
  /** This project's conversations, newest first. */
  conversations: AgentConversation[];
  /**
   * What the host can do, once probed.
   *
   * `null` while unknown: the drawer renders nothing rather than flashing a
   * button it may have to take away.
   */
  capabilities: AgentCapabilities | null;
  /**
   * Providers the host offers, from `GET /agent/providers`.
   *
   * Empty on a host with no catalog (the desktop shell) and on a server whose
   * catalog could not be read. `providerReason` is what distinguishes the two.
   */
  providers: AgentProvider[];
  /** The provider a run will use. Persisted per browser. */
  providerId: string | null;
  /** Why nothing can run, when the catalog has no usable entry. */
  providerReason: string | null;
  /** Model override for the next run. `null` means the provider's default. */
  selectedModel: string | null;

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
  /** Ask the host what it can do. Cached for the session. */
  probe: () => Promise<void>;
  /**
   * Ask again, discarding the cached answer.
   *
   * The cache is right for build shape but wrong for one thing: on desktop
   * `local` depends on whether a key is stored, which the user can change from
   * the settings panel without a reload.
   */
  reprobe: () => Promise<void>;
  /**
   * Load the project's conversations and restore the newest one.
   *
   * Called on mount, so a reload no longer loses the thread.
   */
  restore: () => Promise<void>;
  /** Start a fresh conversation; the next send creates it server-side. */
  newConversation: () => void;
  /** Switch to an existing conversation and load its messages. */
  openConversation: (conversationId: string) => Promise<void>;
  /** Delete a conversation. */
  deleteConversation: (conversationId: string) => Promise<void>;
  /**
   * Load the provider catalog and settle on a selection.
   *
   * Called when the drawer opens. A failure leaves `providers` empty, which
   * reads as "this host has no picker" — the composer still works against the
   * deployment's default, so a catalog outage costs the choice, not the feature.
   */
  loadProviders: () => Promise<void>;
  /** Pick a provider. Clears the model override, which was the old provider's. */
  selectProvider: (id: string) => void;
  /** Set a model override for the selected provider. */
  setSelectedModel: (model: string | null) => void;
}

/** The server id of the open project, or null when it is a local file. */
function serverProjectId(): string | null {
  const path = useProjectStore.getState().projectPath;
  return path && path.startsWith('api://projects/')
    ? path.slice('api://projects/'.length)
    : null;
}

/** Turn stored messages into timeline rows for a restore. */
function restoredEntries(
  messages: { role: string; text: string; tool?: string | null }[],
): AgentEntry[] {
  return messages.map((message) => {
    const id = generateId();
    if (message.role === 'user') return { id, kind: 'user', text: message.text };
    if (message.role === 'tool') {
      // `ok` is inferred, not known: the stored row is the compacted form when
      // it was large, which carries `"ok":true|false`, and a plain error string
      // otherwise. A restored row is history, so a wrong tick costs nothing.
      const ok = !/^error:|"ok":false/.test(message.text);
      return { id, kind: 'tool', tool: message.tool ?? 'tool', ok, summary: message.text };
    }
    return { id, kind: 'thought', text: message.text };
  });
}

/** The prompt of the last send, so `retry` does not need the composer. */
let lastPrompt: string | null = null;
/** The mode of the last send, for the same reason. */
let lastMode: AgentMode = 'pipeline';

/**
 * Where the picker remembers the choice.
 *
 * Per browser, not per project: a user who picked a provider picked it for
 * themselves. Wrapped because storage throws in a private window and in a
 * server-side render, and a preference is never worth an exception.
 */
const PROVIDER_KEY = 'nocode.agent.provider';
const MODEL_KEY = 'nocode.agent.model';

function readStored(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStored(key: string, value: string | null): void {
  try {
    if (value) window.localStorage.setItem(key, value);
    else window.localStorage.removeItem(key);
  } catch {
    // A lost preference is not an error.
  }
}

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
    const projectId = serverProjectId();
    // Local mode edits the file the canvas has open, so there is no server id
    // to pass and none is needed (plan §15.4.1). Remote mode is the one that
    // cannot work without one.
    const local = get().capabilities?.local ?? false;
    if (!projectId && !local) {
      set({
        error: 'Open a project from the server before asking the assistant.',
      });
      return;
    }

    // Flush local edits first. The agent reads the saved copy — the server's
    // row on web, the file on desktop — so anything still in the editor would
    // be silently reverted by the first patch. A project with no path yet has
    // no saved copy to diverge from, and `saveProject()` would open a dialog,
    // so the in-memory JSON is left for the local command to read instead.
    if (projectState.isDirty && projectState.projectPath) {
      await projectState.saveProject();
    }

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
      const conversationId = get().conversationId ?? undefined;
      // An empty id means "local": the host resolves the project from its own
      // working copy, and the desktop command ignores the argument.
      const handle = await platform.agentChat(
        projectId ?? '',
        {
          prompt,
          mode,
          conversation_id: conversationId,
          // The picker's choice. Absent when the host has no catalog, which is
          // the old behaviour: the deployment's default provider.
          provider: get().providerId ?? undefined,
          model: get().selectedModel ?? undefined,
        },
        (event) => get().applyEvent(event),
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
    conversationId: null,
    conversations: [],
    capabilities: null,
    providers: [],
    providerId: null,
    providerReason: null,
    selectedModel: null,

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
        conversationId: null,
        conversations: [],
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
            conversationId: event.conversation_id ?? get().conversationId,
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

    probe: async () => {
      // Cached for the session: the answer cannot change without a reload
      // (it depends on build shape and, on desktop, on a saved key).
      if (get().capabilities) return;
      set({ capabilities: await readCapabilities() });
    },

    reprobe: async () => {
      set({ capabilities: await readCapabilities() });
    },

    restore: async () => {
      const platform = getPlatform();
      const projectId = serverProjectId();
      if (!projectId || typeof platform.agentConversations !== 'function') return;
      // A run in flight owns the timeline; restoring over it would delete the
      // rows the user is watching.
      if (get().status === 'running') return;

      try {
        const conversations = await platform.agentConversations(projectId);
        set({ conversations });
        const latest = conversations[0];
        if (!latest || get().conversationId) return;
        await get().openConversation(latest.id);
      } catch {
        // History is a convenience: a failure leaves the drawer usable, with
        // an empty timeline, rather than showing an error nobody asked for.
      }
    },

    newConversation: () =>
      set({
        conversationId: null,
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
        status: 'idle',
      }),

    openConversation: async (conversationId) => {
      const platform = getPlatform();
      const projectId = serverProjectId();
      if (!projectId || typeof platform.agentConversation !== 'function') return;
      const detail = await platform.agentConversation(projectId, conversationId);
      set({
        conversationId,
        entries: cap(restoredEntries(detail.messages)),
        status: 'idle',
        streaming: '',
        issues: [],
        usage: null,
        error: null,
        runId: null,
        revision: null,
        readyToPreview: false,
        rebased: false,
        conflicts: [],
        lock: null,
      });
    },

    deleteConversation: async (conversationId) => {
      const platform = getPlatform();
      const projectId = serverProjectId();
      if (!projectId || typeof platform.agentDeleteConversation !== 'function') return;
      await platform.agentDeleteConversation(projectId, conversationId);
      const conversations = get().conversations.filter((row) => row.id !== conversationId);
      set({ conversations });
      // Deleting the open thread leaves an empty drawer rather than a timeline
      // whose id no longer exists on the server.
      if (get().conversationId === conversationId) get().newConversation();
    },

    loadProviders: async () => {
      const platform = getPlatform();
      if (typeof platform.agentProviders !== 'function') return;
      try {
        const list = await platform.agentProviders();
        // Keep what the user picked, even across a reload. Fall back to the
        // server's default, then to the first provider that can actually run:
        // a remembered key that has since been disabled must not wedge the
        // composer on a dead selection.
        const remembered = get().providerId ?? readStored(PROVIDER_KEY);
        const chosen =
          list.providers.find((row) => row.id === remembered && !row.unavailable) ??
          list.providers.find((row) => row.id === list.default && !row.unavailable) ??
          list.providers.find((row) => !row.unavailable) ??
          null;
        set({
          providers: list.providers,
          providerReason: list.reason,
          providerId: chosen?.id ?? null,
          selectedModel: get().selectedModel ?? readStored(MODEL_KEY),
        });
      } catch {
        // A catalog outage costs the choice, not the feature: a run with no
        // selection still uses the deployment default.
      }
    },

    selectProvider: (id) => {
      set({ providerId: id, selectedModel: null });
      writeStored(PROVIDER_KEY, id);
      // The old model belonged to the old provider.
      writeStored(MODEL_KEY, null);
    },

    setSelectedModel: (model) => {
      set({ selectedModel: model });
      writeStored(MODEL_KEY, model);
    },
  };
});

/**
 * What this host can do with the assistant.
 *
 * A probe, not a boolean: desktop local mode is a Tauri command, so
 * `typeof agentChat === 'function'` would hide a working feature (plan §15.4.3).
 * A host that predates `agentCapabilities` is described by the old test, which
 * is still the right answer for the web build.
 */
export async function readCapabilities(): Promise<AgentCapabilities> {
  try {
    const platform = getPlatform();
    if (typeof platform.agentCapabilities === 'function') {
      return await platform.agentCapabilities();
    }
    const remote = typeof platform.agentChat === 'function';
    return { local: false, remote, providers: [], preview: remote };
  } catch {
    return { local: false, remote: false, providers: [], preview: false };
  }
}

/** Whether the drawer should exist at all. Awaits the probe once per session. */
export async function hasAgent(): Promise<boolean> {
  const cached = useAgentStore.getState().capabilities;
  const capabilities = cached ?? (await readCapabilities());
  if (!cached) useAgentStore.setState({ capabilities });
  return capabilities.local || capabilities.remote;
}

/**
 * The synchronous form, for code that cannot await.
 *
 * Reads the cached probe only; `false` before the probe has run. Kept because
 * a few call sites gate on a plain boolean and a flash of a hidden drawer is
 * worse than a moment of absence.
 */
export function agentSupported(): boolean {
  const capabilities = useAgentStore.getState().capabilities;
  if (capabilities) return capabilities.local || capabilities.remote;
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
  // The probe depends on the session (a desktop key, a server connection), so
  // it must be re-read after a logout. The catalog does too: it is a property
  // of the server the next session talks to. The *choice* survives in storage.
  useAgentStore.setState({
    capabilities: null,
    providers: [],
    providerId: null,
    providerReason: null,
  });
}
