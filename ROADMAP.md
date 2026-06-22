# ArtifactX Roadmap

> **v0.1.0 shipped.** The product wedge is now: **Import first. Cut over when ready.**

ArtifactX already has enough surface area. The next phase is a feature freeze focused on making the migration path trustworthy, boring, and easy to operate.

## What we have (v0.1.0 — June 2026)

### Repository
- **apt (Debian/Ubuntu)** — generate, sign, serve. Release/InRelease/Release.gpg, by-hash, Acquire-By-Hash.
- **yum/dnf (RHEL/Fedora/Rocky)** — generate, sign, serve. repomd.xml/repomd.xml.asc, primary/filelists/other.xml.gz.
- **Import from existing repos** — pull bounded apt/yum slices, then regenerate metadata under your signing key.
- **Atomic publish** — staging → symlink flip, clients never see half-written metadata.
- **Atomic rollback** — `arx rollback`/`history` for both apt and yum.
- **Incremental publish** — O(changes), not O(repo): file-manifest cache skips unchanged packages.
- **Version-aware GC** — dpkg/rpm EVR sorting, `--keep-within`, `--grace`.
- **OIDC keyless push** — GitHub Actions JWT, no stored token.
- **Contents-<arch>** — apt-file support.
- **Key rotation** — `arx key rotate` + `arx key revoke`.
- **Promote** — move packages between components/repos.
- **Watch** — polling drop-dir auto-ingest.
- **HTTP API** — `/api/v1/packages` (GET/POST/DELETE), `/gc`, `/health`, `/metrics`.
- **Static binary** — x86_64, aarch64 musl. GHCR Docker image.

### Pack (pure-Rust packager)
- **.deb** — ar + tar + flate2, deterministic, reproducible.
- **.rpm** — via rpm crate, SOURCE_DATE_EPOCH reproducible.
- **.apk** — Alpine Linux, tar.gz + .PKGINFO.
- **Cargo.toml-driven** — zero-config for Rust CLIs.
- **Docker backend** — builds inside a pinned container image.

### Verified
- Workspace tests and clippy pass in CI.
- Docker E2E covers apt-get on Debian and dnf on Fedora where host tooling is available.
- Release workflow builds static binaries, self-packages arx, and dogfoods a signed GitHub Pages repo.

## Current focus — import-first polish / feature freeze

No new package formats, storage backends, dashboards, or broad CLI features until the core adoption path is excellent:

```text
existing apt/yum repo
  -> arx import a bounded slice
  -> arx publish signed metadata
  -> arx serve or GitHub Pages
  -> apt/dnf client install
  -> rollback when needed
```

### Polish gates

1. **Import confidence**
   - Keep apt and yum import fixtures realistic: compressed metadata, relative URLs, arch filters, package-name filters, and bounded `--limit` migrations.
   - Document exactly what import preserves and what it regenerates.
   - Make failure messages point to the bad upstream metadata or missing package URL.

2. **Client trust path**
   - Keep signing-key docs explicit: repo metadata is signed; package signatures remain a build-pipeline responsibility.
   - Ensure GitHub Pages and self-hosted examples use stable imported keys, not throwaway demo keys.
   - Verify apt and dnf snippets against the published repo layout.

3. **Release and Pages dogfood**
   - Manual dispatch on `main` must publish the Pages demo without creating tags, releases, or GHCR images.
   - Tag pushes must produce versioned binaries plus stable `latest` download aliases.
   - Pages artifacts must never contain `keys/private.asc`.

4. **Operator ergonomics**
   - Improve first-run docs around `init`, `import`, `publish`, `serve`, backup/restore, and rollback.
   - Keep the 5-minute path honest: install → import/add → publish → consume.
   - Prefer deletion and sharper defaults over additional flags.
   - Roll out service management in order:
     1. `arx serve` defaults to localhost; public exposure is explicit.
     2. Document a copy-pasteable systemd unit with an env file, reverse-proxy stance, and journal/debug commands.
     3. Validate Docker/Compose examples against real containers before promoting them in Pages.
     4. Only after repeated use, consider a generator such as `arx systemd --print`; do not add it before the docs prove stable.

