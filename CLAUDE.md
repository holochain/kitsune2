# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Kitsune2 is a peer-to-peer / DHT communication framework written in Rust, used by Holochain. It is structured as a Cargo workspace of crates that together provide a node implementation, plus a bootstrap server for WAN peer discovery.

## Common commands

This project uses `cargo-make`, configured via `Makefile.toml`, for the standard developer workflow.

- `cargo make verify` — the default task, which runs the full check: format, clippy, doc-check, taplo, and tests.
- `cargo make static` — all static checks, without running tests.
- `cargo make test` — runs `cargo test` across the workspace.
- `cargo make fix` — applies `cargo fmt`, regenerates protobuf code, and runs `taplo format`.
- `cargo make clippy` — runs `cargo clippy --all-targets -- --deny=warnings`, so warnings are treated as errors.
- `cargo make doc-check` — runs `cargo +nightly doc --all-features --no-deps` with `RUSTDOCFLAGS=--cfg docsrs --deny=warnings`. Requires a nightly toolchain; expected to be available from rustup.
- `cargo make proto` — regenerates protobuf-derived Rust from `.proto` files via the `tool_proto_build` crate. Run after editing any `.proto`.
- Run a single test with `cargo test -p <crate> <test_name>`, for example `cargo test -p kitsune2_gossip gossip_round`.

The protobuf wire-protocol code in `crates/api` is generated; do not hand-edit generated files. Edit the `.proto` sources and run `cargo make proto`, or run `cargo make fix` which includes it.

### Pre-commit / pre-push checks

Before committing and pushing, run in order:

1. `cargo fmt`.
2. `cargo make static` — must pass before going further.
3. Tests for the changed crate(s) **and** any downstream crates that depend on them. Scope this by what was touched, so for example:
   - **API change implemented in `core` and/or a transport**: just run `cargo make test`, since the blast radius is wide enough that running the full suite is simpler than reasoning about it.
   - **`transport_iroh` only**: run tests for `kitsune2_transport_iroh` and for `kitsune2`, which is the top-level integration crate that consumes the transport.
   - **Other localised changes**: test the changed crate plus its direct downstream consumers, following the dependency direction described in *Workspace architecture* below.

   Run a single crate's tests with `cargo test -p <crate>`.

## Workspace architecture

The workspace is intentionally split so that the API surface, the production implementations, and the integrations are independently versioned and replaceable. Most crates depend on `kitsune2_api`.

- **`crates/api`**, published as `kitsune2_api` — the trait and type surface that defines a Kitsune2 node. It defines traits such as `Kitsune`, `Space`, `Transport`, `PeerStore`, `OpStore`, `Bootstrap`, `Gossip`, `Fetch`, `Publish`, and `LocalAgentStore`, along with the prost-generated wire `protocol`, config plumbing, and the `Builder` pattern used to assemble a node from pluggable modules. Anything reusable across implementations lives here.
- **`crates/core`**, published as `kitsune2_core` — production-ready and test default implementations of the API traits, such as in-memory stores. Consumers typically reuse some of these and override others; in particular, `OpStore` usually needs persistence.
- **`crates/kitsune2`** — the top-level integration crate exposing a default `Builder` wired together from `core`, `dht`, `gossip`, and a transport. This is the entry point for applications embedding Kitsune2.
- **`crates/dht`**, published as `kitsune2_dht` — the DHT data model, organised as sector, ring, and disc structures. It is used by the gossip crate to drive diff exchange and has no networking of its own.
- **`crates/gossip`**, published as `kitsune2_gossip` — gossip protocol implementation that uses the `dht` model to compare state between peers and exchange missing ops and agents. The state machine is documented as a mermaid diagram in `crates/gossip/README.md`, flowing from init → accept → diff exchange → hashes/agents → terminate. Read it before changing protocol flow.
- **`crates/transport_tx5`** — `Transport` implementation built on the `tx5` WebRTC stack, which is the historical default.
- **`crates/transport_iroh`** — alternative `Transport` implementation built on `iroh`. The two transports are interchangeable behind the API trait.
- **`crates/bootstrap_srv`** — standalone HTTP server, built on axum, that helps nodes discover each other on a WAN. It uses tempfiles instead of RAM for storage and embeds either an SBD signal server for the tx5 transport or a relay service for the Iroh transport. It ships with its own CLI.
- **`crates/bootstrap_client`** — client used by nodes to talk to a `bootstrap_srv`.
- **`crates/test_utils`** — shared test fixtures and helpers, used only as a dev-dependency.
- **`crates/tool_proto_build`** — build tool that runs `prost-build` over the `.proto` files; invoked via `cargo make proto`.
- **`crates/kitsune2_showcase`** — interactive CLI demo app showing how the pieces fit together.

### Key architectural points

