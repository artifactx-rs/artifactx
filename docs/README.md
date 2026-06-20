# ArtifactX documentation

ArtifactX builds, imports, signs, serves, and operates Linux package repositories
from one static binary. Use this page to pick the shortest path for the job in
front of you.

## Start here

| If you want to... | Read this |
| --- | --- |
| Move packages from an existing apt/yum repo with low risk | [Import an existing repo](tutorials/import-existing-repo.md) |
| Create a new repo from local `.deb`/`.rpm` files | [Create and serve a repo](tutorials/create-and-serve-repo.md) |
| Install packages from an ArtifactX repo | [Install clients](how-to/install-clients.md) |
| Run ArtifactX with Docker Compose | [Run with Docker Compose](how-to/run-with-docker-compose.md) |
| Run the server under systemd | [Run as a systemd service](how-to/run-as-systemd-service.md) |
| Use your organization signing key | [Use custom signing keys](how-to/use-custom-signing-keys.md) |
| Find every CLI command and option | [CLI reference](reference/cli.md) |
| Integrate with `arx serve` over HTTP | [HTTP API reference](reference/http-api.md) / [OpenAPI](reference/openapi.yaml) |
| Understand `arx.toml` | [Configuration reference](reference/config.md) |
| Understand repo signing and expiry | [Signing and expiry](explanation/signing-and-expiry.md) |
| Operate backups, restore, and rollback | [Operations guide](OPERATIONS.md) |
| Understand design decisions | [ADR index](adr/README.md) |

## Common workflows

### Painless migration

Use `arx import` against the repo you already publish, limit the first import,
republish metadata under your own key, then canary clients before cutover.

```sh
arx init ./repo
arx import https://packages.example.com --apt --dist stable --component main --match-name myapp --limit 20 --root ./repo
arx publish --root ./repo
arx serve --root ./repo
```

### New repo from build artifacts

Use packages you already built, then publish apt/yum metadata.

```sh
arx init ./repo
arx add dist/*.deb dist/*.rpm --root ./repo
arx publish --root ./repo
arx serve --root ./repo
```

### API-first operation

`arx serve` exposes static repo files and an authenticated write API. Reads are
public. Writes require `ARX_SERVE_TOKEN` or configured GitHub Actions OIDC.
See the [HTTP API reference](reference/http-api.md) for every endpoint.

```sh
ARX_SERVE_TOKEN='change-me' arx serve --root ./repo
arx push --url http://127.0.0.1:8080 --token 'change-me' dist/myapp.deb
```

## What ArtifactX signs

ArtifactX signs repository metadata:

- apt: `InRelease` and `Release.gpg`
- yum/dnf: `repomd.xml.asc`

It does not re-sign individual package payloads. If your organization requires
package-level signatures, keep that policy in your package build pipeline and use
ArtifactX for repository metadata.

## Documentation rules for contributors

- Public examples should be executable or marked as placeholders.
- Default server examples bind to `127.0.0.1` unless the text is explicitly about
  containers or reverse proxies.
- Do not document roadmap items as shipped features.
- Link ADRs for rationale, but keep user tasks in tutorials and how-to guides.

See [Documentation Plan](DOCS_PLAN.md) for the rollout sequence.
