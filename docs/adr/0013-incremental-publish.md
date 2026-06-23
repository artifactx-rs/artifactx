# ADR-0013: Incremental publish — O(changes), not O(repo)

- Status: **Accepted**
- Date: 2026-06-17
- Decided: incremental = **default** (`--full` to opt out); yum cache = **TOML manifest with reusable XML fragments** for `primary`, `filelists`, and `other`.

## Context

Every `arx publish` rebuilds all metadata from every package in the pool
(`debrepo::stage_dist` re-reads every `.deb` body for checksums, `yum::build_repodata`
re-parses every `.rpm`). On a repo with 10k packages this is minutes of wasted work
when zero packages changed. This is the top operational complaint about aptly too
("0 changes, still slow"), and createrepo_c solved it with `--update`.

The core insight: a publish is **idempotent and deterministic** — same inputs →
same bytes. If we can cheaply detect that an input hasn't changed, we can skip
re-processing it and reuse the cached output. The `PublishLock` already serialises
publishes, so there's no concurrent-writer race to worry about.

### Current cost profile

| Step | What it does | Cost for N packages |
| --- | --- | --- |
| `debs_in` | `walkdir` scan, filter `.deb` | O(N) — cheap |
| `read_deb` (per package) | open ar archive, decompress control.tar, parse RFC822 | O(N) — moderate |
| `std::fs::read` (per deb) | read **entire file** for checksums | O(N × file-size) — **expensive** |
| `build_repodata` (yum) | open+parse every .rpm via WorkerPool | O(N × file-size) — **expensive** |
| `render_release` | checksum all index files | O(components × arches) — cheap |

The two expensive steps are **reading every .deb body** (just for SHA256/MD5/SHA1
that we already computed last time and could cache) and **re-parsing every .rpm**.

## Decision (proposed)

### File-manifest cache: detect unchanged packages by (mtime, size)

After every successful publish, write a **file manifest** next to the pool:

```
apt/pool/.arx-manifest.toml     # per-component: {filename: {mtime, size, sha256}}
yum/<repo>/<arch>/.arx-manifest.toml
```

The manifest maps `filename → {mtime, size, sha256}` for every package that was
successfully indexed in the last publish.

On the **next** publish:

1. Scan the pool as usual (O(N), cheap).
2. For each package, compare (mtime, size) against the cached manifest:
   - **Match** → reuse the cached `sha256` + parsed `Control` (apt) or `Package`
     struct (yum). **Never re-open the file.**
   - **Mismatch or absent** → re-process from scratch (the expensive path).
   - **In manifest but not on disk** → removed from the index.
3. Build the index from the mix of cached + freshly-computed stanzas.
4. After commit, write the updated manifest.

This is O(changes + scan), not O(repo). A no-op publish on 10k packages goes from
"read 10k files" to "stat 10k files + read 0".

### Production dogfood benchmark

A 2026-06-23 online benchmark ran against isolated `/tmp` copies of the production
ArtifactX root, leaving `/data/arx/prod`, `/srv/deb`, and `/srv/repo` untouched.
The first new-version publish paid a one-time yum fragment backfill cost; the
steady-state small-add path then reused cached fragments:

| Case | Add | Publish | Export | Total |
| --- | ---: | ---: | ---: | ---: |
| old production binary | 0.071s | 18.185s | 2.323s | 20.579s |
| new binary after backfill | 0.069s | 0.992s | 1.986s | 3.047s |

One-time backfill publish on the copied production root took 18.827s. That cost
is expected once when upgrading a fragmentless yum manifest; it preserves the
safe fallback behavior for existing repositories.

### What gets cached (per format)

**apt:** the full `Packages` stanza text + the file's SHA256 (which we already
have from the manifest). The `Control` fields and `Filename`/`Size`/`MD5sum`/
`SHA1`/`SHA256` don't change unless the file changes → cache them as a pre-built
UTF-8 string.

**yum:** cache the per-package XML fragments that feed `primary.xml`,
`filelists.xml`, and `other.xml`. On mismatch, re-parse only the changed `.rpm`
and regenerate its fragments. On match, reuse the cached fragments and stream the
metadata roots around them. Older manifests that only have `(mtime, size)` are not
trusted as fresh; the first publish with fragment caching performs a full rebuild
and backfills fragments for subsequent incremental publishes.

### cache format: plain TOML (charter — file-based, human-readable)

```toml
# apt/pool/main/.arx-manifest.toml
[files]
"foo_1.0_amd64.deb" = { mtime = 1718123456, size = 12345, sha256 = "abc..." }
"bar_2.0_amd64.deb" = { mtime = 1718123500, size = 23456, sha256 = "def..." }
```

yum manifest additionally stores cached XML fragments (`stanza` for primary,
`contents` for filelists, and `other` for other.xml) needed for repodata
regeneration.

### Guard: `--full` flag skips the cache

`arx publish --full` ignores the manifest and rebuilds everything from scratch.
This is the "trust-but-verify" escape hatch — if the cache ever drifts, one
`--full` fixes it. (Also used after `arx init` when no manifest exists.)

## Consequences

- Good: a no-op publish on a large repo is near-instant (O(scan) instead of
  O(repo)). The `5-minute rule` holds at larger scale.
- Good: no new dependencies, no database, no background process — pure files.
- Good: TOML manifest is human-readable and can be `rm`-ed if corrupted (next
  publish auto-heals with `--full`-equivalent behavior).
- Bad / cost: the manifest is a new file to explain. mtime granularity is 1s on
  most filesystems — two publishes within the same second could miss a change
  (mitigated: `--full` is the escape; `PublishLock` prevents overlapping
  publishes; in practice this is the same window `make` tolerates).

## Explicitly NOT in this ADR

- **Content-addressable pool storage** (store by sha256, not filename). Would make
  dedup natural but is a larger change → separate ADR.
- **yum `createrepo_c --update` reuse.** The crate's `--update` mode checks RPM
  timestamps internally; we may use it or our own manifest — either way the
  architecture is the same. Investigated during implementation; either path works.
- **Partial publish (single component/arch).** The manifest approach works
  naturally with the existing full-repo publish; scoping can come later.

## Alternatives considered

- **No incrementality; always rebuild.** Rejected: breaks the 5-minute rule at
  scale and is the #1 aptly operational complaint.
- **SQLite / LMDB cache.** Rejected: violates charter "stateless" and "one
  binary". A TOML file is as simple as it gets and `serde` is already a dep.
- **Inotify / file watcher daemon.** Rejected: daemon violates charter
  "no background workers".

## Open questions for review

1. **Cache granularity** — per-pool-component (apt) + per-arch-dir (yum) vs one
   global manifest. Lean: per-directory alongside the pool files — avoids lock
   contention, easy to reason about, naturally scoped.
2. **yum fragment caching** — implemented as XML fragments instead of serializing
   `createrepo_rs::types::Package`, avoiding a cache schema tied to upstream
   Rust structs while still skipping unchanged RPM parsing.
3. **mtime vs content hash for change detection** — current proposal uses
   (mtime, size) as the fast path. Pure content-hash would be more robust but
   requires reading the file. Lean: (mtime, size) is the right fast-path;
   `--full` is the escape. If mtime is unreliable (network FS, `git checkout`),
   the user runs `--full`.

## Future improvements

- Optional content-hash mode (`--verify`) for paranoid/network-FS deployments.
- Auto-detect mtime granularity issues and warn.
- Incremental GC: only scan the manifest, not walk the pool.
