# Integrate downstream sync safely

Use this guide when ArtifactX publishes repository metadata first, but another
site-specific tool still copies the public repository tree to mirrors, object
storage, a CDN origin, or a legacy web root.

ArtifactX should own package ingestion, repository metadata, signing, history,
and rollback state. Downstream sync should copy only the public files that
clients are allowed to fetch.

## Recommended chain

Keep the chain explicit:

```text
package drop
-> arx add/import into an ArtifactX root
-> arx publish
-> optional arx export for legacy public layouts
-> validate staged apt/yum clients
-> public-only downstream sync
-> optional debounce or monitoring automation
```

The important boundary is between `arx publish` / `arx export` and the external
sync step. ArtifactX creates a staged, client-consumable public tree; the sync
tool distributes that public tree without learning about private repository
state.

## What downstream sync may copy

Allow downstream tools to copy only public client-facing files, for example:

- apt metadata and payloads: `dists/`, `pool/`, `InRelease`, `Release.gpg`
- yum metadata and payloads: `repodata/`, `*.rpm`
- exported legacy layouts created by `arx export`
- static public keys that clients already need to trust

If a public URL needs a legacy layout, export into a fresh staging directory and
sync that directory instead of teaching downstream tools about ArtifactX internals:

```sh
arx publish --root ./repo
rm -rf ./staging-public
arx export --root ./repo \
  --apt-out ./staging-public/deb \
  --yum-flat-out ./staging-public/repo
# Run your site-specific public-only sync here.
```

## What downstream sync must not copy

Do not copy private or implementation-state paths into a public root:

- signing private keys or passphrase files
- temporary staging directories
- lock files
- rollback state directories
- local cache directories
- import scratch directories
- configuration files that contain secrets
- logs or monitor state
- private package drops that have not been published

A good rule: if a package manager client does not need the file to install a
package, keep it out of the public sync source.

## Preserve atomicity

Prefer a fresh staging destination plus an atomic pointer or directory switch in
your downstream environment:

1. Publish and export into a new local staging directory.
2. Validate clients against the staged directory or a temporary URL.
3. Sync the staged public tree to a versioned destination.
4. Switch the live pointer only after validation succeeds.
5. Keep the previous live pointer until rollback is no longer needed.

Avoid in-place updates to a live public directory when clients may fetch metadata
and payloads concurrently. In-place sync can expose mismatched `Packages`,
`repomd.xml`, and package files during a cutover.

## Validate before syncing

Run validation before the public sync step:

```sh
arx publish --root ./repo --strict
arx export --root ./repo --apt-out ./staging/deb --yum-flat-out ./staging/repo

# Example placeholders: use your own temporary HTTP root or container harness.
apt-get update
apt-cache policy myapp
dnf makecache
dnf repoquery myapp
```

For older yum clients, keep gzip metadata compatibility in mind. ArtifactX writes
yum metadata as gzip by default so CentOS 7-era clients can read it.

## Migration checklist

Before keeping legacy sync automation in the path, verify:

- [ ] ArtifactX is the only writer for package pools, metadata, and repository
      signing state.
- [ ] The downstream sync source contains only public client-facing files.
- [ ] Signing keys, passphrases, rollback state, caches, locks, and logs are
      excluded from the sync source.
- [ ] The sync target is staged or versioned before becoming live.
- [ ] apt validation covers identity fields (`Origin`, `Label`, `Suite`, and
      `Codename`) before cutover.
- [ ] yum validation covers `repomd.xml`, gzip metadata, and package payload
      availability.
- [ ] The previous public tree or pointer remains available for rollback.
- [ ] Monitoring watches the public URL after the live switch, not private build
      paths.
- [ ] Examples, scripts, and dashboards use generic public paths in shared docs.

## When to remove downstream sync

Keep downstream sync only when it still provides value, such as mirror fan-out,
CDN upload, or compatibility with an existing public URL. If ArtifactX already
serves the repository directly and no separate distribution step is needed,
removing redundant sync automation reduces cutover risk.
