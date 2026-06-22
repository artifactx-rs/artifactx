# ADR-0025: Explicit lifecycle hooks

## Status

Accepted for v0.2.0.

## Context

ArtifactX v0.2 focuses on complete publish and API workflows. Operators still
need a small extension point around client-visible state changes for validation,
notifications, audit capture, mirror synchronization, and local policy checks.
Those steps should not require fragile wrapper scripts around every `arx`
command, but ArtifactX also should not embed deployment-specific sync tooling in
core.

## Decision

Add configurable lifecycle hooks to `arx.toml` for publish, export, and rollback
boundaries:

- `pre_publish` / `post_publish`
- `pre_export` / `post_export`
- `pre_rollback` / `post_rollback`

Each hook is configured as an explicit executable plus arguments:

```toml
[[hooks.pre_publish]]
command = "sh"
args = ["-c", "test -f READY"]
```

ArtifactX runs hooks with the repository root as the working directory and adds
operation context through environment variables such as `ARX_HOOK`, `ARX_ROOT`,
`ARX_FORMATS`, `ARX_SUMMARY`, `ARX_TARGET`, and `ARX_STATE`.

Pre-hook failures abort before the corresponding state change. Post-hook
failures are reported after the state change has already completed, so post
hooks must be idempotent and safe to retry.

## Consequences

- Operators get deterministic extension points without maintaining separate
  wrapper choreography.
- Core remains generic: deployment-specific replication, notification, and audit
  commands live outside ArtifactX.
- No shell is invoked implicitly. Users who need shell behavior opt into it by
  setting `command = "sh"` and explicit `args`.
- Hook output is surfaced as command/API errors, but public documentation and
  examples avoid printing secrets. Hooks should receive secret references or
  environment variable names, not raw secret values.

## Alternatives considered

- **Embed downstream sync providers in core.** Rejected because sync topologies
  are deployment-specific and would expand the trust boundary.
- **Use one shell string per hook.** Rejected because implicit shell evaluation
  is harder to quote, test, and document safely.
- **Only support publish hooks.** Rejected because export and rollback also move
  client-visible repository state.
