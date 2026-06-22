# ADR-0024: Keep the public roadmap generic and English-only

- Status: Accepted
- Date: 2026-06-22

## Context

ArtifactX is public. Issues, ADRs, README content, tests, and roadmap entries are
indexed by GitHub and external search engines. Public artifacts should help
contributors understand the product without exposing deployment-specific
hostnames, service names, internal directories, IP addresses, package names, or
operational logs.

The maintainer's working language can be Chinese in private coordination, but the
public repository should be English-only for consistency and contributor access.

## Decision

Public repository content must use generic examples:

- packages: `demo`, `demo-agent`, `otherpkg`, `example-service`;
- repos/scopes: `example`, `staging`, `prod`;
- paths: `/path/to/repo`, `/public/deb`, `/public/repo`, or relative examples;
- hosts: `packages.example.com` or `example.com`.

Do not commit or post deployment-specific hostnames, IP addresses, machine names,
service names, internal package names, live filesystem paths, or log locations.
Public text should be English-only. Private operational lessons belong in private
notes, not public issues or ADRs.

## Consequences

- Good: public docs remain contributor-friendly and safe to index.
- Good: examples are reusable by anyone.
- Good: future agents have a clear rule before opening issues or PRs.
- Cost: dogfood notes require a translation step from private evidence to generic
  product requirements.

## Alternatives considered

1. **Keep raw dogfood details in issues for traceability.** Rejected: public
   search/history can retain sensitive deployment details.
2. **Mix Chinese maintainer notes into public issues.** Rejected: public project
   content should be consistent and accessible to contributors.
3. **Avoid mentioning dogfood entirely.** Rejected: product decisions still need
   evidence; the evidence should be generalized.

## Future improvements

- Add a lightweight pre-commit or CI grep for known sensitive patterns.
- Keep a private operations notebook for raw deployment details and link only to
  sanitized product issues publicly.
