# Governance

ArtifactX is maintainer-led.

## Decision model

- Small fixes and docs improvements can land with normal review.
- Behavior changes should include tests and docs updates.
- Non-trivial product or architecture changes start as an ADR under `docs/adr/`.
- During the current feature freeze, broad new features are parked unless they directly improve import-first polish.

## Maintainer priorities

1. Keep the import/create/publish/serve/client-install paths reliable.
2. Protect signing keys, release artifacts, and public repository trust boundaries.
3. Keep the project understandable as one static binary, not a platform.
4. Prefer operational clarity over feature count.

## Release authority

Maintainers decide when `main` is ready for a tag. Manual workflow dispatch on `main` may update the Pages demo, but must not create a version tag, GitHub Release, or GHCR image.
