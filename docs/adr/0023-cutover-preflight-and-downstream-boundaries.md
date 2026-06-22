# ADR-0023: Cutover preflight stops at the public repository boundary

- Status: Proposed
- Date: 2026-06-22

## Context

ArtifactX can publish apt and yum metadata, export legacy-compatible layouts, and
serve repositories. Production-shaped migrations still involve a broader chain:
staging packages, publishing metadata, exporting public roots, validating clients,
atomically switching live roots, and then letting deployment-specific sync or
monitoring automation distribute the result.

The product should make the common repository cutover safe and boring, but it
should not absorb every downstream replication system into core ArtifactX.

## Decision

Design a one-command publish/cutover workflow for v0.2 that owns the repository
boundary:

1. stage or add/import packages;
2. publish apt/yum metadata;
3. export candidate public layouts;
4. run preflight checks;
5. validate apt/dnf metadata and client compatibility where possible;
6. promote the candidate via an atomic live switch when the filesystem supports
   it;
7. leave an explicit rollback pointer.

Downstream sync, CDN invalidation, monitoring debounce, or site-specific service
orchestration remains outside ArtifactX core. Public docs should describe that as
an integration boundary with generic examples only.

## Consequences

- Good: operators get fewer fragile shell scripts for the common path.
- Good: ArtifactX keeps a crisp product boundary: repository correctness first,
  deployment-specific distribution second.
- Good: public docs can teach safe integration without exposing private topology.
- Cost: preflight will need careful failure messages and rollback semantics.
- Cost: not every deployment can use one atomic switch primitive, so the command
  may need platform-specific checks.

## Alternatives considered

1. **Document shell snippets only.** Rejected: the same risky choreography would
   be copied across deployments.
2. **Bundle downstream sync providers into core.** Rejected: too broad for v0.2
   and likely to turn ArtifactX into an operations platform by accident.
3. **Leave cutover entirely to users.** Rejected: publish/API completeness should
   include a safe migration path, not just metadata generation.

## Future improvements

- Add plugin or hook points only after the repository cutover contract is stable.
- Add remote/object-storage promotion once ADR-0015 is revisited.
- Add UI support in v0.4 after CLI/API semantics are stable.
