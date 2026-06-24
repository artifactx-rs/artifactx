# ADR-0028: Release tags publish only the newest version

- Status: Accepted
- Date: 2026-06-24
- Target: v0.2.x release automation

## Context

ArtifactX maintainers sometimes iterate through several patch tags quickly while
hardening release automation. Every pushed `v*` tag used to be able to run the
full release workflow: build static binaries, create or update a GitHub Release,
push GHCR images, and deploy the dogfood GitHub Pages repository.

That is correct for normal releases, but it is risky during rapid local patch
iteration:

- an older tag can finish after a newer tag and overwrite `latest` release
  aliases, GHCR `latest`, or Pages with stale artifacts;
- multiple full release jobs waste CI time while only the newest tag should be
  client-visible;
- Pages should represent the current Cargo package version, not whichever tag
  happened to deploy last.

The crates.io workflow is already idempotent: it verifies synchronized versions,
publishes missing crates, and skips versions that are already present.

## Decision

Keep tag pushes as the release trigger, but make the release workflow publish
artifacts only for the newest semantic `v*` tag known to the checkout.

For a tag push:

1. verify the tag version exactly matches `crates/arx/Cargo.toml`;
2. fetch all tags so version ordering is meaningful;
3. compute the newest `v*` tag with `git tag --sort=-v:refname`;
4. if the pushed tag is not the newest tag, stop after metadata verification and
   skip static binaries, GitHub Release writes, GHCR, and Pages deployment;
5. if the pushed tag is the newest tag, run the normal release, package, GHCR,
   and Pages jobs.

The release workflow uses one tag-concurrency group so a newer tag can cancel an
older in-flight release. Manual dispatch remains available for maintainers, but
normal public releases should come from annotated `vX.Y.Z` tags.

The standalone `pages` workflow remains the safe path for landing-page or
installer-only changes. It reads the Cargo version, downloads the matching
`arx-latest-amd64` release asset when present, and redeploys Pages without
rebuilding Rust.

## Consequences

- Good: rapid `v0.2.x` iteration cannot leave Pages or `latest` release aliases
  pointing at an older patch release.
- Good: release automation still validates stale tags enough to catch version
  mismatches, but avoids expensive and externally visible publication steps.
- Good: the crates.io workflow can remain idempotent and retry-friendly without
  being coupled to Pages or GHCR publication.
- Cost: a deliberately re-run old tag will not republish public artifacts while a
  newer tag exists. Maintainers must tag a newer version or use a deliberate
  manual recovery path.

## Alternatives considered

- **Publish every tag fully.** Rejected because older in-flight tags can race and
  overwrite public "latest" surfaces after a newer release is available.
- **Delete old rapid-iteration tags.** Rejected because it rewrites release
  history and still does not protect against in-flight workflow races.
- **Move all publication to manual dispatch.** Rejected because normal releases
  should stay reproducible from Git tags.

## Future improvements

- Add an explicit manual recovery input if maintainers ever need to republish an
  older tag for forensic or rollback reasons.
- Surface the "skipped because a newer tag exists" decision in release notes or a
  lightweight workflow summary if GitHub Actions summaries become useful for this
  project.
