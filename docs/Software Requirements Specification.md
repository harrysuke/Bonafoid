# Software Requirements Specification (SRS)

## No-Code Visual Application Development Platform

**Version:** 1.3  
**Date:** October 2026  
**Status:** Living document — amended after Phase 6. Requirements below are
marked with their implementation state in §1.5, and the scope, tech stack and
milestone tables have been reconciled with the tree.
**Classification:** Internal / Open Source Product

---

## 1. Introduction

### 1.1 Purpose
This document specifies the requirements for a **No-Code Visual Application Development Platform** that enables users to build full-stack web, desktop, and mobile applications through visual modeling, without writing code. The platform generates production-ready Rust (Axum), Leptos (WASM), SQL, Docker, and Kubernetes artifacts.

The IDE is delivered in two deployment modes:
- **Desktop IDE** (Tauri v2 + React 18 + TypeScript) — the original local-first application.
- **Web IDE** (browser client + Rust Axum server, multi-user) — the same builders served from a hosted backend, with projects persisted server-side and generated apps runnable in a hosted preview (see §3.5).

Both modes share a single code base via `packages/ide-core` and `crates/ide`.

### 1.2 Scope
The product consists of:
- **IDE Applications** (desktop Tauri + web browser) — visual builders for forms, lists, workflows, data models, pages, themes, plugins
- **IDE Server** (Rust/Axum) — multi-user project storage, authentication, code generation API, hosted preview/run
- **Code Generation Engine** (Rust) — transforms visual models into deployable artifacts
- **Generated Application Runtime** (Rust) — no runtime crate exists to link against: the code generator emits a complete Axum + SQLx + SeaORM backend per project (`crates/codegen/src/backend.rs`), so a generated app owns its own auth, storage and workflow code
- **Plugin System** — WASM-based extensibility (wasmtime)
- **BPMN Engine** — `crates/bpmn`, a token-based process engine in Rust, with BPMN 2.0 XML import/export for `bpmn-js` and a NAPI-RS bridge that exposes the engine itself to the TypeScript editor

### 1.3 Target Audience
- **Citizen Developers** — business users building internal tools
- **Professional Developers** — extending platform via plugins, custom code
- **DevOps Engineers** — deploying generated artifacts (Docker, K8s, bare metal)

### 1.4 Definitions & Acronyms
| Term | Definition |
|------|------------|
| BPMN | Business Process Model and Notation 2.0 |
| WASM | WebAssembly |
| PWA | Progressive Web App |
| DSL | Domain-Specific Language |
| CRUD | Create, Read, Update, Delete |
| RBAC | Role-Based Access Control |
| RLS | Row-Level Security |

### 1.5 Implementation Status

The requirement tables describe the target product. This table says which parts
of it exist in the current tree, so a reader is not left to guess. "Shipped"
means covered by tests in `cargo test --workspace` and `npm test`.

| Area | State | Notes |
|------|-------|-------|
| Visual builders (FR-1…FR-6) | Shipped | Entities, forms, pages, themes, workflows; all editors keyboard-reachable |
| BPMN execution (FR-2.2) | Partial | Engine, XML import/export and a run endpoint exist in the IDE server; the generated app does not execute workflows, and there are no timers, boundary events, multi-instance or compensation |
| Code generation (FR-7…FR-9) | Shipped | Backend, frontend, database, Docker, K8s, PWA, Tauri targets |
| Plugin system (FR-10, FR-11) | Shipped | wasmtime host, per-user isolation, manifests validated |
| Accounts & sessions (FR-12) | Shipped | Argon2id, RS256 cookies, CSRF; per-user ownership enforced on every route |
| Rate limiting / lockout (FR-12.6) | Not implemented | Listed as `Should`; no limiter is wired yet |
| Project management (FR-13) | Shipped | CRUD, import/export, duplicate; debounced autosave with an unsaved-changes guard |
| Git integration (FR-9.2) | Not implemented | No git UI and no server-side git; export/import JSON is the only history story |
| Environments (FR-9.3) | Not implemented | `ProjectSettings.environments` exists in the model but no UI edits it |
| Optimistic concurrency (FR-13.5) | Not implemented | Last write wins today |
| Hosted preview (FR-14) | Shipped | Build, run, serve, proxy, idle teardown, per-user quotas, bounded I/O |
| Container isolation (FR-14.6) | Not implemented | Previews run on the host as an unprivileged child process |
| Postgres backend | Not implemented | Migrations are SQLite-specific; SQLite is the only supported database |

