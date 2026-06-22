# ArtifactX Roadmap

> **Current phase:** v0.2.0 publish/API completeness  
> **Next planning lane:** v0.3.0 pack ergonomics  
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
| ✅ Done | [`v0.1.x — Import-first polish`](https://github.com/artifactx-rs/artifactx/milestone/1) | Shipped | Import → publish → serve/pages → install → rollback is documented, tested, and dogfooded. |
| 🔵 Active | [`v0.2.0 — Publish/API completeness`](https://github.com/artifactx-rs/artifactx/milestone/2) | Design + implementation | Make publish, migration, GC/search, and HTTP API workflows complete enough for developer-facing use. |
| 🟢 Planned | [`v0.3.0 — Pack ergonomics`](https://github.com/artifactx-rs/artifactx/milestone/3) | Designed, parked until v0.2 closes | Focus `arx pack`: directory payloads, Cargo metadata bridges, config files, reproducibility knobs, and pack docs. |
| 🟡 Future | [`v0.4.0 — UI and console`](https://github.com/artifactx-rs/artifactx/milestone/4) | Parked | A focused web console after CLI/API semantics are stable. |
| 🟠 Future | [`v0.5.0 — OCI, Helm, and cloud-native integrations`](https://github.com/artifactx-rs/artifactx/milestone/5) | Parked | OCI registry, Helm charts, and Kubernetes/cloud-native integration. |
| 🟣 Future | [`v0.6.0 — Distributed delivery`](https://github.com/artifactx-rs/artifactx/milestone/6) | Parked | Distributed repository replication, edge delivery, and multi-site distribution. |
| ⚪ Later | No active milestone | Parked | Plausible bets that wait until publish/API, pack, UI, and cloud-native paths are clearer. |

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

## ✅ Done — v0.1.x import-first polish

Milestone: [`v0.1.x — Import-first polish`](https://github.com/artifactx-rs/artifactx/milestone/1)

This lane is shipped. The public docs and e2e coverage now make this path
trustworthy enough for v0.1.x:

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
| Import confidence | ✅ Done ([#16](https://github.com/artifactx-rs/artifactx/issues/16)) | Realistic apt/yum fixtures; clear docs for what import preserves/regenerates; errors name the bad upstream metadata or package URL. |
| Client trust path | ✅ Done ([#17](https://github.com/artifactx-rs/artifactx/issues/17)) | Signing docs explain repo metadata vs package signatures; apt/dnf snippets work against published layout. |
| Release + Pages dogfood | ✅ Done ([#18](https://github.com/artifactx-rs/artifactx/issues/18)) | Manual Pages publish is safe; tag releases produce binaries and aliases; private keys never enter Pages artifacts. |
| Operator ergonomics | ✅ Done ([#19](https://github.com/artifactx-rs/artifactx/issues/19)) | First-run docs cover `init`, `import`, `publish`, `serve`, backup/restore, rollback, systemd, and Docker without overclaiming. |
| Adversarial review | ✅ Done ([#31](https://github.com/artifactx-rs/artifactx/issues/31)) | README/Pages/CI are reviewed for a clear wedge, no vague platform promises, no secret leakage. |

## 🔵 Active — v0.2.0 publish/API completeness

Milestone: [`v0.2.0 — Publish/API completeness`](https://github.com/artifactx-rs/artifactx/milestone/2)

This milestone is about finishing the **Publish** pillar and the developer-facing
HTTP/API surface before broadening `pack`. The product promise for v0.2 is:

```text
packages or existing apt/yum repos
  -> arx add/import/migrate
  -> arx publish / export / cutover with preflight
  -> API/search/GC/history/rollback are scriptable and documented
  -> apt/yum clients keep working, including old CentOS 7 gzip metadata
```

v0.2 is done only when publish/API workflows are complete enough that another
team can automate them without maintaining site-specific shell glue for the
common path.

### v0.2 publish and migration hardening

These items come from the 2026-06-22 `d.qg.net` dogfood and define the publish
contract that must be boring before v0.3 pack work becomes the main focus.

| Work item | Priority | Tracking | Done means |
| --- | --- | --- | --- |
| One-command signed import + publish | P0 | [#43](https://github.com/artifactx-rs/artifactx/issues/43) | Migration can intentionally import and publish/re-sign repo metadata in one flow, with apt+yum client e2e coverage. |
| Preserve apt Release identity | P0 | [#54](https://github.com/artifactx-rs/artifactx/issues/54) | `Origin`, `Label`, `Suite`, and `Codename` are preserved or explicitly overridden so apt clients do not fail on surprise identity changes. |
| Dirty yum metadata report / strict gate | P0 | [#47](https://github.com/artifactx-rs/artifactx/issues/47) | Stale/missing RPM metadata is either a hard blocker or a clear accepted delta before cutover. |
| RPM package-signature preflight | P0 | [#55](https://github.com/artifactx-rs/artifactx/issues/55) | `gpgcheck=1` yum cutovers know whether payload RPMs are signed; repo metadata signing is reported separately. |
| One-command production publish/cutover | P1 | [#49](https://github.com/artifactx-rs/artifactx/issues/49) | The common production path feels like `add -> publish`, with staging validation, atomic promotion, and rollback notes built in. |
| Cutover preflight | P1 | [#56](https://github.com/artifactx-rs/artifactx/issues/56) | A staging export is validated for apt, yum, legacy `/deb` + flat `/repo`, CentOS 7 `.gz`, and rollback before live paths move. |
| Safe service/sync integration guide | P1 | [#57](https://github.com/artifactx-rs/artifactx/issues/57) | Docs distinguish ArtifactX publish success from downstream `sync-srv` / `file-monitor` success and keep private state out of public roots. |
| Migration e2e fixture suite | P1 | [#58](https://github.com/artifactx-rs/artifactx/issues/58) | CI/local fixtures cover apt identity, aptly hash-prefixed debs, dirty yum metadata, CentOS 7 gzip metadata, and API workflows. |

### v0.2 API and operator query surface

| Work item | Priority | Tracking | Done means |
| --- | --- | --- | --- |
| API readiness before stable public use | P0 | [#51](https://github.com/artifactx-rs/artifactx/issues/51) | `/api/v1` has a compatibility stance, stable error shapes, Swagger/OpenAPI docs, auth examples, and e2e examples before being called stable. |
| Search command + package query API | P0 | [#50](https://github.com/artifactx-rs/artifactx/issues/50) | Operators can query package families/versions/scopes before `gc`, `rm`, `promote`, or cutover; JSON output is available. |
| Package-scoped GC + rollback-state retention | P1 | [#52](https://github.com/artifactx-rs/artifactx/issues/52) | Old package families such as `wss-*` can be dry-run and pruned safely, with rollback-state pinning explained and controllable. |
| Directory inputs for add/import | P1 | [#33](https://github.com/artifactx-rs/artifactx/issues/33) | Existing `.deb` / `.rpm` drop directories can be discovered in stable order with clear failure behavior. |
| CI slimming + Rust-idiomatic cleanup | P1 | [#34](https://github.com/artifactx-rs/artifactx/issues/34) | Contributor feedback stays fast while release confidence remains deterministic. |

### v0.2 definition of done

- `publish`, import/migrate, export/cutover, rollback/history, search, GC, and
  API workflows have documented happy paths and failure modes.
- The HTTP API can be safely opened to friendly developers as beta, with OpenAPI
  and Swagger UI/docs entry points.
- Every publish/API feature lands with regression tests plus apt/yum e2e coverage
  where the feature affects client behavior.
- Production dogfood no longer requires bespoke shell glue for the common publish
  path; site-specific sync remains documented as an integration boundary.

## 🟢 Planned — v0.3.0 pack ergonomics

Milestone: [`v0.3.0 — Pack ergonomics`](https://github.com/artifactx-rs/artifactx/milestone/3)

v0.3 deliberately narrows focus to **Package** pillar ergonomics. Pack work stays
parked behind v0.2 publish/API completeness so the repo server path remains
boring before ArtifactX grows more package-authoring surface.

### Directory workflow clarification

Issue: [#14 — proposal: Add a DirEntry struct](https://github.com/artifactx-rs/artifactx/issues/14)

| Candidate | Status | Tracking |
| --- | --- | --- |
| Clarify issue #14 scope | 🔵 Open | Confirm whether the request means `arx pack` payload directories, `arx add` / import directory inputs, or both; v0.3 owns the pack side. |
| Package payload directories | 🔵 Proposed ([#32](https://github.com/artifactx-rs/artifactx/issues/32)) | [ADR-0018](docs/adr/0018-directory-entries-for-package-manifests.md): `[[dirs]]` manifest entries, deterministic expansion, shared `.deb`/`.rpm`/`.apk` semantics. |

### Pack v0.3.0 TODO

| Work item | Priority | Why it matters |
| --- | --- | --- |
| Cargo target/profile controls | P1 ([#26](https://github.com/artifactx-rs/artifactx/issues/26)) | Current Cargo.toml mode assumes `target/release/<bin>`. v0.3.0 should design `--target`, `--profile`, `--target-dir`, and/or explicit binary path without making `pack` drive `cargo build`. |
| Rust packaging bridge: cargo-deb + cargo-rpm + arx overlay | P1 ([#27](https://github.com/artifactx-rs/artifactx/issues/27)) | Reuse the useful common subset of `[package.metadata.deb]`, `[package.metadata.generate-rpm]`, and legacy `[package.metadata.rpm]`, then layer `[package.metadata.arx]` on top for ArtifactX-only cross-format and publish-aware behavior. |
| Config-file marking | P1 ([#28](https://github.com/artifactx-rs/artifactx/issues/28)) | Design deb `conffiles` / equivalent manifest intent before users rely on ad-hoc maintainer scripts for config paths. |
| Explicit source date CLI | P2 ([#29](https://github.com/artifactx-rs/artifactx/issues/29)) | Consider `arx pack --source-date <epoch>` as a discoverable wrapper around `SOURCE_DATE_EPOCH` while preserving reproducible defaults. |
| Pack docs completeness | P1 ([#30](https://github.com/artifactx-rs/artifactx/issues/30)) | Clearly document limits: no inline package signing, no auto dependency detection, no symlink following, no source packages, and no `.apk` repository add path yet. |

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

## 🟡 Future — v0.4.0 UI and console

Milestone: [`v0.4.0 — UI and console`](https://github.com/artifactx-rs/artifactx/milestone/4)  
Tracking issue: [#62 — Roadmap: v0.4 UI and console](https://github.com/artifactx-rs/artifactx/issues/62)

This is intentionally parked until v0.2 publish/API and v0.3 pack ergonomics are
solid. The UI should be a thin, trustworthy console over stable CLI/API semantics,
not a second product with separate behavior.

| Bet | Why it waits |
| --- | --- |
| Repository dashboard | Needs stable publish/history/rollback/search APIs first. |
| Package/search browser | Needs v0.2 search and package query API to be boring. |
| Cutover and rollback console | Needs v0.2 cutover preflight and rollback-state semantics. |
| Auth/user stories | Needs an API compatibility and auth stance before UI commitments. |

## 🟠 Future — v0.5.0 OCI, Helm, and cloud-native integrations

Milestone: [`v0.5.0 — OCI, Helm, and cloud-native integrations`](https://github.com/artifactx-rs/artifactx/milestone/5)  
Tracking issue: [#63 — Roadmap: v0.5 OCI, Helm, and cloud-native integrations](https://github.com/artifactx-rs/artifactx/issues/63)

This lane is for cloud-native distribution only after the native package repo
story is stable. The goal is to meet teams where they already deploy, not to turn
ArtifactX into a giant registry platform.

| Bet | Why it waits |
| --- | --- |
| OCI artifact support | Needs a tight ADR to preserve the one-binary, boring-ops model. |
| Helm chart repository / OCI Helm flow | Needs clear boundaries between package repos, charts, and container registries. |
| Kubernetes deployment examples | Wait until service/API config and auth are stable enough to document cleanly. |
| Cloud-native signing/provenance hooks | Must compose with repo signing instead of blurring package vs metadata trust. |

## 🟣 Future — v0.6.0 distributed delivery

Milestone: [`v0.6.0 — Distributed delivery`](https://github.com/artifactx-rs/artifactx/milestone/6)  
Tracking issue: [#64 — Roadmap: v0.6 distributed delivery](https://github.com/artifactx-rs/artifactx/issues/64)

This is the bigger bet: distribution, replication, and multi-site delivery. It is
explicitly after publish/API, pack, UI, and cloud-native fundamentals because it
adds operational complexity that ArtifactX should only accept with strong demand.

| Bet | Why it waits |
| --- | --- |
| Multi-site repository replication | Needs real measurements and failure-mode ADRs before adding moving parts. |
| Edge/cache-friendly publish model | Needs stable metadata identity, rollback, and retention semantics first. |
| Signed distribution manifests | Needs a clean trust model across repository metadata and mirrors. |
| Read-through proxy cache / full mirroring platform | ArtifactX is not trying to become Artifactory/aptly; prove the small version first. |

## ⚪ Later / parked bets

These are plausible, but intentionally **not current focus**:

| Idea | Why parked |
| --- | --- |
| HSM / KMS-backed repository signing | Needs an ADR proving it preserves the one-binary / 5-minute path. |
| Auto dependency detection (`--auto-deps`) | Usually needs host tools (`ldd`, `objdump`, package DBs) and can undermine deterministic pack. |
| Multi-arch manifests | Useful, but wait until single-arch pack ergonomics are excellent. |
| `arx pack --sign` inline package signing | Package signing is intentionally separate from repository metadata signing today. |
| Arch Linux `.pkg.tar.zst` support | New package ecosystem; wait until the apt/yum/apk paths are consistently excellent. |
| Object storage backend | See [ADR-0015](docs/adr/0015-object-storage-backend-deferred.md); deferred. |
| Large-repo performance beyond current bottlenecks | Optimize when import/publish measurements demand it. |
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
