# ArtifactX Roadmap

> **Current phase:** v0.1.x import-first polish  
> **Next planning lane:** v0.2.0 packaging ergonomics  
> **Product wedge:** **Import first. Cut over when ready.**

ArtifactX is public now, so this roadmap is written for contributors as much as
for maintainers. It answers three questions:

1. What is already shipped?
2. What are we polishing right now?
3. What is planned next, and what is intentionally parked?

## Status at a glance

| Lane | GitHub milestone | Status | What it means |
| --- | --- | --- | --- |
| ✅ Shipped | v0.1.0 / v0.1.x | Done / polish | Core repository + packager exists and is dogfooded. |
| 🟢 Now | [`v0.1.x — Import-first polish`](https://github.com/artifactx-rs/artifactx/milestone/1) | Active | Make migration boring: import → publish → serve/pages → install → rollback. |
| 🔵 Next | [`v0.2.0 — Packaging ergonomics`](https://github.com/artifactx-rs/artifactx/milestone/2) | Design + selected implementation | Improve `arx pack` and directory workflows without breaking the 5-minute path. |
| 🟣 Later | No active milestone | Parked | Plausible bets that wait until the core path is excellent. |

Public project board: <https://github.com/orgs/artifactx-rs/projects/1>

Milestone progress is issue-based: completed shipped work is represented by closed tracking issues, while active/future TODOs stay open until implemented or intentionally deferred.

## ✅ What we have — v0.1.0 / v0.1.x

### Repository pillar

| Capability | Status | Notes |
| --- | --- | --- |
| apt repo generation | ✅ Shipped | `Release`, `InRelease`, `Release.gpg`, by-hash, `Acquire-By-Hash`. |
| yum/dnf repo generation | ✅ Shipped | `repomd.xml`, `repomd.xml.asc`, primary/filelists/other metadata. |
| Import from existing repos | ✅ Shipped | Pull bounded apt/yum slices, then regenerate metadata under your key. |
| Atomic publish | ✅ Shipped | staging → symlink flip; clients never see half-written metadata. |
| Atomic rollback/history | ✅ Shipped | `arx rollback` / `arx history` for apt and yum. |
| Incremental publish | ✅ Shipped | File-manifest cache avoids O(repo) work for unchanged packages. |
| Version-aware GC | ✅ Shipped | dpkg/rpm EVR sorting, `--keep-within`, `--grace`. |
| OIDC keyless push | ✅ Shipped | GitHub Actions JWT; no stored long-lived token. |
| Contents indices | ✅ Shipped | `Contents-<arch>` / apt-file support. |
| Key rotation | ✅ Shipped | `arx key rotate` + revoke/export paths. |
| Promote / watch | ✅ Shipped | Move packages between scopes; polling drop-dir auto-ingest. |
| HTTP API | ✅ Shipped | `/api/v1/packages`, `/gc`, `/health`, `/metrics`. |

### Package pillar

| Capability | Status | Notes |
| --- | --- | --- |
| `.deb` builder | ✅ Shipped | Pure Rust `ar` + `tar` + `flate2`; deterministic. |
| `.rpm` builder | ✅ Shipped | Uses the Rust `rpm` crate; `SOURCE_DATE_EPOCH` reproducible. |
| `.apk` builder | ✅ Shipped | Alpine package output via tar.gz + `.PKGINFO`. |
| Cargo.toml-driven pack | ✅ Shipped | Zero-config default for simple Rust CLIs. |
| Docker backend | ✅ Shipped | Containerized fallback path for build isolation. |

### Verification baseline

- CI runs workspace tests and clippy.
- E2E coverage exercises apt-get on Debian and dnf on Fedora where host tooling is available.
- Release workflow builds static binaries, self-packages arx, and dogfoods a signed GitHub Pages repo.

## 🟢 Now — v0.1.x import-first polish

Milestone: [`v0.1.x — Import-first polish`](https://github.com/artifactx-rs/artifactx/milestone/1)

No new package formats, storage backends, dashboards, or broad CLI surfaces until
this path feels trustworthy:

```text
existing apt/yum repo
  -> arx import a bounded slice
  -> arx publish signed metadata
  -> arx serve or GitHub Pages
  -> apt/dnf client install
  -> rollback when needed
```

| Gate | Status | Done means |
| --- | --- | --- |
| Import confidence | 🟢 Active ([#16](https://github.com/artifactx-rs/artifactx/issues/16)) | Realistic apt/yum fixtures; clear docs for what import preserves/regenerates; errors name the bad upstream metadata or package URL. |
| Client trust path | 🟢 Active ([#17](https://github.com/artifactx-rs/artifactx/issues/17)) | Signing docs explain repo metadata vs package signatures; apt/dnf snippets work against published layout. |
| Release + Pages dogfood | 🟢 Active ([#18](https://github.com/artifactx-rs/artifactx/issues/18)) | Manual Pages publish is safe; tag releases produce binaries and aliases; private keys never enter Pages artifacts. |
| Operator ergonomics | 🟢 Active ([#19](https://github.com/artifactx-rs/artifactx/issues/19)) | First-run docs cover `init`, `import`, `publish`, `serve`, backup/restore, rollback, systemd, and Docker without overclaiming. |
| Adversarial review | 🟢 Active ([#31](https://github.com/artifactx-rs/artifactx/issues/31)) | README/Pages/CI are reviewed for a clear wedge, no vague platform promises, no secret leakage. |

## 🔵 Next — v0.2.0 packaging ergonomics

Milestone: [`v0.2.0 — Packaging ergonomics`](https://github.com/artifactx-rs/artifactx/milestone/2)

This milestone is about making `arx pack` and directory workflows feel natural for
Rust projects and migration-heavy package repos. The goal is **not** to become a
large packaging framework; it is to delete glue around the common paths.

### Directory workflow clarification

Issue: [#14 — proposal: Add a DirEntry struct](https://github.com/artifactx-rs/artifactx/issues/14)

| Candidate | Status | Tracking |
| --- | --- | --- |
| Clarify issue #14 scope | 🔵 Open | Confirm whether the request means `arx pack` payload directories, `arx add` / import directory inputs, or both. |
| Package payload directories | 🔵 Proposed ([#32](https://github.com/artifactx-rs/artifactx/issues/32)) | [ADR-0018](docs/adr/0018-directory-entries-for-package-manifests.md): `[[dirs]]` manifest entries, deterministic expansion, shared `.deb`/`.rpm`/`.apk` semantics. |
| Directory inputs for add/import | 🔵 Proposed ([#33](https://github.com/artifactx-rs/artifactx/issues/33)) | [ADR-0019](docs/adr/0019-directory-inputs-for-add-and-import.md): discover existing `.deb` / `.rpm` files from directories with stable ordering and clear failure behavior. |
| Aptly hash-prefixed `.deb` imports | 🟢 Guarded ([#35](https://github.com/artifactx-rs/artifactx/issues/35)) | `arx import --apt` and `arx publish` must treat aptly hash prefixes as storage detail: follow `Packages` `Filename:` exactly, but read identity from `.deb` control fields. Regression coverage is in place; optional filename normalization stays a separate design decision. |
| Apt migration hardening | 🔵 Open ([#36](https://github.com/artifactx-rs/artifactx/issues/36)) | Grey-test real aptly repositories, then cover remaining migration edge cases: basename collisions after flattening, extra `Packages` compression variants, absolute `Filename:` URLs, and explicit upstream trust/signature behavior. |

### Pack v0.2.0 TODO

| Work item | Priority | Why it matters |
| --- | --- | --- |
| Cargo target/profile controls | P1 ([#26](https://github.com/artifactx-rs/artifactx/issues/26)) | Current Cargo.toml mode assumes `target/release/<bin>`. v0.2.0 should design `--target`, `--profile`, `--target-dir`, and/or explicit binary path without making `pack` drive `cargo build`. |
| Rust packaging bridge: cargo-deb + cargo-rpm + arx overlay | P1 ([#27](https://github.com/artifactx-rs/artifactx/issues/27)) | Reuse the useful common subset of `[package.metadata.deb]`, `[package.metadata.generate-rpm]`, and legacy `[package.metadata.rpm]`, then layer `[package.metadata.arx]` on top for ArtifactX-only cross-format and publish-aware behavior. |
| Config-file marking | P1 ([#28](https://github.com/artifactx-rs/artifactx/issues/28)) | Design deb `conffiles` / equivalent manifest intent before users rely on ad-hoc maintainer scripts for config paths. |
| Explicit source date CLI | P2 ([#29](https://github.com/artifactx-rs/artifactx/issues/29)) | Consider `arx pack --source-date <epoch>` as a discoverable wrapper around `SOURCE_DATE_EPOCH` while preserving reproducible defaults. |
| Pack docs completeness | P1 ([#30](https://github.com/artifactx-rs/artifactx/issues/30)) | Clearly document limits: no inline package signing, no auto dependency detection, no symlink following, no source packages, and no `.apk` repository add path yet. |

### Engineering quality track

| Work item | Priority | Why it matters |
| --- | --- | --- |
| CI slimming + Rust-idiomatic cleanup | P1 ([#34](https://github.com/artifactx-rs/artifactx/issues/34)) | Keep contributor feedback fast by splitting docs/site checks from full Rust CI, then measure and simplify slow paths without trading away deterministic release confidence. |

### Rust packaging bridge design note

ArtifactX should be more than a weak compatibility reader. The intended model is:

```text
Cargo.toml
  [package]
  [package.metadata.deb]          # existing cargo-deb investment
  [package.metadata.generate-rpm] # existing cargo-generate-rpm investment
  [package.metadata.rpm]          # legacy cargo-rpm investment
  [package.metadata.arx]          # ArtifactX overlay: cross-format + publish-aware
        ↓
  arx_pack::Manifest
        ↓
  .deb + .rpm + .apk + optional publish/add flow
```

Rules to design before implementation:

- `arx` metadata wins when schemas overlap.
- Existing cargo-deb / cargo-rpm metadata should reduce migration friction, not
  force ArtifactX to depend on those tools.
- ArtifactX-native config should cover shared `[[dirs]]`, deterministic knobs,
  publish defaults, and future repo integration.
- Rendering stays in ArtifactX: no `cargo-deb`, `cargo-generate-rpm`, `cargo-rpm`,
  or `rpmbuild` dependency.

## 🟣 Later / parked bets

These are plausible, but intentionally **not current focus**:

| Idea | Why parked |
| --- | --- |
| HSM / KMS-backed repository signing | Needs an ADR proving it preserves the one-binary / 5-minute path. |
| Auto dependency detection (`--auto-deps`) | Usually needs host tools (`ldd`, `objdump`, package DBs) and can undermine deterministic pack. |
| Multi-arch manifests | Useful, but wait until single-arch pack ergonomics are excellent. |
| `arx pack --sign` inline package signing | Package signing is intentionally separate from repository metadata signing today. |
| Arch Linux `.pkg.tar.zst` support | New package ecosystem; wait until import-first polish is done. |
| Object storage backend | See [ADR-0015](docs/adr/0015-object-storage-backend-deferred.md); deferred. |
| Read-through proxy cache / full mirroring platform | ArtifactX is not trying to become Artifactory/aptly. |
| Large-repo performance beyond current bottlenecks | Optimize when import/publish measurements demand it. |
| Web UI | Broad surface area; not needed for the 5-minute path. |
| Plug-in system | Too much platform surface until core workflows settle. |

## Contributor guide for roadmap items

- If the change is non-trivial, start with a `Proposed` ADR.
- If it affects public workflows, link the ADR, issue, and project item.
- Prefer designs that delete glue over designs that add modes.
- Keep `.deb`, `.rpm`, and `.apk` behavior aligned unless an ADR explicitly says otherwise.
- Do not implement v0.2.0 items until the relevant issue/ADR has enough agreement.

## Philosophy (from the charter)

1. **Compete by deleting.** Before adding one feature, consider removing two.
2. **One binary.** No database, no daemon, no cluster.
3. **Design for operations.** stateless · deterministic · atomic · observable.
4. **The 5-minute rule.** install → create/import → publish → consume in 5 minutes.
5. **Think like Caddy.** Defaults are correct. Configuration disappears whenever possible.