---

## 2. Overall Description

### 2.1 Product Perspective
```
┌─────────────────────────────────────────────────────────────┐
│                    NO-CODE IDE (Tauri App)                   │
├──────────────┬──────────────┬──────────────┬────────────────┤
│  Form        │  Workflow    │  Data        │  Page          │
│  Builder     │  Builder     │  Modeler     │  Builder       │
├──────────────┼──────────────┼──────────────┼────────────────┤
│  Theme       │  Plugin      │  Code        │  Project       │
│  Builder     │  Manager     │  Generator   │  Settings      │
└──────────────┴──────────────┴──────────────┴────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│              CODE GENERATION ENGINE (Rust)                   │
│  Model → Handlebars Templates → Axum / Leptos / SQL / Docker │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    GENERATED APPLICATION                     │
│  Backend: Axum + SQLx + SeaORM                              │
│  Frontend: Leptos (WASM) + Tailwind + TanStack Query        │
│  Deploy: Docker / Kubernetes / PWA / Tauri Desktop          │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 User Classes
| Class | Description | Key Activities |
|-------|-------------|----------------|
| **Citizen Developer** | Non-technical business user | Drag-drop forms, configure workflows, design pages, publish apps |
| **Power User** | Technical business analyst | Data modeling, complex workflows, API integrations, theme customization |
| **Professional Developer** | Software engineer | Plugin development, custom code extensions, CI/CD integration |
| **Platform Admin** | IT/DevOps | User management, plugin marketplace, deployment templates, monitoring |
| **Team Member** | Collaborator in a shared web workspace | Sign in, own projects, edit via browser, preview generated apps |

### 2.3 Operating Environment
| Component | Requirement |
|-----------|-------------|
| **IDE Client (Desktop)** | Windows 10/11 (x64), macOS 12+ (ARM64/x64), Linux (AppImage/Flatpak) |
| **IDE Client (Web)** | Modern browsers (Chrome 100+, Firefox 100+, Safari 15+, Edge 100+) |
| **IDE Server** | Linux x64/ARM64 (primary), Windows, macOS — Rust 1.75+, SQLite (default) / PostgreSQL 14+ |
| **Generated Backend** | Linux x64/ARM64, Windows, macOS — Rust 1.75+, SQLite (default)/PostgreSQL 14+/MySQL 8+ |
| **Generated Frontend** | Modern browsers (Chrome 100+, Firefox 100+, Safari 15+, Edge 100+) — WASM |
| **Mobile** | iOS 15+ / Android 12+ via PWA or Tauri v2 mobile (alpha) |
| **Container** | Docker 24+, Kubernetes 1.27+, Helm 3.12+ |

### 2.4 Design Constraints
- **Single Language Stack**: Rust for backend/codegen, TypeScript/React for IDE UI
- **Local-First (Desktop)**: the desktop IDE runs entirely offline; no cloud dependency for modeling
- **Server-Backed (Web)**: the web IDE persists projects server-side, requires authentication, and supports multiple concurrent users
- **Shared Core**: desktop and web clients share `packages/ide-core` (React) and `crates/ide` (Rust service); IDE logic must not depend directly on Tauri APIs
- **Extensibility**: Plugin API via WASM Components (wasmtime + wit-bindgen)
- **BPMN Compliance**: BPMN 2.0 XML interchange, executed by the in-tree Rust engine (`crates/bpmn`) — not by an external Node.js engine
- **Licensing**: AGPL-3.0 for community edition; commercial license for enterprise

---

## 3. Functional Requirements

### 3.1 Core Visual Builders

#### FR-1: Form Builder
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-1.1 | Drag-and-drop field palette (18+ field types: text, textarea, email, number, select, checkbox, radio, date, file, signature, rich text, JSON, relation, computed, section, repeater) | Must |
| FR-1.2 | Grid-based layout editor (CSS Grid/Flex) with responsive breakpoints | Must |
| FR-1.3 | Field validation rules (required, min/max length, pattern, min/max value, email, URL, custom expressions) | Must |
| FR-1.4 | Conditional visibility (show/hide fields based on other field values with operators: equals, not equals, contains, greater than, less than, in, is empty) | Must |
| FR-1.5 | Field permissions (read/write roles per field) | Must |
| FR-1.6 | Real-time preview mode (toggle between edit/preview) | Must |
| FR-1.7 | Form submission handling (file uploads, workflow triggers, email notifications) | Must |
| FR-1.8 | JSON Schema export/import for form definitions | Should |

#### FR-2: Workflow Builder (BPMN 2.0)
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-2.1 | Visual BPMN editor (React Flow) with palette: Start/End Events, User Task, Service Task, Script Task, Business Rule Task, Send/Receive/Manual Task, Exclusive/Inclusive/Parallel/Event-Based Gateways, Sub-Process, Call Activity | Must |
| FR-2.2 | BPMN 2.0 validation, XML import/export and single-shot execution in the **IDE server** (`/api/v1/bpmn/{validate,import,export,run}`) by the in-tree Rust engine (`crates/bpmn`): token-based, with exclusive / inclusive / parallel / event-based gateways, user / service / script / business-rule / send / receive / manual tasks, sub-processes and call activities. The **generated application does not execute workflows** — the generated backend has no workflow routes, so a workflow is a design artifact today. Timers, boundary events, multi-instance, compensation, signals, messages and escalations are not executed | Must |
| FR-2.3 | Visual debugger (step-through, breakpoints, variable inspection, execution history) | Must |
| FR-2.4 | Service Task implementations: HTTP (REST/GraphQL), Function (embedded), Plugin (WASM), External Connector | Must |
| FR-2.5 | User Task assignment (static, expression, candidate groups, due dates, form linking) | Must |
| FR-2.6 | Process variables (scoped: global, process, local, task) with JSON/Object/Array types | Must |
| FR-2.7 | Instance management (start, advance, suspend, resume, cancel, retry, history) | Must |
| FR-2.8 | BPMN XML import/export (BPMN 2.0 interchange) | Should |

#### FR-3: Data Modeler
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-3.1 | Entity-Relationship diagram (ERD) with visual canvas | Must |
| FR-3.2 | Entity definition: fields (13+ types: string, text, integer, bigint, float, decimal, boolean, date, datetime, timestamp, json, jsonb, uuid, enum, bytes, relation), primary keys, indexes, constraints | Must |
| FR-3.3 | Relationship types: BelongsTo, HasOne, HasMany, ManyToMany with cascade options (Cascade, SetNull, Restrict, NoAction, SetDefault) | Must |
| FR-3.4 | Auto-generation: UUID, Nanoid, Increment, Timestamp, Custom expressions | Must |
| FR-3.5 | Row-Level Security (RLS) policies with role-based expressions | Must |
| FR-3.6 | Soft delete, audit logging, timestamps, multi-tenancy toggles | Must |
| FR-3.7 | SQL migration generation (PostgreSQL, MySQL, SQLite) with up/down scripts | Must |
| FR-3.8 | Live SQL preview (DDL + indexes + FKs) | Must |

#### FR-4: Page Builder
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-4.1 | Page composer with device preview (Desktop 1440px, Tablet 768px, Mobile 375px) | Must |
| FR-4.2 | Component library (25+ components: Layout, Basic, Input, Data, Navigation) | Must |
| FR-4.3 | Data binding (variables, API data sources, computed, URL params, localStorage, sessionStorage) | Must |
| FR-4.4 | Event system (click, submit, change, focus, custom) with action chains | Must |
| FR-4.5 | Conditional rendering (expression-based show/hide/else) | Must |
| FR-4.6 | SEO configuration (meta tags, Open Graph, Twitter Cards, structured data, canonical URLs) | Should |
| FR-4.7 | Page permissions (view/edit/admin roles, public access) | Must |
| FR-4.8 | List Builder: derive a data-driven list/table view from an entity (columns, filters, sorting, pagination, row actions, bulk actions) backed by a Database data source | Must |

#### FR-5: Theme Builder
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-5.1 | Design token system (Colors, Typography, Spacing, Borders, Shadows, Transitions, Z-indices, Breakpoints) | Must |
| FR-5.2 | Component theming (base, variants, sizes, states) | Must |
| FR-5.3 | Dark mode strategies (class, media query, data attribute, manual) | Must |
| FR-5.4 | CSS Variables export + Tailwind config generation | Must |
| FR-5.5 | Live preview panel | Must |

### 3.2 Code Generation & Deployment

#### FR-6: Code Generation Pipeline
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-6.1 | Model → Code transformation via Handlebars templates | Must |
| FR-6.2 | Backend generation: Axum 0.7 + SQLx 0.7 + SeaORM 0.12 + Utoipa 4.0 (OpenAPI 3.1) + Tower-HTTP | Must |
| FR-6.3 | Frontend generation: Leptos 0.7 (WASM) + Leptos Router + TanStack Query 5 + Tailwind 3.4 | Must |
| FR-6.4 | Database: SQL migrations (up/down), SeaORM entities | Must |
| FR-6.5 | Docker: Multi-stage Dockerfile, docker-compose.yml, .dockerignore | Must |
| FR-6.6 | Kubernetes: Deployment, Service, Ingress, ConfigMap, Secret manifests (Helm-ready) | Must |
| FR-6.7 | Documentation: README, Makefile, .gitignore, env.example | Must |
| FR-6.8 | Incremental generation (only changed files) | Should |
| FR-6.9 | Custom template override support | Could |

#### FR-7: Multi-Target Deployment
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-7.1 | Web (WASM): Static files + SPA routing, PWA manifest, service worker, CDN-ready | Must |
| FR-7.2 | Desktop (Tauri v2): Native menus, system tray, auto-updater, window state, native dialogs, clipboard, notifications | Must |
| FR-7.3 | Mobile (PWA): Installable, offline sync, push notifications (Web Push), app store ready | Must |
| FR-7.4 | Docker: Multi-arch images, health checks, multi-stage builds, nginx reverse proxy | Must |
| FR-7.5 | Kubernetes: Helm charts, operators, GitOps (ArgoCD/Flux), cert-manager, monitoring (Prometheus/Grafana) | Should |

### 3.3 Plugin System

#### FR-8: WASM Plugin Architecture
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-8.1 | Plugin host (wasmtime 20 + wit-bindgen) with Component Model | Must |
| FR-8.2 | Extension points: Component, Action, DataSource, FieldType, WorkflowNode, AuthProvider, StorageDriver, EmailDriver, Hook, Middleware, Command | Must |
| FR-8.3 | Plugin manifest (name, version, author, permissions, entry points, dependencies) | Must |
| FR-8.4 | Sandboxed execution (capability-based: DB, FS, Network, Auth, Email, Scheduler) | Must |
| FR-8.5 | Marketplace (local registry + remote index, semantic versioning, dependency resolution) | Should |
| FR-8.6 | Hot-reload during development | Should |

### 3.4 Project & Settings

#### FR-9: Project Management
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-9.1 | Project CRUD (create, open, save, save-as, export/import JSON) | Must |
| FR-9.2 | Version control integration (Git: status, diff, commit, push, pull, branches) | Should |
| FR-9.3 | Environment management (dev/staging/prod with separate configs) | Must |

#### FR-10: Project Settings (3 Tabs)
| Tab | Requirements | Priority |
|-----|--------------|----------|
| **General** | Project name (renaming rewrites the slug the crate name derives from), read-only slug, description | Must |
| **Database** | Driver (SQLite / PostgreSQL / MySQL / SQL Server / Oracle — SQLite and PostgreSQL are the only two validated end to end), connection URL, throwaway-database mode | Must |
| **Frontend** | Framework (Leptos only), bundler (Trunk; the other options are listed but the select is disabled) | Must |

Locale, timezone, date/number formats, currency, authentication, API, storage,
email, notification and feature-flag settings still exist in the
`ProjectSettings` model, but no tab edits them and no codegen path reads them,
so the previous eight-tab requirement described controls that would have looked
like configuration while changing nothing. They stay in the model so importing
an older project does not lose data; a tab returns only when the generator
consumes the setting.

#### FR-11: Plugin Manager
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-11.1 | Installed tab: enable/disable, configure, update, uninstall, view details | Must |
| FR-11.2 | Marketplace tab: search, filter, install, ratings, downloads | Should |
| FR-11.3 | Develop tab: plugin scaffolding, type templates, testing guide | Should |

### 3.5 Web IDE & Hosted Platform

#### FR-12: Accounts & Authentication (Web IDE)
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-12.1 | User registration with email + password; Argon2id password hashing | Must |
| FR-12.2 | Login/logout with JWT RS256: short-lived access token (15m) + refresh token (7d) | Must |
| FR-12.3 | Access + refresh tokens delivered as HttpOnly, Secure, SameSite cookies; CSRF protection on state-changing requests | Must |
| FR-12.4 | Session management: list active sessions, revoke individual/all refresh tokens | Should |
| FR-12.5 | Per-user project ownership and isolation (no cross-user access) | Must |
| FR-12.6 | Password policy + login rate limiting/lockout (reuse generated-app policy defaults) | Should |

#### FR-13: Server-Side Project Management (Web IDE)
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-13.1 | Project CRUD over REST, persisted server-side (IDE database) | Must |
| FR-13.2 | Project list/dashboard for the signed-in user (name, last modified, thumbnails) | Must |
| FR-13.3 | Autosave (debounced) of model changes to the server | Must |
| FR-13.4 | Duplicate, rename, delete, export (JSON), and import (JSON) projects | Must |
| FR-13.5 | Optimistic concurrency (version/revision) to prevent lost updates across tabs/devices | Should |
| FR-13.6 | Server exposes the exact same generation semantics as the desktop IDE (shared `crates/ide` service) | Must |

#### FR-14: Hosted Preview & Run
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-14.1 | Generate + build a selected project's backend (`cargo build`) and frontend (`trunk build`) on the server | Must |
| FR-14.2 | Run the built generated app as an isolated local process with an allocated port | Must |
| FR-14.3 | Serve the generated frontend at a per-build URL (`/preview/{buildId}/`) and proxy its `/api` to the app backend | Must |
| FR-14.4 | Process lifecycle management: status, logs, restart, stop, and idle teardown | Must |
| FR-14.5 | Build/run status streamed to the IDE preview panel (logs + health) | Should |
| FR-14.6 | Isolation is process/port based on a trusted host; container-based isolation is a later option | Could |
| FR-14.7 | Builds, runs and build slots are quota'd **per user**, so one account cannot occupy the host | Must |
| FR-14.8 | A build has a wall-clock deadline and is cancelled, not left running, when it expires | Must |
| FR-14.9 | The child runs in its own process group, so stopping a preview also stops what it forked | Must |
| FR-14.10 | The proxy strips session credentials (`Authorization`, `Cookie`, `X-CSRF-Token`, `Origin`, `Referer`) and response `Set-Cookie` | Must |
| FR-14.11 | Every proxy hop has a timeout (connect, handshake, send, body) and oversized request bodies are rejected with 413 | Must |
| FR-14.12 | A preview's stdout/stderr is captured into its own run record (capped), not the server log | Must |
| FR-14.13 | A crashed preview is detected by the reaper and stops holding a quota slot | Must |
| FR-14.14 | Preview workspace paths are validated as UUIDs and contained under the preview root before any deletion | Must |

---

## 4. Non-Functional Requirements

### 4.1 Performance
| Metric | Target |
|--------|--------|
| IDE cold start | < 3 seconds |
| IDE hot reload (codegen) | < 2 seconds for single builder change |
| Generated backend cold start | < 500ms (release mode) |
| Generated frontend WASM size | < 2MB gzipped (framework + app) |
| Generated API latency (p99) | < 100ms (simple CRUD) |
| Workflow engine throughput | > 10,000 instances/sec (single node) |
| Memory (IDE) | < 500MB typical, < 1GB max |
| Memory (Generated backend) | < 100MB idle, < 500MB under load |

### 4.2 Scalability
| Dimension | Target |
|-----------|--------|
| Concurrent desktop IDE users | 1 per installation (single-user desktop app) |
| Concurrent web IDE users | 100+ per server instance (horizontal scaling); SQLite today, Postgres requires rewriting the migrations |
| Concurrent hosted previews | Bounded per user, not per host: `IDE_PREVIEW_PARALLEL` builds and `IDE_PREVIEW_MAX_RUNS` runs each (defaults 2 and 4), with idle teardown |
| Projects per user | 1,000+ |
| Generated app concurrent users | 10,000+ (horizontal scaling via K8s) |
| Workflow instances | 1M+ active |
| Database connections | 100/pool (configurable) |
| File storage | Unlimited (S3/GCS/Azure backend) |

### 4.3 Reliability
| Requirement | Target |
|-------------|--------|
| IDE uptime (crash-free) | 99.9% (auto-save every 30s) |
| Generated app availability | 99.95% (multi-replica, health checks) |
| Data durability | 99.999999999% (PostgreSQL + S3) |
| Backup/RTO/RPO | Daily automated, RTO < 1hr, RPO < 1hr |

### 4.4 Security
| Area | Requirements |
|------|--------------|
| **Authentication** | JWT (RS256), short-lived access (15m) + refresh (7d), HttpOnly Secure cookies, CSRF protection |
| **Authorization** | RBAC (roles → permissions), resource-level policies, RLS at DB layer |
| **Data Protection** | AES-256 at rest (DB, S3), TLS 1.3 in transit, PII encryption |
| **Input Validation** | Zod schemas (frontend), validator crate (backend), SQLx compile-time checks |
| **Dependencies** | cargo-audit, npm audit, SBOM generation |
| **Compliance** | GDPR (right to erasure, portability), SOC2 Type II ready, HIPAA-ready |

### 4.5 Usability
| Requirement | Target |
|-------------|--------|
| Learnability (Citizen Dev) | Build first form in < 10 min |
| Learnability (Power User) | Build first workflow in < 30 min |
| Accessibility | WCAG 2.1 AA (IDE + generated apps). Every canvas node, drag handle and icon-only button is a focusable control with an accessible name; reordering is possible with the keyboard (↑/↓ on the handle); inline forms close on Escape and return focus to their trigger; theme text colours are checked against a 4.5:1 contrast ratio in `themeModel.test.ts` |
| Unsaved work | Autosave is debounced and reports failures; leaving a dirty project (navigation, sign-out, tab close) prompts before discarding |
| Error reporting | Failures surface as one shared toast/detail surface driven by `describeError`, not per-page ad-hoc strings |
| Internationalization | 12 locales built-in, RTL support |
| Documentation | In-app guided tours, contextual help, video tutorials, API reference |

### 4.6 Maintainability
| Metric | Target |
|--------|--------|
| Code coverage (core) | > 80% |
| Dependency freshness | < 30 days behind latest stable |
| Build reproducibility | Lockfile + deterministic builds |
| Technical debt ratio | < 5% |

---

## 5. Interface Requirements

### 5.1 User Interfaces
| Interface | Technology | Spec |
|-----------|------------|------|
| **IDE** | Tauri 2 + React 18 + TypeScript + Tailwind | Responsive, docking layout, keyboard shortcuts, dark/light |
| **Generated Web** | Leptos 0.7 + Tailwind 3.4 | SSR + Hydration, PWA, Responsive (320px–1920px) |
| **Generated Desktop** | Tauri 2 + Leptos | Native menus, tray, auto-update, window state persistence |
| **Mobile (PWA)** | Leptos + Workbox | Install prompt, offline cache, push notifications |

### 5.2 APIs
| API | Protocol | Spec |
|-----|----------|------|
| **Generated REST** | HTTP/1.1 + HTTP/2 | OpenAPI 3.1 (Utoipa), JSON, cursor pagination, filtering, sorting |
| **Generated GraphQL** | HTTP/WS | Async-graphql (optional), subscriptions via WebSocket |
| **Workflow Engine** | HTTP | In-process engine in the generated backend (`crates/codegen`); no external workflow service |
| **Plugin Host** | WASM Component Model | wit-bindgen interfaces (wit files) |
| **Real-time** | WebSocket | TanStack Query + native WS for workflow updates, notifications |

---

## 6. Data Requirements

### 6.1 Core Entities (Project JSON)
```
Project
├── Forms (Map<FormId, Form>)
├── Entities (Map<EntityId, Entity>)
├── Workflows (Map<WorkflowId, Workflow>)
├── Pages (Map<PageId, Page>)
├── Themes (Map<ThemeId, Theme>)
├── Plugins (Vec<PluginConfig>)
├── Environments (Map<String, EnvironmentConfig>)
└── Settings (ProjectSettings)
```

### 6.2 Generated Database Schema
| Table | Purpose |
|-------|---------|
| `users` | Authentication, profiles, roles |
| `sessions` | JWT refresh tokens, device tracking |
| `audit_logs` | Immutable action trail |
| `form_submissions` | Form data + files + status |
| `form_files` | Uploaded file metadata |
| `workflow_instances` | BPMN instance state + variables + history |
| Custom entities | Per Data Modeler definitions |

---

## 7. Implementation Constraints

### 7.1 Technology Stack (Locked)
| Layer | Technology | Version |
|-------|------------|---------|
| **IDE Framework** | Tauri | 2.0 |
| **IDE UI** | React | 18.2 |
| **IDE Language** | TypeScript | 5.3 |
| **IDE Styling** | Tailwind CSS | 3.4 |
| **IDE State** | Zustand | 4.4 |
| **IDE Data Fetch** | TanStack Query | 5.0 |
| **IDE Drag-Drop** | native HTML5 drag events | — |
| **IDE Flow** | React Flow | 11.10 |
| **IDE Editor** | plain `<pre>` / `<textarea>` (no code-editor dependency) | — |
| **IDE Server** | Axum | 0.7 |
| **IDE Server DB** | SeaORM + SQLite (default) / PostgreSQL | 0.12 |
| **IDE Auth** | jsonwebtoken (RS256) + argon2 | latest |
| **IDE Static/Proxy** | tower-http (`fs`, `services`) | 0.5 |
| **IDE Shared Core** | `packages/ide-core` (TS) + `crates/ide` (Rust) | — |
| **Codegen Language** | Rust | 1.75+ |
| **Codegen Templates** | Handlebars | 5.1 |
| **Backend Framework** | Axum | 0.7 |
| **Backend ORM** | SeaORM | 0.12 |
| **Backend DB** | SQLx | 0.7 |
| **Frontend Framework** | Leptos | 0.7 |
| **Plugin Runtime** | wasmtime | 20.0 |
| **WASM Interface** | wit-bindgen | 0.35 |
| **BPMN Engine** | `crates/bpmn` (Rust, in-tree) + NAPI-RS bridge | workspace |

---

## 8. Verification & Validation

### 8.1 Test Strategy
| Level | Tool | Coverage Target |
|-------|------|-----------------|
| **Unit (Rust)** | `cargo test` + `proptest` | > 80% (core crates) |
| **Unit (TypeScript)** | Vitest + React Testing Library | > 70% (UI components) |
| **Integration** | `axum-test` + Testcontainers | Critical paths |
| **E2E (IDE)** | Playwright (Tauri WebView) | Core user journeys |
| **E2E (Generated App)** | Playwright + k6 (load) | Auth, CRUD, Workflow, Deploy |
| **Security** | cargo-audit, npm audit, Trivy | Zero critical/high |

---

## 9. Release Plan

### 9.1 Milestones
| Milestone | Timeline | Deliverables | State |
|-----------|----------|--------------|-------|
| **M1: Foundation** | Weeks 1-3 | Workspace, CI, Tauri shell, model schema, Form Builder | Done |
| **M2: Data & Workflow** | Weeks 4-8 | Data Modeler, Workflow Builder, Page Builder | Done |
| **M3: Theme & Codegen** | Weeks 9-12 | Theme Builder, Codegen pipeline (Backend/Frontend/DB/Docker/K8s) | Done |
| **M4: Plugin & Polish** | Weeks 13-16 | Plugin system, Multi-target build, Settings, Docs | Done (settings is 3 tabs, see FR-10) |
| **M5: Beta** | Weeks 17-20 | Installer, auto-update, marketplace, licensing, hardening | Partial — no marketplace (local-manifest install only); installer and updater are unverified |
| **M6: 1.0 Release** | Week 24 | Signed binaries, marketplace launch, commercial licensing | Not started — no signed build and no tag exist |
| **M7: Web IDE (Alpha)** | Weeks 25-32 | Shared `packages/ide-core` + `crates/ide` service, Axum IDE server, accounts/auth, server-side projects, all 4 builders (incl. List), Data Modeler, codegen API | Done |
| **M8: Hosted Preview (Beta)** | Weeks 33-38 | Build worker, process manager, per-build preview URLs + API proxy, autonomy/hardening, Docker packaging | Done |
| **M9: Web IDE GA** | Week 40 | Multi-user GA, hosted preview, SOC2-ready auth, docs | Partial — preview and auth shipped, docs amended; no SOC 2 audit and no GA tag |

---

## 10. Risks & Mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| **Tauri mobile not production-ready** | High | Medium | Ship PWA first; Tauri mobile v2 as v1.1 |
| **BPMN complexity (full spec)** | High | High | MVP subset first; full in v1.1 |
| **Windows build tooling (MSVC)** | High | High | Document VS Build Tools requirement; WSL2 recommended |
| **WASM plugin ecosystem maturity** | Medium | Medium | Start with host API; community plugins post-launch |
| **Leptos nightly instability** | Medium | Low | Pin nightly version; fallback to Yew if needed |
| **Single-developer velocity** | High | Critical | Ruthless scope control; vertical slices every 2 weeks |
| **Hosting untrusted generated code** | Medium | High | Process/port isolation on trusted host first; container isolation later; build sandboxing |
| **Server build resource exhaustion** | Medium | Medium | Concurrency caps, idle teardown, per-user build quotas |
| **Multi-user data isolation** | Low | High | Enforce ownership at query layer; per-user scoping tests |

---

## 11. Appendices
###  Appendix A: Glossary
(See Section 1.4)

###  Appendix B: Sample Project JSON
```text
{
  "id": "proj_abc123",
  "name": "Customer Portal",
  "slug": "customer-portal",
  "version": "1.0.0",
  "forms": { ... },
  "entities": { ... },
  "workflows": { ... },
  "pages": { ... },
  "themes": { ... },
  "settings": { ... }
}
```

###  Appendix C: Generated Project Structure
```text
<project>/
├── backend/           # Rust Axum API
│   ├── src/           # main, config, db, error, routes, models, handlers, forms
│   ├── Cargo.toml
│   └── migrations/
├── frontend/          # Leptos WASM, built by Trunk (not Vite/npm)
│   ├── src/           # app, layout, pages, api client, types
│   ├── style/         # input.css + tailwind.config.js
│   ├── public/        # manifest.webmanifest, sw.js, offline.html
│   ├── index.html
│   ├── Trunk.toml
│   └── Cargo.toml
├── desktop/           # Tauri v2 shell
│   ├── tauri.conf.json
│   └── src-tauri/
├── database/
│   ├── schema.sql
│   └── migrations/    # <version>_up.sql / <version>_down.sql
├── docker/
│   ├── Dockerfile
│   ├── Dockerfile.frontend
│   └── docker-compose.yml
├── kubernetes/
│   ├── deployment.yaml
│   ├── service.yaml
│   ├── ingress.yaml
│   ├── configmap.yaml
│   └── secret.yaml
├── .dockerignore
├── .gitignore
├── Makefile
└── README.md
```

There is no `package.json`, `vite.config.ts` or npm step in a generated
project: the frontend is Leptos compiled by Trunk.

###  Appendix D: API Example (Generated)
```text
paths:
  /api/v1/users:
    get:
      summary: List users
      parameters:
        - $ref: '#/components/parameters/page'
        - $ref: '#/components/parameters/page_size'
      responses:
        '200':
          description: Paginated users
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserPaginatedResponse'
    post:
      summary: Create user
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateUser'
      responses:
        '201':
          description: Created
```
---

**Document Control**
| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-14 | System Architect | Initial draft |
| 1.1 | 2026-10-16 | Engineering | Add Web IDE (server-backed, multi-user), accounts/auth, hosted preview/run, List builder, shared ide-core, updated tech stack & milestones |
| 1.2 | 2026-10-28 | Engineering | §1.5 implementation-status table; FR-14.7-14.14 for the preview hardening; packaging notes |
| 1.3 | 2026-10-29 | Engineering | §1.2 scope and §2.4 constraints corrected (no runtime crate, no bpmn-server — the engine is in-tree and the generated app does not execute workflows); FR-2.2 and FR-10 rewritten to what ships; §7 and §9.1 tables reconciled; Appendix C matches the emitted tree |