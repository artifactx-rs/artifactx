# ArtifactX — Project Board

> Durable source of truth for what's done, in flight, and queued. Survives across
> sessions. Derived from the adversarial user/investor review (2026-06).
> Legend: **P0** = blocks production credibility · **P1** = scale/UX · **P2** = nice-to-have.

## ✅ Done

| Item | Notes |
| --- | --- |
| Workspace scaffold | `crates/arx` (CLI, GPL) + `crates/debrepo` (lib, MIT/Apache) |
| yum/dnf repodata + signing | via `createrepo_rs`; `repomd.xml.asc`; verified with real `dnf` |
| apt repo generation + signing | `debrepo`; `InRelease`/`Release.gpg`; verified with real `apt-get` |
| Built-in HTTP serve + `/metrics` | axum + tower-http; Prometheus; `tracing` logs |
| `compose` generator | `Dockerfile` + `docker-compose.yml` |
| Version stamping | vergen → `arx --version` (git sha / build time / rustc) |
| Tests + CI hygiene | `cargo test --workspace` green; `clippy` clean |
| READMEs (×3) + logo | platform + arx + debrepo |
| **P0 — multi-dist/component atomic publish** | single `Release` per dist; `by-hash`; staging→commit swap; publish lock |
| **P0 — private key encryption + passphrase** | S2K-encrypted key via `ARX_KEY_PASSPHRASE`/`--passphrase-file`; default stays frictionless (5-min rule) + warns |
| **`pack` PoC** (packaging moat) | `crates/pack`: manifest → `.deb`/`.rpm`, pure-Rust native-first, Docker fallback stub, build hygiene; 5 tests green |
| **P0 — package delete / GC / retention** | `arx rm <name> [--version]` (yank) + `arx gc --keep N [--dry-run]` (retention); 3 integration tests |
| **P0 — serve security** | optional bearer-token auth (`ARX_SERVE_TOKEN`); `ServeDir` blocks path traversal; TLS delegated to a reverse proxy by design |
| **`pack` relationships** | manifest `depends`/`conflicts`/`provides`/`replaces` + maintainer scripts → deb control + rpm |
| **`arx push` + REST API** | `POST/GET/DELETE /api/v1/packages`, `/api/v1/gc`, `/api/v1/health`; bearer-auth; `arx push` client; uploads store + sign + publish atomically |
| **`arx pack`** | manifest → `.deb`/`.rpm` in the CLI; `--add` into the pool — Build·Package·Publish in one binary |
| **atomic rollback (apt + yum)** | publish → immutable state dir + atomic symlink flip (shared `debrepo::statedir`); `arx rollback <target>`/`history`; `gc` pins files referenced by retained states ([ADR-0008](docs/adr/0008-atomic-rollback.md)) |
| **docs + design-first** | `docs/DESIGN.md` + 9 ADRs; "design → review → build" in the charter |
| **Dogfood** | `arx` packs + publishes `arx` → signed apt+yum repo on GitHub Pages (`.github/workflows/release.yml`); verified locally incl. `apt-get install arx` from its own repo (ADR-0009) |
| **CI** | `.github/workflows/ci.yml` — clippy + test on every push/PR |
| Competitive teardown | [`COMPETITORS.md`](COMPETITORS.md) + public org landing page |
| Published to GitHub | `artifactx-rs/artifactx` (private) + Project board `artifactx-rs/projects/1` |

## 🔨 In progress

| Item | Owner | Notes |
| --- | --- | --- |
| _(next: yum-side rollback, or incremental publish)_ | main | not started |

## 📋 Backlog

> Prioritized via the competitive teardown — see [`COMPETITORS.md`](COMPETITORS.md).

### P0 — credibility
- _(done — see Done column: Dogfood)_

### P1 — the wedge (steal from aptly + nfpm + Cloudsmith)
- **OIDC keyless auth for push** — mint a short-lived token from GitHub Actions `id-token` instead of a stored `ARX_SERVE_TOKEN` (steal Cloudsmith).
- **`arx rollback` / `arx history`** — atomic publish via immutable content-addressed states + pointer flip; expose ONLY these two verbs (never aptly's full snapshot CRUD).
- **Incremental publish by default** — createrepo_c `--update` style: republish is O(changes), not O(repo).
- **Retention policy** — `gc --keep-within 90d`; `gc --grace` window + bytes-freed report; semver-aware ordering.

### P1 — correctness
- Duplicate-`add` handling (dedupe / reject same name+version); `Contents-<arch>` (`apt-file`); key rotation / revocation.

### Consider later
- `promote` (staging→prod move); `incoming/` drop-dir ingestion; `arx pack --from <staging>`; repo-level overrides; optional read-through proxy cache; apk/arch output.

### Reject (charter — see COMPETITORS.md)
RBAC/identity platform · web UI/dashboard · mirroring-as-core · plugin platform + external DB · 20+ formats · format **conversion** · `.changes` ceremony · deltarpm · billing.

## 🗺 Strategic bets
1. Make apt **and** yum production-grade + trusted (depth > breadth).
2. `pack` = the embeddable, pure-Rust, **deterministic** packager that also **publishes** — the gap nfpm/FPM leave open.
3. The wedge story: **"your own signed apt+rpm repo in 5 minutes, one-line CI push, atomic rollback — one static binary, no platform."**

## Parallelization policy
- Sequential (shared files `arx/src/main.rs`, `cli.rs`, `signing.rs`): key-encryption, GC/delete, serve-security.
- Parallelizable (isolated): `pack` crate, docs, independent test suites → run via worktree-isolated agents.
