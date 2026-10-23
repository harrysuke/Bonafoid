# Contributing to Bonafoid IDE

Thank you for your interest in contributing! This document explains how to set up
your environment, the conventions we follow, and how to submit changes.

## Table of Contents

- [Getting Started](#getting-started)
  - [Prerequisites](#prerequisites)
  - [Setup](#setup)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Documentation](#documentation)
- [Submitting Changes](#submitting-changes)
- [Code of Conduct](#code-of-conduct)
- [Architecture Notes](#architecture-notes)
- [License](#license)

## Getting Started

### Prerequisites

- Rust (stable 1.75+)
- Node.js 18+ and npm
- wasm32-unknown-unknown target
- Trunk (for generated-app frontends)
- Tauri CLI 2
- WebView2 runtime on Windows

```powershell
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
cargo install --locked tauri-cli --version "^2"
```

### Setup

```powershell
git clone <repository-url>
cd nocode-ide
cd apps/ide-tauri
npm install
cd ../..
cargo test --workspace --lib
```

## Project Structure

- `crates/`
  - `model` — domain model (projects, entities, forms, pages, workflows, themes)
  - `codegen` — Handlebars-based code generation (backend, frontend, database, targets)
  - `plugins` — plugin runtime and manifests
  - `bpmn` — BPMN token engine, expression evaluator, BPMN-XML import/export
  - `ide` — shared IDE services (extracted from Tauri commands)
- `apps/`
  - `ide-tauri` — desktop IDE (Tauri v2 shell + React frontend)
  - `ide-web` — web IDE (server-backed)
- `packages/`
  - `ide-core` — shared UI/logic shared between desktop and web
- `xtask/` — developer CLI (`new`, `generate`)
- `docs/` — specification, architecture, data models, and wireframes

## Development Workflow

1. Create a feature branch: `git checkout -b feat/short-description`
2. Make focused changes
3. Add or update tests
4. Ensure all checks pass (see [Testing](#testing))
5. Update documentation if behavior changes
6. Submit a pull request

Use `feat/` for features and `fix/` for bug fixes. Keep each pull request scoped
to a single change.

## Coding Standards

- **Rust:** run `rustfmt` and `clippy`, prefer `Result`, avoid `unwrap` in library crates
- **TS/React:** strict TypeScript, functional components with hooks, Tailwind for styling
- **Templates:** Handlebars templates live in `crates/codegen/src/templates/`; use the
  `view`/DTO layer and register helpers via the `view` module rather than ad-hoc values
- **Commits:** short imperative subject line, with the affected area as a prefix where
  useful (e.g. `codegen: fix sqlite driver serde`)

## Testing

```powershell
cargo test --workspace --lib
cargo test --workspace -- --ignored generated_backend_compiles
cargo test --workspace -- --ignored generated_frontend_compiles
cd apps/ide-tauri && npx tsc --noEmit && npm run build && cd ../..
```

The two `--ignored` tests compile generated projects end to end and are the primary
guard for code generation changes. Run them before submitting codegen or template work.

## Documentation

- Update the relevant documents under `docs/` (specification, architecture, data models,
  wireframes) when you change behavior or scope
- Bump the document version at the top of a doc when you make substantive edits
- Keep `README.md` accurate for setup and the demo workflow

## Submitting Changes

Before opening a pull request, confirm:

- [ ] `cargo test --workspace --lib` passes
- [ ] Relevant `--ignored` compile tests pass (for codegen/template changes)
- [ ] `npx tsc --noEmit` and `npm run build` pass in `apps/ide-tauri`
- [ ] New behavior is covered by tests
- [ ] Documentation is updated where needed
- [ ] The pull request is focused and has a clear description

## Code of Conduct

This project follows the [Contributor Covenant, version 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/).
By participating, you are expected to uphold this code. Report unacceptable behavior
to the maintainers.

## Architecture Notes

- Web IDE: shared `packages/ide-core` + `crates/ide`; `TauriPlatform` vs `WebPlatform`
- Server: `crates/ide-server` (Axum), SQLite/Postgres, JWT RS256 (HttpOnly cookies + CSRF), Argon2
- Builders: Form, List, UI/Page, Process (BPMN); Data Modeler + generation
- Preview: build and run with a process manager, port allocation, idle teardown;
  served at `/preview/{buildId}/` with an API proxy

## License

No-Code IDE is published under the AGPL-3.0-or-later. By contributing, you agree that
your contributions are licensed under the same terms. Generated applications belong to
their authors.
