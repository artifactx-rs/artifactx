# Competitive Teardown — what we steal, what we refuse

A product-level study of the package-repository / packaging landscape, read through
the [ArtifactX charter](CLAUDE.md): **Build · Package · Publish**, the 5-minute rule,
delete-first, one static binary, no platform.

> **The unoccupied position:** *aptly's safety (multi-version, atomic publish,
> rollback) + nfpm's one-manifest-many-formats + Cloudsmith's push DX — in a single
> self-hosted static binary, with none of the database/UI/RBAC platform.*
>
> We didn't invent the pool, the suite-as-index, or the one-manifest-many-formats
> idea — Debian's **dak**, **FPM**, and **EPM** did. We put all three in one Rust
> binary, deleted the database and the Ruby, and added the **publish** step none of
> them had.

## Per-tool essence

| Tool | Steal | Refuse |
| --- | --- | --- |
| **aptly** | multi-version pool; **snapshot→publish→switch** = atomic publish + instant rollback; `db cleanup` mark-and-sweep GC with `--dry-run` | the 5-step toolkit workflow; slow republish even on 0 changes; mirroring as half the surface; full snapshot CRUD |
| **reprepro** | the "one config, one repo, done" beginner feel | **latest-version-only** (no downgrade) — the footgun we must clear |
| **createrepo_c** | **`--update` incremental** metadata (skip unchanged by size+mtime) → kills slow republish | deltarpm |
| **nfpm** | **one declarative manifest → deb/rpm/apk**, zero deps, single binary | nothing major — but it *stops at a file on disk*; publishing is our wedge |
| **Pulp** | the *concept* Content→immutable Version→Publication→Distribution (validates atomic publish) | Django + Postgres + workers + plugins. A platform, not a binary. |
| **Nexus / JFrog** | almost nothing for core; *maybe later* optional proxy cache | JVM, external DB, RBAC, web UI, licensing. We are the **anti-Artifactory**. |
| **Cloudsmith** | **one-line CI push** + **GitHub OIDC keyless auth** (no stored secret) | SaaS control plane; 20-format sprawl; quotas/billing |
| **packagecloud** | the **verbs**: `push` / `yank` / `promote`; retention = auto-yank old | collaborator/RBAC; web UI |
| **deb-s3** | **stateless server-side mutation** — never babysit a local repo tree | S3-only; single-version limits |
| **Gemfury** | **curl-able push** as a zero-install fallback; "add one apt line, done" | SaaS hosting/billing |
| **FPM** | the minimal CLI: `--depends`, `--after-install`, `-s dir -t deb` | Ruby + tar + host-tool chain (literally why nfpm exists); non-deterministic |
| **alien** | (cautionary) the cross-format dream | **conversion is a lie** — drops scripts/deps. Never artifact→artifact. |
| **dak** (Debian's own) | **pool layout + suite-as-index + overrides** — the canonical, battle-tested data model | Postgres + ftp-master workflow engine |
| **mini-dinstall / freight / debarchiver** | **no-database 5-minute repo**; `incoming/` drop-dir ingestion; cheap `rm` | latest-only footgun; `.changes` upload ceremony; unmaintained Perl era |
| **checkinstall** | "point at a build, get a package" | captures by mutating the live system — wrong primitive |
| **EPM** | the "list file" → many native formats (1999!) — manifests are timeless | bespoke GUI installer runtime + EULA |

## Adopt now (P0/P1)

- **`arx rm` (yank).** Remove a bad/CVE version from the index immediately; cheap and safe. *(shipped)*
- **`arx gc` (reclaim disk).** Mark-and-sweep over the content pool; `--dry-run` shows bytes freed; add a `--grace` window so a just-yanked blob isn't reaped mid-deploy. *(shipped; grace + byte report = follow-up)*
- **Retention as one knob, not a rules engine.** `--keep N` / `--keep-within 90d`. Reject Nexus's policy DSL.
- **`arx push` — one-line publish + keyless CI.** Auto-detect dist/component/arch from the package; `curl -T` fallback; **GitHub Actions OIDC** so no long-lived secret.
- **Incremental publish by default** (createrepo_c `--update`): republish is O(changes), not O(repo).
- **Atomic publish + rollback via immutable state + pointer-flip** (the highest-leverage steal): every `push`/`rm` produces a new content-addressed published state; going live is an atomic pointer flip; expose only `arx rollback` / `arx history` — **never** aptly's full snapshot surface. This is both the safety story and the no-torn-reads correctness story in one mechanism. *(`by-hash` + staging→commit swap already deliver torn-read safety; rollback is the next step)*
- **`pack`: nfpm-style single manifest** → deb + rpm, sane defaults, no spec files; FPM's `--depends`/`--after-install` surface; **manifest → native per-format, never conversion**. Pure-Rust, zero shelled-out host tools, deterministic output.

## Consider later
- `promote` (staging→prod as a *move*); `incoming/` drop-dir ingestion (scp a file, repo updates); `arx pack --from <staging-dir>` (checkinstall's value, sandboxed); repo-level overrides (component/section without re-upload); optional read-through proxy cache; apk/ipk/arch output.

## Reject (scope creep — named)
RBAC/identity platform · web UI/dashboard · one-way package sync (arx mirror, not bidirectional platform mirroring) · plugin/content-type platform + external DB/workers · 20+ format universality · deltarpm · full snapshot CRUD · policy-DSL retention · billing/quotas · format **conversion** (alien) · `.changes` upload ceremony · bespoke GUI installer (EPM).

## Why not… (honest, landing-page ready)

- **Aptly?** A powerful toolkit — and a five-step workflow, a database to clean, and a publish that's slow even when nothing changed. ArtifactX gives you `push`, `rm`, `rollback`: same multi-version safety and atomic publish, none of the snapshot bookkeeping.
- **reprepro?** Simple because it forgets your old versions. ArtifactX is just as simple to start but keeps every version, so you roll back a bad release in one command.
- **Nexus / Artifactory?** JVM platforms with a database, a setup guide, and a license. ArtifactX is one static binary you run in under five minutes.
- **Pulp?** A beautiful content model shipped with Postgres, workers, and Django. We borrow the good idea — immutable, atomically-published repo states — and throw away the cluster.
- **Cloudsmith / Gemfury / packagecloud?** Someone else's servers and someone else's bill. Same one-line CI push and keyless OIDC — on infrastructure you own, no per-package pricing.
- **nfpm / FPM?** They package beautifully and stop at a file on disk (and FPM needs Ruby + tar). ArtifactX packages *and publishes* from one manifest — Build, Package, Publish, end to end, in a single pure-Rust binary with deterministic output.
- **alien?** It converts a finished `.deb` into an `.rpm` and quietly drops your scripts and dependencies. ArtifactX renders each native format from one manifest — correct on every platform, no conversion surprises.

---
*Sources: aptly.info, createrepo-c manpage, nfpm.goreleaser.com, pulpproject.org, docs.cloudsmith.com, packagecloud.io/docs, github.com/krobertson/deb-s3, gemfury.com/guide, github.com/jordansissel/fpm, wiki.debian.org (dak/pool), mini-dinstall/freight docs.*
