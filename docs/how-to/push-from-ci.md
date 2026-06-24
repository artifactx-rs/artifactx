# Push packages from CI

`arx serve` exposes an authenticated write API. CI can push packages with either
a static bearer token or GitHub Actions OIDC. OIDC is preferred for GitHub-hosted
pipelines because the job mints a short-lived JWT instead of storing a long-lived
`ARX_SERVE_TOKEN` secret.

## Server: enable one write-auth mode

Static token:

```sh
ARX_SERVE_TOKEN='replace-with-a-long-random-token' arx serve --root /data/arx/repo
```

GitHub Actions OIDC:

```toml
# /data/arx/repo/arx.toml
[oidc]
enabled = true
audience = "arx"
allowed_repos = ["OWNER/REPO"]
```

Then run the server normally:

```sh
arx serve --root /data/arx/repo
```

Reads remain public. Write endpoints return `401` for missing/invalid bearer
credentials when a write-auth mode is configured, and `403` when no write-auth
mode is configured.

## GitHub Actions with OIDC

The workflow needs `id-token: write`. `arx push` reads GitHub's OIDC environment,
requests a JWT for the configured audience, and sends it as the bearer token.

```yaml
name: publish packages
on: [push]

permissions:
  contents: read
  id-token: write

jobs:
  push:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install artifactx
      - run: arx push dist/*.deb dist/*.rpm --url https://repo.example.com --oidc-audience arx
```

Keep the server allowlist tight. Prefer exact `OWNER/REPO` entries; use
`OWNER/*` only for a trusted organization-wide repository server.

## Static token fallback

Use a static token for non-GitHub CI or for a quick internal setup:

```yaml
steps:
  - uses: actions/checkout@v7
  - uses: dtolnay/rust-toolchain@stable
  - run: cargo install artifactx
  - run: arx push dist/*.deb --url https://repo.example.com --token "$ARX_SERVE_TOKEN"
    env:
      ARX_SERVE_TOKEN: ${{ secrets.ARX_SERVE_TOKEN }}
```

For shell scripts outside GitHub Actions, `arx push` also reads
`ARX_SERVE_TOKEN` from the environment when `--token` is omitted.
