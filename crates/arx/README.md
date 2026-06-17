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
| `arx pack <manifest> [--add]` | Build `.deb`/`.rpm` from a manifest (optionally into the pool) |
| `arx publish [--apt] [--yum] [--strict]` | Generate and sign repository metadata (`--strict` fails if any package is unreadable/colliding instead of skipping it) |
| `arx rollback [dist] [--to <id>]` | Flip an apt dist back to a previous published state |
| `arx history [dist]` | List retained published states for an apt dist |
| `arx push <pkg…> --url <server>` | Upload to a running `arx serve` (stores + publishes remotely) |
| `arx rm <name> [--version V]` | Remove a package from the pool (yank), then `publish` |
| `arx gc --keep N [--dry-run]` | Keep the N newest **versions** per package (dpkg/rpm version-aware), prune the rest, then `publish` |
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

## HTTP API

`arx serve` exposes a small REST API under `/api/v1` — the same operations as the
CLI, for tools and CI. Reads are public if no token is set; **writes always require
`ARX_SERVE_TOKEN`** (bearer auth).

| Method & path | Does | Equivalent |
| --- | --- | --- |
| `GET /api/v1/health` | `{name, version}` | — |
| `GET /api/v1/packages` | list pooled packages (JSON) | `arx list` |
| `POST /api/v1/packages` | upload a `.deb`/`.rpm`, then publish | `arx push` / `arx add`+`publish` |
| `DELETE /api/v1/packages/:name?version=&yum=` | remove + publish | `arx rm` |
| `POST /api/v1/gc?keep=N&dry_run=` | prune old versions + publish | `arx gc` |

Upload headers: `X-Arx-Filename` (required), optional `X-Arx-Component` (deb) /
`X-Arx-Repo` (rpm). Push from CI in one line:

```bash
arx push ./dist/*.deb --url https://repo.example.com   # ARX_SERVE_TOKEN in env
# or with curl:
curl -fsSL -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
     -H "X-Arx-Filename: app_1.0_amd64.deb" \
     --data-binary @app_1.0_amd64.deb \
     https://repo.example.com/api/v1/packages
```

## Configuration (`arx.toml`)

`arx init` writes `arx.toml` at the repo root: repository identity
(`Origin`/`Label`), signing key paths, default apt `dist`/`component`, default yum
`repo`, and the server listen address. CLI flags override config values.

Two `[apt]` keys govern publishing behavior:

```toml
[apt]
dist = "stable"
component = "main"
valid_days = 7    # Release `Valid-Until` window; 0 = no expiry. init writes 7.
strict = false    # true = a skipped package fails publish (push returns 422)
```

- **`valid_days`** stamps `Valid-Until` into the apt `Release` so a stale-metadata
  (freeze/replay) attack has only a small window; republishing refreshes it.
  `arx init` sets `7` (secure-by-default); `0` omits the field. yum has no
  server-side expiry in its spec.
- **`strict`** is the source of truth for the `push`/server path; the CLI
  `--strict` flag forces it on for one publish.

## Notes

- Signing keys default to **RSA-2048** (fast `init`, verifiable everywhere).
- Publishing is **resilient**: one unreadable or colliding package is skipped
  (with a loud stderr summary + an `arx_publish_skipped_total` metric), not fatal
  — unless `--strict`/`[apt].strict`. The live repo is never half-written
  (staging → atomic symlink flip).
- The built-in server is plain HTTP. Set **`ARX_SERVE_TOKEN`** to require an
  `Authorization: Bearer <token>` on every request (unset = public, for the
  zero-config quickstart). For TLS, front it with a reverse proxy (nginx/Caddy).
- Backup/restore & rollback: see [`docs/OPERATIONS.md`](../../docs/OPERATIONS.md)
  — metadata is derived, so protect the inputs and `arx publish` rebuilds the rest.

## License

GPL-2.0-or-later (links `createrepo_rs`, which is GPL).
