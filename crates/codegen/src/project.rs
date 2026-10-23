use anyhow::Result;
use nocode_model::{Project, ProjectId};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use tokio::fs;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[derive(Debug, Default)]
pub struct GeneratedProject {
    pub project_id: ProjectId,
    pub output_dir: PathBuf,
    /// The database driver the artifacts were generated for.
    ///
    /// The deployment files and the README are written in terms of it — a
    /// SQLite project needs no database server, a Postgres one does — so
    /// the generator has to carry it alongside the output.
    pub driver: nocode_model::DatabaseDriver,
    pub backend: BackendOutput,
    pub frontend: FrontendOutput,
    pub database: DatabaseOutput,
    pub docker: DockerOutput,
    pub kubernetes: KubernetesOutput,
    /// PWA and desktop (Tauri) artifacts for multi-target builds.
    pub targets: crate::targets::TargetsOutput,
}

use crate::backend::BackendOutput;
use crate::database::DatabaseOutput;
use crate::frontend::FrontendOutput;

#[derive(Debug, Default)]
pub struct DockerOutput {
    pub dockerfile: String,
    /// The frontend image: a static site served by nginx.
    pub dockerfile_frontend: String,
    pub docker_compose: String,
    pub dockerignore: String,
}

#[derive(Debug, Default)]
pub struct KubernetesOutput {
    pub deployment: String,
    pub service: String,
    pub ingress: String,
    pub configmap: String,
    pub secret: String,
}

pub async fn generate_project(project: &Project, output_dir: &Path) -> Result<GeneratedProject> {
    let mut generated = generate_project_in_memory(project).await?;
    generated.output_dir = output_dir.to_path_buf();
    write_output(&generated).await?;
    Ok(generated)
}

/// Generate every in-memory artifact for `project` without touching the
/// filesystem. The returned [`GeneratedProject::output_dir`] is left empty;
/// callers that need a concrete location set it themselves.
pub async fn generate_project_in_memory(project: &Project) -> Result<GeneratedProject> {
    let empty = Path::new("");
    let mut generated = GeneratedProject {
        project_id: project.id.clone(),
        driver: project.settings.database.driver.clone(),
        ..Default::default()
    };

    // Generate backend (Rust/Axum)
    generated.backend = crate::backend::generate_backend(project, empty).await?;

    // Generate frontend (Leptos/WASM)
    generated.frontend = crate::frontend::generate_frontend(project, empty).await?;

    // Generate database migrations
    generated.database = crate::database::generate_database(project, empty).await?;

    // Generate Docker files
    generated.docker = generate_docker(project, empty).await?;

    // Generate Kubernetes manifests
    generated.kubernetes = generate_kubernetes(project, empty).await?;

    // Generate multi-target artifacts (PWA + desktop/Tauri)
    generated.targets = crate::targets::generate_targets(project);

    Ok(generated)
}

impl GeneratedProject {
    /// The content of one generated artifact, by its relative path.
    ///
    /// Paths are matched with `/` separators so a caller can use the same
    /// string on every platform.
    pub fn file(&self, relative: &str) -> Option<String> {
        self.files()
            .into_iter()
            .find(|(path, _)| path.to_string_lossy().replace('\\', "/") == relative)
            .map(|(_, content)| content)
    }

