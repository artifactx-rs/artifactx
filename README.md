# ArtifactX — import existing apt/yum repos into a signed static repo

[![CI](https://github.com/artifactx-rs/artifactx/actions/workflows/ci.yml/badge.svg)](https://github.com/artifactx-rs/artifactx/actions/workflows/ci.yml) [![Release](https://github.com/artifactx-rs/artifactx/actions/workflows/release.yml/badge.svg)](https://github.com/artifactx-rs/artifactx/actions/workflows/release.yml) [![Latest release](https://img.shields.io/github/v/release/artifactx-rs/artifactx)](https://github.com/artifactx-rs/artifactx/releases/latest)

**Import first. Cut over when ready.** Pull packages from the repos you already have, regenerate apt/yum metadata under your key, and serve the result from one static binary.

ArtifactX (`arx`) is a small Rust tool for teams that ship Linux packages but do not want to operate Nexus, aptly, Pulp, S3 glue scripts, custom signing jobs, and a web server just to let users run `apt install` or `dnf install`.

```bash
# Path 1: migrate a slice of an existing repo, then serve it
arx init ./repo
arx import https://packages.example.com --apt --dist stable --component main --match-name myapp
arx publish --root ./repo
arx serve --root ./repo --addr 0.0.0.0:8080
```

```bash
# Path 2: start a new repo from packages you already built
arx init ./repo
arx add dist/*.deb dist/*.rpm --root ./repo
arx publish --root ./repo
arx serve --root ./repo --addr 0.0.0.0:8080
```

What users get:

```bash
sudo apt-get update && sudo apt-get install myapp
# or
sudo dnf install myapp
```

## Why ArtifactX

Most package repo setups become a pile of special cases: one path for `.deb`, another for `.rpm`, a signing key somewhere else, a CI upload script, a separate server, and no easy rollback.

ArtifactX keeps the package repository as a directory you can inspect, back up, move, and rebuild:

- **Import first** — pull packages from an existing apt or yum/dnf repository into your own signed repo.
- **One binary** — pack, add, import, publish, serve, push, promote, GC, rollback.
- **Metadata-signed repos** — apt `InRelease` / `Release.gpg`, yum `repomd.xml.asc`. ArtifactX does not re-sign individual packages.
- **Atomic publish** — build metadata in staging, then flip the live state.
- **Rollbackable** — keep published states and flip back when a bad release escapes.
- **CI-friendly** — `arx push` uploads to `arx serve`; token or GitHub OIDC auth.
- **No daemon required** — static binary, Docker image, or GitHub Pages-hosted repo. Public Pages repos should use a stable imported signing key.

## The migration path: import, publish, serve

Use `import` when you already have packages somewhere and want a cleaner repo in front of them. Start with a bounded slice, verify clients, then cut over when the repo is boring.

### Import from apt

```bash
arx init ./repo

arx import https://packages.example.com \
  --root ./repo \
  --apt \
  --dist stable \
  --component main \
  --arch amd64 \
  --match-name myapp \
  --limit 20

arx publish --root ./repo
arx serve --root ./repo --addr 0.0.0.0:8080
```

ArtifactX reads `Packages.gz` or `Packages`, downloads matching `.deb` files into the pool, then regenerates signed apt metadata under `apt/dists/<dist>`.

### Import from yum/dnf

```bash
arx init ./repo

arx import https://packages.example.com/yum/x86_64 \
  --root ./repo \
  --yum \
  --component myrepo \
  --limit 20

arx publish --root ./repo
arx serve --root ./repo --addr 0.0.0.0:8080
```

ArtifactX reads `repodata/repomd.xml`, follows the primary metadata stream, downloads `.rpm` files, then regenerates signed yum repodata.

## What import does — and does not do

ArtifactX import is a migration path, not a magic mirror.

- **It imports package files** from existing apt `Packages` metadata or yum/dnf `repodata`.
- **It regenerates repository metadata** under your ArtifactX signing key, so clients trust your repo boundary.
- **It is intentionally sliceable** with filters like `--match-name`, `--arch`, and `--limit` so the first migration is small and observable.
- **It is not a bit-for-bit mirror** of the upstream repository metadata. Use mirroring when you need continuous upstream sync.
- **It does not re-sign individual packages.** Keep package signing in your build pipeline if your clients enforce package-level signatures.

## Current focus: feature freeze, polish the migration path

The next work is not broad feature expansion. ArtifactX is in an import-first polish phase: make `import -> publish -> serve -> client install -> rollback` feel reliable, documented, and easy to verify before adding more package ecosystems or storage backends.

## Build your own packages too

ArtifactX is not only a repo importer. It can build packages from a standalone manifest or directly from a Rust `Cargo.toml`:

```bash
arx pack ./Cargo.toml --out dist
arx add dist/*.deb dist/*.rpm --root ./repo
arx publish --root ./repo
```

From zero to a signed repo:

```bash
arx init                              # create repo + signing key
arx pack ./Cargo.toml                 # build .deb .rpm .apk
arx publish                           # sign + index
arx serve                             # HTTP server on :8080
```

## Install arx

### Download static binary

```bash
curl -fsSLO https://github.com/artifactx-rs/artifactx/releases/latest/download/arx-latest-amd64
sudo install -m 755 arx-latest-amd64 /usr/local/bin/arx
arx --version
```

### Docker

```bash
docker run --rm -v $(pwd)/repo:/repo -p 8080:8080 \
  ghcr.io/artifactx-rs/arx:latest serve --root /repo --addr 0.0.0.0:8080
```

### Docker Compose

```yaml
services:
  arx:
    image: ghcr.io/artifactx-rs/arx:latest
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

## Commands

| Command | Why you use it |
|---|---|
| `arx init` | Create repository layout, config, and signing key |
| `arx import` | Migrate packages from an existing apt/yum repo into ArtifactX |
| `arx pack` | Build `.deb`, `.rpm`, `.apk` from a manifest or `Cargo.toml` |
| `arx add` | Put existing `.deb` / `.rpm` files into the pool |
| `arx publish` | Generate and sign apt + yum metadata atomically |
| `arx serve` | Serve apt/dnf-compatible repo + REST API + `/metrics` |
| `arx push` | Upload packages to a remote `arx serve` from CI |
| `arx promote` | Move packages between staging/prod components or repos |
| `arx rm` | Yank a package from the pool |
| `arx gc` | Prune old versions with version-aware retention |
| `arx rollback` | Restore a previous published state |
| `arx history` | Inspect retained published states |
| `arx watch` | Watch a directory and auto-add + publish new packages |
| `arx key` | Generate, import, rotate, revoke, or export signing keys |

## Client setup

### apt

```bash
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL http://REPO_HOST:8080/keys/public.asc \
  | sudo tee /etc/apt/keyrings/arx.asc >/dev/null

echo "deb [signed-by=/etc/apt/keyrings/arx.asc] http://REPO_HOST:8080/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/arx.list

sudo apt-get update
sudo apt-get install myapp
```

### dnf/yum

ArtifactX signs yum repository metadata (`repomd.xml.asc`). It does not re-sign individual `.rpm` packages; keep package signing in your build pipeline if you require `gpgcheck=1`.

```ini
# /etc/yum.repos.d/arx.repo
[arx]
name=ArtifactX
baseurl=http://REPO_HOST:8080/yum/myrepo/$basearch
enabled=1
gpgcheck=0
repo_gpgcheck=1
gpgkey=http://REPO_HOST:8080/keys/public.asc
```

```bash
sudo dnf install myapp
```

## Repository layout

```text
repo/
  arx.toml
  keys/
    private.asc
    public.asc
  apt/
    pool/<component>/
    dists/<dist>/
  yum/
    <repo>/<arch>/
      repodata/
```

Back it up with `tar`. Restore by extracting. Metadata is deterministic; if generated files are lost, run `arx publish` again.

## Verified

- Workspace tests pass with `cargo test --workspace`.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- Real apt/dnf flows are covered by integration tests where host tools are available.
- Package output is deterministic with `SOURCE_DATE_EPOCH` support.

## Project links

- [Roadmap](ROADMAP.md)
- [Operations guide](docs/OPERATIONS.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [AI contributor rules](AI_RULES.md)
- [Support](SUPPORT.md)
- [Governance](GOVERNANCE.md)
- [ADR index](docs/adr/README.md)
- [Competitive analysis](COMPETITORS.md)

## License

- `crates/arx` CLI: GPL-2.0-or-later
- `crates/arx-debrepo` and `crates/arx-pack`: MIT OR Apache-2.0
