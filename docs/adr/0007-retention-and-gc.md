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
- `arx gc --keep N [--dry-run]` — retention: per `(scope, name, arch)`, keep the `N`
  most recent files, delete older. `--dry-run` previews.
- Both touch the pool only and print "run `arx publish`". Over the API, `DELETE` and
  `POST /gc` do the same and **republish automatically**.

Retention orders by **recency (file mtime)**, not semantic version. This is simple
and *safe* — a buggy version comparator could delete the wrong package (data loss).

## Consequences

- Good: removal/pruning exists and is predictable; one knob (`--keep`), no policy DSL.
- Good: mtime ≈ upload order ≈ version order in practice for normal workflows.
- Bad: re-adding an old version resets its mtime; a hand-edited pool can reorder.
  Documented honestly; not version-correct.

## Alternatives considered

- **Hand-rolled dpkg/rpm `vercmp`.** Correct ordering, but both algorithms are
  fiddly; a subtle bug deletes the wrong file. Deferred until done with a tested
  implementation.
- **Nexus-style retention policy engine.** Rejected — scope creep; one knob beats a
  rules language.

## Future improvements

Semver-aware retention (proper deb/rpm version comparison); `--keep-within 90d`;
`gc --grace` so a just-yanked blob isn't reaped mid-deploy; a bytes-freed report.
