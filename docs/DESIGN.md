# ArtifactX — Design Overview

This is the map of how ArtifactX is built and why. It is deliberately short — if a
part can't be explained in a paragraph, that part is probably too complicated (see
the [charter](../CLAUDE.md)). For the *why* behind specific choices, read the
[ADRs](adr/).

## Mission shapes architecture

**Build Once · Package Once · Publish Everywhere.** Three verbs, three pieces,
**one binary**:

| Pillar | Where | What |
| --- | --- | --- |
| **Package** | `crates/arx-pack` | A manifest → native `.deb`/`.rpm`/`.apk`/`.pkg.tar.zst`, pure Rust, no toolchain. |
| **Publish (apt)** | `crates/arx-debrepo` | A pool of `.deb` → signed `Packages`/`Release`/`Contents`. |
| **Publish (yum)** | `createrepo_rs` (dep) | A pool of `.rpm` → signed `repodata`. |
| **Orchestration** | `crates/arx` | The CLI + HTTP server that wires it all together. |

```
                          ┌──────────────────────── arx (CLI + server) ───────────────────────┐
  manifest ──▶ arx pack ──┤  pool/  ──▶ arx publish ──▶ dists/ + repodata/ ──▶ arx serve / push │──▶ apt-get / dnf
                          │   (debrepo)         (createrepo_rs)        (axum)                     │
                          └────────────────────────────────────────────────────────────────────┘
```

## The data model (the one thing to understand)

A repository is **a directory**. There is no database.

```
<repo>/
  arx.toml                 # config (identity, signing, defaults, server)
  keys/{private,public}.asc # PGP signing key (private optionally encrypted)
  apt/
    pool/<component>/*.deb              # source of truth (the packages)
    dists/<dist>/{Release,InRelease,Release.gpg}
    dists/<dist>/<component>/binary-<arch>/Packages{,.gz} + by-hash/
  yum/
    <repo>/<arch>/*.rpm                 # source of truth
    <repo>/<arch>/repodata/{repomd.xml, *.xml.gz, repomd.xml.asc}
```

**The pool is the source of truth; everything under `dists/`/`repodata/` is
derived.** `add`/`pack --add`/`push` mutate the pool; `publish` regenerates the
derived metadata from the pool and signs it. This is the whole mental model — and
why the design is *stateless, deterministic, atomic, easy to back up* (charter
principle 8): back up the directory, you've backed up everything.

## How a publish stays safe

`publish` never edits the live `dists/<dist>` in place. It builds the entire new
tree into `dists/.<dist>.staging`, signs it there, then **atomically swaps** it into
place with a directory rename. `Acquire-By-Hash` + `by-hash/` copies mean a client
mid-`apt-get update` never sees a torn index (`Hash Sum mismatch`). A lockfile
(`.arx-publish.lock`) serialises concurrent publishes. See
[ADR-0004](adr/0004-atomic-publish-by-hash.md).

## How writes happen (CLI and API are the same)

Everything you can do on the CLI you can do over HTTP — `arx serve` exposes a
bearer-auth REST API under `/api/v1` (`packages`, `gc`, `health`). `arx push` is the
client. Uploads land in the pool and trigger an atomic republish under the same
lock, signed with the server's key. Reads are public unless a token is set; **writes
always require `ARX_SERVE_TOKEN`.** See [ADR-0006](adr/0006-http-api-and-push.md).

## Signing

Keys are **v4 RSA** so signatures verify under the stock gpg that apt and dnf ship,
old and new. The private key may be passphrase-encrypted at rest (OpenPGP S2K). apt
gets `InRelease`/`Release.gpg`, yum gets `repomd.xml.asc`. See
[ADR-0003](adr/0003-v4-rsa-signing.md).

## Crates & licensing

ArtifactX is one Cargo workspace with split crate boundaries:
`arx-debrepo` and `arx-pack` are **independent, MIT/Apache** libraries,
while `artifactx` (`crates/arx`) links the GPL `createrepo_rs`, so the binary is
GPL. See [ADR-0001](adr/0001-workspace-and-licensing.md).

## Where to start reading the code

- `crates/arx/src/main.rs` — every subcommand is a `cmd_*` function.
- `crates/arx-debrepo/src/lib.rs` — `stage_dist` → `commit_dist` is the whole apt engine.
- `crates/arx/src/server.rs` — the HTTP API.
- `crates/arx-pack/src/{deb,rpm,apk,arch}.rs` — the native package builders.

## Non-goals

ArtifactX is **not** a database, a web UI, an RBAC/identity platform, a mirror/proxy,
or a CI system. Those are deliberate rejections — see [`COMPETITORS.md`](../COMPETITORS.md)
and the charter.
