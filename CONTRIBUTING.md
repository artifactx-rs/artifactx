# Contributing to ArtifactX

ArtifactX is in an import-first polish phase. The highest-value contributions make the migration path more reliable:

```text
existing apt/yum repo -> arx import -> arx publish -> arx serve/Pages -> apt/dnf install -> rollback
```

## Good first areas

- apt/yum import fixtures from real-world repositories;
- clearer errors for malformed upstream metadata or missing package URLs;
- docs that make signing, client setup, backup/restore, and rollback unambiguous;
- CI hardening that prevents accidental secret leakage or release mistakes;
- tests around `import`, `publish`, `serve`, and rollback behavior.

## Before adding a feature

ArtifactX is feature-frozen around import-first polish. Open a proposal issue first if the change adds a new package format, storage backend, dashboard, proxy mode, or broad CLI surface.

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

AI-assisted changes are welcome when they are reviewable and accountable. Follow [`AI_RULES.md`](AI_RULES.md), disclose validation, and keep generated changes inside the current feature-freeze scope.
