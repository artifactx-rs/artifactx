# ADR-0009: Dogfood — ArtifactX packages and publishes ArtifactX

- Status: Accepted (low-risk ops; charter-checked)
- Date: 2026-06-17

## Context

Charter principle 5: *if the maintainers don't rely on it in production, users
shouldn't either.* The most persuasive demo is also the most honest one — `arx`
should build, package, and publish **itself**, and users should `apt-get install
arx` / `dnf install arx` from a repo `arx` produced.

The key question is **where the published repo lives**. A repository is just a
static directory (the whole design — ADR-0004), so it needs no running server in
production.

## Decision

A GitHub Actions workflow, on tag push, does the full pipeline:

1. Build the `arx` binary (musl static).
2. `arx pack packaging/arx.toml` → `arx_*.deb` + `arx-*.rpm`.
3. `arx init` a repo, `arx add` the packages, `arx publish` (signed).
4. Deploy the repo directory to **GitHub Pages**.

Users then add `https://artifactx-rs.github.io/artifactx/apt` (apt) or the yum
`baseurl` and `arx`'s public key. **No server runs in production** — Pages serves
the static tree. `arx serve` remains for self-hosters who want one binary.

Signing uses GitHub Actions **secrets**: `ARX_SIGNING_KEY` (armored private key,
imported via `arx key import`) and `ARX_KEY_PASSPHRASE`.

## Consequences

- Good: real dogfooding; a free, server-less public demo repo; proves "the repo
  is just a directory" end-to-end.
- Good: the same `pack → publish` path users run is what ships `arx`.
- Bad: the maintainer must add two secrets + enable Pages (documented).
- Bad: GitHub Pages is HTTP(S) static only — fine for a signed repo, but pushes
  (`arx push`) don't apply to a Pages target (that path is for self-hosters).

## Alternatives considered

- **Self-hosted `arx serve` + `arx push` from CI.** The truest test of the push
  path, but needs hosting we'd have to operate. Pages is free and zero-ops.
- **Release tarballs only (no repo).** Doesn't dogfood the repo/publish pillars.

## Future improvements

Also publish to a self-hosted `arx serve` to dogfood `arx push` + OIDC; multi-arch
packages; a `Containerfile` so `arx` ships as a `scratch` image too.
