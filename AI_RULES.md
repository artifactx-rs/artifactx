# AI Contributor Rules

These rules apply to AI-assisted changes from Codex, Claude, Copilot, or any other agent.

## Product boundary

Before changing code or docs, read:

1. [`CLAUDE.md`](CLAUDE.md) — product charter and ship gate;
2. [`README.md`](README.md) — current user-facing promise;
3. [`ROADMAP.md`](ROADMAP.md) — current freeze/polish scope;
4. relevant ADRs under [`docs/adr/`](docs/adr/).

ArtifactX is currently feature-frozen around import-first polish. Do not expand package formats, storage backends, dashboards, proxy modes, or broad CLI surface unless the user explicitly asks and the change has a design note or ADR.

## Required behavior

- Prefer small, reviewable diffs.
- Prefer deletion and clearer defaults over new abstractions.
- Preserve the 5-minute paths:
  - import existing apt/yum repo → publish → serve/Pages → client install;
  - init a new repo → add/pack → publish → serve → client install.
- Keep trust boundaries explicit: ArtifactX signs repository metadata; package payload signing is separate.
- Never commit private keys, tokens, generated repository private keys, or release credentials.
- Do not print secrets in logs, final reports, PR bodies, or issue comments.
- If a workflow touches GitHub Pages artifacts, verify `keys/private.asc` is absent from uploaded output.

## Validation expectations

Run the smallest checks that prove the claim. For normal code changes:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For GitHub Actions changes:

```bash
actionlint .github/workflows/*.yml
```

For docs-only changes, verify links, commands, and fenced snippets that were touched.

## PR disclosure

AI-assisted PRs should state:

- what files changed;
- what validation ran;
- known gaps;
- whether any generated content, credentials, or release artifacts were involved.

The human author remains responsible for the result.