5. **Adversarial review**
   - Re-check the README and Pages copy from a user/investor perspective: one clear wedge, no overclaiming, no vague platform promises.
   - Re-check CI from a maintainer perspective: clear gates, no accidental version bumps, no secret leakage.

## Parked until after the freeze

These are plausible, but intentionally not current focus:

### v0.2 research candidates

#### v0.2.0 TODO

- **Clarify directory workflow from issue #14** — confirm whether the request is
  `arx pack` package payload directories, `arx add` / import directory inputs,
  or both before implementation.
- **Package payload directories** — design `[[dirs]]` manifest entries for
  deterministic, cross-format directory payloads; tracked by
  [ADR-0018](docs/adr/0018-directory-entries-for-package-manifests.md).
- **Directory inputs for add/import** — design directory discovery for existing
  `.deb` / `.rpm` package files, including recursion, filtering, stable ordering,
  and failure behavior; tracked by
  [ADR-0019](docs/adr/0019-directory-inputs-for-add-and-import.md).

#### Pack v0.2.0 TODO

- **Cargo target selection controls** — design `arx pack` support for the common
  Cargo build matrix without driving the build itself: `--target`, `--profile`,
  `--target-dir`, and/or an explicit binary path override. Current Cargo.toml
  mode assumes `target/release/<bin>`.
- **Rust packaging bridge: cargo-deb + cargo-rpm, one ArtifactX path** — evaluate
  reading the useful common subset of `[package.metadata.deb]`,
  `[package.metadata.generate-rpm]`, and legacy `[package.metadata.rpm]` into the
  shared `arx_pack::Manifest`, then layer ArtifactX-native
  `[package.metadata.arx]` on top for cross-format and publish-aware features.
  The goal is more than compatibility: projects already using cargo-deb plus
  cargo-generate-rpm/cargo-rpm should be able to keep that Cargo.toml investment,
  add small ArtifactX-specific overrides where needed, and get one pure-Rust
  pack/publish path for `.deb`, `.rpm`, and `.apk`. `arx` metadata should win
  when schemas overlap, and should cover ArtifactX-only features such as shared
  `[[dirs]]`, publish defaults, deterministic knobs, and future repo integration.
  Keep rendering in ArtifactX; do not depend on `cargo-deb`, `cargo-generate-rpm`,
  `cargo-rpm`, or `rpmbuild`.
- **Config-file marking** — design deb `conffiles` / equivalent manifest intent
  for config paths before users start relying on ad-hoc postinst behavior.
- **Explicit source date CLI** — consider `arx pack --source-date <epoch>` as a
  discoverable wrapper around `SOURCE_DATE_EPOCH`, preserving reproducible
  defaults while avoiding hidden environment-only behavior.
- **Pack docs completeness** — document current limitations clearly: no package
  signing, no auto dependency detection, no symlink following, no source packages,
  and no `.apk` repository add path yet.

- **HSM / KMS-backed repository signing spike** — explore whether `arx publish` can sign apt/yum metadata through an external signing boundary instead of loading `keys/private.asc` directly. Scope the design first: PKCS#11/HSM, cloud KMS, or `gpg-agent` may have very different tradeoffs. Do not implement before an ADR proves it preserves the one-binary/5-minute path for normal users.

### Later product bets

- Auto-dependency detection (`--auto-deps` using ldd or objdump).
- Multi-arch manifests.
- `arx pack --sign` inline package signing.
- Arch Linux `.pkg.tar.zst` support.
- Object-storage backend such as S3/MinIO.
- Read-through proxy cache or full upstream mirroring.
- Large-repo performance work beyond import/publish bottlenecks found during polish.
- Web UI.
- Plug-in system for custom checks/transformations.

## Philosophy (from the charter)

1. **Compete by deleting.** Before adding one feature, consider removing two.
2. **One binary.** No database, no daemon, no cluster.
3. **Design for operations.** stateless · deterministic · atomic · observable.
4. **The 5-minute rule.** install → create/import → publish → consume in 5 minutes.
5. **Think like Caddy.** Defaults are correct. Configuration disappears whenever possible.
