# ADR-0004: Atomic publish via staging + by-hash

- Status: Accepted
- Date: 2026-06-17

## Context

A repository publish rewrites many index files. If a client runs `apt-get update`
while we're halfway through, it can fetch a `Release` that disagrees with the
`Packages` it then downloads → `Hash Sum mismatch`. aptly's `./public` has hit
exactly this inconsistency on a bad publish. We want publish to be **atomic** and
**multi-component** (a single `Release` per dist covering all components — an early
bug overwrote it per-component).

## Decision

`debrepo` builds the entire `dists/<dist>` tree into a staging directory
(`dists/.<dist>.staging`), the caller signs *inside* staging, then `commit_dist`
**atomically swaps** it into place with a directory rename. We also write
`by-hash/SHA256/<hash>` copies of every index and set `Acquire-By-Hash: yes`, so a
client using a slightly older `Release` can still fetch the exact index it expects.
A lockfile serialises concurrent publishes.

## Consequences

- Good: clients never see a torn publish; signatures are part of the atomic unit.
- Good: one `Release` correctly spans all components/arches.
- Bad: a republish currently rewrites the whole tree (O(repo), not O(changes)).
- Bad: the swap is two renames (old aside, new in) — a microscopic window, not a
  true single-rename atomic flip.

## Alternatives considered

- **In-place writes.** Simple, but the torn-read bug is unacceptable.
- **aptly-style snapshots.** Full snapshot CRUD is toolkit complexity we reject; we
  took the *guarantee* (atomic + immutable state), not the surface. See ADR-0008.

## Future improvements

Incremental publish (reuse unchanged index data, à la `createrepo_c --update`); a
true pointer-flip (publish to a content-addressed dir, swap a symlink) which also
unlocks one-command rollback (ADR-0008).
