# System Architecture Diagrams

## No-Code Visual Application Development Platform

**Version:** 1.2  
**Date:** October 2026  
**Status:** Living document — amended after Phase 6 (crate graph, codegen
template paths and BPMN execution model reconciled with the tree)  

---

## 1. High-Level System Architecture

```mermaid
graph TB
    subgraph "User Layer"
        CitizenDev["👤 Citizen Developer"]
        PowerUser["🔧 Power User"]
        ProDev["👨‍💻 Professional Developer"]
        Admin["🛡️ Platform Admin"]
    end

    subgraph "IDE Application (Desktop)"
        subgraph "Tauri Shell"
            TauriCore["Tauri 2.0 Runtime"]
            WebView["System WebView (WebKit/Chromium)]"]
        end
        
        subgraph "React Frontend (TypeScript)"
            Routing["React Router"]
            StateMgmt["Zustand Store"]
            UIComponents["UI Component Library (Tailwind CSS)"]
            Builders["Visual Builders"]
            Theme["Theme System"]
        end
    end

    subgraph "Core Workspace (Rust Crates)"
        Model["nocode-model<br/>(Domain Models)"]
        Codegen["nocode-codegen<br/>(Code Generation Engine)"]
        Plugins["nocode-plugins<br/>(WASM Plugin Host)"]
        BPMN["nocode-bpmn<br/>(BPMN Engine + NAPI Bridge)"]
        Ide["nocode-ide<br/>(IDE Core Logic)"]
        IdeServer["nocode-ide-server<br/>(Axum API + Preview Subsystem)"]
    end

    subgraph "Generation Pipeline"
        Handlebars["Handlebars Templates"]
        Templates["Code Templates<br/>(Axum, Leptos, SQL, Docker, K8s)"]
        XtTask["xtask CLI Tool"]
    end

    subgraph "Generated Application"
        subgraph "Backend (Rust)"
            AxumServer["Axum 0.7 Server"]
            SQLx["SQLx + SeaORM ORM"]
            Auth["Authentication (JWT)"]
            WorkflowEngine["BPMN Workflow Engine"]
            API["REST API (OpenAPI/Swagger)"]
        end
        
        subgraph "Frontend (WASM)"
            LeptosApp["Leptos 0.7 SPA"]
            TanStackQuery["TanStack Query 5"]
            TailwindGen["Tailwind CSS 3.4"]
            PWA["PWA + Service Worker"]
        end
        
        subgraph "Database"
            PG["PostgreSQL 14+"]
            MySQL["MySQL 8+"]
            SQLite["SQLite"]
        end
    end

    subgraph "External Systems"
        PluginFiles["Plugin manifests + .wasm files<br/>(local install only — no registry)"]
        Storage["S3/GCS/Azure Blob"]
        Email["SMTP/SendGrid/Mailgun/SES"]
        DockerHub["Container Registry"]
        K8s["Kubernetes Cluster"]
    end

    %% User to IDE
    CitizenDev -->|Drag & Drop| Builders
    PowerUser -->|Configure| Builders
    ProDev -->|Extend| Plugins
    Admin -->|Manage| Ide

    %% IDE Structure
    TauriCore --> WebView
    WebView --> UIComponents
    UIComponents --> Builders
    Builders --> Routing
    Builders --> StateMgmt
    StateMgmt --> Model
    StateMgmt --> Ide
    StateMgmt --> Plugins
    StateMgmt --> Codegen
    StateMgmt --> BPMN

    %% Core Crates Relationships
    Ide --> Model
    Codegen --> Model
    Codegen --> Handlebars
    Codegen --> Templates
    Plugins --> Model
    BPMN --> Model
    IdeServer --> Model
    IdeServer --> Codegen
    IdeServer --> Plugins
    IdeServer --> AxumServer

    %% Code Generation
    Codegen -->|Generates| AxumServer
    Codegen -->|Generates| LeptosApp
    Codegen -->|Generates| SQLx
    Codegen -->|Generates| DockerHub
    Codegen -->|Generates| K8s

    %% Generated App
    AxumServer --> SQLx
    AxumServer --> Auth
    AxumServer --> WorkflowEngine
    SQLx --> PG
    SQLx --> MySQL
    SQLx --> SQLite
    LeptosApp --> API
    LeptosApp --> PWA
    API --> AxumServer
    WorkflowEngine --> BPMN
    Auth --> Email
    AxumServer --> Storage

    %% Plugins — installed from a manifest the user supplies; there is no
    %% registry to fetch from.
    Plugins --> PluginFiles
    Plugins --> WasmHost["WASM Host (wasmtime 20.0)"]

    %% XtTask
    XtTask --> Codegen
    XtTask --> Model
```

