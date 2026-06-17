# arx — ArtifactX CLI

`arx` is the command-line repository manager for [ArtifactX](https://github.com/jamesarch/artifactx).
It turns a directory into a **signed apt + yum repository** that `apt-get` and
`dnf` consume directly, and serves it over HTTP — from a single binary.

- **apt** repositories via [`debrepo`](../debrepo) (in-house generator)
- **yum/dnf** repodata via [`createrepo_rs`](https://crates.io/crates/createrepo_rs)
- **PGP signing** (v4 RSA) via rpgp — `InRelease`/`Release.gpg` and `repomd.xml.asc`
- **Built-in HTTP server** (axum) with a Prometheus `/metrics` endpoint
- structured logging via `tracing`

## Install

```bash
cargo install --path crates/arx        # or: cargo build --release -p artifactx
```

The binary is `arx`. A static single-file build:

```bash
cargo build --release --target x86_64-unknown-linux-musl -p artifactx
```

## Quick start

```bash
arx init ./myrepo                       # scaffold + generate signing key
arx add ./nginx_1.0_amd64.deb --root ./myrepo
arx add ./redis-7.x86_64.rpm  --root ./myrepo
arx publish --root ./myrepo             # generate + sign apt & yum metadata
arx serve   --root ./myrepo --addr 0.0.0.0:8080
```

Or `arx compose --root ./myrepo` to emit a `Dockerfile` + `docker-compose.yml`
and run `docker compose up`.

## Commands

| Command | Purpose |
| --- | --- |
| `arx init [path]` | Scaffold directories + `arx.toml`, generate a signing key |
| `arx key {generate\|import <file>\|export}` | Manage the signing key |
| `arx add <pkg…>` | Add `.deb`/`.rpm` into the pool (arch detected from metadata) |
| `arx publish [--apt] [--yum]` | Generate and sign repository metadata |
| `arx serve [--addr] [--root]` | Serve the repo over HTTP (+ `/metrics`) |
| `arx compose [--addr]` | Generate `Dockerfile` + `docker-compose.yml` |

`arx --version` reports the build's git sha, build time, and rustc version.

## Client configuration

**apt** (Debian/Ubuntu) — armored key + `signed-by`:

```bash
sudo curl -fsSL http://REPO_HOST:8080/keys/public.asc -o /etc/apt/keyrings/arx.asc
echo "deb [signed-by=/etc/apt/keyrings/arx.asc] http://REPO_HOST:8080/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/arx.list
sudo apt-get update && sudo apt-get install <package>
```

**dnf/yum** (RHEL/Rocky/Fedora) — `repo_gpgcheck` validates `repomd.xml.asc`:

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
sudo dnf install <package>
```

## Configuration (`arx.toml`)

`arx init` writes a commented `arx.toml` at the repo root: repository identity
(`Origin`/`Label`), signing key paths, default apt `dist`/`component`, default yum
`repo`, and the server listen address. CLI flags override config values.

## Notes

- Signing keys default to **RSA-2048** (fast `init`, verifiable everywhere).
- The built-in server is plain HTTP with no auth/TLS — put it behind a reverse
  proxy (nginx/Caddy) for anything public.

## License

GPL-2.0-or-later (links `createrepo_rs`, which is GPL).
