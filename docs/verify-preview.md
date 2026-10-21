# Verifying a generated app through the hosted preview

The compile fixtures in `crates/codegen` prove the generated code
*compiles*. The e2e test in `crates/ide-server/src/preview/live_tests.rs`
proves the generated backend *answers*. Neither of them proves that the
generated **frontend** renders, because that needs a browser and a WASM
build.

This is the procedure for that last leg. It is deliberately a script plus
a short checklist rather than a browser test: the IDE has no browser
automation, and adding one is a bigger decision than a verification
procedure should be.

## What you need on the host

- `cargo` (the IDE server builds the generated backend with it)
- `trunk` (`cargo install trunk`) and the `wasm32-unknown-unknown` target
  (`rustup target add wasm32-unknown-unknown`) for the frontend
- Node.js 20+ and `npm ci` for the IDE itself

## 1. Start the IDE server

```bash
npm ci
npm run build:web          # serves apps/ide-web/dist
cargo run -p nocode-ide-server
```

The server listens on `http://127.0.0.1:8081`. It must be started from the
**repository root**: `IDE_WEB_DIST` defaults to the relative path
`apps/ide-web/dist`.

## 2. Create a project with a page, a form and a data source

Either use the IDE at `http://127.0.0.1:8081` (register, then open a
project), or drive the API directly. The API path is shorter to repeat:

```bash
# Register and keep the cookies + CSRF token.
curl -sc cookies.txt -X POST http://127.0.0.1:8081/api/v1/auth/register \
  -H 'content-type: application/json' \
  -d '{"email":"you@example.com","password":"supersecret"}'

CSRF=$(grep nocode_csrf cookies.txt | awk '{print $7}')

# Create a project (the body is the project model; see
# crates/model/src/project.rs for the shape).
curl -sb cookies.txt -X POST http://127.0.0.1:8081/api/v1/projects \
  -H 'content-type: application/json' -H "x-csrf-token: $CSRF" \
  -d @project.json
```

The project needs at least one entity, and — for the page leg — a page
whose route is not `/` (an index page is skipped so it cannot collide
with the home page) and a data source pointing at the entity's list
endpoint.

## 3. Build it

```bash
curl -sb cookies.txt -X POST \
  http://127.0.0.1:8081/api/v1/projects/$PROJECT_ID/preview/build \
  -H 'content-type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"target":"all"}'
```

Poll until the state is `ready`:

```bash
curl -sb cookies.txt http://127.0.0.1:8081/api/v1/builds/$BUILD_ID
```

The response's `logs` array is the build log. If the state is `failed`,
that array says why.

## 4. Run it

```bash
curl -sb cookies.txt -X POST http://127.0.0.1:8081/api/v1/builds/$BUILD_ID/run \
  -H "x-csrf-token: $CSRF"
```

The response has a `port`. The preview is served from
`http://127.0.0.1:8081/preview/$BUILD_ID/`.

## 5. Check the API through the proxy

The generated frontend calls `{API_BASE}/api/v1/{entity}`, and `API_BASE`
is baked into the bundle as `http://127.0.0.1:8081/preview/$BUILD_ID`. So
the proxy path keeps the `api` segment on both sides:

```bash
curl -sb cookies.txt http://127.0.0.1:8081/preview/$BUILD_ID/api/api/v1/product
```

## 6. Check the pages in a browser

Open `http://127.0.0.1:8081/preview/$BUILD_ID/` and walk the checklist:

- [ ] The page's components render — no `[Type]` labelled boxes where a
      real component was designed. A labelled box means the component type
      has no renderer (see 2.3e in `completion-plan.txt`).
- [ ] The data source's rows appear. The page renders `(data from X)`
      placeholders for components the page-level binding does not feed; a
      Table bound to a data source should show rows.
- [ ] A form page's inputs have the right widgets: a boolean is a
      checkbox, an enum is a select, a date is a date picker, a relation
      is a select of the related entity's records.
- [ ] Submitting the form posts to `/api/v1/forms/{form}` and the
      response is 201.
- [ ] A required form field that is left blank is rejected.
- [ ] The browser console has no errors.

## 7. Stop it

```bash
curl -sb cookies.txt -X POST http://127.0.0.1:8081/api/v1/builds/$BUILD_ID/stop \
  -H "x-csrf-token: $CSRF"
```

The run is also stopped automatically after `IDE_PREVIEW_IDLE_SECS`
(default 900) of inactivity.

## The automated half

The backend half of this is automated:

```bash
cargo test -p nocode-ide-server -- --ignored live --test-threads=1
```

It generates a project, builds its backend, runs it, and drives entity
CRUD plus a form submission through the proxy. It is `#[ignore]`d
because it needs cargo, the network and about a minute.

## If the build fails

The generated backend is a fresh crate with its own dependency tree, so
its first build fetches everything. On a Windows host, cargo sometimes
fails with

```text
failed to rename archive file ...: The process cannot access the file
because it is being used by another process. (os error 32)
```

That is antivirus or the Windows search indexer holding a file cargo is
renaming, not a defect in the generated code. Build again. The e2e test
retries on exactly this error and on no other.
