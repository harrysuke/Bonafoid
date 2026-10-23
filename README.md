# Bonafoid

An open-source workflow platform IDE that generates web, desktop (Tauri) and mobile
(PWA) applications from visual models, with full BPMN 2.0 workflow support.

This repository contains the Rust core (model, codegen, plugins, BPMN engine,
runtime), the code-generation CLI (`xtask`), a React + Tauri desktop IDE, and a
native egui IDE shell.

## Layout

```
crates/
  model/      Domain models (Form, Entity, Workflow, Page, Theme, Project)
  codegen/    Model -> backend / frontend / database / docker / k8s / targets
  plugins/    wasmtime-based plugin host
  bpmn/       BPMN 2.0 engine (+ optional NAPI bridge); import .bpmn from workspace via xtask/import-bpmn.py
  ide/        egui IDE core (state, commands, backend, app shell)
  ide-server/ Web IDE server (Axum + SeaORM): auth, projects, BPMN, previews
apps/
  ide-web/    Browser IDE (React + Vite, served by ide-server)
  ide-tauri/  Desktop IDE (React + Tauri v2)
xtask/        Build / code-generation CLI
```

## Prerequisites

- Rust (stable) and Cargo
- Node.js 18+ and npm (for the React/Tauri IDE)
- On Windows: WebView2 runtime (bundled with recent Windows)

To build or run a **generated** application you also need:

- The WebAssembly target: `rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev) for the Leptos frontend: `cargo install trunk`

The codegen's `#[ignore]`d compile checks only need the wasm target (they run
`cargo check --target wasm32-unknown-unknown`), not Trunk.

## Build and test the core

```powershell
cargo build --workspace
cargo test --workspace --lib
cargo check --workspace
```

## Generate an application (xtask CLI)

The fastest end-to-end path. Creates a project model, then generates the full
application tree (backend, frontend, database, docker, kubernetes, PWA, desktop).

```powershell
# 1. Scaffold a project model (writes ./playground/my-app/project.json)
cargo run --manifest-path xtask/Cargo.toml -- new "My App" --output .\playground

# 2. Generate the application from the model
cargo run --manifest-path xtask/Cargo.toml -- generate `
  --model .\playground\my-app\project.json --output .\generated
```

Other xtask subcommands: `backend`, `frontend`, `database`, `build-web`,
`build-pwa`, `build-desktop`, `build-tauri`.

Generated output is written under the `--output` directory:

```
generated/
  backend/     Rust Axum API server
  frontend/    Leptos/WASM web frontend (+ public/ PWA assets)
  desktop/     Tauri desktop shell
  database/    SQL schema + migrations
  docker/      Dockerfile, docker-compose.yml
  kubernetes/  Kubernetes manifests
```

## Run the desktop IDE (Tauri)

```powershell
cd apps\ide-tauri
npm install
npm run tauri:dev
```

`npm run dev` alone starts just the Vite web frontend on http://localhost:1420.
`npm run tauri:build` produces a bundled desktop binary.

## Quickstart (web IDE)

```powershell
npm ci && npm run build:web
cargo run -p nocode-ide-server
# open http://127.0.0.1:8081
```

Key environment variables: `DATABASE_URL`, `IDE_PREVIEW_ROOT`, `IDE_PREVIEW_TIMEOUT_SECS`, `IDE_PREVIEW_PARALLEL`, `IDE_PREVIEW_API_BASE`, `IDE_PREVIEW_TRUNK_OFFLINE`, `IDE_PREVIEW_DATABASE_URL`, `IDE_PLUGIN_ROOT`, `PREVIEW_PUBLIC_ORIGIN`.

Note: `packages/ide-core/` is the shared core (stores, pages, platform types); `crates/plugins/` provides the WebAssembly sandbox.

```powershell
npm run build:web
cargo run -p nocode-ide-server
# then open http://127.0.0.1:8081
```

The server hosts the IDE at `/` and the hosted preview/proxy under
`/preview/{buildId}`.

**New to Bonafoid?** [docs/user-manual.md](docs/user-manual.md) walks
through building a CRUD application from sign-in to deployment.

## Run a generated application (backend + Leptos frontend)

Using the output from the `generate` step above:

```powershell
# Backend (Axum + SeaORM; driver selected by DATABASE_URL)
cd generated\backend
$env:DATABASE_URL="sqlite://app.db?mode=rwc"   # or postgres://...
cargo run

# Frontend (Leptos -> WebAssembly; served with Trunk on :8080)
cd generated\frontend
trunk serve
```

`trunk serve` builds the WASM bundle, compiles the Tailwind CSS and serves the
app. Point it at a non-default backend with `API_BASE=http://host:port` (read at
compile time).

## Demo workflow (desktop IDE)

1. `cd apps\ide-tauri && npm install && npm run tauri:dev`
2. The IDE loads a seeded **Sample Commerce** project (Customer / Product /
   Order) backed by SQLite; use the toolbar to **New / Open / Save** a project.
3. Edit the model under **Data Model**, then choose the database driver and
   frontend framework under **Settings**.
4. Open **Generate**, pick an output folder, and click **Generate Code**. This
   invokes the real Rust code generator and writes the backend, Leptos
   frontend, migrations, Docker and Kubernetes files.
5. Run the generated app with the commands in the previous section.

## Verification

```powershell
cargo test --workspace --lib
# Heavy, opt-in: generate a project and compile it.
cargo test -p nocode-codegen -- --ignored generated_backend_compiles
cargo test -p nocode-codegen -- --ignored generated_frontend_compiles
```

## Docker

```cmd
# Docker
docker compose up --build
```

## License

Bonafoid is published under the AGPL-3.0-or-later. The full text is in
[LICENSE](LICENSE).

## Security

The hosted preview subsystem compiles and runs user-generated Rust on the
host, without container isolation. Read [SECURITY.md](SECURITY.md) before
exposing the IDE server to anyone you do not trust.