---

## 2. System Context Diagram

```mermaid
graph LR
    User["👤 End User"] -->|"Uses Generated Application"| GenApp["Generated App<br/>(Web/Desktop/Mobile)"]
    
    Dev["👨‍💻 No-Code Developer"] -->|"Uses IDE"| NocodeIDE["Bonafoid<br/>(Tauri Desktop App)"]
    
    NocodeIDE -->|"Generates Code"| GenApp
    NocodeIDE -->|"Publishes to"| Deployment["Deployment Targets<br/>(Docker/K8s/Web/Tauri)"]
    
    GenApp -->|"Stores Data"| Database["Database<br/>(SQLite/Postgres/MySQL)"]
    GenApp -->|"Plugin Execution"| WasmRuntime["WASM Runtime<br/>(wasmtime)"]
    
    Admin["🛡️ System Admin"] -->|"Monitors"| Deployment
    Admin -->|"Configures"| Database
```

---

## 3. IDE Architecture (Frontend + Tauri)

```mermaid
graph TD
    subgraph "Tauri 2.0 Layer"
        TauriMain["Rust Backend (src-tauri/src/main.rs)"]
        TauriConfig["tauri.conf.json"]
        Commands["Tauri Commands"]
        Window["Window Management"]
        Events["Event System"]
    end
    
    subgraph "React 18 + Vite Frontend"
        Index["index.html + main.tsx"]
        App["App.tsx (Router)"]
        subgraph "Pages (packages/ide-core/src/pages)"
            Dashboard["Dashboard"]
            FormBuilder["Form Builder"]
            ListBuilder["List Builder"]
            WorkflowBuilder["Workflow Builder"]
            DataModeler["Data Modeler"]
            PageBuilder["Page Builder"]
            ThemeBuilder["Theme Builder"]
            PluginManager["Plugin Manager"]
            CodeGenerator["Code Generator"]
            PreviewPanel["Preview Panel<br/>(build, run, log, proxy)"]
            ProjectSettings["Project Settings"]
        end
        
        subgraph "Components"
            UI["UI Kit (Button, Input, Select, Card)"]
            Forms["DraggableFormField"]
            Flow["React Flow Nodes/Edges"]
            Layout["Layout Components"]
        end
        
        subgraph "State & Utils"
            Zustand["Zustand stores<br/>(projectStore, pluginStore, uiStore)"]
            Utils["Helpers, Types, Constants<br/>(utils/errors, utils/themeModel)"]
            Platform["PlatformApi<br/>(web + tauri implementations)"]
            ReactQuery["TanStack Query"]
        end
    end
    
    TauriMain --> Commands
    TauriMain --> Window
    Commands --> Platform
    Platform --> Pages
    Pages --> Components
    Components --> Zustand
    Zustand --> Utils
    Pages --> ReactQuery
    ReactQuery --> Platform
    
    Index --> App
    App --> Pages
    Pages --> Dashboard
    Pages --> FormBuilder
    Pages --> WorkflowBuilder
    Pages --> DataModeler
    Pages --> PageBuilder
    Pages --> ThemeBuilder
    Pages --> PluginManager
    Pages --> CodeGenerator
    Pages --> ProjectSettings
```

---

## 4. Core Crates Dependency Diagram

```mermaid
graph TD
    Model["nocode-model"] --> Base["External: serde, serde_json, schemars, uuid, chrono, indexmap"]
    
    Ide["nocode-ide"] --> Model
    Ide --> Tauri["External: tauri 2.0"]
    
    Plugins["nocode-plugins"] --> Model
    Plugins --> Wasmtime["External: wasmtime 20.0, wit-bindgen 0.35"]
    
    BPMN["nocode-bpmn"] --> Model
    BPMN --> NAPI["External: napi 2.0, napi-derive 2.0"]
    
    Codegen["nocode-codegen"] --> Model
    Codegen --> Templates["External: handlebars 5.0, quote 1.0, syn 2.0, proc-macro2 1.0, prettyplease 0.2"]
    
    Codegen --> Axum["External: axum 0.7, sqlx 0.7, sea-orm 0.12, utoipa 4.0, tower-http 0.5"]
    Codegen --> Leptos["External: leptos 0.7, leptos_router 0.7"]
    
    XT["xtask"] --> Codegen
    XT --> Model
    XT --> CLI["clap (external)"]
```

