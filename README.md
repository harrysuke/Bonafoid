# Bonafoid

An open-source no-code platform. You model an application visually — entities,
forms, pages, themes, BPMN workflows — and Bonafoid generates a complete,
runnable codebase from it: a Rust/Axum backend, a Leptos/WASM frontend,
migrations, Docker and Kubernetes manifests.

The product is the **web IDE**. `nocode-ide-server` (Axum + SeaORM) serves the
React IDE, owns projects and accounts, executes BPMN workflows, and can build
and run a hosted preview of the generated app. A Tauri desktop build of the same
UI exists under `apps/ide-tauri`; the browser is the supported path and the one
the docs describe.

## What works today

- **Accounts** — email + password (Argon2), JWT RS256 in HttpOnly cookies with
  double-submit CSRF; every project, plugin and preview is scoped to its owner.
- **Modelling** — entities and fields, forms with validation, pages with a
  component tree, themes with a live colour scale, and a BPMN 2.0 process
  editor with a token-level execution engine.
- **Code generation** — the same Rust generator the `xtask` CLI uses, driven
  from the IDE: backend, frontend, database, docker, kubernetes, PWA, Tauri.
- **Hosted preview** — build the generated app on the server, run it on an
  allocated port, and browse it at `/preview/{buildId}/` with the API proxied
  through the IDE server. Reports honestly when the host lacks the toolchain.
- **Plugins** — per-user WebAssembly plugins executed in a wasmtime sandbox.

## Repository layout

```
crates/
  model/       Domain models (Form, Entity, Workflow, Page, Theme, Project)
  codegen/     Model -> backend / frontend / database / docker / k8s / targets
  plugins/     wasmtime plugin host
  bpmn/        BPMN 2.0 engine (+ optional NAPI bridge)
  ide/         Rust core shared by the server and the desktop shell: state,
               commands, service layer, BPMN helpers, and an egui widget shell
               (`IdeApp`) that no shipped binary currently launches
  ide-server/  Web IDE server (Axum + SeaORM): auth, projects, generation,
               BPMN, plugins, hosted previews
packages/
  ide-core/    The IDE itself: React components, pages, zustand stores, types.
               Both frontends are thin shells around this package
apps/
  ide-web/     Browser IDE (React + Vite), served by ide-server
  ide-tauri/   Desktop IDE (React + Tauri v2), same ide-core
xtask/         Build / code-generation CLI
```

## Prerequisites

- Rust (stable) and Cargo
- **Node.js 20+** and npm — the workspace declares `engines.node >= 20`
- On Windows: the WebView2 runtime (bundled with recent Windows), only for the
  Tauri desktop build

To build or run a **generated** application, and for hosted previews:

- `rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev): `cargo install trunk --locked`

The codegen's `#[ignore]`d compile checks only need the wasm target (they run
`cargo check --target wasm32-unknown-unknown`), not Trunk.

## Quickstart (web IDE)

```bash
npm ci
npm run build:web            # builds packages/ide-core + apps/ide-web -> apps/ide-web/dist
cargo run -p nocode-ide-server
# open http://127.0.0.1:8081
```

On first run the server creates `nocode-ide.db` (SQLite, migrated
automatically) and `jwt_keys/` (an RSA keypair). Register an account and you
land in the IDE. `GET /health` is a liveness probe; `GET /ready` also pings the
database.

For frontend work, `npm run dev:web` serves the IDE on Vite's port with a proxy
to the API, so Rust does not have to recompile for a UI change.

## Configuration

Everything is environment-driven; no config file is required.

| Variable | Default | Meaning |
| --- | --- | --- |
| `DATABASE_URL` | `sqlite://nocode-ide.db?mode=rwc` | SeaORM connection string. **SQLite only** — the migrations are written for it. |
| `HOST` | `127.0.0.1` | Bind address. Loopback by default so a self-hosted IDE does not appear on the LAN by accident; containers set `0.0.0.0`. |
| `PORT` | `8081` | Listen port. |
| `JWT_KEYS_DIR` | `jwt_keys` | Directory holding the RSA signing keypair; generated on first start. |
| `IDE_WEB_DIST` | `apps/ide-web/dist` | Built SPA to serve. |
| `IDE_PLUGIN_ROOT` | `plugin-workspaces` | Per-user plugin directories. |
| `IDE_PREVIEW_ROOT` | `preview-workspaces` | Preview workspaces, logs and the cargo target cache. |
| `IDE_PREVIEW_TIMEOUT_SECS` | `900` | Wall-clock limit for one preview build. |
| `IDE_PREVIEW_PARALLEL` | `2` | Concurrent builds **per user**. |
| `IDE_PREVIEW_MAX_RUNS` | `4` | Concurrently *running* previews **per user**. |
| `IDE_PREVIEW_IDLE_SECS` | `900` | A preview with no proxy traffic for this long is stopped. |
| `IDE_PREVIEW_API_BASE` | `http://$PREVIEW_PUBLIC_ORIGIN/preview` | API base baked into the generated frontend so a preview can reach its own backend. |
| `PREVIEW_PUBLIC_ORIGIN` | `127.0.0.1:8081` | Host:port a browser uses to reach the IDE server; feeds the default above. |
| `IDE_PREVIEW_TRUNK_OFFLINE` | `false` | Pass `--offline` to Trunk (air-gapped hosts with a warm cargo registry). |
| `IDE_PREVIEW_DATABASE_URL` | unset | Database handed to a non-SQLite preview; without it such a preview is refused rather than pointed at the IDE's own database. |
| `RUST_LOG` | `info` | `tracing` filter. |

