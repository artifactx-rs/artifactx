# ArtifactX — Project Board

> Durable source of truth for what's done, in flight, and queued. Survives across
> sessions. Derived from the adversarial user/investor review (2026-06).
> Legend: **P0** = blocks production credibility · **P1** = scale/UX · **P2** = nice-to-have.

## ✅ Done

> Commit links for traceability. Base: `https://github.com/artifactx-rs/artifactx/commit/`.

| Item | Notes | Commit |
| --- | --- | --- |
| Workspace scaffold | `crates/arx` (CLI, GPL) + `crates/debrepo` (lib, MIT/Apache) | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| yum/dnf repodata + signing | via `createrepo_rs`; `repomd.xml.asc`; verified with real `dnf` | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| apt repo generation + signing | `debrepo`; `InRelease`/`Release.gpg`; verified with real `apt-get` | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| Built-in HTTP serve + `/metrics` | axum + tower-http; Prometheus; `tracing` logs | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| `compose` generator | `Dockerfile` + `docker-compose.yml` | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| Version stamping | vergen → `arx --version` (git sha / build time / rustc) | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| READMEs (×3) + logo | platform + arx + debrepo | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| **P0 — multi-dist/component atomic publish** | single `Release` per dist; `by-hash`; staging→commit swap; publish lock | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| **P0 — private key encryption + passphrase** | S2K-encrypted key via `ARX_KEY_PASSPHRASE`/`--passphrase-file`; default stays frictionless (5-min rule) + warns | [`26f0260`](https://github.com/artifactx-rs/artifactx/commit/26f0260) |
| **`pack` PoC** (packaging moat) | `crates/pack`: manifest → `.deb`/`.rpm`, pure-Rust native-first, Docker fallback stub, build hygiene | [`dd2ee59`](https://github.com/artifactx-rs/artifactx/commit/dd2ee59) |
| Competitive teardown | [`COMPETITORS.md`](COMPETITORS.md) + public org landing page | [`002c88e`](https://github.com/artifactx-rs/artifactx/commit/002c88e) |
| **P0 — package delete / GC / retention** | `arx rm <name> [--version]` (yank) + `arx gc --keep N [--dry-run]`; 3 integration tests | [`56b9ace`](https://github.com/artifactx-rs/artifactx/commit/56b9ace) |
| **P0 — serve security** | optional bearer-token auth (`ARX_SERVE_TOKEN`); `ServeDir` blocks path traversal; TLS via reverse proxy | [`237ac1f`](https://github.com/artifactx-rs/artifactx/commit/237ac1f) |
| **`pack` relationships** | manifest `depends`/`conflicts`/`provides`/`replaces` + maintainer scripts → deb control + rpm | [`237ac1f`](https://github.com/artifactx-rs/artifactx/commit/237ac1f) |
| **`arx push` + REST API** | `/api/v1/packages` (GET/POST/DELETE), `/api/v1/gc`, `/health`; bearer-auth; `arx push` client | [`7e9a770`](https://github.com/artifactx-rs/artifactx/commit/7e9a770) |
| **`arx pack`** | manifest → `.deb`/`.rpm` in the CLI; `--add` into the pool — Build·Package·Publish in one binary | [`bc64612`](https://github.com/artifactx-rs/artifactx/commit/bc64612) |
| **Cargo.toml-driven `pack`** | `arx pack` reads `[package]` + `[package.metadata.arx]`; convention default binary — zero-config for Rust CLIs (steal cargo-deb's idea; ADR-0010) | [`288a6c4`](https://github.com/artifactx-rs/artifactx/commit/288a6c4) |
| **docs + design-first** | `docs/DESIGN.md` + 9 ADRs; "design → review → build" in the charter | [`e6311d4`](https://github.com/artifactx-rs/artifactx/commit/e6311d4) |
| **atomic rollback (apt + yum)** | immutable state dir + atomic symlink flip (shared `debrepo::statedir`); `arx rollback`/`history`; `gc` pins referenced files ([ADR-0008](docs/adr/0008-atomic-rollback.md)) | [`2fad4e9`](https://github.com/artifactx-rs/artifactx/commit/2fad4e9) · [`2b5a000`](https://github.com/artifactx-rs/artifactx/commit/2b5a000) · [`60598c5`](https://github.com/artifactx-rs/artifactx/commit/60598c5) |
| **Dogfood + CI** | `arx` packs + publishes `arx` → GitHub Pages (`release.yml`); `ci.yml` clippy+test; verified incl. `apt-get install arx` (ADR-0009) | [`8513dc1`](https://github.com/artifactx-rs/artifactx/commit/8513dc1) |
| Published to GitHub (public) | `artifactx-rs/artifactx` + [Project board](https://github.com/orgs/artifactx-rs/projects/1) + [Wiki](https://github.com/artifactx-rs/artifactx/wiki) | — |

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