---

## 5. Code Generation Flow

```mermaid
flowchart LR
    subgraph "Input (Visual Models)"
        FormModel["Form Model"]
        EntityModel["Entity Model"]
        WorkflowModel["Workflow Model"]
        PageModel["Page Model"]
        ThemeModel["Theme Model"]
        ProjectModel["Project Model"]
    end
    
    subgraph "Codegen Engine (nocode-codegen)"
        Parser["Model Parser"]
        ProjectGen["Project Generator"]
        BackendGen["Backend Generator"]
        FrontendGen["Frontend Generator"]
        DatabaseGen["Database Generator"]
        DockerGen["Docker Generator"]
        K8sGen["Kubernetes Generator"]
    end
    
    subgraph "Handlebars Templates (crates/codegen/src/templates)"
        TBackend["backend/*.rs.hbs<br/>11 files: main, config, db, routes,<br/>model, handler, form + module roots"]
        TFrontend["frontend/*.hbs<br/>13 files: *.rs.hbs, Cargo.toml, index.html,<br/>input.css, tailwind.config.js"]
    end
    
    subgraph "Generated Output"
        OutBackend["backend/<br/>src/, Cargo.toml, migrations/"]
        OutFrontend["frontend/<br/>src/, Cargo.toml, package.json,<br/>tailwind.config.js, vite.config.ts"]
        OutDB["database/<br/>schema.sql, migrations/*.sql"]
        OutDocker["docker/<br/>Dockerfile, docker-compose.yml"]
        OutK8s["kubernetes/<br/>*.yaml manifests"]
        OutDocs["README.md, Makefile, .env.example"]
    end
    
    FormModel --> Parser
    EntityModel --> Parser
    WorkflowModel --> Parser
    PageModel --> Parser
    ThemeModel --> Parser
    ProjectModel --> Parser
    
    Parser --> ProjectGen
    ProjectGen --> BackendGen
    ProjectGen --> FrontendGen
    ProjectGen --> DatabaseGen
    ProjectGen --> DockerGen
    ProjectGen --> K8sGen
    
    BackendGen --> TBackend
    FrontendGen --> TFrontend
    %% Database, Docker and Kubernetes artifacts are written by Rust code
    %% (database.rs, targets.rs), not by templates — there is no
    %% templates/database, templates/docker or templates/kubernetes directory.
    DatabaseGen -.->|writes directly| OutDB
    DockerGen -.->|writes directly| OutDocker
    K8sGen -.->|writes directly| OutK8s
    
    TBackend -->|Render| OutBackend
    TFrontend -->|Render| OutFrontend
    ProjectGen -->|Generates| OutDocs
    
    OutBackend -->|Build with Cargo| CompiledAPI["Compiled API Binary"]
    OutFrontend -->|Build with trunk/cargo-leptos| WASMBundle["WASM Frontend"]
```

---

## 6. BPMN Workflow Architecture

```mermaid
graph TD
    subgraph "Workflow Builder (IDE)"
        BPMNEditor["React Flow BPMN Editor"]
        NodePalette["Node Palette<br/>(Tasks, Events, Gateways)"]
        PropsPanel["Properties Panel"]
        VisualDebug["Visual Debugger"]
        BPMNCanvas["BPMN Canvas"]
    end
    
    subgraph "BPMN Integration Layer"
        BPMNCrate["nocode-bpmn Crate"]
        NAPIBridge["NAPI-RS Bridge"]
        BPMNTypes["BPMN Model Types"]
    end
    
    subgraph "BPMN Execution Engine (crates/bpmn — in-process, Rust)"
        ExecutionEngine["Token-based Process Engine"]
        InstanceManager["Instance Management"]
        TaskManager["User/Service Task Manager"]
        VariableStore["Process Variables Store"]
        HistoryStore["Execution History"]
    end
    
    subgraph "IDE Server Surface"
        BpmnAPI["/api/v1/bpmn/{validate,import,export,run}"]
    end
    
    BPMNEditor --> NodePalette
    BPMNEditor --> BPMNCanvas
    BPMNEditor --> PropsPanel
    BPMNEditor --> VisualDebug
    BPMNCanvas -->|Export BPMN XML/JSON| BPMNCrate
    BPMNCrate --> BPMNTypes
    BPMNCrate --> NAPIBridge
    NAPIBridge -->|JSON in / JSON out| ExecutionEngine
    ExecutionEngine --> InstanceManager
    ExecutionEngine --> TaskManager
    ExecutionEngine --> VariableStore
    ExecutionEngine --> HistoryStore
    
    %% The engine is reached over HTTP by the IDE server, and over NAPI by the
    %% desktop shell. The generated app has no workflow routes, so there is no
    %% "generated app runtime" edge here.
    BpmnAPI --> ExecutionEngine
    InstanceManager -->|JSON summary| BpmnAPI
    TaskManager -->|task list| BpmnAPI
    TaskManager --> ServiceTasks["Service Task Executors<br/>(HTTP, function)"]
    ServiceTasks --> HTTP["External HTTP APIs"]
```