    /// Flatten every artifact into a deterministic `(relative path, content)`
    /// list. Paths use `/` separators and are sorted for reproducibility.
    pub fn files(&self) -> Vec<(PathBuf, String)> {
        let mut files: Vec<(PathBuf, String)> = Vec::new();

        // Backend (Rust/Axum)
        files.push((
            PathBuf::from("backend/Cargo.toml"),
            self.backend.cargo_toml.clone(),
        ));
        for (path, content) in &self.backend.files {
            files.push((Path::new("backend").join(path), content.clone()));
        }

        // Frontend (Leptos/WASM)
        for (path, content) in &self.frontend.files {
            files.push((Path::new("frontend").join(path), content.clone()));
        }

        // Database
        files.push((
            PathBuf::from("database/schema.sql"),
            self.database.schema_sql.clone(),
        ));
        for migration in &self.database.migrations {
            files.push((
                PathBuf::from(format!("database/migrations/{}_up.sql", migration.version)),
                migration.up_sql.clone(),
            ));
            files.push((
                PathBuf::from(format!(
                    "database/migrations/{}_down.sql",
                    migration.version
                )),
                migration.down_sql.clone(),
            ));
        }

        // Docker
        files.push((
            PathBuf::from("docker/Dockerfile"),
            self.docker.dockerfile.clone(),
        ));
        files.push((
            PathBuf::from("docker/Dockerfile.frontend"),
            self.docker.dockerfile_frontend.clone(),
        ));
        files.push((
            PathBuf::from("docker/docker-compose.yml"),
            self.docker.docker_compose.clone(),
        ));
        files.push((
            PathBuf::from(".dockerignore"),
            self.docker.dockerignore.clone(),
        ));

        // Kubernetes
        files.push((
            PathBuf::from("kubernetes/deployment.yaml"),
            self.kubernetes.deployment.clone(),
        ));
        files.push((
            PathBuf::from("kubernetes/service.yaml"),
            self.kubernetes.service.clone(),
        ));
        files.push((
            PathBuf::from("kubernetes/ingress.yaml"),
            self.kubernetes.ingress.clone(),
        ));
        files.push((
            PathBuf::from("kubernetes/configmap.yaml"),
            self.kubernetes.configmap.clone(),
        ));
        files.push((
            PathBuf::from("kubernetes/secret.yaml"),
            self.kubernetes.secret.clone(),
        ));

        // PWA assets ship with the frontend; desktop shell under `desktop/`.
        for (path, content) in self.targets.pwa.files() {
            files.push((Path::new("frontend").join(path), content));
        }
        for (path, content) in self.targets.desktop.files() {
            files.push((Path::new("desktop").join(path), content));
        }

        // Root files
        files.push((PathBuf::from("README.md"), generate_readme(self)));
        files.push((PathBuf::from(".gitignore"), generate_gitignore()));
        files.push((PathBuf::from("Makefile"), generate_makefile()));

        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    /// The binary artifacts, as `(relative path, bytes)`.
    ///
    /// The PWA icons are PNGs, so they cannot travel through the text-only
    /// [`GeneratedProject::files`] list.
    pub fn binary_files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        for (path, bytes) in &self.targets.pwa.icons {
            files.push((Path::new("frontend").join(path), bytes.clone()));
        }
        files
    }

    /// Encode the whole generated project as an in-memory ZIP archive.
    pub fn to_zip(&self) -> Result<Vec<u8>> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);

        for (path, content) in self.files() {
            let name = path.to_string_lossy().replace('\\', "/");
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file(name, options)?;
            zip.write_all(content.as_bytes())?;
        }

        for (path, bytes) in self.binary_files() {
            let name = path.to_string_lossy().replace('\\', "/");
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file(name, options)?;
            zip.write_all(&bytes)?;
        }

        let cursor = zip.finish()?;
        Ok(cursor.into_inner())
    }
}

async fn write_output(generated: &GeneratedProject) -> Result<()> {
    let base = &generated.output_dir;

    for (path, content) in generated.files() {
        let full_path = base.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(full_path, content).await?;
    }

    for (path, bytes) in generated.binary_files() {
        let full_path = base.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(full_path, bytes).await?;
    }

    Ok(())
}

/// Write the multi-target (PWA + desktop) artifacts under `base`.
///
/// PWA assets are placed alongside the web frontend (`frontend/public/`),
/// and the Tauri shell under `desktop/`.
#[allow(dead_code)]
async fn write_targets(base: &Path, targets: &crate::targets::TargetsOutput) -> Result<()> {
    let frontend_dir = base.join("frontend");
    for (path, content) in targets.pwa.files() {
        let full_path = frontend_dir.join(path);
        fs::create_dir_all(full_path.parent().unwrap()).await?;
        fs::write(full_path, content).await?;
    }

    let desktop_dir = base.join("desktop");
    for (path, content) in targets.desktop.files() {
        let full_path = desktop_dir.join(path);
        fs::create_dir_all(full_path.parent().unwrap()).await?;
        fs::write(full_path, content).await?;
    }

    Ok(())
}

