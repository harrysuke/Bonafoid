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
      /**
       * The conversation this turn belongs to.
       *
       * Additive: a host with no conversation store omits it. The drawer keeps
       * it so a follow-up continues the thread and a reload can restore it.
       */
      conversation_id?: string;
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
  /** Continue an existing conversation. Absent means "start a new one". */
  conversation_id?: string;
  /**
   * Which provider to run, from `GET /api/v1/agent/providers`.
   *
   * Absent means "the deployment's default". The picker always sends one; a
   * client that predates it sends none and gets the old behaviour.
   */
  provider?: string;
  /** Model override for this run. Absent means the provider's default. */
  model?: string;
}

/**
 * One provider the assistant can run.
 *
 * Never carries a key: the server opens the sealed credential itself, and this
 * shape is what the drawer is allowed to see (plan §12 decision 5). A provider
 * that cannot run says why in `unavailable`, so the picker can disable it with
 * a reason instead of hiding it.
 */
export interface AgentProvider {
  /** Selection token: a key row id, or `env` for the server's own provider. */
  id: string;
  platform: string;
  label: string;
  /** Wire format: `openai`, `anthropic`, `ollama` or `unsupported`. */
  kind: string;
  /** Model used when the request names none. */
  model: string;
  /** Suggestions for the model field. */
  models: string[];
  /** Last recorded health: `healthy`, `invalid`, `error`, `unknown`. */
  status: string;
  enabled: boolean;
  base_url: string | null;
  /** True when the platform needs no credential, so a missing key is fine. */
  keyless: boolean;
  /** Why this provider cannot run. `null` means usable. */
  unavailable: string | null;
}

/** `GET /api/v1/agent/providers`. */
export interface AgentProviderList {
  providers: AgentProvider[];
  /** Selection a request that names none will use. */
  default: string | null;
  /** Why nothing can run, when nothing can. */
  reason: string | null;
}

/**
 * One platform the build supports, whether or not it is configured.
 *
 * `GET /api/v1/agent/providers/catalog`. The drawer uses this to offer a
 * provider that has no key row yet; `AgentProviderList` only describes what is
 * already configured.
 */
export interface AgentPlatform {
  platform: string;
  label: string;
  /** Wire format: `openai`, `anthropic` or `ollama`. */
  kind: string;
  /** Default endpoint. Empty when the key row must supply one. */
  base_url: string;
  keyless: boolean;
  /** True when the credential is `account_id:api_token` (Cloudflare). */
  account_scoped: boolean;
  default_model: string;
  models: string[];
  /** Setup caveat, e.g. "needs a base URL on its key row". */
  note: string | null;
}

/**
 * `POST /api/v1/agent/providers` — add a provider.
 *
 * The server validates the draft against the same catalog it would use to send
 * a request, so a rejected draft comes back as a 400 whose message names the
 * fix. Nothing is stored until it passes.
 */
export interface AgentKeyDraft {
  /** Catalog platform id (`groq`, `google`, `custom`, …). Case-insensitive. */
  platform: string;
  /** Required unless the platform is keyless. */
  api_key?: string;
  /** Display name. Defaults to the platform's label. */
  label?: string;
  /** Endpoint override. Required only by `custom` and unknown platforms. */
  base_url?: string;
  /** Defaults to enabled. */
  enabled?: boolean;
}

/**
 * `PATCH /api/v1/agent/providers/{id}` — change one row.
 *
 * Omitting a field leaves it alone. An **empty string** clears it: `api_key: ''`
 * removes the stored credential and `base_url: ''` drops the override. That is
 * why a caller that only wants to disable a row never has to resend a key it
 * cannot see.
 */
export interface AgentKeyPatch {
  api_key?: string;
  label?: string;
  base_url?: string;
  enabled?: boolean;
}

/**
 * `POST /api/v1/agent/providers/{id}/test` — the verdict of a live check.
 *
 * `ok: false` is a normal answer, not a failed request: the provider replied,
 * and the reply was "no". Only a row that cannot even be built is an error.
 */
export interface AgentProbeResult {
  ok: boolean;
  /** What was recorded on the row: `healthy`, `invalid` or `error`. */
  status: string;
  /** The provider's own words, bounded to 300 characters. */
  detail: string;
  latency_ms: number;
}

/** One conversation in the drawer's history menu. */
export interface AgentConversation {
  id: string;
  project_id: string;
  title: string;
  message_count: number;
  created_at: string;
  updated_at: string;
}

/**
 * One restored message.
 *
 * Flattened on purpose: the drawer renders text and tool names, and should not
 * be coupled to the storage encoding of the server's `ChatMessage` enum.
 */
export interface AgentConversationMessage {
  seq: number;
  role: 'user' | 'assistant' | 'tool';
  text: string;
  tool?: string | null;
  created_at: string;
}

/** `GET …/agent/conversations/{cid}`. */
export interface AgentConversationDetail extends AgentConversation {
  messages: AgentConversationMessage[];
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
