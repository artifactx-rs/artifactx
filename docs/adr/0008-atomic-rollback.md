# ADR-0008: Atomic rollback (pointer-flip)

- Status: **Accepted** (reviewed 2026-06-17) — apt implemented; yum pending
- Date: 2026-06-17

## Implementation status

- **apt: done & safe.** Publish commits into `dists/.states/<dist>/<NNNNNN>`;
  `dists/<dist>` is a symlink flipped atomically; `arx rollback`/`arx history`;
  `gc` won't prune a pool file pinned by a retained state. Verified e2e.
- **yum: pending.** Mirror the state-dir + symlink approach for `repodata`, and
  extend `gc`'s referenced-set to parse retained `primary.xml`.

## Review outcome

- **Swap = symlink flip.** Verified empirically that `tower-http` `ServeDir`
  follows a symlinked directory within the root (serves through `live → .states/v1`,
  HTTP 200). The single-syscall flip is viable. (Verifying this also caught a
  regression: `arx serve` had started requiring a signing key — now fixed.)
- **Retention K = the existing `gc --keep`.** One knob, no new config.
- **CLI-only first.** `arx rollback` / `arx history`; `POST /api/v1/rollback` later.

## Context

The review's highest-leverage idea: **roll back a bad publish in one command.**
Today (ADR-0004) publish is atomic but keeps **no history** — `commit_dist` deletes
the previous `dists/<dist>` on swap. And the pool is mutable, so naively restoring an
old `Release` can reference a `.deb`/`.rpm` that `rm`/`gc` has since deleted → the
client gets a 404. So rollback needs *metadata history* **and** *pool consistency*.

## Decision (proposed)

Make every publish produce a retained, immutable published state and flip a pointer:

1. **Publish to a content-addressed state dir**, e.g. `apt/dists/<dist>/` becomes a
   symlink to `apt/.states/<dist>/<id>/` (id = hash of the staged tree or a counter).
   Going live = `commit_dist` swaps the **symlink** (one atomic syscall — a true
   pointer flip, tightening ADR-0004's two-rename window).
2. **Retain the last `K` states** per dist (config, default e.g. 5). Older states are
   reaped.
3. `arx rollback <dist> [--to <id>]` flips the symlink to the previous (or chosen)
   state. `arx history <dist>` lists retained states with timestamps.
4. **Pool consistency:** `arx gc`/`rm` must **not** delete a pool file still
   referenced by any retained state. GC becomes mark-and-sweep over *live + retained*
   states. So rollback depth (`K`) bounds how long a removed package's bytes linger —
   one honest, predictable knob.

Expose exactly two verbs — `rollback`, `history`. **Not** aptly's full snapshot CRUD
(create/merge/diff/filter/drop); that is the toolkit complexity we reject.

## Consequences

- Good: one-command recovery from a bad publish; true single-syscall atomic flip.
- Good: rollback is safe because retained states pin their pool files.
- Bad: GC must consult retained states (more bookkeeping than today's "pool is truth").
- Bad: symlink-based layout — must confirm `tower-http` `ServeDir` follows the
  symlink and that the swap is transparent to in-flight requests.
- Bad: disk cost — `K` retained metadata trees + pinned pool files.

## Alternatives considered

- **Metadata-only rollback (no pool pinning).** Simplest, but unsafe — a rolled-back
  index can 404 on a since-deleted package. Rejected.
- **Snapshot the whole pool per publish (copy/hardlink).** Correct but heavy and
  duplicates the deb-s3 "no local tree" ideal in reverse. Overkill for `K` small.
- **Do nothing (re-push to fix).** The status quo; loses the headline safety story
  the wedge needs.

## Future improvements

`promote` (flip prod's pointer to staging's state — a move, not a copy); pruning
policy for states by age as well as count; surfacing the active state id in
`/api/v1/health`.

## Open questions for review

1. Is symlink-swap acceptable on the target deploys (containers, Windows dev)? If
   symlinks are a problem, fall back to the two-rename swap + a `current` marker file.
2. Default `K` (retained states)? Tie it to the existing `gc --keep`, or separate?
3. Should rollback be exposed over the HTTP API (`POST /api/v1/rollback`) in v1, or
   CLI-only first?