---

## 7. Plugin System Architecture

```mermaid
graph TD
    subgraph "Plugin Host (nocode-plugins)"
        Host["WASM Plugin Host"]
        ManifestParser["Manifest Parser (plugin.toml)"]
        CapabilityManager["Capability Manager"]
        Sandbox["Security Sandbox"]
        Registry["Local Plugin Registry"]
        ExtensionPoints["Extension Point Registry"]
    end
    
    subgraph "WASM Runtime"
        Wasmtime["wasmtime 20.0<br/>(Component Model)"]
        WitBindgen["wit-bindgen 0.35<br/>(WIT Interfaces)"]
        Components["WASM Components"]
    end
    
    subgraph "Extension Points"
        EP_Component["Component Extension"]
        EP_Action["Action Extension"]
        EP_DataSource["Data Source Extension"]
        EP_FieldType["Field Type Extension"]
        EP_WorkflowNode["Workflow Node Extension"]
        EP_Auth["Auth Provider Extension"]
        EP_Storage["Storage Driver Extension"]
        EP_Email["Email Driver Extension"]
        EP_Hook["Lifecycle Hook"]
        EP_Middleware["HTTP Middleware"]
        EP_Command["Tauri Command Extension"]
    end
    
    subgraph "IDE Integration"
        PluginManagerUI["Plugin Manager UI"]
        DevTools["Plugin DevTools"]
        HotReload["Hot Reload"]
    end
    
    Host --> ManifestParser
    Host --> CapabilityManager
    Host --> Sandbox
    Host --> Registry
    Host --> ExtensionPoints
    Host --> Wasmtime
    Wasmtime --> WitBindgen
    Wasmtime --> Components
    Components --> ExtensionPoints
    
    ExtensionPoints --> EP_Component
    ExtensionPoints --> EP_Action
    ExtensionPoints --> EP_DataSource
    ExtensionPoints --> EP_FieldType
    ExtensionPoints --> EP_WorkflowNode
    ExtensionPoints --> EP_Auth
    ExtensionPoints --> EP_Storage
    ExtensionPoints --> EP_Email
    ExtensionPoints --> EP_Hook
    ExtensionPoints --> EP_Middleware
    ExtensionPoints --> EP_Command
    
    PluginManagerUI --> Host
    DevTools --> HotReload
    HotReload --> Wasmtime
    CapabilityManager -->|Enforces| Sandbox
```

---

## 8. Data Model Architecture

