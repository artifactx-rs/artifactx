# ArtifactX — import existing apt/yum repos into a signed static repo

[![CI](https://github.com/artifactx-rs/artifactx/actions/workflows/ci.yml/badge.svg)](https://github.com/artifactx-rs/artifactx/actions/workflows/ci.yml) [![Release](https://github.com/artifactx-rs/artifactx/actions/workflows/release.yml/badge.svg)](https://github.com/artifactx-rs/artifactx/actions/workflows/release.yml) [![Latest release](https://img.shields.io/github/v/release/artifactx-rs/artifactx)](https://github.com/artifactx-rs/artifactx/releases/latest)

**Import first. Cut over when ready.** Pull packages from the repos you already have, regenerate apt/yum metadata under your key, and serve the result from one static binary.

ArtifactX (`arx`) is a small Rust tool for teams that ship Linux packages but do not want to operate Nexus, aptly, Pulp, S3 glue scripts, custom signing jobs, and a web server just to let users run `apt install` or `dnf install`.

Start with the [documentation map](docs/README.md) if you want the import tutorial, Docker/systemd guides, signing notes, or CLI/config reference.

```bash
# Path 1: migrate a slice of an existing repo, then serve it
arx init ./repo
arx import https://packages.example.com --apt --dist stable --component main --match-name myapp
arx publish --root ./repo
arx serve --root ./repo
```

```bash
# Path 2: start a new repo from packages you already built
arx init ./repo
arx add dist/*.deb dist/*.rpm --root ./repo
arx publish --root ./repo
arx serve --root ./repo
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
arx serve --root ./repo
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
arx serve --root ./repo
```

ArtifactX reads `repodata/repomd.xml`, follows the primary metadata stream, downloads `.rpm` files, then regenerates signed yum repodata.

## Signing keys: the simple model

ArtifactX signs repository metadata, not individual packages. apt clients verify `InRelease` / `Release.gpg`; dnf/yum clients verify `repomd.xml.asc`. The generated keys are OpenPGP v4 RSA keys so old and new distro tooling can verify them.

There are three supported paths:

### 1. Local/dev: let ArtifactX generate the key

```bash
arx init ./repo
arx publish --root ./repo
```

This creates `keys/private.asc` and `keys/public.asc`. If no passphrase is provided, the private key is stored unencrypted and ArtifactX warns you. That is acceptable for throwaway local repos, not for public or production repos.

### 2. Production: encrypt the repo signing key

```bash
printf '%s\n' 'use-a-real-secret-here' > passphrase.txt
arx init ./repo --passphrase-file passphrase.txt
arx publish --root ./repo --passphrase-file passphrase.txt
```

CI can use the environment variable instead of a file:

```bash
export ARX_KEY_PASSPHRASE='use-a-real-secret-here'
arx publish --root ./repo
```

### 3. Company key: import your existing armored private key

```bash
arx init ./repo --no-key
arx key import company-private.asc --root ./repo --passphrase-file passphrase.txt
arx key export --root ./repo > public.asc
arx publish --root ./repo --passphrase-file passphrase.txt
```

Clients trust the exported public key. If you rotate the key, clients must trust the new public key before the next cutover:

```bash
arx key rotate --root ./repo --passphrase-file passphrase.txt
arx key export --root ./repo > public.asc
```

What is intentionally not configurable yet: RSA bit size, key algorithm, and signature policy knobs. ArtifactX currently uses a conservative RSA/OpenPGP profile because compatibility with stock apt/dnf matters more than exposing crypto tuning in the happy path.

## Expiry policy: what ArtifactX owns

ArtifactX has one built-in expiry default: new apt repositories get `valid_days = 7` in `arx.toml`, so each `arx publish` writes a signed `Valid-Until` roughly seven days in the future. Republish refreshes the window.

```toml
[apt]
valid_days = 7
```

Set it to `0` only if you intentionally want to omit `Valid-Until`:

```toml
[apt]
valid_days = 0
```

Everything else is an operator policy, not something ArtifactX should guess:

- yum/dnf metadata does not get an ArtifactX-specific expiry field.
- ArtifactX-generated OpenPGP keys do not expire automatically.
- Organizations with key expiry, HSM/KMS, audit, or rotation requirements should import an organization-managed OpenPGP key and run their own trust-rollout process.

That boundary is deliberate: ArtifactX keeps the default repo safe against stale apt metadata, but it does not pretend to be your key-governance system.

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
arx serve                             # local server on 127.0.0.1:8080
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

Generate the files instead of hand-copying YAML:

```bash
arx compose --root ./repo --out ./deploy
cd deploy
docker compose up -d
```

The generated compose file deliberately binds the container to `0.0.0.0:8080` because Docker port publishing needs a non-localhost listener inside the container.

### systemd service

`arx serve` defaults to `127.0.0.1:8080`. That is intentional: put Caddy, nginx, or another TLS/reverse proxy in front when exposing a repo publicly.

```ini
# /etc/systemd/system/arx.service
[Unit]
Description=ArtifactX package repository
Documentation=https://github.com/artifactx-rs/artifactx
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=arx
Group=arx
EnvironmentFile=-/etc/arx/arx.env
ExecStart=/usr/local/bin/arx serve --root /var/lib/arx/repo
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/var/lib/arx/repo

[Install]
WantedBy=multi-user.target
```

```bash
id -u arx >/dev/null 2>&1 || sudo useradd --system --home /var/lib/arx --shell /usr/sbin/nologin arx
sudo install -d -o arx -g arx /var/lib/arx/repo
sudo install -d -o root -g root -m 0750 /etc/arx
sudo systemctl daemon-reload
sudo systemctl enable --now arx
curl -fsS http://127.0.0.1:8080/api/v1/health
journalctl -u arx -f
```

Use `/etc/arx/arx.env` for secrets only when the server needs write APIs:

```sh
ARX_SERVE_TOKEN=change-me
ARX_KEY_PASSPHRASE=optional-if-your-repo-key-is-encrypted
```

If you really want direct LAN exposure without a reverse proxy, be explicit:

```bash
arx serve --root /var/lib/arx/repo --addr 0.0.0.0:8080
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

The `arx.toml` defaults are intentionally boring and branded:

```toml
[repo]
origin = "ArtifactX"
label = "ArtifactX"
description = "Signed package repository managed by ArtifactX"

[signing]
enabled = true
keys_dir = "keys"
private_key = "keys/private.asc"
public_key = "keys/public.asc"
user_id = "ArtifactX Repository Signing <signing@artifactx.local>"

[server]
addr = "127.0.0.1:8080"
```

Change `origin`, `label`, `description`, and `signing.user_id` before generating or importing a production key if your repo should present your company identity instead of the ArtifactX default.

## Verified

- Workspace tests pass with `cargo test --workspace`.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- Real apt/dnf flows are covered by integration tests where host tools are available.
- Package output is deterministic with `SOURCE_DATE_EPOCH` support.

## Project links

- [Documentation](docs/README.md)
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
