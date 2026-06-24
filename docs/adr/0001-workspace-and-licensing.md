# ADR-0001: Cargo workspace + split licensing

- Status: Accepted
- Date: 2026-06-17

## Context

ArtifactX is one product but several reusable pieces. We also link
`createrepo_rs`, which is **GPL-2.0**. Linking GPL code makes the linking binary
GPL. We want the apt generator and the packager to be reusable by *anyone*, GPL or
not.

## Decision

A Cargo workspace with three crates and deliberate licensing:

- `crates/arx-debrepo` (`arx-debrepo`) — apt repo generator — **MIT OR Apache-2.0**.
- `crates/arx-pack` (`arx-pack`) — packager — **MIT OR Apache-2.0**.
- `crates/arx` (`artifactx`, binary `arx`) — the CLI/server (links
  `createrepo_rs`) — **GPL-2.0-or-later**.

The GPL boundary stops at `arx`. `arx-debrepo` and `arx-pack` never depend on the
GPL code.

## Consequences

- Good: anyone can `cargo add arx-debrepo`/`arx-pack` into a closed-source or
  permissively-licensed tool. The reusable value isn't trapped behind GPL.
- Good: one repo, one `cargo test --workspace`, shared deps — low coordination cost.
- Bad: the headline binary is GPL. Acceptable: it's an application, not a library.

## Alternatives considered

- **One GPL crate.** Simplest, but traps `arx-debrepo`/`arx-pack` behind GPL —
  kills their reuse, which is half their point (especially `arx-pack` as the
  embeddable moat).
- **Separate repos now.** Premature: path deps + one build are simpler while the
  APIs are young. The split is designed so a crate can "graduate" to its own repo by
  moving a directory.

## Future improvements

When `debrepo`/`pack` APIs stabilise, publish them to crates.io and (optionally)
graduate to their own repos. Reconsider `createrepo_rs`'s GPL if a permissive
yum-metadata path appears — it would let `arx` relicense.
