# Agentic app generation

Build a whole application from one sentence. The assistant reads a prompt,
edits the project model through MCP tools, streams every change to the canvas,
validates the result, and saves it as a new revision — then offers **Build &
Preview**.

Status: implemented (Milestone 5, Phases 22–28 of
[`Agentic-App-Generation-Plan.md`](Agentic-App-Generation-Plan.md)).

```text
"An expense tracker: employees submit expenses, managers approve them"
        │
        ▼
POST /api/v1/projects/:id/agent/chat        (SSE)
        │
        ▼
   nocode-agent ──► LlmProvider (Anthropic | OpenAI | Ollama)
        │                 │
        │                 └─► tool calls
        │                        │
        │                        ▼
        │            nocode-mcp McpToolExecutor   ← the same 23 + 19 tools
        │                        │                   POST /mcp exposes
        │                        ├─ model changes
        │                        └─ domain diffs ──► `model_patch` ──► canvas
        │
        ├─ validate (cross-domain + BPMN)
        └─ save (one revision, message "AI: …")
```

## Enabling it

The assistant is off unless a model is configured. The IDE works normally
without it; the drawer simply does not appear.

| Variable | Meaning | Default |
|----------|---------|---------|
| `AGENT_PROVIDER` | `anthropic`, `openai` or `ollama` | unset (assistant off) |
| `AGENT_API_KEY` | Provider key. Not needed for `ollama` | unset |
| `AGENT_MODEL` | Model id | provider default |
| `AGENT_BASE_URL` | API base, for a gateway or a local Ollama | provider default |
| `AGENT_MAX_TURNS` | Turns per pipeline step | 12 |
| `AGENT_MAX_TOOL_CALLS` | Tool calls per run | 60 |
| `AGENT_MAX_TOKENS` | Tokens per run | 200000 |
| `AGENT_MAX_SECONDS` | Wall clock per run | 300 |
| `AGENT_TEMPERATURE` | Sampling temperature | 0.2 |
| `AGENT_MAX_OUTPUT_TOKENS` | Per-completion output cap | 4096 |
| `AGENT_REQUEST_TIMEOUT` | Per-request timeout, seconds | 120 |

The key is read once at startup and never leaves the server: it is not in any
response, not in the audit row, and `AgentConfig`'s `Debug` prints
`<redacted>` so a stray log line cannot leak it.

```bash
AGENT_PROVIDER=anthropic AGENT_API_KEY=sk-ant-… npm run dev:web
```

## The two modes

| Mode | What it does | When to use |
|------|--------------|-------------|
| `pipeline` (default) | Four steps: **Data** → **Process** → **UI** → **Verify** | Building an app from a description |
| `auto` | One loop with every tool | A small change to an app that already exists |

The pipeline is not cosmetic. Between steps the server — never the model —
rebuilds a context card of the ids that now exist, so step 3 cannot invent an
entity id: it has the real ones in front of it. Step 4 runs only when
validation finds an error, and its prompt is the list of findings.

## The SSE contract

`POST /api/v1/projects/:id/agent/chat`

```json
{ "prompt": "An expense tracker with approvals", "mode": "pipeline" }
```

The response is `text/event-stream`. Each frame's `event:` name is also the
payload's `type` tag, so a client that only reads `data:` still knows what it
got.

| Event | Payload | Meaning |
|-------|---------|---------|
| `run_started` | `run_id`, `mode`, `provider`, `model` | The drawer opens its timeline |
| `step_started` | `step`, `title` | Pipeline progress |
| `agent_thought` | `text` | Narration before a tool call |
| `agent_delta` | `text`, `step`, `seq` | Text while it is being written |
| `tool_call` | `tool`, `args`, `step` | A tool is about to run |
| `tool_result` | `tool`, `ok`, `summary`, `duration_ms` | It finished (or was refused) |
| `model_patch` | `domain`, `value`, `revision` | One domain changed; apply `value` |
| `validation` | `valid`, `issues[]` | The integrity report |
| `project_updated` | `project`, `revision` | Full snapshot at milestones and at the end |
| `conflict` | `objects[]`, `current_revision` | The save collided; **nothing was written** |
| `run_finished` | `status`, `usage`, `revision`, `rebased`, `error?` | Terminal; enables Build & Preview |
| `error` | `message`, `recoverable`, `kind` | A problem, with whether a retry helps |

`status` is one of `succeeded`, `validation_failed`, `conflict`, `failed`,
`cancelled`.

`agent_delta` is **additive**: it is the model's text arriving in pieces, so a
client can show it while it is written. A client that ignores it behaves
exactly as it did before the event existed. Frames are coalesced server-side
(48 characters or 40 ms, whichever comes first), and `seq` rises monotonically
across the whole run — it lets a client drop a duplicate, and is not a resume
protocol. The complete text still arrives afterwards as `agent_thought`, so
the delta stream is never the only copy.

`conflict` is a decision, not a failure: the run's save was rejected and the
per-object merge found objects both sides changed, so **no revision was
written** and the server's copy is intact. `objects` names them
(`{domain, object_id}`). `run_finished` follows with `status: "conflict"` and
`rebased: false`. When the merge *did* succeed against a newer revision,
`run_finished` reports `rebased: true`.

### One writer per project

The endpoint holds a per-project lease for the length of a run. A second caller
is refused **before** any stream exists:

```json
{ "error": "project_locked",
  "message": "another run (…) is generating this project; it expires in 74s",
  "holder": { "run_id": "…" },
  "expires_in_seconds": 74 }
```

That is a `409`, so a client can show the holder and a countdown instead of
opening a stream that would die immediately. The lease expires on its own after
90 seconds without a heartbeat, so a crashed run cannot wedge a project.

