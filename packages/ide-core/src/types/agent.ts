/**
 * Wire contract for the assistant.
 *
 * These types mirror `crates/agent/src/events.rs`. The `type` field of every
 * event is both the SSE `event:` name and the payload tag, so a client that
 * reads only the data line still knows what it got.
 *
 * Changing a variant here without changing the Rust enum is a breaking change
 * for the drawer; `agentStore.test.ts` covers the frames that matter.
 */

/** How a run is driven. `pipeline` builds data → process → UI; `auto` is one loop. */
export type AgentMode = 'pipeline' | 'auto';

/**
 * Terminal status reported by `run_finished`.
 *
 * `conflict` means the save was rejected *and* the three-way merge found
 * overlapping edits, so nothing was written. It is not a failure: the run did
 * its work, another writer got there first, and the project on the server is
 * intact. The drawer answers it with a choice, not an error toast.
 */
export type AgentRunStatus =
  | 'running'
  | 'succeeded'
  | 'validation_failed'
  | 'failed'
  | 'cancelled'
  | 'conflict';

/** Token and call accounting for one run. */
export interface AgentUsage {
  tokens_in: number;
  tokens_out: number;
  tool_calls: number;
  turns: number;
}

/** One validation finding. `severity` is `error` or `warning`. */
export interface AgentValidationIssue {
  domain: string;
  object_id?: string | null;
  message: string;
  severity: string;
}

/** The domain slices a patch can carry. */
export type AgentDomain = 'entities' | 'forms' | 'pages' | 'themes' | 'workflows';

/**
 * One object that both the run and another writer changed.
 *
 * Ids, not values: the drawer's job is to name what collided, and a full pair of
 * domain maps per conflict would be larger than the rest of the frame.
 */
export interface AgentConflict {
  domain: string;
  object_id: string;
}

/** Everything the server streams. */
export type AgentEvent =
  | {
      type: 'run_started';
      run_id: string;
      mode: string;
      provider: string;
      model: string;
    }
  | { type: 'step_started'; step: string; title: string }
  | { type: 'agent_thought'; text: string }
  | {
      /**
       * Assistant text as it is produced, before the turn completes.
       *
       * Additive: a client that ignores this frame behaves exactly as it did
       * before streaming existed. `seq` is monotonic per run so a duplicate can
       * be dropped — it is not a resume protocol.
       */
      type: 'agent_delta';
      text: string;
      step: string;
      seq: number;
    }
  | { type: 'tool_call'; tool: string; args: unknown; step: string }
  | {
      type: 'tool_result';
      tool: string;
      ok: boolean;
      summary: string;
      duration_ms: number;
    }
  | {
      type: 'model_patch';
      domain: AgentDomain;
      value: unknown;
      revision: number;
    }
  | { type: 'validation'; valid: boolean; issues: AgentValidationIssue[] }
  | { type: 'project_updated'; project: unknown; revision: number }
  | {
      /**
       * The save was stale and the merge found overlapping edits. Nothing was
       * written; `current_revision` is what the server actually holds.
       */
      type: 'conflict';
      objects: AgentConflict[];
      current_revision: number;
    }
  | {
      type: 'run_finished';
      status: string;
      usage: AgentUsage;
      revision: number;
      /**
       * True when a stale save was merged and written on the retry. Optional
       * because it is additive: a server that predates it omits the field, and
       * absence reads as "no rebase happened".
       */
      rebased?: boolean;
      error?: string;
    }
  | { type: 'error'; message: string; recoverable: boolean; kind: string };

/** Body of `POST /api/v1/projects/:id/agent/chat`. */
export interface AgentChatRequest {
  prompt: string;
  mode?: AgentMode;
}

/**
 * Body of a 409 from the chat endpoint.
 *
 * One writer per project: the second caller is refused *before* the SSE stream
 * opens, so a contended project never produces a stream that dies mid-build.
 * `expires_in_seconds` is the remaining lease TTL — the drawer counts down with
 * it, so Retry is a decision rather than a guess.
 */
export interface AgentProjectLockedBody {
  error: 'project_locked';
  message: string;
  holder: { run_id: string };
  expires_in_seconds: number;
}

/**
 * A run in flight.
 *
 * `abort` stops the stream; the server sees the disconnect, cancels the run and
 * records it as cancelled. `done` resolves when the stream ends, whether it
 * finished, failed or was aborted — it never rejects, because the failure has
 * already been delivered as an `error` event.
 */
export interface AgentChatHandle {
  abort(): void;
  readonly done: Promise<void>;
}
