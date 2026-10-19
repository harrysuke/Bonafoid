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
            PathBuf::from("docker/docker-compose.yml"),
            self.docker.docker_compose.clone(),
        ));
        files.push((
            PathBuf::from("docker/.dockerignore"),
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
├── database/         # SQL migrations and schema
├── docker/           # Docker configuration
└── kubernetes/       # Kubernetes manifests
```

## Build Targets

| Target  | Command                                    | Output              |
| ------- | ------------------------------------------ | ------------------- |
| Web     | `cd frontend && trunk build --release`     | `frontend/dist/`    |
| PWA     | served from `frontend/dist/` (installs)    | `frontend/public/`  |
| Desktop | `cd desktop/src-tauri && cargo tauri build`| native installer    |

## Quick Start

### Using Docker Compose (Recommended)

```bash
cd docker
docker-compose up -d
```

This starts:
- PostgreSQL database
- Backend API server (port 3000)
- Frontend dev server (port 8080)

### Manual Development

#### Backend

```bash
cd backend
cargo run
```

#### Frontend

Requires the `wasm32-unknown-unknown` target and [Trunk](https://trunkrs.dev):

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cd frontend
trunk serve
```

## Deployment

### Docker

```bash
docker build -t my-app -f docker/Dockerfile .
docker run -p 3000:3000 my-app
```

### Kubernetes

```bash
kubectl apply -f kubernetes/
```

## Environment Variables

Copy `.env.example` to `.env` and configure:

```env
DATABASE_URL=postgres://user:pass@localhost/db
JWT_SECRET=your-secret-key
FRONTEND_URL=http://localhost:8080
```

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

# Build stage
FROM rust:1.78 as builder

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace
COPY backend/Cargo.toml backend/Cargo.lock ./
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

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/{project_name} /app/{project_name}

# Copy frontend assets
COPY frontend/dist ./frontend/dist

# Create non-root user
RUN useradd -r -s /bin/false appuser && chown -R appuser:appuser /app
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

    let docker_compose = format!(
        r#"version: '3.8'

services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: {project_name}
      POSTGRES_PASSWORD: ${{DB_PASSWORD}}
      POSTGRES_DB: {project_name}
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./database/migrations:/docker-entrypoint-initdb.d
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U {project_name}"]
      interval: 10s
      timeout: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5

  backend:
    build:
      context: ..
      dockerfile: docker/Dockerfile
    environment:
      DATABASE_URL: postgres://{project_name}:${{DB_PASSWORD}}@postgres:5432/{project_name}
      REDIS_URL: redis://redis:6379
      JWT_SECRET: ${{JWT_SECRET}}
      FRONTEND_URL: ${{FRONTEND_URL}}
      RUST_LOG: info
    ports:
      - "3000:3000"
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 10s

  frontend:
    image: ghcr.io/trunk-rs/trunk:latest
    working_dir: /app
    volumes:
      - ../frontend:/app
      - cargo_registry:/usr/local/cargo/registry
    ports:
      - "8080:8080"
    environment:
      API_BASE: http://localhost:3000
    command: sh -c "rustup target add wasm32-unknown-unknown && trunk serve --address 0.0.0.0 --port 8080"
    depends_on:
      - backend

volumes:
  postgres_data:
  redis_data:
  cargo_registry:
"#
    );

    let dockerignore = r#"# Git
.git
.gitignore

# Rust
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
docker/
"#
    .to_string();

    Ok(DockerOutput {
        dockerfile,
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
            - name: REDIS_URL
              valueFrom:
                secretKeyRef:
                  name: {project_name}-secrets
                  key: redis-url
            - name: JWT_SECRET
              valueFrom:
                secretKeyRef:
                  name: {project_name}-secrets
                  key: jwt-secret
            - name: FRONTEND_URL
              valueFrom:
                configMapKeyRef:
                  name: {project_name}-config
                  key: frontend-url
            - name: RUST_LOG
              value: "info"
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
          image: nginx:alpine
          ports:
            - containerPort: 80
          volumeMounts:
            - name: frontend-assets
              mountPath: /usr/share/nginx/html
          resources:
            requests:
              memory: "64Mi"
              cpu: "50m"
            limits:
              memory: "128Mi"
              cpu: "100m"
      volumes:
        - name: frontend-assets
          configMap:
            name: {project_name}-frontend-assets
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
  frontend-url: "https://{project_name}.example.com"
  rust-log: "info"
  api-base-path: "/api"
"#
    );

    let secret = format!(
        r#"apiVersion: v1
kind: Secret
metadata:
  name: {project_name}-secrets
  labels:
    {labels}
type: Opaque
stringData:
  database-url: "postgres://user:password@postgres:5432/{project_name}"
  redis-url: "redis://redis:6379"
  jwt-secret: "CHANGE_ME_IN_PRODUCTION"
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
mod tests {
    use super::*;

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
        use nocode_model::entity::{EntityField, EntityFieldType, FieldId};
        use nocode_model::{Entity, Form, Page, Workflow};

        let mut project = Project::new("Artifacts Demo");

        let mut entity = Entity::new("Product");
        entity.fields.push(EntityField {
            id: FieldId::new(),
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
            id: FieldId::new(),
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
