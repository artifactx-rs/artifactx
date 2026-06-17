# ADR-0002: In-house apt generator (`debrepo`)

- Status: Accepted
- Date: 2026-06-17

## Context

We need to turn a pool of `.deb` files into signed `Packages`/`Release` indices.
The mature option is `debian-packaging` (from `indygreg/linux-packaging-rs`). Its
repository builder is built around **async + S3/HTTP publishing** and pulls
`async-std`/`tokio` and a large surface.

The apt on-disk format we actually need is a small, stable subset: parse `.deb`
(`ar` → `control.tar.{gz,xz,zst}` → `control`), emit `Packages`/`Release`.

## Decision

Write a focused, **synchronous, signing-agnostic** crate (`debrepo`). Reuse mature
crates for the mechanical parts (`ar`, `tar`, `flate2`/`xz2`/`zstd`, `sha*`/`md-5`).
`debrepo` returns the `Release` text; the caller signs.

## Consequences

- Good: tiny sync dependency tree, fits "one minimal static binary" (charter
  principle 8). No async runtime dragged in for a file-writing task.
- Good: signing-agnostic → reusable; `arx` injects rpgp, someone else can inject gpg.
- Bad: we own the format correctness (mitigated by tests + real `apt-get` verification).

## Alternatives considered

- **`debian-packaging`.** Mature and correct, but async/S3-oriented and heavy —
  wrong shape for a local, synchronous, single-binary tool. (Its RPM sibling
  `rpm-repository` is read-only, so it couldn't replace `createrepo_rs` anyway.)

## Future improvements

`Contents-<arch>`, incremental updates, `by-hash` (done), and multi-suite overrides —
each as a small addition. If correctness ever gets hard, revisit borrowing
`debian-packaging`'s index logic behind our sync API.
