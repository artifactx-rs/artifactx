# ADR-0027: CI and verification boundaries

## Status

Accepted for v0.2.0.

## Context

v0.2 added realistic migration, API, cutover, and packaging E2E coverage. The
project needs that confidence without making documentation-only changes wait on
full Rust builds.

The current GitHub Actions timing around the v0.2 closeout was approximately:

- docs workflow: 6-7 seconds;
- Rust CI workflow: about 3 minutes when cache is warm;
- local pre-push full workspace gate: under 1 minute on a warm developer
  machine for these closeout branches.

## Decision

Keep CI split by concern:

- `docs.yml` validates docs, roadmap, Pages source, and site-generation scripts
  only for docs/site/roadmap changes.
- `ci.yml` skips docs/site-only paths and runs the Rust correctness gate for
  source, packaging, workflow, and test changes.
- `ci.yml` keeps `Swatinem/rust-cache@v2` and installs real package validators
  (`dpkg-deb`, `rpm`) for packaging tests.
- The Rust gate remains `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, and a dogfood `arx pack` build from `Cargo.toml`.

Do not split slow/real-tool/dogfood checks further for v0.2. The current warm CI
time is acceptable, and keeping one Rust job avoids false confidence where a PR
passes fast tests but fails packaging or dogfood checks after merge.

## Consequences

- Documentation-only PRs stay lightweight.
- Rust PRs keep strong release confidence.
- Future performance work should first collect timing evidence from multiple
  runs before splitting jobs or changing cache keys.
- Release-only tool caching, such as Zig or `cargo-zigbuild`, should be handled
  in release workflows and not coupled to regular PR CI unless those tools move
  into the PR gate.

## Alternatives considered

- **Split unit, integration, real-tool, and dogfood checks into separate jobs.**
  Rejected for v0.2 because current warm timing is acceptable and a single gate
  is easier to reason about.
- **Skip dogfood on PRs.** Rejected because package generation is part of the
  product promise and has caught release-path issues before.
- **Run Rust CI for docs-only changes.** Rejected because docs have their own
  focused workflow and path filtering already protects code changes.
