# ADR-0023: Cutover preflight stops at the public repository boundary

- Status: Accepted & implemented
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

Ship one-command publish/cutover workflows for v0.2 that own the repository
boundary:

1. stage or add/import packages;
2. publish apt/yum metadata;
3. export candidate public layouts;
4. run preflight checks;
5. validate apt/dnf metadata and client compatibility where possible;
6. promote the candidate via an atomic live switch when the filesystem supports
   it;
7. leave an explicit rollback pointer.

The implementation exposes that boundary through:

- `arx publish --apt-live ... --yum-flat-live ... --staging-dir ...` for the
  common publish/export/preflight/cutover path;
- `arx cutover --no-publish ...` for the less-common case where metadata was
  already published and only the live pointer switch should run;
- `arx publish-dir <DIR>` for repeated package-drop ingestion followed by the
  same publish/cutover flow;
- explicit RPM payload signing gates (`--require-signed-rpms`, `--sign-rpms`,
  and `--rpm-sign-cmd`) so repository metadata signing is not confused with RPM
  payload signing.

Downstream sync, CDN invalidation, monitoring debounce, or site-specific service
orchestration remains outside ArtifactX core. Public docs describe that as an
integration boundary with generic examples only. `publish-dir --sync-cmd` is a
non-invasive hook after a successful non-no-op publish, not a built-in sync
provider.

## Consequences

- Good: operators get fewer fragile shell scripts for the common path.
- Good: ArtifactX keeps a crisp product boundary: repository correctness first,
  deployment-specific distribution second.
- Good: public docs can teach safe integration without exposing private topology.
- Cost: not every deployment can use the symlink live-path primitive, so some
  environments still need a one-time public-root migration or downstream wrapper.

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
