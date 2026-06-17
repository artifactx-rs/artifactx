# ArtifactX Roadmap

> **v0.1.0 shipped.** Build Once. Package Once. Publish Everywhere.

## What we have (v0.1.0 — June 2026)

### Repository
- **apt (Debian/Ubuntu)** — generate, sign, serve. Release/InRelease/Release.gpg, by-hash, Acquire-By-Hash.
- **yum/dnf (RHEL/Fedora/Rocky)** — generate, sign, serve. repomd.xml/repomd.xml.asc, primary/filelists/other.xml.gz.
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
- **57 tests** across 4 workspace crates, all green.
- **Docker E2E**: `apt-get install` on Debian + `dnf install` on Fedora — both pass.
- **CI**: clippy, test, dogfood pack.

## Next milestones

### v0.2.0 — packaging ecosystem
- Auto-dependency detection (opt-in, `--auto-deps` using ldd or objdump)
- Multi-arch manifests (one manifest → cross-compile targets)
- `arx pack --sign` inline signing
- Arch Linux `.pkg.tar.zst` support

### v0.3.0 — scale
- Object-storage backend (S3/MinIO) for pool blobs
- Read-through proxy cache for upstream repos
- `arx mirror` — mirror an upstream apt/yum repo
- Large-repo performance (10k+ packages)

### Beyond
- Web UI (read-only dashboard, not a management console — charter restricts scope)
- apt/yum proxy (transparent caching proxy)
- Plug-in system for custom checks/transformations

## Philosophy (from the charter)

1. **Compete by deleting.** Before adding one feature, consider removing two.
2. **One binary.** No database, no daemon, no cluster.
3. **Design for operations.** stateless · deterministic · atomic · observable.
4. **The 5-minute rule.** install → create → package → publish → consume in 5 minutes.
5. **Think like Caddy.** Defaults are correct. Configuration disappears whenever possible.

## v0.1.0 shipped features (June 2026)

Repository: apt+yum generate/sign/serve, atomic publish+rollback, incremental
publish (file-manifest cache), version-aware GC, import from existing repos
(Ubuntu/ClickHouse/Docker CE verified), mirror (incremental upstream sync),
OIDC keyless push, Contents-<arch>.

Pack: pure-Rust .deb/.rpm/.apk builder, reproducible-by-construction,
Cargo.toml-driven zero-config, Docker backend.

CLI (20 commands): init, key generate/import/rotate/revoke, add, pack, publish,
serve, push, rm, gc, rollback, history, import, mirror, promote, watch, compose.

API (10 endpoints): health, packages CRUD, gc, publish, rollback, history,
import, promote. Token + OIDC auth.

Verified: 57 tests green, clippy zero-warning. Docker E2E: apt-get (Debian) +
dnf (Fedora) both pass.
