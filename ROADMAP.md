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

5. **Adversarial review**
   - Re-check the README and Pages copy from a user/investor perspective: one clear wedge, no overclaiming, no vague platform promises.
   - Re-check CI from a maintainer perspective: clear gates, no accidental version bumps, no secret leakage.

## Parked until after the freeze

These are plausible, but intentionally not current focus:

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
