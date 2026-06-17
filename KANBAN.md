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
| **P0 — repo product-readiness (ADR-0011)** | apt `Release` `Valid-Until` (freeze protection); bad/duplicate package isolation (skip-and-warn, always visible; `--strict`; push→422); version-aware GC (dpkg/rpm EVR, not mtime). Verified e2e | [ADR](docs/adr/0011-repo-product-readiness.md) · [`785f9ba`](https://github.com/artifactx-rs/artifactx/commit/785f9ba) · [`5bd5126`](https://github.com/artifactx-rs/artifactx/commit/5bd5126) · [`bb3e31e`](https://github.com/artifactx-rs/artifactx/commit/bb3e31e) · [`6f8abe0`](https://github.com/artifactx-rs/artifactx/commit/6f8abe0) |
| **yum e2e test + backup runbook** | yum repodata integration test (binary-driven, structure + signature) + [`docs/OPERATIONS.md`](docs/OPERATIONS.md) backup/restore (ADR-0011 bars #4/#5) | [`c286725`](https://github.com/artifactx-rs/artifactx/commit/c286725) |
| **P0 — pack product-readiness (ADR-0012)** | reproduciblity (rpm source_date fix; all 3+4 timestamp sites clamped); fail-loud arch + file-type gate; Cargo workspace support ([[bin]].name, target-dir, inherited fields); real dpkg-deb/rpm validation (CI forced). Verified: deb+rpm byte-identical, all 49 tests green incl. 4 real-tool. | [ADR](docs/adr/0012-pack-product-readiness.md) · [`a8da659`](https://github.com/artifactx-rs/artifactx/commit/a8da659) · [`b9abe31`](https://github.com/artifactx-rs/artifactx/commit/b9abe31) · [`4c0af1b`](https://github.com/artifactx-rs/artifactx/commit/4c0af1b) · [`24016bf`](https://github.com/artifactx-rs/artifactx/commit/24016bf) |
| **Incremental publish (ADR-0013)** | apt: file-manifest cache (mtime,size→sha256+stanza) — no-op publish skips .deb body reads; yum: manifest detects no-change → skips repodata rebuild. End-to-end verified (manifest → cache hit → --full) | [ADR](docs/adr/0013-incremental-publish.md) · [`a82bfec`](https://github.com/artifactx-rs/artifactx/commit/a82bfec) · [`9ac9081`](https://github.com/artifactx-rs/artifactx/commit/9ac9081) |
| **OIDC keyless push (ADR-0014)** | server: JWT validation (GitHub JWKS, RS256, repo allowlist); client: auto-detect GitHub Actions OIDC. 52 tests green | [ADR](docs/adr/0014-oidc-keyless-push.md) · [`e498651`](https://github.com/artifactx-rs/artifactx/commit/e498651) |
| **gc --keep-within** | Time-based retention window — `--keep-within 90d` protects recent files regardless of version count | [`751d5ae`](https://github.com/artifactx-rs/artifactx/commit/751d5ae) |
| **Custom key dir + pool dir** | `arx init --key-dir --pool-dir`; config `[signing].keys_dir`, `[apt].pool_dir`, `[yum].base_dir` | [`06e55c2`](https://github.com/artifactx-rs/artifactx/commit/06e55c2) |
| **CI dogfood** | `ci.yml` builds release + runs `arx pack crates/arx/Cargo.toml` + validates with dpkg-deb | [`421e3b6`](https://github.com/artifactx-rs/artifactx/commit/421e3b6) |
| **Contents-<arch>** | `apt-file` support — extracts data.tar paths, writes Contents-<arch> + Contents-<arch>.gz with tab-separated `<path>\t<package>` format | [`dcc9737`](https://github.com/artifactx-rs/artifactx/commit/dcc9737) |
| **Key rotation** | `arx key rotate` (generates new key, backs up old) + `arx key revoke` (deletes backup) | _(pending push)_ |
| **gc --grace + bytes-freed** | `--grace N` defers deletion for N days; output shows human-readable bytes freed | _(pending push)_ |
| **arx promote** | `arx promote <name> --from <comp> --to <comp>` moves packages between components | _(pending push)_ |
| **arx watch (incoming/)** | `arx watch <dir> --root <repo>` polls for new .deb/.rpm, auto-adds + publishes | [`fa2f505`](https://github.com/artifactx-rs/artifactx/commit/fa2f505) |
| **Pack Docker backend** | Real implementation — mounts arx + source files into container, builds, extracts artifacts | [`c628a3f`](https://github.com/artifactx-rs/artifactx/commit/c628a3f) |
| **Pack .apk builder** | Pure-Rust Alpine Linux `.apk` assembler (tar.gz + .PKGINFO + payload). Native + Backend API. | [`c628a3f`](https://github.com/artifactx-rs/artifactx/commit/c628a3f) |
| **Object-storage ADR** | [ADR-0015](docs/adr/0015-object-storage-backend-deferred.md) — deferred with architecture sketch | [`c628a3f`](https://github.com/artifactx-rs/artifactx/commit/c628a3f) |
| **Coverage** | 57 tests green (4 new: human_bytes, promote CLI, key rotate/revoke, apk). All 4 workspace crates covered. | [`c628a3f`](https://github.com/artifactx-rs/artifactx/commit/c628a3f) |
| **arx import** | Import packages from existing apt/yum repos (Ubuntu, ClickHouse, Docker CE verified). Packages.gz autodetect, --match-name, --limit. Heavyweight feature. | [`fc355b1`](https://github.com/artifactx-rs/artifactx/commit/fc355b1) |
| Published to GitHub (public) | `artifactx-rs/artifactx` + [GitHub Project **Done=24 Todo=1**](https://github.com/orgs/artifactx-rs/projects/1) + [Wiki](https://github.com/artifactx-rs/artifactx/wiki) | — |

## 🔨 In progress

_(empty)_

## 📋 Backlog

| Item | Notes |
|---|---|
| **arx mirror** | Full repository mirror — sync upstream apt/yum repo, incremental fetch, version diff, scheduled sync. Builds on import infrastructure. |
| ~~REST API first-class parity~~ | ✅ Done — 5 new endpoints (publish/rollback/history/import/promote). Adversarial review: caught missing PublishLock + unwrap, both fixed. [`884913d`](https://github.com/artifactx-rs/artifactx/commit/884913d) |
| **Nix flake** | `flake.nix` — zero-install for Nix users: `nix run github:artifactx-rs/artifactx` builds + runs arx; `nix develop` drops into a dev shell with the Rust toolchain. |
- More formats beyond deb/rpm/apk → `pack` crate extension point

### Reject (charter — see COMPETITORS.md)
RBAC/identity platform · web UI/dashboard · mirroring-as-core · plugin platform + external DB · 20+ formats · format **conversion** · `.changes` ceremony · deltarpm · billing.

## 🗺 Strategic bets
1. Make apt **and** yum production-grade + trusted (depth > breadth).
2. `pack` = the embeddable, pure-Rust, **deterministic** packager that also **publishes** — the gap nfpm/FPM leave open.
3. The wedge story: **"your own signed apt+rpm repo in 5 minutes, one-line CI push, atomic rollback — one static binary, no platform."**

## Parallelization policy
- Sequential (shared files `arx/src/main.rs`, `cli.rs`, `signing.rs`): key-encryption, GC/delete, serve-security.
- Parallelizable (isolated): `pack` crate, docs, independent test suites → run via worktree-isolated agents.
