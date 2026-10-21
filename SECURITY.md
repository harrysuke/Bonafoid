# Security Policy

## Supported Versions

The No-Code IDE is pre-1.0 (`0.1.0`). Fixes land on `main` and are picked up
by the next release. There are no long-term-support branches yet.

| Version | Supported |
| ------- | --------- |
| `main`   | yes       |
| `0.1.x`  | yes       |
| `< 0.1`  | no        |

## Reporting a Vulnerability

Please report security issues privately rather than opening a public issue.
Email **security@nocode-ide.dev** with:

- a description of the issue and its impact,
- the version or commit you tested,
- steps to reproduce, and
- any proof-of-concept you have.

We aim to acknowledge within 3 working days and to send a remediation plan
within 10. If a fix is not practical in that window we will say so, and
still credit the reporter unless they ask otherwise.

Please do not test against deployments you do not own, and do not use other
users' data in a proof of concept. We will treat a good-faith report that
stays in scope as safe harbour.

## Threat Model

The IDE server has two very different responsibilities, and they have
different risk profiles.

### The IDE itself (low risk)

The IDE server stores user projects and serves the IDE UI. It authenticates
with JWT (RS256) and session cookies, and mutating requests carry a
double-submit CSRF token.

Controls in place:

- Passwords are hashed with Argon2 (`crates/ide-server/src/auth/password.rs`).
- CSRF: the `nocode_csrf` cookie must be echoed in the `x-csrf-token`
  header for cookie-authenticated mutations. Bearer-token requests are
  exempt, because a cross-origin browser cannot read or attach the token
  (`crates/ide-server/src/error.rs:98-107`).
- Every project query is scoped by `user_id`, so one user cannot read or
  write another user's projects
  (`crates/ide-server/src/projects/store.rs`).
- The preview subsystem never receives the server's `DATABASE_URL`,
  `JWT_KEYS_DIR`, `RUST_LOG` or any operator secret. Preview children are
  started from a cleared environment with an explicit allowlist
  (`crates/ide-server/src/preview/env.rs`). Non-SQLite previews are refused
  unless a separate preview database is configured, and a preview that
  points at the project's own database is refused outright.

### The hosted preview (HIGH RISK — read this)

**The preview subsystem compiles and runs user-generated Rust on the host
machine, with no container, no chroot, no cgroup and no resource limits
beyond an idle timeout.** This is a deliberate, documented trade-off:
container isolation for the build worker is out of scope for this release
(see `completion-plan.txt`, "EXPLICITLY OUT OF SCOPE").

Concretely, this means:

- A generated backend's `build.rs` is arbitrary code, and it executes with
  the privileges of the IDE server process.
- A running preview is a normal OS process. It can read any file the
  server user can read, open any network socket the server user can
  open, and consume CPU and memory.
- **Do not expose the IDE server to untrusted users on a host you care
  about.** The preview subsystem is intended for a trusted-host or
  single-tenant deployment. Running it on a shared machine, or serving
  strangers from it, is out of threat model.

Known gaps in the preview hardening are tracked in `completion-plan.txt`
Phase 4. The most significant are a shared cargo target directory, an
unvalidated build id, and proxy requests with no timeout.

### Third-party plugins

Plugin manifests are validated (id, semver, api_version, duplicate
permissions and entry points) before installation, and plugin code runs in
a Wasmtime sandbox with a fuel meter. Plugins are installed from a local
manifest only; there is no marketplace, and the IDE does not fetch plugins
from the network on your behalf.

## Severity

| Severity | Example | Response |
| -------- | ------- | -------- |
| Critical | Authentication bypass; cross-tenant data access; credential exposure | Immediate triage, same-day acknowledgement |
| High | Privilege escalation between users; injection into generated code that runs on the host | Triage within 1 working day |
| Medium | Stored XSS in the IDE; CSRF bypass; path traversal in the preview workspace | Triage within 5 working days |
| Low | Missing hardening headers; information disclosure of non-sensitive data | Next release |

## Out of Scope

- The preview subsystem's lack of container isolation. This is a known,
  documented design decision, not a bug. See "EXPLICITLY OUT OF SCOPE" in
  `completion-plan.txt`.
- Vulnerabilities in code that a *user* generates and runs in their own
  preview. That code is the user's own; the IDE does not audit it.
- Vulnerabilities in upstream dependencies with no working exploit against
  the IDE's own use of them.
- Physical access to the host, and denial of service by a user who is
  already allowed to run arbitrary code through the preview subsystem.
