# Bonafoid MCP server — agentic workflow authoring

The Bonafoid MCP server lets an MCP-compatible AI client build BPMN workflows
from natural-language prompts. Agents call small, stateful graph-editing tools;
they never author a whole BPMN 2.0 document. The server owns node/edge UUIDs
and all diagram coordinates, so LLM output cannot produce dangling
`sourceRef`/`targetRef` flows or broken `dc:Bounds`.

Status: implemented (Milestone 4, Phases 17–21 of
[`MCP-Integration-Plan.md`](MCP-Integration-Plan.md)).

## Transports

| Transport | Endpoint | Typical clients |
|-----------|----------|-----------------|
| stdio | `cargo run -p nocode-mcp` (newline-delimited JSON-RPC 2.0) | Claude Desktop, Cursor, custom local agents |
| Streamable HTTP | `POST /mcp` on `nocode-ide-server` (default port 8081) | ChatGPT connectors, remote/custom agents |

Both transports share the same dispatcher and session store. HTTP mode is
mounted in-process by the IDE server; the standalone binary intentionally
refuses `MCP_TRANSPORT=http` (it has no database or JWT keys). HTTP responses
are JSON, which the Streamable HTTP specification permits; SSE streaming is
not implemented in this release.

## Quick start (local)

```bash
cargo build -p nocode-mcp
MCP_TRANSPORT=stdio ./target/debug/nocode-mcp
```

Handshake example:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"bpmn://guide/authoring"}}
```

A typical agent sequence:

```text
create_workflow → add_node × N → connect_nodes × N → auto_layout
→ validate_workflow → simulate_workflow → save_workflow(target="project")
```

## Tool surface

All mutating tools take `session_id` and an optional `expected_revision`.
Results are compact (affected ids/objects), not whole workflows.

| Group | Tools |
|-------|-------|
| Document | `create_workflow`, `get_workflow`, `load_workflow`, `save_workflow`, `discard_workflow` |
| Graph | `add_node`, `update_node`, `remove_node`, `connect_nodes`, `update_edge`, `remove_edge`, `auto_layout` |
| Quality | `validate_workflow`, `simulate_workflow`, `export_bpmn_xml`, `undo`, `redo` |
| Definitions | `add_variable`, `update_variable`, `remove_variable`, `add_signal`, `add_timer`, `set_workflow_settings` |

Each tool has a JSON `inputSchema` generated from its Rust parameter struct;
`tools/list` is discoverable with no out-of-band knowledge. The `build_workflow`
prompt (with a `requirements` argument) encodes the preferred sequence.

## Resources

| URI | Content |
|-----|---------|
| `bpmn://schema/workflow` | `schemars::schema_for!(Workflow)` — byte-for-byte from the model |
| `bpmn://schema/project` | Generated project schema (where `workflows` lives) |
| `bpmn://catalog/node-types` | All 16 tags with config keys, defaults and examples |
| `bpmn://examples/json/minimal` | Smallest valid workflow JSON |
| `bpmn://examples/json/gateway` | Approval workflow with edge conditions |
| `bpmn://examples/xml/minimal` | Exporter output with `BPMNDiagram` and `dc:Bounds` |
| `bpmn://guide/authoring` | Tool sequence, condition syntax, coordinate rules |

Resources are read-only and MIME-typed (`application/schema+json`,
`application/json`, `application/xml`, `text/markdown`).

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `MCP_TRANSPORT` | `stdio` | `stdio` or `http` (HTTP only in the IDE server) |
| `MCP_BIND_ADDR` | `127.0.0.1:8090` | Reserved for standalone HTTP; currently unused |
| `MCP_API_BASE_URL` | `http://127.0.0.1:8081` | Base URL for `save_workflow(target="project")` |
| `MCP_API_TOKEN` | — | Bearer JWT required for project saves |
| `MCP_MAX_NODES` | `200` | Per-session node guardrail |
| `MCP_UNDO_DEPTH` | `20` | Snapshot history depth |
| `MCP_AUDIT_LOG` | — | Optional JSON-lines audit log (argument hashes only) |
| `MCP_FILE_ROOT` | process cwd | Writable root for `save_workflow(target="file")` |

Project saves use GET (for the optimistic-concurrency revision) then PUT on
`/api/v1/projects/:id`, so an agent-authored workflow appears in the visual
Workflow Builder after a reload. `MCP_API_BASE_URL` must be plain `http://`;
put a local reverse proxy in front of the server if TLS is required.

## Authentication and scopes (HTTP)

`POST /mcp` reuses the IDE's `AuthUser` extractor: an `Authorization: Bearer
<JWT>` works without the cookie/CSRF flow. Missing credentials produce 401.

- `workflows:read` — all authenticated users; allows `get_workflow`,
  `validate_workflow`, `simulate_workflow`, `export_bpmn_xml` and all
  resources/prompts.
- `workflows:write` — tenant owners/admins, platform super admins, or tokens
  whose signed roles include `workflows:write`; required for every other tool.

A caller without the required scope receives HTTP 403. Sessions are keyed by
`signed tenant : user`, so one user cannot reach another user's session even
with a known session UUID.

## Client configuration

**Claude Desktop / Cursor** — see
[`crates/mcp/examples/claude_desktop.json`](../crates/mcp/examples/claude_desktop.json).

**ChatGPT connector / custom agents** — point the HTTP MCP client at
`http://<host>:8081/mcp` with a Bearer JWT carrying the required scope.

**Microsoft Agent Framework** — use `StdioClientTransport` with the command
above, or `HttpClientTransport` with the URL above, then register the
discovered tools as agent tools. No .NET-side SDK is needed.

## Security notes

- Node tags and config keys are validated against the generated catalogue;
  unknown nested keys are rejected by a canonical round-trip check.
- File paths must be relative and stay below `MCP_FILE_ROOT`; symlink escapes
  are blocked by canonicalising the resolved parent directory.
- All XML is produced by `nocode_bpmn::export_bpmn`, which escapes text.
- The audit log records `{timestamp, owner, tool, arguments_digest,
  result_status}` — never tokens or full arguments.
- stdio mode runs with the local user's privileges. Only connect trusted
  clients. HTTP mode inherits the IDE server's tenant isolation.

## Implementation notes and known limitations

- The transport boundary is a small, dependency-free JSON-RPC 2.0 dispatcher
  (plan §4.3 fallback) instead of an external SDK such as `rmcp`. Tool handlers
  are plain Rust functions over `SessionStore`, so swapping in an SDK later
  touches only `protocol.rs` and `transport/*`. This also keeps Cargo.lock and
  the compile surface unchanged.
- Sessions are in-memory only and are not expired automatically; restart the
  process or call `discard_workflow` to release them.
- HTTP does not issue `Mcp-Session-Id` values; a client-supplied id is echoed
  but not enforced.
- `auto_layout` places cycle nodes deterministically but does not guarantee a
  cycle-free diagram; validate separately.

## Verification

```bash
cargo test -p nocode-mcp
cargo test -p nocode-ide-server mcp
cargo fmt --all --check
npm run typecheck && npm test
```

The MCP test suite includes a golden build → export → re-import round trip,
protocol/resource discovery tests, an HTTP auth/scope integration test, and a
deterministic five-prompt agent evaluation proxy (`crates/mcp/evals/prompts.json`).