```mermaid
erDiagram
    PROJECT ||--o{ FORM : contains
    PROJECT ||--o{ ENTITY : contains
    PROJECT ||--o{ WORKFLOW : contains
    PROJECT ||--o{ PAGE : contains
    PROJECT ||--o{ THEME : contains
    PROJECT ||--o{ ENVIRONMENT : has
    PROJECT ||--o{ PLUGIN_CONFIG : configures
    
    FORM ||--o{ FORM_FIELD : has
    FORM ||--o{ FORM_SUBMISSION : receives
    
    ENTITY ||--o{ ENTITY_FIELD : has
    ENTITY ||--o{ RELATIONSHIP : relates
    RELATIONSHIP }|--|| ENTITY : targets
    
    WORKFLOW ||--o{ WORKFLOW_NODE : contains
    WORKFLOW ||--o{ WORKFLOW_EDGE : contains
    WORKFLOW ||--|| WORKFLOW_SETTINGS : has
    WORKFLOW_NODE ||--|| WORKFLOW_NODE_TYPE : of_type
    WORKFLOW_NODE ||--o{ WORKFLOW_EDGE : source_target
    
    PAGE ||--o{ PAGE_COMPONENT : has
    PAGE_COMPONENT ||--|| COMPONENT_TYPE : of_type
    PAGE_COMPONENT ||--o{ DATA_BINDING : has
    PAGE_COMPONENT ||--o{ COMPONENT_EVENT : has
    
    THEME ||--|| DESIGN_TOKENS : defines
    THEME ||--|| COMPONENT_THEMING : defines
    THEME ||--|| DARK_MODE_CONFIG : defines
    
    WORKFLOW_INSTANCE ||--|| WORKFLOW : instance_of
    WORKFLOW_INSTANCE ||--o{ TASK_INSTANCE : has
    WORKFLOW_INSTANCE ||--o{ VARIABLE : holds
    WORKFLOW_INSTANCE ||--o{ HISTORIC_ACTIVITY : tracks
    
    USER ||--o{ SESSION : has
    USER ||--o{ ROLE : assigned
    ROLE ||--o{ PERMISSION : grants
```

---

## 9. Deployment Architecture (Multi-Target)

```mermaid
graph TD
    subgraph "Build Targets"
        Source["Source: Project JSON + Models"]
        Gen["Code Generation<br/>(nocode-codegen)"]
    end
    
    subgraph "Target 1: Web (PWA)"
        Gen --> WebBuild["Build Leptos Frontend<br/>(trunk/cargo-leptos build --release)"]
        Gen --> APIBuild["Build Axum API<br/>(cargo build --release)"]
        WebBuild --> Static["Static Files<br/>(dist/, .wasm)"]
        Static --> CDN["CDN/Static Hosting<br/>(Vercel/Netlify/Cloudflare Pages)"]
        APIBuild --> APIDeploy["REST API<br/>(Railway/Render/Fly.io)"]
    end
    
    subgraph "Target 2: Desktop (Tauri)"
        Gen --> TauriBuild["cargo tauri build<br/>(Windows/macOS/Linux)"]
        TauriBuild --> MSI["Windows Installer (.msi)"]
        TauriBuild --> DMG["macOS Disk Image (.dmg)"]
        TauriBuild --> AppImage["Linux AppImage"]
        TauriBuild --> DMG_App[".app Bundle"]
    end
    
    subgraph "Target 3: Docker (Containerized)"
        Gen --> DockerBuild["docker build -t app:latest"]
        DockerBuild --> Image["Multi-Arch Docker Image"]
        Image --> Compose["docker-compose.yml<br/>(app + postgres + redis + nginx)"]
        Image --> Registry["Container Registry<br/>(Docker Hub/GHCR/ECR)"]
    end
    
    subgraph "Target 4: Kubernetes (Production)"
        Gen --> K8sManifests["Kubernetes Manifests<br/>(Deployment, Service, Ingress,<br/>ConfigMap, Secret, HPA)"]
        K8sManifests --> Helm["Helm Chart (optional)"]
        K8sManifests --> Cluster["K8s Cluster<br/>(EKS/GKE/AKS/On-Prem)"]
        Cluster --> Ingress["NGINX Ingress<br/>+ cert-manager + TLS"]
        Cluster --> PV["Persistent Volumes<br/>(Postgres, File Storage)"]
        Cluster --> Monitor["Monitoring<br/>(Prometheus + Grafana)"]
    end
    
    subgraph "Target 5: Mobile PWA"
        WebBuild --> PWAConfig["PWA Manifest + Service Worker"]
        PWAConfig --> AppStore["App Stores<br/>(PWA Builder/TWA)"]
    end
    
    Source --> Gen
```

---

## 10. Web IDE Architecture

### 10.1 Platform Split (Desktop vs Web)

