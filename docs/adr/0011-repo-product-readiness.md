# ADR-0011: Repository product-readiness — trust & robustness hardening

- Status: **Accepted** (revised after adversarial review — see "Review outcome")
- Date: 2026-06-17
- Decided: `valid_days` init default = **7**; bad-package default = **forgiving +
  always-visible** (`--strict` for CI). Comparators: `debversion` + `rpm-version`.

## Context

`repo` (apt + yum generation/publish) is the deepest, most-verified part of
ArtifactX — strategic bet #1 (*depth > breadth*). Before we invest further in
`pack`, `repo` must clear a **product-ready bar**: a user can trust it in
production. An evidence-based audit of `crates/arx-debrepo` and `crates/arx`
confirmed the atomic core is sound — `stage_dist` writes only a staging
directory and `commit_dist` flips a symlink, so a failed publish (bad package,
disk-full, partial write) **never touches the live repo** and self-heals on the
next run. That integrity guarantee is the foundation; the gaps below are about
*trust signals*, *robustness UX*, and *correctness*, not corruption.

### The product-ready bar (the gate this ADR establishes)

A user can trust `repo` in production when:

1. **Metadata carries freshness/expiry** — protects against repository
   *freeze / replay* attacks (a MITM serving stale, vulnerable metadata forever).
2. **A single bad or duplicate package cannot wedge publishing** — one corrupt
   `.deb` must not block every future publish.
3. **Retention is semantically correct** — GC keeps the *newest by version*, not
   the newest by mtime.
4. **Both apt *and* yum have end-to-end, real-client test evidence** — we may
   only *claim* trust we have verified.
5. **There is a backup/restore runbook** — pool loss is recoverable.

This ADR decides items **1–3** (the code-bearing trust/robustness/correctness
changes). Items 4–5 (yum integration tests, backup runbook) are follow-up work
tracked on the board; tests and docs are trivial enough to skip their own ADR.

## Review outcome (adversarial pass — incorporated)

An independent review (against the charter, COMPETITORS.md, ADR-0004/0007, and
the code) returned **Accept-with-changes**. The three decisions are
charter-aligned and don't conflict with the atomic-publish/GC design; signing
covers the whole `release_text` so `Valid-Until` is inside the signed payload
(a self-listed concern, dismissed). Six required changes, all folded in below:

- **C1** — `pool::Entry` (pool.rs:22-31) stores only `version`, dropping epoch
  and release; rpm EVR ordering is impossible without them. → Decision #3 now
  extends `Entry` with `epoch`/`release` (a stated data-structure change).
- **C2** — `stage_dist` has **no** dedup today (lib.rs:218-237); dedup is **new**
  logic, not a tweak. → Decision #2 states this explicitly with the dedup key,
  collision rule, and determinism basis.
- **M1** — skip-and-warn must not be *silent*: a forgiving default that exits 0
  while dropping packages violates "design for operations: observable". → skips
  are **always** surfaced (stderr summary + metrics counter in *this* batch).
- **M2** — the `push`/server path needs defined strict semantics. → server uses
  `[apt].strict`; a skipped package under strict returns 4xx, else 200 with the
  skip list in the response body.
- **M3** — hand-rolled Debian vercmp (epoch/`~`/segment alternation) is
  data-loss-prone; ADR-0007 deferred GC ordering for exactly this reason. → use
  a **tested** comparator (crate), not a hand-roll, with dpkg test vectors as
  acceptance criteria.
- **M4** — `arx init` cannot write a *commented* `valid_days = 7`
  (`toml::to_string_pretty` emits no comments) and serde-default would be `0`. →
  `Apt.valid_days` serde-defaults to `0`; `cmd_init` explicitly sets `7` before
  `save`; the explanatory note lives in docs/README, not an inline TOML comment.

## Decision (proposed)

### 1. `Valid-Until` in apt `Release` (freeze protection)

`render_release` emits `Valid-Until: <Date + N days>` when a positive window is
configured. Add `valid_days: u32` to `[apt]` (`config.rs` `Apt`) and to
`ReleaseMeta`. **serde default = `0`.**

- `valid_days = 0` → **omit** `Valid-Until` (today's behavior; never surprises a
  publish-and-walk-away repo by silently expiring).
