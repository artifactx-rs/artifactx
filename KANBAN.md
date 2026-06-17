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
| Published to GitHub | `artifactx-rs/artifactx` (private) + Project board `artifactx-rs/projects/1` |

## 🔨 In progress

| Item | Owner | Notes |
| --- | --- | --- |
| _(next: P0 — package delete / GC / retention)_ | main | not started |

## 📋 Backlog

### P0 — production credibility
- **Package delete / yank + GC / retention** — `arx rm` / `arx gc`; remove from pool, prune old versions, republish.
- **serve security** — built-in TLS + token auth (or official reverse-proxy/TLS templates); audit `/keys`·`/apt`·`/yum` path handling.

### P1 — scale & correctness
- Incremental `publish` (don't re-hash the whole pool every time).
- Duplicate-`add` handling (dedupe / reject same name+version).
- `Contents-<arch>` (enables `apt-file`).
- Key rotation / revocation story.

### P2 — later
- `Translation-*`, delta-rpm, snapshots/rollback, mirroring.
- Object-storage backends (S3); hosted/managed mode; web UI.
- More formats: `.apk` (Alpine), Arch.

## 🗺 Strategic bets (from review)
1. Make **one** format production-grade + trusted (depth > breadth).
2. Ship an **embeddable** packaging lib (`pack`) — the only defensible moat vs Cloudsmith/JFrog/Pulp.
3. Pick a sharp wedge audience + a "5-minute signed apt+rpm repo (with a GitHub Action)" story.

## Parallelization policy
- Sequential (shared files `arx/src/main.rs`, `cli.rs`, `signing.rs`): key-encryption, GC/delete, serve-security.
- Parallelizable (isolated): `pack` crate, docs, independent test suites → run via worktree-isolated agents.
