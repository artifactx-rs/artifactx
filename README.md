# ArtifactX — Build Once. Package Once. Publish Everywhere.

**One static binary.** apt + yum repository manager with PGP signing.
Pure-Rust `.deb` / `.rpm` / `.apk` packager.
Built-in HTTP server. Zero dependencies, zero daemons.

[![ci](https://github.com/artifactx-rs/artifactx/actions/workflows/ci.yml/badge.svg)](https://github.com/artifactx-rs/artifactx/actions/workflows/ci.yml)
[![v0.1.0](https://img.shields.io/badge/version-0.1.0-blue)](https://github.com/artifactx-rs/artifactx/releases/tag/v0.1.0)
[![Project](https://img.shields.io/badge/project-Done%3A24%20%2F%20Todo%3A0-green)](https://github.com/orgs/artifactx-rs/projects/1)

```bash
# 1 minute from zero to signed repo
arx init                              # create repo + signing key
arx pack ./Cargo.toml                 # build .deb .rpm .apk from Cargo.toml
arx publish                           # sign + index
arx serve                             # HTTP server on :8080
```

## Install

### Download static binary
```bash
curl -fsSLO https://github.com/artifactx-rs/artifactx/releases/download/v0.1.0/arx-amd64
sudo install -m 755 arx-amd64 /usr/local/bin/arx
arx --version
```

### Docker
```bash
docker run --rm -v $(pwd)/repo:/repo -p 8080:8080 \
  ghcr.io/artifactx-rs/arx:v0.1.0 serve --root /repo --addr 0.0.0.0:8080
```

### Docker Compose
```yaml
# docker-compose.yml
services:
  arx:
    image: ghcr.io/artifactx-rs/arx:v0.1.0
    ports: ['8080:8080']
    volumes: ['./repo:/repo']
    command: serve --root /repo --addr 0.0.0.0:8080
```
```bash
docker compose up -d
```

### Build from source
```bash
git clone https://github.com/artifactx-rs/artifactx.git && cd artifactx
cargo build --release
./target/release/arx --version
```

## What it does

| Command | Purpose |
|---|---|
| `arx init` | Scaffold a repository + generate PGP signing key |
| `arx pack` | Build `.deb` / `.rpm` / `.apk` from a Cargo.toml or manifest |
| `arx publish` | Sign and index all metadata (atomic staging → symlink flip) |
| `arx serve` | HTTP server (apt/dnf-compatible + REST API + `/metrics`) |
| `arx push` | Upload + publish to a remote `arx serve` (OIDC or static token) |
| `arx rm` | Yank a package from the pool |
| `arx gc` | Prune old versions (EVR-aware, `--keep-within`, `--grace`, bytes-freed) |
| `arx rollback` | Flip back to the previous published state (apt + yum) |
| `arx history` | List retained published states |
| `arx promote` | Move packages between components/repos (staging→prod) |
| `arx import` | Migrate packages from an existing apt/yum repo |
| `arx watch` | Poll a directory for new packages, auto-add + publish |
| `arx key` | Generate / import / rotate / revoke signing keys |

## Verified

- **57 tests** across 4 workspace crates, all green, clippy clean.
- **Real apt-get** install on Debian bookworm-slim (Docker).
- **Real dnf install** on Fedora 44 (Docker).
- **Reproducible builds** — `SOURCE_DATE_EPOCH` support, deterministic .deb/.rpm output.

## Repository layout

```
repo/                  # `arx init` creates this
  arx.toml             # config (Origin, signing, apt/yum defaults)
  keys/                # PGP private + public key
  apt/
    pool/<component>/  # .deb packages + .arx-manifest.toml (incremental cache)
    dists/<dist>/      # generated: Release, InRelease, Packages, Contents-<arch>
  yum/
    <repo>/<arch>/     # .rpm packages + repodata/ (repomd.xml + .xml.gz streams)
```

Back it up with `tar`. Restore by extracting. Metadata is deterministic — if lost, `arx publish` rebuilds it.

## Project

- [Roadmap](ROADMAP.md) — what's next (v0.2.0 → v0.3.0)
- [ADR index](docs/adr/README.md) — 15 architecture decisions
- [Operations guide](docs/OPERATIONS.md) — backup, restore, rollback
- [Competitive analysis](COMPETITORS.md)
- [Wiki](https://github.com/artifactx-rs/artifactx/wiki)
- [Kanban](https://github.com/orgs/artifactx-rs/projects/1) — Done:24 Todo:0

## License

`crates/arx` (CLI): GPL-2.0-or-later · `crates/debrepo` + `crates/pack`: MIT OR Apache-2.0