fn generate_readme(generated: &GeneratedProject) -> String {
    // The service list and the DATABASE_URL example follow the project's
    // driver: a SQLite project runs no database server at all.
    let (database_service_line, database_url_example) = match generated.driver {
        nocode_model::DatabaseDriver::SQLite => (String::new(), "sqlite://app.db?mode=rwc"),
        nocode_model::DatabaseDriver::Postgres => (
            "\n- **postgres** - the database, on port 5432".to_string(),
            "postgres://user:pass@localhost/db",
        ),
        nocode_model::DatabaseDriver::MySQL => (
            "\n- **mysql** - the database, on port 3306".to_string(),
            "mysql://user:pass@localhost/db",
        ),
        nocode_model::DatabaseDriver::SqlServer | nocode_model::DatabaseDriver::Oracle => {
            (String::new(), "")
        }
    };
    let service_count = if database_service_line.is_empty() {
        2
    } else {
        3
    };
    format!(
        r#"# {} - Generated No-Code Application

This application was generated from a No-Code IDE project.

## Structure

```
.
├── backend/          # Rust Axum API server
├── frontend/         # Leptos WASM frontend (web + PWA)
│   └── public/       # manifest.webmanifest, sw.js, offline.html
├── desktop/          # Tauri desktop shell (src-tauri/)
├── database/         # SQL schema and migrations
├── docker/           # Dockerfiles and the Compose file
├── kubernetes/       # Kubernetes manifests
├── .dockerignore     # build-context excludes
├── Makefile
└── README.md
```

## Quick Start

### With Docker Compose (recommended)

Run this from the PROJECT ROOT, not from `docker/`:

```bash
docker compose -f docker/docker-compose.yml up -d
```

This starts {service_count} services:

- **backend** - the API server, on port 3000
- **frontend** - the built site, on port 8080{database_service_line}

Open <http://localhost:8080>. The backend creates its own schema on
startup, so there is nothing else to run first.

To stop everything:

```bash
docker compose -f docker/docker-compose.yml down
```

### For local development

#### Backend

```bash
cd backend
cargo run
```

The API listens on <http://127.0.0.1:3000> by default and creates the
schema on startup.

#### Frontend

Requires the `wasm32-unknown-unknown` target and [Trunk](https://trunkrs.dev):

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cd frontend
trunk serve
```

The site is served on <http://localhost:8080>. The frontend calls the
backend through `API_BASE`, which is **baked into the bundle when it is
compiled** - changing it means rebuilding:

```bash
API_BASE=http://127.0.0.1:3000 trunk serve
```

Do not put a trailing slash on `API_BASE`: the client appends the API path
to it directly.

## Environment Variables

The backend reads these. Put them in a `.env` file in `backend/`, or export
them before running it.

| Variable       | Required | Default     | Meaning                                     |
| -------------- | -------- | ----------- | ------------------------------------------- |
| `DATABASE_URL` | no       | project URL | The database connection string              |
| `HOST`         | no       | `127.0.0.1` | The bind address                            |
| `PORT`         | no       | `3000`      | The listen port                             |
| `APP_NAME`     | no       | project name| Used in the startup log line                |
| `RUST_LOG`     | no       | `info`      | `tracing` filter                            |

The default connection string for this project:

```
DATABASE_URL={database_url_example}
```

SQLite needs `?mode=rwc` so the file is created if it is missing.

## Deployment

### Docker

Two images: one for the API, one for the static site.

```bash
# The API
docker build -t my-app -f docker/Dockerfile .
docker run -p 3000:3000 -e DATABASE_URL={database_url_example} my-app

# The frontend. API_BASE is baked in, so it is a build argument.
docker build -t my-app-frontend -f docker/Dockerfile.frontend \
  --build-arg API_BASE=https://api.example.com .
docker run -p 8080:80 my-app-frontend
```

### Kubernetes

Build and push both images first, then apply the manifests:

```bash
docker build -t my-app:latest -f docker/Dockerfile .
docker build -t my-app-frontend:latest -f docker/Dockerfile.frontend \
  --build-arg API_BASE=https://my-app.example.com .
docker push my-app:latest
docker push my-app-frontend:latest

kubectl apply -f kubernetes/
```

Before the Ingress becomes ready you also need a TLS secret for the host
it names:

```bash
kubectl create secret tls my-app-tls --cert=tls.crt --key=tls.key
```

Two more things to change before a real deployment:

- `kubernetes/secret.yaml` ships a placeholder database password.
- The Ingress host and the `API_BASE` the frontend image was built with
  must match.

## Generated at

{}

## Project ID

{}
"#,
        generated.project_id,
        chrono::Utc::now().to_rfc3339(),
        generated.project_id
    )
}

fn generate_gitignore() -> String {
    r#"# Rust
/target
**/*.rs.bk
Cargo.lock

# Node
/node_modules
/dist
/build
*.local

# IDE
.idea
.vscode
*.swp
*.swo

# OS
.DS_Store
Thumbs.db

# Environment
.env
.env.local
.env.*.local

# Logs
*.log
logs/

# Database
*.db
*.sqlite
*.sqlite3

# Docker
.docker/

# Kubernetes
.kube/
"#
    .to_string()
}

fn generate_makefile() -> String {
    r#".PHONY: help dev build test docker-up docker-down k8s-deploy

help:
	@echo "Available commands:"
	@echo "  make dev          - Start development servers"
	@echo "  make build        - Build all packages"
	@echo "  make test         - Run all tests"
	@echo "  make docker-up    - Start Docker Compose"
	@echo "  make docker-down  - Stop Docker Compose"
	@echo "  make k8s-deploy   - Deploy to Kubernetes"

dev:
	cd backend && cargo run &
	cd frontend && trunk serve

build:
	cd backend && cargo build --release
	cd frontend && trunk build --release

test:
	cd backend && cargo test
	cd frontend && cargo test

docker-up:
	cd docker && docker-compose up -d

docker-down:
	cd docker && docker-compose down

k8s-deploy:
	kubectl apply -f kubernetes/
"#
    .to_string()
}

pub async fn generate_docker(project: &Project, _output_dir: &Path) -> Result<DockerOutput> {
    let project_name = &project.slug;

    let dockerfile = format!(
        r#"# Multi-stage Dockerfile for {project_name}
#
# This image contains the BACKEND only. The frontend is a separate image
# (docker/Dockerfile.frontend) because it is a static site, not a server.

# Build stage
FROM rust:1.78 as builder

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the manifest first so the dependency build is cached. The lockfile is
# not copied: the generator does not produce one, and .dockerignore excludes
# it anyway.
COPY backend/Cargo.toml ./
COPY backend/src ./src

# Build dependencies first (caching)
RUN mkdir -p src && echo "fn main() {{}}" > src/main.rs && cargo build --release && rm -rf src

# Copy actual source
COPY backend/src ./src

# Build application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies.
# curl is required: the health check below shells out to it.
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    libssl3 \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/{project_name} /app/{project_name}

# Create non-root user.
# The data directory exists and belongs to the runtime user, so a named
# volume mounted here inherits that ownership and a SQLite backend can
# create its database file.
RUN useradd -r -s /bin/false appuser \
    && mkdir -p /app/data \
    && chown -R appuser:appuser /app
USER appuser

# Expose port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Run
CMD ["./{project_name}"]
"#
    );

    // The frontend is a static site: build the WASM bundle with Trunk and
    // serve it with nginx. API_BASE is baked into the bundle at BUILD time,
    // so it is a build argument rather than a runtime environment variable.
    let dockerfile_frontend = format!(
        r#"# Frontend image for {project_name}.
#
# The Leptos frontend is compiled to WebAssembly and served as static files.
# API_BASE is baked into the bundle when it is compiled, so it must be passed
# as a build argument and the image must be rebuilt when it changes.

FROM rust:1.78 as builder

WORKDIR /app

RUN rustup target add wasm32-unknown-unknown
RUN cargo install trunk --version 0.21.14 --locked

COPY frontend/Cargo.toml ./
COPY frontend/src ./src
COPY frontend/index.html ./index.html
COPY frontend/style ./style
COPY frontend/public ./public
COPY frontend/Trunk.toml ./Trunk.toml

# The bundle's API base. No trailing slash: the client concatenates paths
# directly onto it.
ARG API_BASE=http://127.0.0.1:3000
ENV API_BASE=${{API_BASE}}

RUN trunk build --release

FROM nginx:alpine

COPY --from=builder /app/dist /usr/share/nginx/html

EXPOSE 80

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null http://localhost/ || exit 1

CMD ["nginx", "-g", "daemon off;"]
"#
    );

    // The compose file is DRIVER-AWARE. A SQLite project needs no database
    // server at all, so starting a Postgres for it would be both wrong and
    // a needless thing to install.
    let driver = project.settings.database.driver.clone();
    let (database_service, database_url, backend_depends_on, backend_volumes, compose_volumes) =
        match driver {
            nocode_model::DatabaseDriver::SQLite => (
                String::new(),
                "sqlite://./data/app.db?mode=rwc".to_string(),
                String::new(),
                r#"    volumes:
      - app_data:/app/data
"#
                .to_string(),
                "  app_data:\n".to_string(),
            ),
            nocode_model::DatabaseDriver::Postgres => (
                format!(
                    r#"  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: {project_name}
      POSTGRES_PASSWORD: ${{DB_PASSWORD:-{project_name}}}
      POSTGRES_DB: {project_name}
    volumes:
      - postgres_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U {project_name}"]
      interval: 10s
      timeout: 5s
      retries: 5

"#
                ),
                format!(
                    "postgres://{project_name}:${{DB_PASSWORD:-{project_name}}}@postgres:5432/{project_name}"
                ),
                r#"    depends_on:
      postgres:
        condition: service_healthy
"#
                .to_string(),
                String::new(),
                "  postgres_data:\n".to_string(),
            ),
            nocode_model::DatabaseDriver::MySQL => (
                format!(
                    r#"  mysql:
    image: mysql:8
    environment:
      MYSQL_ROOT_PASSWORD: ${{DB_PASSWORD:-{project_name}}}
      MYSQL_DATABASE: {project_name}
      MYSQL_USER: {project_name}
      MYSQL_PASSWORD: ${{DB_PASSWORD:-{project_name}}}
    volumes:
      - mysql_data:/var/lib/mysql
    ports:
      - "3306:3306"
    healthcheck:
      test: ["CMD", "mysqladmin", "ping", "-h", "127.0.0.1"]
      interval: 10s
      timeout: 5s
      retries: 5

"#
                ),
                format!(
                    "mysql://{project_name}:${{DB_PASSWORD:-{project_name}}}@mysql:3306/{project_name}"
                ),
                r#"    depends_on:
      mysql:
        condition: service_healthy
"#
                .to_string(),
                String::new(),
                "  mysql_data:\n".to_string(),
            ),
            // SeaORM 0.12 has no driver for either, so `generate_cargo_toml`
            // has already refused. This arm exists only so the match is
            // exhaustive.
            nocode_model::DatabaseDriver::SqlServer | nocode_model::DatabaseDriver::Oracle => {
                return Err(anyhow::anyhow!(
                    "no deployment files are generated for {:?}: SeaORM 0.12 has no driver \
                     for it",
                    driver
                ));
            }
        };

    let docker_compose = format!(
        r#"# Local deployment for {project_name}.
#
# Run it from the PROJECT ROOT:
#
#   docker compose -f docker/docker-compose.yml up -d
#
# Relative paths in this file are resolved against `docker/`, which is where
# the file lives, so `../frontend` is the project's frontend directory.
#
# The backend creates its own schema on startup, so the database needs no
# init scripts.

services:
{database_service}  backend:
    build:
      context: ..
      dockerfile: docker/Dockerfile
    image: {project_name}-backend
    environment:
      DATABASE_URL: {database_url}
      RUST_LOG: info
    ports:
      - "3000:3000"
{backend_depends_on}{backend_volumes}    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 10s

  frontend:
    build:
      context: ..
      dockerfile: docker/Dockerfile.frontend
      args:
        # Baked into the WASM bundle, so it must be the URL the BROWSER
        # uses - not the compose service name.
        API_BASE: ${{API_BASE:-http://localhost:3000}}
    image: {project_name}-frontend
    ports:
      - "8080:80"
    depends_on:
      - backend

volumes:
{compose_volumes}"#
    );

    // Docker reads `.dockerignore` from the BUILD CONTEXT root, and the
    // context is the project root (`context: ..` in the compose file), so
    // the file has to live at the project root to have any effect.
    let dockerignore = r#"# Git
.git
.gitignore

# Rust
/target
**/*.rs.bk

# Node
/node_modules
/build
*.local

# Build output (both images build inside the container)
/frontend/dist

# IDE
.idea
.vscode
*.swp
*.swo

# OS
.DS_Store
Thumbs.db

# Environment
.env
.env.local
.env.*.local

# Logs
*.log
logs/

# Database
*.db
*.sqlite
*.sqlite3
"#
    .to_string();

    Ok(DockerOutput {
        dockerfile,
        dockerfile_frontend,
        docker_compose,
        dockerignore,
    })
}

pub async fn generate_kubernetes(
    project: &Project,
    _output_dir: &Path,
) -> Result<KubernetesOutput> {
    let project_name = &project.slug;
    let labels = format!("app: {project_name}");

    let deployment = format!(
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {project_name}
  labels:
    {labels}
spec:
  replicas: 3
  selector:
    matchLabels:
      {labels}
  template:
    metadata:
      labels:
        {labels}
    spec:
      containers:
        - name: {project_name}
          image: {project_name}:latest
          ports:
            - containerPort: 3000
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: {project_name}-secrets
                  key: database-url
            - name: RUST_LOG
              valueFrom:
                configMapKeyRef:
                  name: {project_name}-config
                  key: rust-log
          resources:
            requests:
              memory: "256Mi"
              cpu: "250m"
            limits:
              memory: "512Mi"
              cpu: "500m"
          livenessProbe:
            httpGet:
              path: /health
              port: 3000
            initialDelaySeconds: 10
            periodSeconds: 30
          readinessProbe:
            httpGet:
              path: /ready
              port: 3000
            initialDelaySeconds: 5
            periodSeconds: 10
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {project_name}-frontend
  labels:
    {labels}
    component: frontend
spec:
  replicas: 2
  selector:
    matchLabels:
      {labels}
      component: frontend
  template:
    metadata:
      labels:
        {labels}
        component: frontend
    spec:
      containers:
        - name: frontend
          # Built from docker/Dockerfile.frontend. The bundle's API_BASE is
          # baked in when the image is built, so it must point at the
          # Ingress host.
          image: {project_name}-frontend:latest
          ports:
            - containerPort: 80
          resources:
            requests:
              memory: "64Mi"
              cpu: "50m"
            limits:
              memory: "128Mi"
              cpu: "100m"
"#
    );

    let service = format!(
        r#"apiVersion: v1
kind: Service
metadata:
  name: {project_name}
  labels:
    {labels}
spec:
  type: ClusterIP
  ports:
    - port: 80
      targetPort: 3000
      protocol: TCP
      name: http
  selector:
    {labels}
---
apiVersion: v1
kind: Service
metadata:
  name: {project_name}-frontend
  labels:
    {labels}
    component: frontend
spec:
  type: ClusterIP
  ports:
    - port: 80
      targetPort: 80
      protocol: TCP
      name: http
  selector:
    {labels}
    component: frontend
"#
    );

    let ingress = format!(
        r#"apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: {project_name}
  labels:
    {labels}
  annotations:
    nginx.ingress.kubernetes.io/rewrite-target: /
    cert-manager.io/cluster-issuer: letsencrypt-prod
spec:
  ingressClassName: nginx
  tls:
    - hosts:
        - {project_name}.example.com
      secretName: {project_name}-tls
  rules:
    - host: {project_name}.example.com
      http:
        paths:
          - path: /api
            pathType: Prefix
            backend:
              service:
                name: {project_name}
                port:
                  number: 80
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {project_name}-frontend
                port:
                  number: 80
"#
    );

    let configmap = format!(
        r#"apiVersion: v1
kind: ConfigMap
metadata:
  name: {project_name}-config
  labels:
    {labels}
data:
  # Read by the backend Deployment. Change it to tune the log level
  # (for example "info,sqlx=debug") without editing the Deployment.
  rust-log: "info"
"#
    );

    // The generated backend reads DATABASE_URL, HOST, PORT, APP_NAME and
    // RUST_LOG. It does not read JWT_SECRET, FRONTEND_URL or REDIS_URL, so
    // the manifests do not pretend to provide them.
    //
    // The secret's database URL is DRIVER-AWARE. A SQLite deployment puts
    // its database file on a volume rather than pointing at a server.
    let secret_database_url = match project.settings.database.driver {
        nocode_model::DatabaseDriver::SQLite => "sqlite://./data/app.db?mode=rwc".to_string(),
        nocode_model::DatabaseDriver::Postgres => {
            format!("postgres://{project_name}:CHANGE_ME@postgres:5432/{project_name}")
        }
        nocode_model::DatabaseDriver::MySQL => {
            format!("mysql://{project_name}:CHANGE_ME@mysql:3306/{project_name}")
        }
        // `generate_cargo_toml` has already refused these two.
        nocode_model::DatabaseDriver::SqlServer | nocode_model::DatabaseDriver::Oracle => {
            String::new()
        }
    };
    let secret = format!(
        r#"apiVersion: v1
kind: Secret
metadata:
  name: {project_name}-secrets
  labels:
    {labels}
type: Opaque
stringData:
  # CHANGE THIS before deploying, if it names a database server: it is the
  # password the backend uses to reach the database.
  database-url: "{secret_database_url}"
"#
    );

    Ok(KubernetesOutput {
        deployment,
        service,
        ingress,
        configmap,
        secret,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) mod deployability;

    #[tokio::test]
    async fn writes_multi_target_artifacts() {
        let project = Project::new("Multi Target");
        let targets = crate::targets::generate_targets(&project);
        let dir = std::env::temp_dir().join(format!("nocode-codegen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        write_targets(&dir, &targets).await.expect("write targets");

        assert!(dir.join("frontend/public/manifest.webmanifest").exists());
        assert!(dir.join("frontend/public/sw.js").exists());
        assert!(dir.join("frontend/public/offline.html").exists());
        assert!(dir.join("desktop/tauri.conf.json").exists());
        assert!(dir.join("desktop/src-tauri/Cargo.toml").exists());
        assert!(dir.join("desktop/src-tauri/src/main.rs").exists());

        let manifest =
            std::fs::read_to_string(dir.join("frontend/public/manifest.webmanifest")).unwrap();
        assert!(manifest.contains("Multi Target"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn generate_project_writes_all_artifacts() {
        use nocode_model::entity::{EntityField, EntityFieldId, EntityFieldType};
        use nocode_model::{Entity, Form, Page, Workflow};

        let mut project = Project::new("Artifacts Demo");

        let mut entity = Entity::new("Product");
        entity.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "id".to_string(),
            field_type: EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        entity.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "name".to_string(),
            field_type: EntityFieldType::String {
                max_length: Some(120),
            },
            nullable: false,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        project.entities.insert(entity.id.clone(), entity);

        let form = Form::new("Contact");
        project.forms.insert(form.id.clone(), form);

    let workflow = Workflow::new("Approval");
    // Phase C: BPMN workflow imports from D:\\nocode-ide\\bpmn/
    // Import-map: Car-Wash (Customer), Pizza-Store (Pizza customer),
    // Recruitment-and-Selection (Employee selection), SelfServeRestaurant (Guest food),
    // Smart-Parking (Motorists) — 5 workflows parsed by xml::parse_bpmn.
    project.workflows.insert(workflow.id.clone(), workflow);

        let page = Page::new("Home");
        project.pages.insert(page.id.clone(), page);

        let dir = std::env::temp_dir().join(format!("nocode-gen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let generated = generate_project(&project, &dir)
            .await
            .expect("generate full project");
        assert_eq!(generated.output_dir, dir);

        for relative in [
            "backend/Cargo.toml",
            "backend/src/main.rs",
            "frontend/Cargo.toml",
            "frontend/index.html",
            "frontend/src/main.rs",
            "frontend/src/app.rs",
            "frontend/src/pages/mod.rs",
            "frontend/public/manifest.webmanifest",
            "frontend/public/sw.js",
            "desktop/tauri.conf.json",
            "desktop/src-tauri/Cargo.toml",
            "database/schema.sql",
            "docker/Dockerfile",
            "kubernetes/deployment.yaml",
            "README.md",
            ".gitignore",
            "Makefile",
        ] {
            assert!(
                dir.join(relative).exists(),
                "expected generated file missing: {relative}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
