# Contributing to ArtifactX

ArtifactX is early-stage but already has shipped repository and package-building
paths. The highest-value contributions make those paths more reliable:

```text
existing apt/yum repo or built packages -> arx import/add/pack -> arx publish -> arx serve/Pages -> apt/dnf install -> rollback
```

## Good first areas

- apt/yum import fixtures from real-world repositories;
- pack fixtures for `.deb`, `.rpm`, `.apk`, and Arch package output;
- clearer errors for malformed upstream metadata, package payloads, or missing package URLs;
- docs that make signing, client setup, backup/restore, pack, and rollback unambiguous;
- CI hardening that prevents accidental secret leakage or release mistakes;
- tests around `import`, `add`, `pack`, `publish`, `serve`, and rollback behavior.

## Before adding a feature

GitHub issues and milestones are the live planning source. Open a proposal issue
first if the change adds a new package ecosystem, storage backend, dashboard,
proxy mode, trust model, or broad CLI/API surface.

## Local validation

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

If you change GitHub Actions:

```bash
actionlint .github/workflows/*.yml
```

## Pull request expectations

- Keep the diff small and reversible.
- Include tests for behavior changes.
- Update docs when commands, outputs, or trust boundaries change.
- Do not commit signing keys, package repository private keys, tokens, or generated release artifacts.


## AI-assisted contributions

AI-assisted changes are welcome when they are reviewable and accountable. Follow
[`AI_RULES.md`](AI_RULES.md), disclose validation, and keep generated changes
inside the current roadmap scope.