- `valid_days > 0` → emit it; republishing refreshes the window.
- **`cmd_init` explicitly sets `cfg.apt.valid_days = 7` before `save`** (M4): the
  serde default stays `0` so legacy/programmatic repos that never re-init are
  unchanged, while freshly `init`-ed repos are **secure-by-default** (Caddy:
  correct defaults). The `toml` serializer emits no inline comments, so the
  rationale lives in the README / wiki, **not** in `arx.toml`.
- **Format/snapshot (m1/m2):** `render_release` takes one `Utc::now()` snapshot;
  `Date` and `Valid-Until` both derive from it (`Valid-Until = now + N days`),
  both formatted with the identical RFC822 pattern
  `%a, %d %b %Y %H:%M:%S UTC` so apt parses them.

yum side: `repomd.xml` has no server-side expiry in the spec — out of scope.
(Freshness on yum is client-side via `metadata_expire`; documented, not our
concern here.)

### 2. Bad / duplicate package handling in `stage_dist` (**new** logic)

Today one unreadable `.deb` aborts the whole stage (`deb::read_control(..)?` at
`debrepo/lib.rs:219`), and there is **no de-duplication at all** — two files with
the same name+version emit two stanzas (lib.rs:218-237). Both are *new* behavior:

- **Skip-and-warn default, always visible (M1).** A `.deb` that fails to parse is
  `tracing::warn!`-logged and **skipped**; the publish proceeds with the good
  packages. `StagedDist` gains `skipped: Vec<SkippedDeb { path, reason }>`. Even
  in the forgiving (non-strict) default, the CLI prints a loud **stderr** summary
  (`WARNING: skipped N package(s): …`) and increments a metrics counter **in this
  same change** (not deferred) — a forgiving default must still be observable.
- **Strict mode (`--strict`; `[apt].strict`, serde default `false`).** Any skip
  becomes a hard error → publish fails, nothing committed (staging is discarded,
  live dist untouched). For CI that wants all-or-nothing.
- **Push/server path (M2).** Remote `POST /api/v1/packages` → server republish
  reads `[apt].strict` from the server's config as the source of truth. Under
  strict, a package that would be skipped makes the request return **4xx** with
  the reason; otherwise **200** with the skip list in the response body, so
  one-line CI push can't silently drop a package behind a green check.
- **De-duplication (new).** Within the stage, packages are keyed by
  `(Package, Version, Architecture)`. Iteration order is `debs_in`'s result,
  which is **`debs.sort()`-ed (lib.rs:156)** — that sorted order is the
  determinism basis. First occurrence wins. If a later file shares the key but
  has a **different SHA256** (a real collision), warn loudly and record it as
  skipped (kept out of the index); identical re-adds are simply idempotent. This
  makes accidental double-push deterministic instead of emitting duplicate
  stanzas.

### 3. semver-aware GC ordering (with full EVR data — C1)

`pool.rs` GC currently orders by mtime (`pool.rs:247` notes this is provisional),
and `pool::Entry` (pool.rs:22-31) only stores `version` — **dropping epoch and
release**, without which rpm EVR comparison is impossible and `1.0-1` vs `1.0-2`
are indistinguishable (a delete-the-wrong-file hazard).

- **Extend `Entry`** with `epoch: Option<i32>` and `release: String`. `scan_yum`
  (pool.rs:118-126) fills them from `pkg.epoch`/`pkg.release` (upstream
  `createrepo_rs` splits EVR into three fields). For `.deb`, parse the Debian
  version string (epoch before `:`, revision after the last `-`); leave `release`
  empty when absent. `group_key` (pool.rs:34) keys on name only; ordering uses
  the full version triple.
- **Tested comparator, not hand-rolled (M3) — crates selected:**
  - `.deb` → **`debversion` 0.5.4** (Apache-2.0, compatible; `Version: Ord`;
    maintained by a Debian core dev — `jelmer/debian-parsers`). Note: pulls
    `num-bigint` + `lazy-regex` (chrono is already a dep). Accepted: data-loss
    correctness outweighs a heavier dep (M3).
  - `.rpm` → **`rpm-version` 0.5.0** (MIT, compatible; `rpm_evr_compare`/`Evr`;
    the algorithm the main `rpm` crate uses, 50+ tilde/epoch test cases).
  - The dead `deb-version` 0.1.1 (2019, unmaintained) the review suggested is
    **rejected**. Hand-rolling dpkg vercmp stays rejected (ADR-0007's data-loss
    rationale).
