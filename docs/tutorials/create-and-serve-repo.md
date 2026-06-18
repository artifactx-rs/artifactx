# Create and serve a new repo

Use this tutorial when you already have `.deb` or `.rpm` files and want to expose
them as a signed apt/yum repository.

## 1. Initialize the repo

```sh
arx init ./repo
```

This creates `arx.toml`, repository directories, and signing keys under
`./repo/keys/` unless `--no-key` is used.

## 2. Add packages

Add Debian and RPM packages in one command:

```sh
arx add dist/*.deb dist/*.rpm --root ./repo
```

You can override the default apt component or yum repo name:

```sh
arx add dist/*.deb --component main --root ./repo
arx add dist/*.rpm --repo myrepo --root ./repo
```

## 3. Publish metadata

```sh
arx publish --root ./repo
```

This generates apt/yum metadata and signs repository metadata when signing is
enabled.

For a strict publish that fails instead of skipping unreadable or colliding
packages:

```sh
arx publish --root ./repo --strict
```

## 4. Serve locally

```sh
arx serve --root ./repo
```

By default this listens on `127.0.0.1:8080`. Use a reverse proxy for public TLS
exposure, or pass `--addr` only when you intentionally want a different bind
address.

```sh
arx serve --root ./repo --addr 127.0.0.1:8080
```

## 5. Verify from clients

Follow [Install clients](../how-to/install-clients.md) for apt and dnf/yum
configuration.

## Optional: build packages from a manifest

ArtifactX can also build packages before adding them to a repo:

```sh
arx pack ./arx.toml --out dist
arx add dist/*.deb dist/*.rpm --root ./repo
arx publish --root ./repo
```

For Rust projects, omitting the manifest reads `./Cargo.toml` and
`[package.metadata.arx]`:

```sh
arx pack --out dist
```

## Optional: push to a running server

If `arx serve` is running with a write token, push packages over HTTP:

```sh
ARX_SERVE_TOKEN='change-me' arx serve --root ./repo
arx push --url http://127.0.0.1:8080 --token 'change-me' dist/myapp.deb
```

Reads are public. Writes require `ARX_SERVE_TOKEN` or configured OIDC.