- The API uses dynamic dispatch via factory traits plus a `Builder` so consumers can mix and match implementations; for example, a custom persistent `OpStore` can be combined with the default gossip and tx5 transport. When adding new pluggable functionality, follow the existing pattern: a trait in `api/src/<thing>.rs`, a `*Factory` trait, a default implementation in `core` or its own crate, and registration via the `Builder`.
- `kitsune2_api` is the only crate everything else depends on; it must stay light and avoid runtime- or transport-specific dependencies. Heavier dependencies belong in the implementation crate that needs them. The root `Cargo.toml` separates dev dependencies and tool dependencies; new dependencies must go in the right section.
- Tokio is used as the async runtime, but the API is written so it could in principle be abstracted later. Don't bake tokio-specific types into `kitsune2_api` public surface.
- Logging is via `tracing`; consumers pick the subscriber. See *Logging conventions* below for level guidance.
- Rust edition is 2024; toolchain is pinned via `rust-toolchain.toml`.

### Logging conventions

Pick the level based on whether the event is a real problem and whether anyone could act on it:

- **`error`** — a real failure: something has gone wrong, and it isn't transient. Reserve this for cases where you genuinely know the system is in a bad state.
- **`warn`** — something isn't really expected to happen, or might be a problem worth a human's attention, but the system is still functioning. Do *not* use `warn` (or `error`) for transient conditions a user can't fix. For example, being offline is a tolerated case: if we can't reach the bootstrap server or other peers, keep the relevant Tokio tasks and loops alive *quietly* — don't spam warnings on every retry.
- **`info`** — information that's useful to somebody trying to understand what Kitsune2 is doing at a high level. Should not be noisy.
- **`debug`** — information that would be too noisy for `info` but is still useful when actively debugging.
- **`trace`** — permitted, but rarely enabled in practice.

## Testing strategy

Tests are layered, and you should reach for the fastest, most reliable layer that can meaningfully cover the change. Usually, that means we follow the "testing triangle" with more unit tests than functional test, and more functional tests than integration tests. Almost all code should be unit tested, but it's acceptable for a few integration tests to cover a lot of functionality in one test.

1. **Unit tests** — the default. Cover as much as possible at this level. It is acceptable to adjust the API of *new* code to make it test-friendly; reshaping a brand-new module so it can be unit tested is preferred over leaving it untested. If making code more testable would require **breaking changes to existing code that isn't from the current branch**, stop and raise it as a question first: the right call depends on whether the change needs to be backported to a `release-X.Y` line, where breaking changes are not allowed (see *Branching and versioning strategy*). Only new code can be reshaped freely.
2. **Functional tests, in-memory where possible** — for behavior that spans modules, prefer wiring together the in-memory implementations from `kitsune2_core` (in-memory stores, etc.) rather than reaching for the filesystem or the network. This keeps the bulk of the suite fast and reliable.
3. **Integration tests with real modules** — a small number, in the crates where they make sense. Genuine integration tests that use the "real" production modules from `core` together with the real transport implementations belong in the **`kitsune2`** crate, since that is where everything is naturally pulled together. Don't push this kind of test down into the lower-level crates.

## Releases

Releases are driven by a pair of GitHub Actions workflows that delegate to the shared `holochain/actions` reusable workflows:

- `.github/workflows/prepare-release.yml` — manually dispatched. It calls `holochain/actions/.github/workflows/prepare-release.yml`, which bumps versions, runs semver checks, and opens a release PR. The `force_version` input lets you override the default semver bump, and `skip_semver_checks` bypasses the semver compatibility checks but is not recommended.
- `.github/workflows/publish-release.yml` — runs on pushes to `main` / `release-*`. Calls `holochain/actions/.github/workflows/publish-release.yml`, which publishes the crates to crates.io and tags the release once a prepared release PR is merged.

### Branching and versioning strategy

- **`main`** is the development branch and is where breaking changes land.
- **`release-X.Y`** branches are maintained for each released minor version. Only non-breaking changes such as bug fixes and backwards-compatible improvements are published from these branches; breaking changes are never released on an existing `release-X.Y` line.
- **Breaking changes** are permitted between `0.X.Y` and `0.(X+1).Y`, never within a single minor version.
- **Pre-release validation** is done with `0.X.Y-dev.Z` versions, starting at `Z=0`, which let downstream consumers integration-test upcoming changes before the project commits to a stable `0.X.Y`. Dev releases are cut from whichever branch the upcoming version will ship from: from `main` when validating the next minor, such as `0.4.0-dev.Z` ahead of `0.4.0`, and from a `release-0.X` branch when validating a patch on an existing line, such as `0.3.3-dev.0` from `release-0.3` ahead of `0.3.3`. Once the dev series has validated the changes, the stable version is released from the same branch.
