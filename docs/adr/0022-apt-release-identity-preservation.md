# ADR-0022: Preserve apt Release identity during imports

- Status: Accepted
- Date: 2026-06-22

## Context

ArtifactX imports package payloads from an existing apt repository and then
regenerates repository metadata under the ArtifactX signing boundary. That is the
right trust model: upstream `InRelease` / `Release.gpg` signatures cannot be
reused after paths, checksums, expiry, and signing keys change.

However, apt clients also treat `Origin`, `Label`, `Suite`, and `Codename` as
repository identity. If those fields change accidentally during migration,
`apt-secure` requires explicit operator acceptance before updates continue. A
safe migration should not create that client-facing identity change unless the
operator chose it deliberately.

## Decision

During `arx import --apt`, read upstream `dists/<dist>/Release` when available and
copy the `Origin`, `Label`, `Suite`, and `Codename` fields into `[apt.release]` in
`arx.toml`. Older configs used `[repo]`; that remains a compatibility alias.

During `arx publish --apt`, render `Suite` and `Codename` from those config
fields when present; otherwise fall back to `[apt].dist` for both fields.

This preserves apt identity while still regenerating and signing fresh metadata
under ArtifactX control.

## Consequences

- Good: migration cutovers avoid accidental apt-secure identity prompts.
- Good: the behavior is inspectable and overrideable in `arx.toml`.
- Good: tests cover importing identity and publishing distinct `Suite` /
  `Codename` values.
- Cost: `arx import --apt` now mutates repository config when upstream identity
  is readable.
- Cost: intentional identity changes require operators to edit `[apt.release]` and
  understand that clients may need explicit acceptance.

## Alternatives considered

1. **Always use ArtifactX defaults.** Rejected: it breaks smooth migrations by
   changing apt identity unexpectedly.
2. **Reuse upstream signatures.** Rejected: regenerated metadata has a new trust
   boundary and cannot safely reuse upstream signatures.
3. **Require manual config edits only.** Rejected: too easy to miss during a
   migration; preserving identity is the safer default.

## Future improvements

- Add a cutover preflight that compares the current live apt identity against the
  candidate export and reports identity deltas before promotion.
- Add an explicit `--no-preserve-apt-identity` or `--set-origin` style override
  only if real workflows need it.
