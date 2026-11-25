# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

**Agentic app generation** (`crates/agent`, `crates/ide-server`, `packages/ide-core`, `docs/agentic-app-generation.md`)
- In-process agent that builds a whole application — entities, workflows,
  forms, pages and theme — from one prompt, calling the same `McpToolExecutor`
  `POST /mcp` exposes, so the two surfaces cannot drift.
- `POST /api/v1/projects/:id/agent/chat` streams `run_started`, `step_started`,
  `tool_call`, `tool_result`, `model_patch`, `validation`, `project_updated`,
  `run_finished` and `error` frames as SSE.
- Two modes: a four-step `pipeline` (data → process → UI → verify) that rebuilds
  a context card of real ids between steps, and `auto` for a single edit.
- Providers: Anthropic Messages, OpenAI Chat Completions and Ollama, over the
  workspace's existing hyper + rustls stack (no `reqwest`).
- Guardrails enforced in code, not in the prompt: no step may call
  `project_save`; a tool outside the step's allowlist is refused; an invalid
  project is never saved; the model cannot see or choose a session id; turn,
  tool-call, token and wall-clock budgets bound every run.
- Project tools (`project_open`/`get`/`save`/`close`), domain tools
  (`entity_*`, `form_*`, `page_*`, `theme_update`), backend tools
  (`workflow_*`), `project://` resource templates and whole-project integrity
  validation composed from the model and BPMN passes.
- `project:read`/`project:write` scopes; the endpoint answers 401/403/404/400/503
  before the stream opens and reports every later failure as an event.
- Audit trail in `agent_runs` (migration `0010`): prompt, mode, status, saved,
  revision, tokens, tool calls, turns, error.
- Assistant drawer in the shared IDE: a hand-written SSE decoder, a Zustand
  reducer, domain-slice patches applied straight to the canvas, and a one-click
  **Build & Preview** once a run has saved.

**Agentic generation, hardening pass** (plan §15, Phases 29–30)
- Per-token streaming. `LlmProvider::stream` is now the single parse path for
  all three providers (`complete` delegates to it with a null sink), and the
  new `agent_delta` SSE frame carries assistant text while the model writes.
  A `DeltaCoalescer` batches fragments at 48 characters / 40 ms, so 120 deltas
  become three frames instead of 120 and the run's channel cannot fill.
  `AGENT_STREAM_DELTAS` (default on) turns the frames off without a downgrade.
- A dropped client now stops the read instead of finishing a generation nobody
  will see: `DeltaSink::closed()` is checked per line and the provider returns
  `Cancelled` rather than a half-read response — a truncated `tool_use` block
  would otherwise parse to `{}` and run a tool with no arguments.
- One writer per project. `save_project` takes a mandatory
  `expected_revision: u64`; the "read the current row and write against it"
  escape hatch is gone, so an unguarded write cannot be expressed. A
  per-project lease (90 s TTL, 30 s heartbeat, released on `Drop`) refuses the
  second tab with `409 project_locked` **before** the SSE stream opens.