```mermaid
graph TB
    subgraph "Shared Core"
        IdeCore["packages/ide-core (React)"]
        IdeRust["crates/ide (Rust service)"]
        Model["crates/model"]
        Codegen["crates/codegen"]
    end
    
    subgraph "Desktop IDE"
        TauriShell["apps/ide-tauri (Tauri v2)"]
        TauriCmds["Tauri Commands"]
        TauriPlatform["TauriPlatform (native dialogs/invoke)"]
    end
    
    subgraph "Web IDE"
        WebApp["apps/ide-web (Browser)"]
        IdeServer["crates/ide-server (Axum)"]
        WebPlatform["WebPlatform (REST fetch)"]
        AuthMW["Auth + CSRF middleware"]
        DB["IDE DB: SQLite/Postgres"]
    end
    
    IdeCore --> IdeRust
    IdeRust --> Model
    IdeRust --> Codegen
    
    TauriShell --> IdeCore
    TauriShell --> TauriPlatform
    TauriPlatform --> TauriCmds
    TauriCmds --> IdeRust
    
    WebApp --> IdeCore
    WebApp --> WebPlatform
    WebPlatform -->|"REST/JSON (cookies)"| IdeServer
    IdeServer --> AuthMW
    AuthMW --> IdeRust
    IdeServer --> DB
```

### 10.2 Web IDE High-Level

```mermaid
graph TB
    Browser["Browser (Web IDE)"] -->|"HTTPS"| LB["Load Balancer"]
    LB --> IdeServer["ide-server (Axum)"]
    subgraph "Server"
        IdeServer --> Auth["Auth: JWT RS256 + Argon2"]
        IdeServer --> Projects["Projects API (CRUD, autosave)"]
        IdeServer --> GenAPI["Generation API"]
        IdeServer --> Preview["Preview/Run Manager"]
        IdeServer --> BuildWorker["Build Worker"]
        IdeServer --> ProcMgr["Process Manager (port alloc, PIDs, idle teardown)"]
    end
    Projects --> DB["SQLite/Postgres (users, projects, refresh_tokens)"]
    GenAPI --> IdeRust["crates/ide::service"]
    BuildWorker -->|"cargo build + trunk build"| Dist["dist/ per buildId"]
    ProcMgr -->|"spawn generated app"| AppProc["Isolated OS Process (localhost:PORT)"]
    Preview -->|"serve /preview/{buildId}/ + proxy /api"| Browser
    Preview --> AppProc
```

### 10.3 Auth Flow (JWT RS256 + HttpOnly Cookies)

```mermaid
sequenceDiagram
    participant B as Browser
    participant S as ide-server
    participant DB as IDE DB
    B->>S: POST /api/v1/auth/register (email, password)
    S->>DB: create user (Argon2id hash)
    S-->>B: 201; set HttpOnly access/refresh cookies + CSRF token
    B->>S: POST /api/v1/auth/login
    S->>DB: verify hash, revoke old refresh (optional)
    S-->>B: 200 + HttpOnly cookies (15m access, 7d refresh)
    B->>S: GET /api/v1/projects (with cookies + CSRF header)
    S->>S: validate JWT + CSRF
    S->>DB: scope by user_id
    S-->>B: 200 list
    B->>S: POST /api/v1/auth/refresh (cookie refresh)
    S->>DB: validate refresh token (rotation, jti)
    S-->>B: rotate cookies
    B->>S: POST /api/v1/auth/logout
    S->>DB: revoke refresh token
    S-->>B: clear cookies
```

### 10.4 Hosted Preview/Run Flow

Building and running are two separate steps: a build is cheap to keep, a
running process is not, so the IDE starts a process only when someone asks for
one.

```mermaid
sequenceDiagram
    participant B as Browser (Web IDE)
    participant S as ide-server
    participant BW as Build Worker
    participant PM as Run Manager
    participant App as Generated App (child process)
    B->>S: POST /api/v1/projects/{id}/preview/build (+ CSRF)
    S->>BW: enqueue (202 Accepted, status = queued)
    Note over BW: per-user build slot<br/>workspace under IDE_PREVIEW_ROOT
    BW->>BW: cargo build (backend), trunk build (frontend, API_BASE injected)
    B->>S: GET /api/v1/builds/{id} (poll status + log)
    B->>S: POST /api/v1/builds/{id}/run
    S->>PM: start (alloc free port, spawn with cleared env + own process group)
    PM->>App: launch
    PM-->>S: status = starting, logs streamed from the child's stdout/stderr
    B->>S: GET /preview/{buildId}/ (static dist)
    S-->>B: index.html + WASM + assets
    B->>S: /preview/{buildId}/api/* (proxied)
    S->>App: forward to 127.0.0.1:{port} (timeouts on connect/handshake/send/body)
    App-->>B: response
    B->>S: POST /api/v1/builds/{id}/stop
    S->>PM: kill process group, wait up to 5s, drop the record
```

