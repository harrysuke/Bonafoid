# No-Code IDE server — lean image.
#
# This image runs the IDE server and serves the web IDE, but it has NO Rust
# toolchain and no `trunk`, so it cannot compile a preview. That is a deliberate
# choice, not an oversight: the preview subsystem builds and runs user-generated
# Rust on the host, which needs cargo, trunk and the wasm32 target (about 2 GB
# of image). Shipping them in the default image would mean every deployment
# carries a compiler whether or not it wants hosted previews.
#
# Use `Dockerfile.preview` when previews are wanted; it is this image plus the
# toolchain. `GET /api/v1/preview/capabilities` reports what the running host
# can actually do, and the IDE shows that honestly instead of failing a build.
#
# Build:  docker build -t nocode-ide .
# Run:    docker run -p 8081:8081 -v nocode-data:/data nocode-ide

# --- stage 1: the web IDE (static files served by the server) ----------------
FROM node:20-bookworm-slim AS web
WORKDIR /src

# The workspace manifests first, so a source-only change does not reinstall.
COPY package.json package-lock.json ./
COPY packages/ide-core/package.json packages/ide-core/
COPY apps/ide-web/package.json apps/ide-web/
COPY apps/ide-tauri/package.json apps/ide-tauri/
RUN npm ci --ignore-scripts

COPY packages ./packages
COPY apps/ide-web ./apps/ide-web
RUN npm run build:web

# --- stage 2: the Rust server ------------------------------------------------
FROM rust:bookworm AS server
WORKDIR /src

# Build the dependency graph before the sources so a code change only relinks.
# Every workspace member's manifest has to be present: cargo refuses to load a
# workspace whose `members` list points at a missing Cargo.toml, even when the
# crate being built does not depend on it.
COPY Cargo.toml Cargo.lock ./
COPY crates/model/Cargo.toml crates/model/
COPY crates/codegen/Cargo.toml crates/codegen/
COPY crates/bpmn/Cargo.toml crates/bpmn/
COPY crates/plugins/Cargo.toml crates/plugins/
COPY crates/ide/Cargo.toml crates/ide/
COPY crates/ide-server/Cargo.toml crates/ide-server/
COPY xtask/Cargo.toml xtask/
RUN set -eux; \
    for crate in model codegen bpmn plugins ide ide-server; do \
      mkdir -p "crates/$crate/src"; \
      echo "" > "crates/$crate/src/lib.rs"; \
    done; \
    mkdir -p xtask/src; \
    echo "fn main() {}" > xtask/src/main.rs; \
    echo "fn main() {}" > crates/ide-server/src/main.rs; \
    cargo build --release -p nocode-ide-server; \
    rm -rf crates/*/src xtask/src

COPY crates ./crates
COPY xtask ./xtask
RUN cargo build --release -p nocode-ide-server

# --- stage 3: runtime --------------------------------------------------------
FROM debian:bookworm-slim

# ca-certificates for outbound TLS (GitHub, package registries); curl for the
# container healthcheck. Nothing else is installed.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl; \
    rm -rf /var/lib/apt/lists/*; \
    useradd --system --create-home --home-dir /app --shell /usr/sbin/nologin nocode; \
    mkdir -p /data; \
    chown -R nocode:nocode /app /data

COPY --from=server /src/target/release/nocode-ide-server /usr/local/bin/nocode-ide-server
COPY --from=web /src/apps/ide-web/dist /app/web

WORKDIR /app
USER nocode

ENV PORT=8081 \
    HOST=0.0.0.0 \
    DATABASE_URL=sqlite:///data/ide.db \
    JWT_KEYS_DIR=/data/jwt \
    IDE_WEB_DIST=/app/web \
    IDE_PREVIEW_ROOT=/data/previews \
    RUST_LOG=info

# The database file, the signing keys and (on the preview image) the preview
# workspaces all live here, so one volume keeps a deployment reproducible.
VOLUME ["/data"]
EXPOSE 8081

# /health is a liveness probe; /ready also touches the database.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/health" || exit 1

ENTRYPOINT ["nocode-ide-server"]