- **Acceptance test vectors (required):** epoch dominates (`2:1.0` > `1:9.9`);
  `1.0~rc1` < `1.0`; release compared (`1.0-2` > `1.0-1`); pure-alpha versions;
  **per-pair** fallback to mtime only when a *single* pairwise compare is
  unparseable (not whole-group) — granularity fixed to avoid ambiguity.
- **Rollback interaction (confirmed):** the retained set becomes (newest-by-EVR
  for `--keep N`) ∪ (files pinned by `retained_for_rollback`, pool.rs:202-232).
  The union may exceed N; that is existing, intended behavior (rollback safety
  wins) and is unchanged by this ADR.

## Consequences

- Good: real freeze-attack protection; publishing is resilient to one bad input;
  retention does what users expect. All three are "toy → product" signals.
- Good: defaults preserve the 5-minute path (`valid_days=0` unless `init` opts
  in; skip-and-warn is the forgiving default) — yet skips stay observable.
- Bad / cost: `ReleaseMeta`, `StagedDist`, and `pool::Entry` grow fields (minor
  internal API churn); one new dependency (a vetted version comparator) lands —
  justified because deleting the wrong package is a worse bug than a dependency
  (charter principle 1: complexity is a bug, but data loss is a bigger one).

## Explicitly NOT in this ADR (charter — compete by deleting)

- **`Contents-<arch>` index** (`apt-file`): niche; defer (separate item).
- **Key rotation / revocation**: real but a larger security design → own ADR.
- **Multi-dist single publish**: call `arx publish` per dist today; defer.
- **apt+yum cross-target two-phase commit**: apt and yum are *independent*
  repos at different URLs. A partial publish = "yum didn't update", recoverable
  via `arx rollback`. A 2PC across two namespaces is over-engineering; instead
  **document the recovery path**. (Audit flagged this P0; downgraded to "doc".)

## Alternatives considered

- **Always emit `Valid-Until` with a fixed window.** Rejected: silently breaks
  publish-and-walk-away repos. The `init`-writes-default compromise is safer.
- **Manage OpenPGP key expiry automatically.** Rejected: key lifetime is an
  operator/governance policy. ArtifactX should generate a compatible default key
  for the 5-minute path and let organizations import their own managed keys when
  they have expiry, HSM/KMS, audit, or trust-rollout requirements.
- **Invent yum/dnf metadata expiry.** Rejected: yum repo metadata does not have an
  ArtifactX-owned equivalent to apt `Valid-Until`; rely on signed `repomd.xml`,
  client cache policy, and republish/rollback rather than adding a fake policy
  knob.
- **Hard-fail on any bad package (status quo).** Rejected as the default: one
  bad input shouldn't deny service to good packages. Kept as `--strict`.
- **Pull in a full Debian-version crate.** Considered for #3; lean to a tiny
  vendored comparator first (deterministic, dependency-light per ADR-0005),
  revisit if rpm EVR edge cases demand a crate.

## Open questions for review (decided)

1. **`valid_days` default for `arx init`** — **decided: `7`** (Debian stable
   cadence; dogfood CI republishes well within the window; small freeze window
   without expiring weekly-cadence projects). serde default stays `0`.
2. **Strict mode default** — **decided: forgiving (skip-and-warn) but always
   visible**, `--strict` for CI; server uses `[apt].strict`. (M1/M2.)
3. **Version comparator** — **decided: `debversion` 0.5.4 (.deb, Apache-2.0) +
   `rpm-version` 0.5.0 (.rpm, MIT).** Both license-compatible with the workspace
   MIT/Apache and actively maintained; the `deb-version` crate is dead and was
   rejected. No hand-roll.

## Future improvements

- yum end-to-end integration test (real `dnf` in a container) — closes bar #4.
- Backup/restore runbook + optional `arx backup` convenience — closes bar #5.
- Publish metrics: skipped/collision counters, pool size gauge.
