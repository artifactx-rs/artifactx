# ADR-0014: OIDC keyless push — ditch the long-lived token

- Status: **Accepted**
- Date: 2026-06-17
- Decided: GitHub Actions only (others later); OIDC + ARX_SERVE_TOKEN
  **coexist** (auto-detect JWT vs static token by format).

## Context

`arx push` and `POST /api/v1/packages` today require `ARX_SERVE_TOKEN` — a
long-lived, stored-in-secrets bearer token. Cloudsmith / `cosign` / modern CI
platforms use **OIDC** (OpenID Connect) to avoid stored secrets altogether:
the CI job mints a short-lived identity token that the server validates, so
leaked tokens expire in minutes and the trust root is the platform's signing key.

GitHub Actions exposes this via `ACTIONS_ID_TOKEN_REQUEST_URL` — any workflow
can request a JWT asserting `repo`, `actor`, `ref`, etc. The token is valid for
~5 minutes. If arx's serve can validate that token, a GitHub Actions job can
push with **no stored secret at all**.

This is the wedge: "one-line CI push" with zero secrets management.

## Decision (proposed)

### Flow

1. **Client** (`arx push`) detects `GITHUB_ACTIONS=true && ACTIONS_ID_TOKEN_REQUEST_URL` →
   fetches an audience-specific JWT from the endpoint, sends it as
   `Authorization: Bearer <jwt>` with an `X-Arx-Auth: oidc` header.
2. **Server** receives a write request with `X-Arx-Auth: oidc` →
   validates the JWT against GitHub's public JWKS, checks claims, matches
   against an allowlist. If valid, the request proceeds. Fallback:
   `X-Arx-Auth: token` (or absent) → existing static-token path.

### Server-side validation (new middleware)

A new `OidcConfig` section in `arx.toml`:

```toml
[server.oidc]
enabled = true
# Repositories allowed to push (glob patterns), e.g.:
allowed_repos = ["artifactx-rs/*"]
# Optional: require a specific workflow ref or environment.
# allowed_refs = ["refs/heads/main"]
```

Validation steps (mandatory, implemented in the auth middleware):

1. **Signature**: verify JWT RS256 signature against GitHub's JWKS
   (`https://token.actions.githubusercontent.com/.well-known/jwks`).
2. **Issuer**: must be `https://token.actions.githubusercontent.com`.
3. **Audience**: must match the server's configured `audience` (defaults to
   the server URL, or a configurable value).
4. **Expiry**: `exp` must be in the future (allow ~30s clock skew).
5. **Repository**: the `repository` claim must match one of the
   `allowed_repos` patterns.

JWKS is fetched once on first validation request and cached in-memory with a
TTL (key rotation happens rarely; re-fetch every hour or on validation failure).

### Client-side (arx push)

When `GITHUB_ACTIONS=true` and `ACTIONS_ID_TOKEN_REQUEST_URL` is set:

```bash
# In GitHub Actions: nothing to do — arx push auto-detects.
arx push ./*.deb --url https://repo.example.com

# Equivalent manual flow (for other OIDC providers):
arx push ./*.deb --url https://repo.example.com --oidc-token "$MY_TOKEN"
```

`arx push` reads the token URL + bearer token from the environment, POSTs
`&audience=<server-url>`, and sends the resulting JWT as the bearer token
with `X-Arx-Auth: oidc`.

### Fallback: static token still works

OIDC is additive — `ARX_SERVE_TOKEN` still works for non-GitHub-CI push, or
when the user chooses to use a token. The server decides the auth method per
request by inspecting `X-Arx-Auth` (or the token format — JWTs are three
dot-separated base64 segments, easy to detect).

## Consequences

- Good: no stored secret for GitHub Actions CI; one-line push is a true wedge.
  Leaked tokens expire in ~5 minutes. Compete by deleting: `ARX_SERVE_TOKEN`
  disappears from the common case.
- Good: OIDC config is additive — existing setups are undisturbed.
- Bad / cost: new dependency (`jsonwebtoken` or `jwt` crate for JWT+RSA
  validation); JWKS cache adds in-memory state; GitHub OIDC is the first
  provider — others (GitLab, Buildkite) come later.

## Explicitly NOT in this ADR

- **Full OIDC provider framework** with pluggable providers. GitHub Actions is
  the first and only provider; the architecture supports adding others but we
  don't pre-build for them.
- **Fine-grained RBAC** (per-repo scoping within one arx server). Allowlist is
  a glob pattern match; detailed claims like `actor`/`ref`/`environment` come
  later.
- **Token exchange / minting** — arx is a consumer, not an issuer.

## Alternatives considered

- **Supabase / Dex / OpenFGA as OIDC proxy.** Rejected: external service
  violates "one binary, stateless" (charter). arx's JWKS validation is ~200
  lines and needs no external process.
- **Reuse `reqwest` for HTTPS JWKS fetch** — yes, already a dep.
- **JWT crate selection:** `jsonwebtoken` (popular, maintained, pure Rust) vs
  `jwt` (lightweight) vs manual RSA. Lean: `jsonwebtoken` — JWT parsing +
  RSA signature verification + claims extraction in one API.

## Open questions for review

1. **OIDC vs static token priority** — when both are configured, does OIDC
   take precedence or is it additive? Lean: **additive** — either works; the
   auth header determines which path to validate.
2. **Audience** — default to server URL or a configurable string? Lean:
   configurable `[server.oidc].audience` with a sane default (`arx`).
3. **JWKS cache TTL** — how often to re-fetch? Lean: **1 hour or re-fetch on
   validation failure** — GitHub rotates keys rarely (years-scale).

## Implementation plan

1. Add `jsonwebtoken` dep to artifactx.
2. Add `OidcConfig` to `config.rs` with `enabled`, `allowed_repos`, `audience`.
3. Server middleware: `oidc_auth` — extract JWT from `Authorization: Bearer`,
   detect OIDC-vs-static via first segment (`eyJ` prefix), validate JWT
   (fetch+cache JWKS, verify signature, check claims).
4. Client: `arx push` auto-detects GitHub Actions, fetches OIDC token, sends it.
5. Tests: mock JWKS endpoint (local RSA key pair), validate a crafted JWT.
6. CI dogfood: the release workflow uses `arx push` with OIDC instead of
   `ARX_SERVE_TOKEN`.
