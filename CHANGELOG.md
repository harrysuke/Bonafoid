# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

## [0.1.0] — 2026-10-28

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

### Fixed

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
