# ArtifactX Documentation Plan

ArtifactX is now more than a single CLI. It has a CLI, HTTP API, repository
metadata generators, signing policy, import/mirror flows, Docker/systemd
operation, GitHub Pages dogfooding, and release automation. The documentation
must stop using README and Pages as the only source of truth.

This plan uses the Diátaxis documentation model: tutorials, how-to guides,
reference, and explanation. Keep marketing copy in README/Pages, task execution
in tutorials/how-to guides, machine detail in reference, and tradeoff/rationale
in explanation/ADR documents.

## Target readers

- Package maintainers migrating existing apt/yum repositories.
- Infra/platform engineers who need a small signed Linux package repository.
- OSS maintainers who want GitHub Pages style package distribution.
- CI/CD users who want a CLI or HTTP API push path.
- Operators who need Docker Compose or systemd deployment guidance.

## Documentation boundaries

- README: product promise, install, first commands, links to docs.
- GitHub Pages: landing page + live repository install path only.
- `docs/README.md`: documentation map and decision tree.
- ADRs: architectural decisions, not user-facing tutorials.
- `crates/*/README.md`: crate-local developer notes only.

## P0: ship before the next polish release

These documents remove the biggest adoption blockers.

| File | Type | Reader goal | Notes |
| --- | --- | --- | --- |
| `docs/README.md` | index | Choose the right document quickly | Add decision tree: import, create, serve, operate, integrate CI, understand signing. |
| `docs/tutorials/import-existing-repo.md` | tutorial | Import an existing apt or yum repo and serve it safely | Lead with painless migration. Include dry-run/limit/filter guidance. |
| `docs/tutorials/create-and-serve-repo.md` | tutorial | Create a repo from local packages and serve it | Cover `.deb`/`.rpm`, `arx publish`, `arx serve`, and client verification. |
| `docs/how-to/install-clients.md` | how-to | Install packages from an ArtifactX repo | Include one-command installer, manual apt, manual yum/dnf. Avoid APT-only bias. |
| `docs/how-to/run-with-docker-compose.md` | how-to | Run ArtifactX via generated Compose | Cover generated compose file, mounted repo root, port binding, secrets. |
| `docs/how-to/run-as-systemd-service.md` | how-to | Run the API/server under systemd | Default bind address must be localhost unless explicitly exposed. |
| `docs/how-to/use-custom-signing-keys.md` | how-to | Use organization-owned RSA/GPG signing material | Document implemented behavior only: importing keys, generating defaults, expiry responsibilities. |
| `docs/reference/cli.md` | reference | Find all commands and stable options | Generated or checked against `arx --help`; include composable subcommand examples. |
| `docs/reference/config.md` | reference | Understand config keys and defaults | Include branded/default secret guidance and override paths. |
| `docs/explanation/signing-and-expiry.md` | explanation | Understand what ArtifactX signs and what operators own | Be explicit: repo metadata signing is implemented; package signing policy is separate. |

## P1: needed for serious operators

| File | Type | Reader goal | Notes |
| --- | --- | --- | --- |
| `docs/how-to/push-from-ci.md` | how-to | Publish from CI without manual shell access | Cover API token, safe default bind, curl examples, GitHub Actions example. |
| `docs/reference/http-api.md` | reference | Integrate directly with the server API | Endpoint list, auth header, request/response shapes, error semantics. |
| `docs/reference/repository-layout.md` | reference | Inspect generated apt/yum repositories | Map root paths, apt metadata, yum repodata, keys, install script. |
| `docs/how-to/promote-and-rollback.md` | how-to | Promote, rollback, and recover from bad publishes | Tie to implemented publish/rollback behavior. |
| `docs/how-to/prune-and-gc.md` | how-to | Clean old repository state safely | Retention policy, dry-run, backup warning. |
| `docs/operations/production-checklist.md` | how-to | Decide whether a repo is production-ready | Backups, key ownership, reverse proxy, auth, monitoring, disaster recovery. |
| `docs/troubleshooting.md` | how-to | Debug common client/repo failures | GPG trust, apt cache, dnf key import, wrong arch, expired metadata, localhost binding. |

## P2: polish and education

| File | Type | Reader goal | Notes |
| --- | --- | --- | --- |
| `docs/explanation/import-vs-mirror.md` | explanation | Pick import, mirror, or create-new flow | Clarify one-way sync assumptions and safe migration. |
| `docs/explanation/static-hosting-model.md` | explanation | Understand why package repos can be hosted like static blogs | Explain GitHub Pages/dumb HTTP fit and limits. |
| `docs/explanation/apt-yum-differences.md` | explanation | Understand client-specific behavior | Keep user-facing; implementation detail stays in reference/ADRs. |
| `docs/migration-checklist.md` | how-to | Move from a legacy repository with low risk | Inventory, limited import, client canary, cutover, rollback. |
| `docs/adr/README.md` | reference index | Navigate decisions by topic | Link ADRs from explanation pages; do not force users through ADRs first. |

## Validation policy for examples

Examples in public docs must be executable or clearly marked as placeholders.

- CLI examples: validate against `arx --help` or an integration test fixture.
- Pages install examples: test in Debian and Fedora containers before release.
- systemd examples: validate syntax with `systemd-analyze verify` when available.
- Compose examples: validate with `docker compose config`.
- HTTP API examples: run against a localhost server fixture when possible.
- Signing examples: state whether they cover repository metadata signing or package signing.

## Writing rules

- Prefer short commands that produce a visible success signal.
- Do not imply custom HSM/KMS support is implemented; keep it in roadmap until built.
- Do not imply package payload signatures are verified when only repository metadata is signed.
- Default server examples bind to `127.0.0.1`; external exposure requires an explicit reverse proxy/auth section.
- Keep README and Pages persuasive, but move operational detail to docs.
- Every document must answer: "What should the reader do next?"

## Implementation sequence

1. Add `docs/README.md` and link it from README and Pages.
2. Write the two tutorials: import existing repo, create and serve new repo.
3. Write install, Docker Compose, systemd, and custom signing how-to guides.
4. Generate or hand-check CLI/config/API references from current behavior.
5. Add signing/expiry and static-hosting explanations.
6. Add validation scripts or CI smoke checks for public docs examples.
