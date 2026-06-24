# arx — ArtifactX CLI

`arx` is the command-line repository manager for [ArtifactX](https://github.com/artifactx-rs/artifactx).
It turns a directory into a **signed apt + yum repository** that `apt-get` and
`dnf` consume directly, and serves it over HTTP — from a single binary.

- **apt** repositories via [`arx-debrepo`](../arx-debrepo) (in-house generator)
- **yum/dnf** repodata via ArtifactX's in-tree `createrepo_rs` subset
- **PGP signing** (v4 RSA) via rpgp — `InRelease`/`Release.gpg` and `repomd.xml.asc`
- **Built-in HTTP server** (axum) with a Prometheus `/metrics` endpoint
- structured logging via `tracing`

## Install

```bash
cargo install artifactx                # from crates.io
# or, from a checkout: cargo build --release -p artifactx
# or with Nix: nix run github:artifactx-rs/artifactx -- --version
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
arx serve   --root ./myrepo
```

Or `arx compose --root ./myrepo` to emit a `Dockerfile` + `docker-compose.yml`
and run `docker compose up`.

## Commands

| Command | Purpose |
| --- | --- |
| `arx init [path]` | Scaffold directories + `arx.toml`, generate a signing key |
| `arx key {generate\|import <file>\|export}` | Manage the signing key |
| `arx add <pkg-or-dir…>` | Add `.deb`/`.rpm` files, or discover them recursively from directories, into the pool |
| `arx pack [manifest-or-Cargo.toml]` | Build `.deb`, `.rpm`, `.apk`, and Arch `.pkg.tar.zst` packages |
| `arx publish [--apt] [--yum] [--strict]` | Generate and sign repository metadata; optionally export and cut over live public paths |
| `arx publish-dir <dir>` | Ingest a package drop directory, no-op unchanged inputs, publish, and optionally switch live symlinks |
| `arx rollback [dist] [--to <id>]` | Flip an apt dist back to a previous published state |
| `arx history [dist]` | List retained published states for an apt dist |
| `arx push <pkg…> --url <server>` | Upload to a running `arx serve` (stores + publishes remotely) |
| `arx rm <name> [--version V]` | Remove a package from the pool (yank), then `publish` |
| `arx search [query]` | Search local apt/yum pool entries before GC, remove, promote, or cutover |
| `arx gc --keep N [--dry-run]` | Keep the N newest **versions** per package (dpkg/rpm version-aware), prune the rest, then `publish` |
| `arx promote --from <from> --to <to> <name>` | Promote packages between apt components or yum repos |
| `arx serve [--addr] [--root]` | Serve the repo over HTTP (defaults to `127.0.0.1:8080`, + `/metrics`) |
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

`arx serve` exposes static repo files plus a REST API under `/api/v1` — the same
operations as the CLI, for tools and CI. Reads are public. Writes require
`ARX_SERVE_TOKEN` bearer auth or configured GitHub Actions OIDC.

See the full [HTTP API reference](../../docs/reference/http-api.md) and
[OpenAPI spec](../../docs/reference/openapi.yaml) for endpoints, schemas, status
codes, auth, and `curl` examples. A running server also serves the spec at
`/api/openapi.yaml` and Swagger UI at `/api/docs`.

Push from CI in one line:

```bash
arx push ./dist/*.deb --url https://repo.example.com   # token or GitHub OIDC
```

## Configuration (`arx.toml`)

`arx init` writes `arx.toml` at the repo root: repository identity
(`[apt.release]` `Origin`/`Label`), signing key paths, default apt `dist`/`component`, default yum
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

GPL-2.0-or-later. The CLI/server includes a minimal GPL-derived
`createrepo_rs` subset for yum metadata; reusable `arx-debrepo` and `arx-pack`
remain MIT OR Apache-2.0.
