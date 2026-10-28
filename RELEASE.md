# Release checklist

How to cut a release of Bonafoid, and what has to be true first.

## 1. Version and changelog

- [ ] Decide the version. `0.x` until the SRS §1.5 gaps that matter for your
      users are closed; the workspace crates, `package.json` and the Tauri
      bundle all carry `0.1.0` today.
- [ ] Move `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) to a dated version
      heading, add the compare links, and leave an empty `Unreleased` section.
- [ ] Confirm the "Known limitations" list is still true. It is the honest
      counterweight to the feature list.

## 2. Gates

Every one of these must pass on the commit you are tagging:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features

npm ci
npm run typecheck
npm run lint
npm test

# Generated types must not have drifted
cargo run -p nocode-xtask --quiet -- types
node packages/ide-core/scripts/gen-model-types.mjs --check \
  packages/ide-core/src/types/model.schema.json \
  packages/ide-core/src/types/model.generated.ts
```

The two `#[ignore]`d compile fixtures are worth running before a release, since
they are the only check that generated code *compiles*:

```bash
cargo test -p nocode-codegen -- --ignored generated_backend_compiles
cargo test -p nocode-codegen -- --ignored generated_frontend_compiles
```

## 3. Docs

- [ ] [README.md](README.md) quickstart still works from a clean clone.
- [ ] Every environment variable the server reads is in the README table
      (`grep -rho 'env::var("[A-Z_]*")' crates/ide-server/src | sort -u`).
- [ ] [docs/Software Requirements Specification.md](docs/Software%20Requirements%20Specification.md)
      §1.5 reflects what shipped.
- [ ] [docs/user-manual.md](docs/user-manual.md) screenshots and button labels
      still match the UI.

## 4. Packaging

- [ ] `docker build -t nocode-ide .` and, if previews are in scope,
      `docker build -f Dockerfile.preview -t nocode-ide:preview .`
- [ ] `docker compose config` validates, and `docker compose up -d` serves the
      IDE with `/ready` reporting `ok`.
- [ ] The Tauri desktop build (`npm run tauri:build` in `apps/ide-tauri`) still
      bundles; the desktop shell is secondary but must not be broken.

### Release automation decision: hand-rolled, not cargo-dist

`.github/workflows/release.yml` builds `nocode-xtask` for four targets and
attaches `tar.gz` archives to a GitHub release. It is deliberately **not**
cargo-dist:

- The previous `dist-workspace.toml` was never invoked by any workflow, and its
  keys were not valid for cargo-dist 0.22.
- It advertised shell, PowerShell and Homebrew installers that nothing produced.
- cargo-dist is worth adopting when there is a reason to ship installers. Until
  then, a workflow that does exactly what it says beats a tool that promises
  more.

Revisit this if the CLI grows a user base that needs package-manager installs.

## 5. Tag

```bash
git tag -a v0.1.0 -m "Bonafoid 0.1.0"
git push origin v0.1.0
```

The tag triggers `release.yml`. Check that the four archives are attached and
that the release notes point at `CHANGELOG.md`.
