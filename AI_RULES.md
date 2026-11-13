# AI Contributor Rules

These rules apply to AI-assisted changes from Codex, Claude, Copilot, or any other agent.

## Product boundary

Before changing code or docs, read:

1. [`CLAUDE.md`](CLAUDE.md) — product charter and ship gate;
2. [`README.md`](README.md) — current user-facing promise;
3. [`ROADMAP.md`](ROADMAP.md) — live planning links and product narrative;
4. relevant ADRs under [`docs/adr/`](docs/adr/).

ArtifactX is early-stage and should stay narrow. Do not expand package
ecosystems, storage backends, dashboards, proxy modes, trust models, or broad CLI
surface unless the user explicitly asks and the change has a design note or ADR.

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

## Compatibility gate

Compatibility is a hard release condition; see
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md).

- Adding a field to a public struct that downstream crates construct is
  source-breaking even with `#[serde(default)]`; verify with a throwaway
  downstream crate, not by reasoning.
- A breaking change needs an ADR, a version that communicates it (never a patch
  release), and a `Breaking` release-note section.
- Before tagging, run the full gate: fmt, clippy, workspace tests, downstream
  compile probe, client e2e (apt/dnf), public API diff, registry state.
- A missing GitHub tag or Release does not mean a crate was never published: the
  weekly `crates.yml` schedule publishes the current Cargo version without a
  tag. Check the crates.io sparse index.

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