Every save carries a mandatory `expected_revision`. There is no code path that
reads the current row and writes against it, which is what made two tabs able
to overwrite each other.

### Why patches, not JSON Patch

`model_patch` carries a **whole domain map** (`entities`, `forms`, `pages`,
`themes`, `workflows`) rather than an RFC 6902 document. The client applies it
with one assignment, no patch library, and no chance of a half-applied edit.
The cost is resending a domain for a one-field change — acceptable at these
sizes, and `project_updated` exists for very large projects.

`revision` in every event is the **database** revision of the content the
client holds, not an edit counter. It stays put during a run and moves once,
when the run saves, so it stays comparable with `project.revision` from
`GET /api/v1/projects/:id`.

## What the agent may and may not do

| Guard | Where it is enforced |
|-------|----------------------|
| Never sees an API key | Keys live in the provider; the model gets tool schemas only |
| Never calls `project_save` | No step's allowlist contains it; the orchestrator saves once, after validation |
| Never saves an invalid project | `finish()` validates first; on error the run ends `validation_failed` and nothing is written |
| Never chooses a session | `session_id` is removed from the tool schemas the model sees and injected server-side |
| Never escapes its step | A tool outside the allowlist is refused with an error result, and the refusal is shown in the timeline |
| Cannot loop forever | `max_turns`, `max_tool_calls`, `max_tokens`, `max_wall_clock` |
| Cannot be prompt-injected by project content | Tool output and project content are labelled untrusted data in every system prompt |
| Cannot read another tenant's project | Every storage call carries the signed `tenant_id` and `user_id`; a foreign project is `404`, never `403` |

## Authorization

The endpoint requires `project:write`. Scopes come from the signed JWT:
every authenticated user gets `project:read` and `workflows:read`;
`project:write` and `workflows:write` need a tenant admin/owner role, a
platform super admin, or an explicit role claim. Bearer tokens skip the
double-submit CSRF check; cookie sessions must echo `x-csrf-token`.

Before the stream opens, the endpoint can answer:

| Status | When |
|--------|------|
| `401` | No session |
| `403` | No `project:write` scope |
| `404` | Project missing, or not this tenant's |
| `400` | Empty prompt, over 4000 characters, or an unknown mode |
| `503` | No provider configured |

Once the first frame is sent the status is `200` and every later failure is an
event, because the response has already been committed.

## Storage

Saving goes through the same `projects` table the REST API uses, so the
revision the agent reports is the revision `GET /projects/:id` returns. Each
save also appends a `project_revisions` row, so an agent edit appears in the
Version panel like any other commit, with the message `AI: <first line of the
prompt>`.

`agent_runs` (migration `0010`) records one row per run: prompt (capped at
4000 characters), mode, status, saved, revision, token counts, tool calls,
turns, error. Rows are written at start and completed at the end, so a crashed
run leaves a `running` row behind. The row deliberately holds no model
messages and no tool arguments.

## The MCP surface it uses

The agent calls the same executor `POST /mcp` does, so the two cannot drift.

| Group | Tools |
|-------|-------|
| Project | `project_open`, `project_get`, `project_save`, `project_close` |
| Data | `entity_create`, `entity_add_field`, `entity_add_relation` |
| UI | `form_create`, `form_add_field`, `page_create`, `page_add_component`, `theme_update` |
| Backend | `workflow_create`, `workflow_scaffold_crud`, `workflow_open`, `workflow_commit`, `workflow_set_start_trigger`, `workflow_bind_service_task` |
| BPMN (Milestone 4) | 23 graph-editing tools, used by the Process and Verify steps |

Resources: `project://{id}/{model,entities,forms,workflows,pages,themes,catalog}`
and `project://schema/{entity,form,page,theme}`.

## Frontend

- `agentStore` (Zustand) reduces the event stream; `applyEvent` is the only
  place a frame is interpreted, which is why it is testable without a browser.
- `projectStore.applyModelPatch` applies a domain slice and deliberately does
  **not** mark the project dirty: the server already saved, so an autosave
  would fail the revision check.
- `AssistantDrawer` is mounted in `Layout`, so it floats over the canvas the
  agent is changing. It hides itself when the host has no `agentChat` (the
  desktop shell).
- `createSseDecoder` in `ide-core/src/utils/sse.ts` is a hand-written frame
  decoder: `EventSource` cannot POST, and a full SSE client would be dead code.

## Testing

The orchestrator is a plain function over a tool executor and an
`LlmProvider`, so the whole loop is testable with a scripted model:

```bash
cargo test -p nocode-agent          # pipeline, allowlist, budget, cancel, save
cargo test -p nocode-mcp            # tools, sessions, integrity, resources
cargo test -p nocode-ide-server agent   # SSE endpoint, audit row, scopes
npm test                            # agentStore reducer, SSE decoder
```

`crates/agent/tests/pipeline.rs` runs a scripted model against a real
`McpToolExecutor` and an in-memory store, and asserts the four properties that
matter: a valid run saves exactly once, an invalid run saves nothing, a step
cannot reach a tool outside its allowlist, and the UI step sees ids created by
the data step.

## Limitations

- **No streaming tokens.** Frames are emitted per tool call and per step, not
  per model token; the drawer shows what the agent is *doing*, not a typing
  animation.
- **One run per project at a time is not enforced.** Two tabs can start two
  runs; the second save wins and the first sees `stale_revision`. A lock is
  the obvious next step.
- **No conversation memory.** Each run starts from the project model, not from
  the previous chat. Follow-up prompts work because the model is re-read, not
  because the transcript is replayed.
- **Desktop has no assistant.** The Tauri shell generates code locally and has
  no server-side agent; `agentChat` is absent there and the drawer hides.
