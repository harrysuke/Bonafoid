# syntax=docker/dockerfile:1
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
#
# CI publishes this image to GHCR as ghcr.io/<owner>/<repo> — see
# .github/workflows/docker.yml. The cargo and npm caches are BuildKit cache
# mounts rather than image layers: on a runner with a warm cache a source-only
# change relinks instead of re-downloading and rebuilding the dependency graph.

# --- stage 1: the web IDE (static files served by the server) ----------------
FROM node:20-bookworm-slim AS web
WORKDIR /src

# The workspace manifests first, so a source-only change does not reinstall.
COPY package.json package-lock.json ./
COPY packages/ide-core/package.json packages/ide-core/
COPY apps/ide-web/package.json apps/ide-web/
COPY apps/ide-tauri/package.json apps/ide-tauri/
RUN --mount=type=cache,target=/root/.npm,sharing=locked \
    npm ci --ignore-scripts

COPY packages ./packages
COPY apps/ide-web ./apps/ide-web
RUN npm run build:web

# --- stage 2: the Rust server ------------------------------------------------
FROM rust:bookworm AS server
WORKDIR /src

# Build the dependency graph before the sources so a code change only relinks.
# Every workspace member's manifest has to be present: cargo refuses to load a
# workspace whose `members` list points at a missing Cargo.toml, even when the
# crate being built does not depend on it. `--parents` keeps the glob's
# directory structure, so adding a crate no longer needs an edit here — the
# hand-maintained list this replaces had already missed two workspace members
# and the image did not build at all.
COPY Cargo.toml Cargo.lock ./
COPY --parents crates/*/Cargo.toml ./
COPY xtask/Cargo.toml xtask/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    set -eux; \
    # A workspace member with no target file makes cargo fail while it is still
    # loading manifests, so every crate gets one; the list comes from the
    # manifests above rather than from a second hand-written list.
    for manifest in crates/*/Cargo.toml; do \
      dir="$(dirname "$manifest")"; \
      mkdir -p "$dir/src"; \
      : > "$dir/src/lib.rs"; \
      : > "$dir/src/main.rs"; \
    done; \
    mkdir -p xtask/src; \
    echo "fn main() {}" > xtask/src/main.rs; \
    echo "fn main() {}" > crates/ide-server/src/main.rs; \
    cargo build --release -p nocode-ide-server; \
    rm -rf crates/*/src xtask/src

COPY crates ./crates
COPY xtask ./xtask
# `target/` is a cache mount, so it is not part of the image layer and cannot be
# COPY --from'd out of this stage. Install the binary to a normal path instead.
#
# The touch is load-bearing. The stub build above left artifacts in the cache
# for these very crates, and COPY preserves the build context's timestamps,
# which are older than those artifacts — cargo decides freshness by mtime, so
# without this it declares the crates fresh and installs the stub binary.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    find crates xtask -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -exec touch {} + && \
    cargo build --release -p nocode-ide-server && \
    install -m 0755 target/release/nocode-ide-server /usr/local/bin/nocode-ide-server

# --- stage 3: runtime --------------------------------------------------------
FROM debian:bookworm-slim

# OCI metadata. `docker.yml` passes these from the git ref and SHA so the
# registry, `docker inspect` and GHCR's package page all agree on provenance.
ARG VERSION=dev
ARG REVISION=unknown
ARG SOURCE=https://github.com/harrysuke/Bonafoid
LABEL org.opencontainers.image.title="nocode-ide" \
      org.opencontainers.image.description="No-Code IDE server: visual app builder, codegen and hosted previews" \
      org.opencontainers.image.source="${SOURCE}" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.licenses="AGPL-3.0-only"

# ca-certificates for outbound TLS (GitHub, package registries); curl for the
# container healthcheck. Nothing else is installed.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl; \
    rm -rf /var/lib/apt/lists/*; \
    useradd --system --create-home --home-dir /app --shell /usr/sbin/nologin nocode; \
    mkdir -p /data; \
    chown -R nocode:nocode /app /data

COPY --from=server /usr/local/bin/nocode-ide-server /usr/local/bin/nocode-ide-server
COPY --from=web /src/apps/ide-web/dist /app/web

WORKDIR /app
USER nocode

ENV PORT=8081 \
    HOST=0.0.0.0 \
    DATABASE_URL=sqlite:///data/ide.db?mode=rwc \
    JWT_KEYS_DIR=/data/jwt \
    IDE_WEB_DIST=/app/web \
    IDE_PREVIEW_ROOT=/data/previews \
    RUST_LOG=info

# The database file, the signing keys and (on the preview image) the preview
# workspaces all live here, so one volume keeps a deployment reproducible.
VOLUME ["/data"]
EXPOSE 8081

# SIGTERM is what `docker stop` and Kubernetes send; the server drains
# in-flight requests on it (crates/ide-server/src/server.rs) instead of
# waiting out the runtime's grace period.
STOPSIGNAL SIGTERM

# /health is a liveness probe; /ready also touches the database. Shell form
# because the port comes from the environment, and exec form does not expand it.
# (hadolint flags the shell form as DL3025; that rule assumes an entrypoint that
# takes arguments, which a healthcheck does not.)
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/health" || exit 1

ENTRYPOINT ["nocode-ide-server"]

