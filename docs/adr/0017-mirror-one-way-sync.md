# ADR-0017: `arx mirror` — one-way upstream sync (not mirroring-as-platform)

- Status: **Accepted**
- Date: 2026-06-17

## Context

`arx mirror` was implemented without a prior ADR. COMPETITORS.md listed
"mirroring-as-core" under Reject. This ADR resolves the apparent contradiction.

## What mirror IS

`arx mirror` is a **one-way pull sync**: fetch an upstream Packages index,
compare against a local SHA256 cache, download only new/changed packages.
It is `arx import` + a diff algorithm — a convenience for users who want to
keep a local copy of an upstream repo up to date.

It is NOT:
- A bidirectional sync platform
- A replication/mirroring engine
- A replacement for `aptly mirror` (which has snapshot management, multi-source
  merging, publishing workflows)
- A daemon or scheduled service

## What COMPETITORS.md rejected

The "Reject" entry said "mirroring-as-core" — meaning ArtifactX is not a
**mirroring platform** in the way aptly/Artifactory are. Those tools treat
mirroring as a first-class workflow with snapshot trees, merge policies, and
publish targets. ArtifactX does one thing: pull packages from upstream into
a local pool. One direction, one command.

## Decision

**Keep `arx mirror`** as a convenience command that builds on `arx import`.
It serves the Repository pillar (managing what's in your pool) and directly
supports the migration story (pull from old repo → publish with arx).

The COMPETITORS.md Reject entry is clarified: "mirroring-as-core
(bidirectional platform with snapshot management)" — arx mirror is not that.

## Consequences

- Good: users coming from aptly can `arx mirror` their old repo as a
  migration stepping stone.
- Good: keeps the import infrastructure useful for ongoing sync, not just
  one-shot migration.
- Bad: adds a top-level command that could confuse users expecting full
  mirroring capabilities. Mitigated by clear documentation.

## Alternatives considered

- **Remove mirror entirely.** Rejected: breaks the migration story. Users
  need a way to pull from their old repo.
- **Merge mirror into import (`arx import --sync`).** Considered but
  rejected for now — mirror has different semantics (SHA256 diff, prune,
  auto-publish). If the two converge, this can be revisited.
