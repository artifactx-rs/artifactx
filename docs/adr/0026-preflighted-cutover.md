# ADR-0026: Preflighted live cutover

## Status

Accepted for v0.2.0.

## Context

Publish, export, validation, and live promotion are often chained together in
operator scripts. That is risky because a script can accidentally switch a live
repository path before generated apt/yum metadata has been validated, or can
confuse repository metadata signing with RPM payload signing.

ArtifactX needs a first-class cutover path for v0.2 that stays generic and does
not embed deployment-specific downstream synchronization.

## Decision

Add `arx cutover` as a preflighted workflow:

1. publish selected repository metadata unless `--no-publish` is passed;
2. export apt and/or flat yum layouts into a versioned staging directory;
3. validate apt `Release`/`Packages.gz` and yum `repomd.xml`/gzip metadata;
4. optionally require every staged RPM payload to carry an RPM signature with
   `--require-signed-rpms`;
5. atomically switch live paths by replacing symlinks;
6. leave `<live>.previous` symlink pointers for rollback to the prior live
   target when a prior symlink existed.

Live paths must be absent or symlinks. If a live path is an ordinary directory,
ArtifactX refuses to replace it so operators can perform the one-time migration
explicitly.

`--dry-run` performs publish/export/preflight and leaves the staged export in
place without switching live pointers.

## Consequences

- Operators get a single safe command for the common publish/export/cutover
  sequence.
- Existing public URL contracts can be preserved by pointing the live symlink at
  the latest versioned export.
- Rollback is fast when the previous pointer exists.
- RPM package signature policy is explicit and separate from repository metadata
  signing.
- Downstream replication remains outside ArtifactX core; lifecycle hooks can
  invoke deployment-specific sync commands after a successful cutover.

## Alternatives considered

- **Overwrite live directories directly.** Rejected because partial writes are
  harder to roll back and can expose invalid metadata to clients.
- **Allow non-symlink live paths to be renamed automatically.** Rejected because
  that is destructive in surprising environments; the one-time migration should
  be operator-controlled.
- **Treat signed `repomd.xml` as sufficient for `gpgcheck=1`.** Rejected because
  yum repository metadata signatures and RPM payload signatures protect
  different trust boundaries.