### 10.5 Preview Hardening

The preview subsystem runs code the platform did not write, on the same host as
the IDE server, so the boundaries are explicit:

| Boundary | Implementation |
|---------|----------------|
| Who may build/run/serve | Every preview route is owner-scoped; a build record carries `user_id` and is invisible to anyone else |
| Resource quotas | `IDE_PREVIEW_PARALLEL` builds and `IDE_PREVIEW_MAX_RUNS` running processes, counted **per user**, so one account cannot starve the host |
| Cargo target isolation | One target directory per user (`IDE_PREVIEW_ROOT/.cargo-target/<user>`), so two users' builds cannot overwrite each other's binary |
| Path safety | Build ids must parse as UUIDs; `manager::is_within` checks lexical containment under the preview root before any `remove_dir_all` |
| Process lifetime | Child in its own process group (`setpgid` / `CREATE_NEW_PROCESS_GROUP`); `stop` kills the group and waits, reporting failure if the process survives |
| Child environment | `env_clear` plus an allowlist, so `DATABASE_URL`, `JWT_KEYS_DIR` and friends never reach generated code |
| Proxy credentials | The request hop drops `Authorization`, `Cookie`, `X-CSRF-Token`, `Origin`, `Referer`; the response hop drops `Set-Cookie` |
| Proxy limits | 8 MiB request body (`413 Payload Too Large`), and connect/handshake/send/body timeouts (`504 Gateway Timeout`) with the upstream connection aborted |
| Output capture | The child's stdout/stderr are piped into its run record (last 500 lines, flushed every 250 ms) instead of the server's log |
| Crash detection | The reaper refreshes active runs before reaping, so a preview that died frees its quota slot instead of holding it forever |

---

## 11. Security Architecture

> This diagram is the **target** security model, not a description of the
> current tree. Shipped today: JWT (RS256) with HttpOnly cookies, CSRF
> double-submit, owner-scoped authorization on every route, request validation
> on names and preview ids, the WASM plugin sandbox with manifest permissions,
> and the preview boundaries in §10.5. Not shipped: MFA, RBAC roles, RLS,
> rate limiting, AES-256 at rest, audit logging, secrets management. See the
> SRS §1.5 for the requirement-by-requirement status.

```mermaid
graph TD
    subgraph "Authentication & Authorization"
        JWT["JWT (RS256)<br/>Access: 15min, Refresh: 7d"]
        MFA["Multi-Factor Auth<br/>(TOTP/SMS/Email/WebAuthn)"]
        RBAC["Role-Based Access Control<br/>(Roles → Permissions)"]
        RLS["Row-Level Security (DB)"]
        Session["Secure Session Management<br/>(HttpOnly, Secure, SameSite)"]
    end
    
    subgraph "Input & Data Security"
        Validation["Input Validation<br/>(Zod + validator crate)"]
        Sanitization["HTML/SQL Sanitization"]
        Encryption["AES-256 Encryption<br/>(At Rest)"]
        TLS["TLS 1.3<br/>(In Transit)"]
        PII["PII Protection"]
    end
    
    subgraph "Plugin Security"
        Sandbox["WASM Sandbox (wasmtime)"]
        Capabilities["Capability-based Permissions"]
        ManifestPerm["Plugin Manifest Permissions"]
        ResourceLimits["CPU/Memory/Time Limits"]
        NetworkPolicy["Network Access Control"]
    end
    
    subgraph "API Security"
        CORS["CORS Policy"]
        RateLimit["Rate Limiting"]
        CSRF["CSRF Protection"]
        APIKeys["API Key Authentication"]
        AuditLog["Audit Logging<br/>(Immutable)"]
        Helmet["Security Headers"]
    end
    
    subgraph "Infrastructure Security"
        Secrets["Secrets Management<br/>(Vault/SealedSecrets)"]
        SBOM["Software Bill of Materials"]
        Scanning["Dependency Scanning<br/>(cargo-audit, npm audit, Trivy)"]
        RBAC_K8s["K8s RBAC"]
        NetworkSeg["Network Segmentation"]
        Backup["Encrypted Backups"]
    end
    
    Client["Client Request"] --> CORS
    CORS --> Helmet
    Helmet --> RateLimit
    RateLimit --> CSRF
    CSRF --> Validation
    Validation --> AuthCheck{"Authenticated?"}
    AuthCheck -->|Yes| RBAC
    AuthCheck -->|No| JWT
    JWT --> MFA
    MFA --> RBAC
    RBAC --> RLS
    RLS --> API["API Handler"]
    API --> AuditLog
    API --> Encryption
    
    Plugins --> Sandbox
    Sandbox --> Capabilities
    Capabilities --> ManifestPerm
    ManifestPerm --> ResourceLimits
    ResourceLimits --> NetworkPolicy
```

