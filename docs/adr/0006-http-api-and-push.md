# ADR-0006: HTTP write API + `arx push`

- Status: Accepted
- Date: 2026-06-17

## Context

The product wedge is "package *and* publish — one line from CI." That needs a way to
get a package into a running repository remotely, plus an API so other tools can
manage packages without the CLI.

## Decision

`arx serve` exposes a small REST API under `/api/v1` mirroring the CLI:
`GET /health`, `GET /packages` (list), `POST /packages` (upload = push),
`DELETE /packages/:name` (= rm), `POST /gc`. Uploads land in the pool (rpm arch
auto-detected) and trigger an **atomic republish under the publish lock**, signed
with the serve process's key. `arx push <pkg> --url <server>` is the client.

Auth: a single bearer token (`ARX_SERVE_TOKEN`). **Reads are public if no token is
set; writes always require a token** (403 otherwise). TLS is delegated to a reverse
proxy — the binary doesn't terminate TLS (charter principle 8: one binary, no hidden
magic; let Caddy/nginx do the thing they're great at).

## Consequences

- Good: CLI and API are one surface; CI push is one command; `curl` works too.
- Good: `serve` is the single stateful component, still one binary, still no DB.
- Bad: `serve` must hold the signing key (and passphrase) to publish on upload.
- Bad: a static bearer token is coarse; no per-package or short-lived auth yet.

## Alternatives considered

- **No write endpoint (deb-s3 style object-store mutation).** Elegant but ties us to
  S3; we want a self-hosted single binary first.
- **Built-in TLS.** Rejected — TLS termination is a proxy's job; bundling it adds
  cert plumbing against the charter.
- **RBAC/users.** Rejected — identity systems are scope creep (`COMPETITORS.md`).

## Future improvements

GitHub Actions **OIDC** keyless auth (mint a short-lived token from `id-token`, no
stored secret); constant-time token compare; optional `promote` (staging→prod move);
`incoming/` drop-dir ingestion as a second, CLI-less path.
