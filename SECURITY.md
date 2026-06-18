# Security Policy

## Supported versions

ArtifactX is early-stage. Security fixes target `main` first and are included in the next release.

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability.

Use GitHub private vulnerability reporting if available for this repository, or email the maintainer address listed in the package metadata. Include:

- affected command or API endpoint;
- whether the issue involves signing keys, repository metadata, package ingestion, authentication, or CI release artifacts;
- a minimal reproduction or proof of concept;
- whether any private package data or signing material may have been exposed.

## Security boundaries

- ArtifactX signs repository metadata: apt `InRelease` / `Release.gpg` and yum `repomd.xml.asc`.
- ArtifactX does not re-sign individual `.deb` or `.rpm` package payloads during import.
- Treat `keys/private.asc`, CI signing secrets, and push tokens as production credentials.
- Public demo repositories must publish only public keys and package metadata. They must never include private signing keys.