---

## 12. Technology Stack Diagram

```mermaid
mindmap
  root((No-Code Platform))
    IDE Layer
      Tauri 2.0
      React 18
      TypeScript 5.3
      Vite 5.x
      Tailwind CSS 3.4
      Zustand 4.4
      TanStack Query 5.x
      React Flow 11.x
      Lucide React
    Core Crates
      Rust 1.75+
      serde/serde_json
      schemars
      uuid/chrono
      indexmap
      thiserror/anyhow
      walkdir
    Code Generation
      Handlebars 5.0
      quote 1.0
      syn 2.0
      proc-macro2 1.0
      prettyplease 0.2
    Backend Runtime
      Axum 0.7
      SQLx 0.7
      SeaORM 0.12
      Utoipa 4.0
      Tower-HTTP 0.5
      Tokio 1.0
      jsonwebtoken
      argon2
    Frontend Generated
      Leptos 0.7
      Leptos Router 0.7
      TanStack Query 5.x
      Tailwind CSS 3.4
      WASM
    Plugins & Extensibility
      wasmtime 20.0
      wit-bindgen 0.35
      Component Model
    BPMN
      nocode-bpmn (in-tree Rust engine)
      napi 2.0
      napi-derive 2.0
    DevOps & Build
      cargo
      cargo-dist
      Docker
      Kubernetes
      Helm
      xtask CLI
```

---

## 13. Request Flow (Complete User Journey)

```mermaid
sequenceDiagram
    participant U as User (Developer)
    participant IDE as Bonafoid
    participant Model as Model Store
    participant CG as Code Generator
    participant FS as File System
    participant Build as Build System
    participant App as Generated App
    
    U->>IDE: Create New Project
    IDE->>Model: Initialize Project Model (JSON)
    Model-->>IDE: Project Created
    
    U->>IDE: Design Data Model (ERD)
    IDE->>Model: Add Entities & Relations
    Model-->>IDE: Updated Schema
    
    U->>IDE: Build Form UI
    IDE->>Model: Add Form Fields & Rules
    Model-->>IDE: Form Config Saved
    
    U->>IDE: Design Workflow (BPMN)
    IDE->>Model: Add Nodes, Edges, Tasks
    Model-->>IDE: BPMN Model Saved
    
    U->>IDE: Create Pages
    IDE->>Model: Compose Components & Bind Data
    Model-->>IDE: Page Layout Saved
    
    U->>IDE: Customize Theme
    IDE->>Model: Set Design Tokens
    Model-->>IDE: Theme Config Saved
    
    U->>IDE: Click "Generate Code"
    IDE->>CG: Request Code Generation
    CG->>Model: Read All Models
    CG->>FS: Generate Backend (Axum)
    CG->>FS: Generate Frontend (Leptos)
    CG->>FS: Generate DB Migrations
    CG->>FS: Generate Docker/K8s
    FS-->>CG: Files Written
    CG-->>IDE: Generation Complete
    
    U->>IDE: Click "Build & Run"
    IDE->>Build: cargo build (backend)
    IDE->>Build: trunk build (frontend WASM)
    Build-->>IDE: Build Success
    IDE->>App: Launch Generated App
    App-->>U: Running at localhost:8080
    
    U->>App: Test CRUD Operations
    App->>DB: Query/Mutate
    DB-->>App: Results
    App-->>U: UI Updates
```

---

**Document Control**

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-15 | System Architect | Initial Draft - Complete system architecture diagrams for Bonafoid platform |
| 1.1 | 2026-10-16 | Engineering | Add Web IDE platform split, server/auth/preview/run diagrams, hosted preview architecture |
| 1.2 | 2026-10-29 | Engineering | §10.4 preview sequence redrawn and §10.5 trust boundaries added (Phases 4-5); then the crate graph, codegen template paths, builder list and BPMN execution model reconciled with the tree — `nocode-runtime` and `apps/generated-app` removed, `nocode-ide-server` added, no external bpmn-server |