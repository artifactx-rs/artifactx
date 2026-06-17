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
| Published to GitHub | `artifactx-rs/artifactx` (private) + Project board `artifactx-rs/projects/1` |

## 🔨 In progress

| Item | Owner | Notes |
| --- | --- | --- |
| Competitor teardown (`scout` agent) | research | aptly/Nexus/Pulp/JFrog/Cloudsmith/nfpm + classics (FPM/alien/dak/mini-dinstall…) → `COMPETITORS.md` + org README positioning |

## 📋 Backlog

> Prioritized via the competitive teardown — see [`COMPETITORS.md`](COMPETITORS.md).

### P0 — production credibility
- **serve security** — built-in TLS + token auth (or reverse-proxy/TLS templates); audit `/keys`·`/apt`·`/yum` path handling.

### P1 — the wedge (steal from aptly + nfpm + Cloudsmith)
- **`arx push`** — one-line publish; auto-detect dist/component/arch from the package; `curl -T` fallback; **GitHub Actions OIDC** keyless auth (no stored secret).
- **`arx rollback` / `arx history`** — atomic publish via immutable content-addressed states + pointer flip; expose ONLY these two verbs (never aptly's full snapshot CRUD).
- **Incremental publish by default** — createrepo_c `--update` style: republish is O(changes), not O(repo).
- **Retention policy** — `gc --keep-within 90d`; `gc --grace` window + bytes-freed report; semver-aware ordering.
- **`pack` manifest surface** — `--depends` / `--after-install` scripts; manifest→native per-format (never conversion); deterministic byte-output.

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
