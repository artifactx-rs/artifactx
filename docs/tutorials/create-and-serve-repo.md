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

You can also point `arx add` at one or more directories:

```sh
arx add ./dist --root ./repo
```

Directory inputs are recursive, sorted before processing, and ignore unrelated
files. ArtifactX does not follow symlinked directories during discovery. If a
directory contains no `.deb` or `.rpm` files, the command fails with an
actionable error instead of silently doing nothing.

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

ArtifactX can also build packages before adding them to a repo. A manifest can
install individual files and whole directory trees:

```toml
[[files]]
source = "target/release/myapp"
dest = "/usr/bin/myapp"
mode = "0755"

[[dirs]]
source = "assets"
dest = "/usr/share/myapp/assets"
```

```sh
arx pack ./arx.toml --out dist
arx add dist --root ./repo
arx publish --root ./repo
```

For Rust projects, omitting the manifest reads `./Cargo.toml` and
`[package.metadata.arx]`:

```sh
arx pack --out dist
```

## Optional: operate a repeated package drop directory

Use `publish-dir` instead of hand-written wrapper scripts when a build system
keeps dropping already-built `.deb` or `.rpm` files into the same directory. It
detects unchanged inputs, publishes only when needed, and can reuse the live
cutover flags from `arx publish`:

```sh
arx publish-dir ./dist --root ./repo \
  --apt-live ./public/deb \
  --yum-flat-live ./public/repo
```

This is repository ingestion for already-built packages. It is different from
the `[[dirs]]` pack manifest feature above, which installs a directory tree
inside a package payload.

## Optional: push to a running server

If `arx serve` is running with a write token, push packages over HTTP:

```sh
ARX_SERVE_TOKEN='change-me' arx serve --root ./repo
arx push --url http://127.0.0.1:8080 --token 'change-me' dist/myapp.deb
```

Reads are public. Writes require `ARX_SERVE_TOKEN` or configured OIDC.
