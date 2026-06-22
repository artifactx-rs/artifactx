# ADR-0007: Pool retention & GC

- Status: Accepted
- Date: 2026-06-17

## Context

A repository that only grows is a footgun: you must be able to yank a bad/CVE
package and to prune old versions. The pool is the source of truth, so removal is a
pool operation followed by a republish (the aptly model: edit, then publish).

## Decision

- `arx rm <name> [--version V]` — yank: delete matching pool file(s). Exact
  name/version match (read from package metadata, not the filename).
- `arx gc [name] --keep N [--dry-run]` — retention: per `(scope, name, arch)`,
  keep the `N` newest versions and delete older files. `--dry-run` previews.
- `--name-prefix` scopes GC to a package family, and `--keep-within` / `--grace`
  add time-based protection around the version policy.
- Retained rollback states pin referenced files by default so `arx rollback`
  stays valid. Operators may pass `--ignore-rollback-states` only after
  intentionally giving up that rollback safety net.
- Both touch the pool only and print "run `arx publish`". Over the API, `DELETE` and
  `POST /gc` do the same and **republish automatically**.

Retention orders by tested Debian/RPM version comparison where possible, falling
back to mtime only when a version is unparseable.

## Consequences

- Good: removal/pruning exists and is predictable; a small set of knobs covers
  common cleanup without a policy DSL.
- Good: package-scoped cleanup supports real production workflows such as
  pruning one stale package family while leaving unrelated packages untouched.
- Bad: rollback-state retention can keep old blobs longer than expected, but the
  command reports that count and exposes an explicit override.

## Alternatives considered

- **Hand-rolled dpkg/rpm `vercmp`.** Correct ordering, but both algorithms are
  fiddly; a subtle bug deletes the wrong file. Deferred until done with a tested
  implementation.
- **Nexus-style retention policy engine.** Rejected — scope creep; one knob beats a
  rules language.

## Future improvements

Dry-run summaries grouped by package family; a separate command to expire or
compact rollback states before package cleanup.