Two more variables are read by **generated** applications, not by the server:
`APP_NAME` and `CORS_ORIGINS` (see `crates/codegen/src/templates/backend/`).

## Generate an application (xtask CLI)

The fastest end-to-end path, and the same generator the IDE calls.

```bash
# 1. Scaffold a project model (writes ./playground/my-app/project.json)
cargo run --manifest-path xtask/Cargo.toml -- new "My App" --output ./playground

# 2. Generate the application from the model
cargo run --manifest-path xtask/Cargo.toml -- generate \
  --model ./playground/my-app/project.json --output ./generated
```

Other subcommands: `backend`, `frontend`, `database`, `build-web`, `build-pwa`,
`build-desktop`, `build-tauri`, `import-bpmn`, and `types`.

Generated output:

```
generated/
  backend/     Rust Axum API server
  frontend/    Leptos/WASM web frontend (+ public/ PWA assets)
  desktop/     Tauri desktop shell
  database/    SQL schema + migrations
  docker/      Dockerfile, docker-compose.yml
  kubernetes/  Kubernetes manifests
```

### Regenerating the shared types

`crates/model` derives `schemars::JsonSchema`; that schema is the source of
truth for the TypeScript types the IDE uses. After changing the model:

```bash
cargo run -p nocode-xtask --quiet -- types        # rewrites model.schema.json
node packages/ide-core/scripts/gen-model-types.mjs --check \
  packages/ide-core/src/types/model.schema.json \
  packages/ide-core/src/types/model.generated.ts
```

CI runs exactly these two steps and fails on drift, so run them before pushing.

## Run a generated application

```bash
# Backend (Axum + SeaORM; driver selected by DATABASE_URL)
cd generated/backend
DATABASE_URL="sqlite://app.db?mode=rwc" cargo run

# Frontend (Leptos -> WebAssembly, served by Trunk on :8080)
cd generated/frontend
trunk serve
```

`trunk serve` builds the WASM bundle, compiles the Tailwind CSS and serves the
app. Point it at a non-default backend with `API_BASE=http://host:port` (read at
compile time).

## Run the desktop IDE (Tauri)

```bash
cd apps/ide-tauri
npm install
npm run tauri:dev
```

`npm run dev` alone starts just the Vite frontend on <http://localhost:1420>;
`npm run tauri:build` produces a bundled desktop binary.

## Tests and checks

```bash
# Rust
cargo test --workspace --lib
cargo test --workspace --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Frontend
npm run typecheck            # ide-core, ide-web, ide-tauri
npm run lint
npm test                     # vitest, packages/ide-core

# Heavy, opt-in: generate a project and actually compile it
cargo test -p nocode-codegen -- --ignored generated_backend_compiles
cargo test -p nocode-codegen -- --ignored generated_frontend_compiles
```

The ignored fixtures are the only check that the generated code *compiles*; the
default suite checks that it is well-formed, escaped and lint-clean.

## Docker

Two images, because the preview subsystem needs a compiler and most deployments
do not want one.

```bash
# Lean: IDE server + web IDE, no Rust toolchain, previews report as unavailable
docker build -t nocode-ide .
docker run -p 8081:8081 -v nocode-data:/data nocode-ide

# Preview: the same, plus cargo, trunk and the wasm32 target (~2 GB)
docker build -f Dockerfile.preview -t nocode-ide:preview .

# Or both through compose (the preview service sits behind a profile)
docker compose up -d
docker compose --profile preview up -d
```

Both images run as an unprivileged `nocode` user, keep the database, signing
keys and preview workspaces on the `/data` volume, and declare a healthcheck
against `/health`. There is no Postgres option: the migrations are SQLite-specific,
so offering one would only produce a container that fails on first boot.

## Documentation

| Document | What it covers |
| --- | --- |
| [docs/user-manual.md](docs/user-manual.md) | Building a CRUD app from sign-in to deployment |
| [docs/System Architecture Diagrams.md](docs/System%20Architecture%20Diagrams.md) | Crate graph, request paths, preview pipeline |
| [docs/Software Requirements Specification.md](docs/Software%20Requirements%20Specification.md) | Functional and non-functional requirements |
| [docs/Data Models.md](docs/Data%20Models.md) | Every model, field and wire shape |
| [docs/UI-UX Wireframes & Mockups.md](docs/UI-UX%20Wireframes%20%26%20Mockups.md) | Screen-by-screen UI reference |
| [docs/verify-preview.md](docs/verify-preview.md) | How to verify the hosted preview path end to end |

**New to Bonafoid?** Start with the user manual; it walks through building a
CRUD application from sign-in to deployment.

## License

Bonafoid is published under the AGPL-3.0-or-later. The full text is in
[LICENSE](LICENSE).

## Security

The hosted preview subsystem compiles and runs user-generated Rust on the host
without container isolation, and a plugin runs with the privileges of the
server. Read [SECURITY.md](SECURITY.md) before exposing the IDE server to anyone
you do not trust.