- Stale saves are merged, not lost. A three-way per-object merge (`base` vs the
  run's result vs the current row) writes disjoint edits and reports overlaps
  as a `conflict` event naming the objects, with `run_finished { status:
  "conflict" }` and nothing written. The merged project is re-validated before
  it is saved, and the retry happens at most once.
- Tool results carry a machine-readable `code` (`stale_revision`, `not_found`,
  …), so the orchestrator branches on the code rather than on error prose.
- Drawer: live streaming bubble capped at 8 KB, a conflict panel with **Try
  again** / **Use the server's version**, and a lock notice with the remaining
  TTL — a 409 reads as a rule, not as a broken product.

**Agentic workflow authoring** (`crates/mcp`, `docs/mcp-server.md`)
- MCP server exposing 23 typed tools for creating, editing, validating,
  simulating, exporting and saving BPMN workflows; agents never emit raw XML.
- Read-only grounding resources for the generated workflow/project JSON
  Schemas, the 16-type node catalogue, known-good JSON/XML examples and an
  authoring guide.
- Deterministic layered auto-layout and server-owned node/edge UUIDs; sessions
  carry revisions, bounded undo/redo and caller-scoped isolation.
- stdio transport (`nocode-mcp` binary) and an authenticated Streamable HTTP
  route at `POST /mcp` mounted by `nocode-ide-server` with read/write scopes.
- Claude Desktop/Cursor configuration example and a five-prompt agent
  evaluation proxy.

**Container images and CI/CD** (`.github/workflows/docker.yml`, `.hadolint.yaml`)
- Workflow that lints both Dockerfiles with hadolint, validates
  `docker-compose.yml` on the default and preview profiles, then builds the
  lean image and boots it on a fresh volume.
- The boot is asserted, not assumed: `/health`, `/ready`, the served web IDE, a
  non-root UID, a database created on the volume, `docker compose up` against
  the built image, and a SIGTERM stop in under five seconds.
- Multi-arch (`amd64`, `arm64`) publishes to GHCR on `main` and on `v*` tags;
  the `-preview` image is published on tags. No secrets required — GHCR
  authenticates with the workflow's own token.
- Trivy scan of the built image with SARIF upload to the Security tab.
- `.hadolint.yaml` sets the threshold to `error` and documents why apt package
  versions are not pinned.

### Fixed

- **`Dockerfile` did not build at all.** Its manifest list was hand-maintained
  and had missed `crates/mcp` and `crates/deploy`, so cargo refused to load the
  workspace. Both Dockerfiles now copy `crates/*/Cargo.toml` with
  `COPY --parents`, and the stub-source list is derived from those manifests
  instead of a second hand-written list.
- **The image ignored SIGTERM.** `docker stop`, `compose down` and Kubernetes
  all signal first and kill ten seconds later; the server had no handler, so
  every stop waited out the grace period and then killed the process
  mid-request. `serve` now drains in-flight requests on SIGTERM
  (`crates/ide-server/src/server.rs`); a stop takes about two seconds.
- **`DATABASE_URL` in the image had no `mode=rwc`**, so a container started on
  an empty volume could not create its SQLite file. The image default now
  matches `docker-compose.yml`.
- **The workspace did not compile on current stable Rust.** `sqlx-postgres`
  0.7.4 fails on the never-type fallback change (rust-lang/rust#148922). The
  workspace enabled `sqlx-postgres` and `sqlx-mysql` through `sea-orm` although
  the IDE is SQLite-only, so both drivers were dropped — less compile time, a
  smaller image, and the build works on stable again.

### Changed

- Cargo and npm caches are BuildKit cache mounts rather than image layers, so a
  source-only change relinks instead of rebuilding the dependency graph.
- The build re-touches the copied sources before compiling: the stub build
  caches artifacts for the same crates, `COPY` preserves the context's older
  timestamps, and cargo decides freshness by mtime — without the touch it
  declares the crates fresh and installs the stub `fn main() {}` binary.

## [0.1.0] — 2026-10-29

First version with a web IDE, a hosted preview that can be trusted with more than
one account, and packaging.

### Added

**Web IDE server** (`crates/ide-server`)
- Accounts with Argon2id hashing and JWT RS256 in HttpOnly cookies, plus
  double-submit CSRF on every state-changing route.
- Project CRUD, import/export and duplicate, all owner-scoped.
- Code generation over HTTP, using the same `crates/ide` service as the CLI.
- BPMN 2.0 import, validation and execution.
- Per-user WebAssembly plugin hosts (wasmtime), replayed from the database on
  startup.

**Hosted preview**
- Build a project's backend (`cargo build`) and frontend (`trunk build`) on the
  server, then run the result on an allocated port.
- Serve the built frontend at `/preview/{buildId}/` and proxy
  `/preview/{buildId}/api/*` to the running backend.
- `GET /api/v1/preview/capabilities` reports the host toolchain, so the IDE can
  say "previews unavailable" instead of failing a build.

**Shared frontend** (`packages/ide-core`)
- Builders, stores and types shared by the browser and Tauri frontends.
- `describeError`/`notifyError` and a single notification surface used by every
  page, replacing per-page error strings.
- `useUnsavedChangesGuard` and `confirmLeave()` for dirty-project navigation.
- WCAG contrast helpers (`relativeLuminance`, `contrastRatio`,
  `readableTextColor`) used to pick theme text colours.

**Packaging**
- Three-stage `Dockerfile` for the lean IDE server image (no Rust toolchain).
- `Dockerfile.preview` adding cargo, trunk and the wasm32 target for hosted
  previews.
- `docker-compose.yml` with healthchecks, a named data volume, and the preview
  service behind a `preview` profile.
- `.dockerignore` and a `.gitignore` that keeps the database, signing keys and
  preview workspaces out of the repository.

### Changed

- **Preview isolation and quotas.** Build slots, running previews and build
  directories are now per user. Previously one account could occupy every slot
  and overwrite another account's compiled binary, because all builds shared a
  single cargo target directory.
- **Preview builds are bounded.** A build has a wall-clock deadline and is
  cancelled rather than left running; the generated backend runs in its own
  process group so stopping it stops what it forked.
- **The proxy is bounded.** Session credentials are stripped on the request hop
  and `Set-Cookie` on the response hop; every hop has a connect, handshake, send
  and body timeout; request bodies over 8 MiB return `413` and upstream timeouts
  return `504`.
- **Preview output is captured per run.** A preview's stdout/stderr goes into its
  own run record (last 500 lines, flushed every 250 ms) instead of flooding the
  IDE server's log; it used to be inherited and invisible.
- **Preview workspace paths are validated.** Build ids must parse as UUIDs and
  deletion is contained under the preview root.
- **The reaper notices crashed previews**, so a dead preview stops holding a
  quota slot.
- **Project names are validated** (trimmed, non-empty, ≤ 120 characters, no
  control characters) instead of being written into the project file verbatim.
- **The server binds loopback by default**, with a `HOST` override so containers
  can bind `0.0.0.0` deliberately.
- **The IDE is keyboard-operable.** Canvas nodes are focusable controls, the
  drag handle is a real button with ↑/↓ reordering, icon-only buttons have
  accessible names, disclosures report `aria-expanded`, and inline forms return
  focus to their trigger.
- **Autosave can no longer be bypassed**, and a failed autosave or a null project
  path now tells the user instead of failing silently.
- **Theme text colours are chosen by measured contrast** rather than hardcoded
  per role, and the choice is asserted at ≥ 4.5:1 in tests.
- **README is web-first** and documents every environment variable; the desktop
  IDE is described as a secondary shell.
- **Dependencies now match what is imported.** `@dnd-kit/*`,
  `@hookform/resolvers`, `@monaco-editor/react`, `monaco-editor`, `date-fns`,
  `axios`, `zod` and `react-hook-form` were declared but never imported, along
  with a second copy of `clsx`/`lucide-react`/`reactflow`/`tailwind-merge`/
  `zustand` in the desktop app; they are gone, and the manual chunk rules that
  could never fire went with them. `apps/ide-tauri` now declares the
  `@nocode/ide-core` dependency it was importing without declaring.
- **The documentation describes the tree that exists.** The four `docs/*.md`
  files predated the web IDE by a full milestone. They no longer describe a
  runtime crate or an `apps/generated-app` crate (both deleted), an external
  `bpmn-server` engine (the engine is in-tree, and generated apps do not
  execute workflows), eight settings tabs (there are three), a plugin
  marketplace (install is local-manifest only), target checkboxes in the code
  generator (codegen always emits every tree), or a `Uuid`-keyed server schema
  with a sessions table (ids are `TEXT` and a session *is* a refresh token).
  Each file gained a Document Control row explaining the change.

### Fixed

- **A stopped preview or cancelled build left its grandchildren running.** The
  child was placed in its own process group, but nothing ever killed the
  *group*: `Child::kill` signals one pid. A preview backend that had forked a
  worker kept the inherited stdout/stderr pipes open, which parked the log pump
  and made the runtime wait for the grandchild to exit on its own. Both paths
  now sweep the tree — a group `SIGKILL` on Unix, `taskkill /T` on Windows —
  and a `TreeGuard` covers the early-return and cancelled-future paths.
- **Dialogs did not manage focus.** `Modal` set `aria-modal="true"` but never
  moved focus into the dialog, never returned it to the trigger, and let Tab
  walk into the page behind the overlay — the exact failure the attribute is
  supposed to prevent. It now does all three.
- **Tailwind was pinned to 4 while the stylesheets were written for 3.** A
  dependency bump moved `tailwindcss` to 4, but the CSS still used the v3
  `@tailwind base/components/utilities` directives and both apps hand
  `tailwindcss` straight to PostCSS, which v4 refuses. The breakage was
  invisible locally because `node_modules` still held a v3 install; a clean
  `npm ci` exposed it. Pinned back to `^3.4.17`.
- The `cargo build` of a preview wrote its binary into a directory the run
  manager did not know about, so a successful build could not be started.
- `enqueue` accepted arbitrary strings as build ids, which then became directory
  names.
- Cancellation flags and build slots leaked when a build worker stopped early.
- A run record survived its process, so a crashed preview kept consuming quota.
- `stop` reported success even when the process ignored the kill.
- There was no unsaved-changes guard at all: navigating away, signing out or
  closing the tab discarded edits silently. `useBlocker` — the usual solution —
  cannot work here, because the app uses `BrowserRouter` rather than a data
  router, so the guard is a `beforeunload` listener plus an explicit
  `confirmLeave()` at the navigation points.

### Known limitations

- SQLite is the only supported database; the migrations are SQLite-specific.
- Previews run on the host as an unprivileged child process. There is no
  container isolation yet, so a preview is trusted-host code.
- No rate limiting or login lockout yet.
- Optimistic concurrency is not implemented: concurrent edits to one project
  last-write-wins.
- Many-to-many relationships are skipped by the generator.

[Unreleased]: https://github.com/harrysuke/Bonafoid/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/harrysuke/Bonafoid/releases/tag/v0.1.0
