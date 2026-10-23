# Lean IDE server image; preview disabled when cargo/trunk unavailable.
FROM rust:1.75 AS builder
WORKDIR /app
COPY crates/ide-server crates/plugins crates/model crates/codegen crates/bpmn crates/ide xtask .
RUN cargo build --release -p nocode-ide-server

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/nocode-ide-server /usr/local/bin/
EXPOSE 8081
ENV IDE_PREVIEW_TIMEOUT_SECS=900 IDE_PREVIEW_PARALLEL=2
ENTRYPOINT ["nocode-ide-server"]
