# ADR-0005: `pack` — manifest → native, never conversion

- Status: Accepted
- Date: 2026-06-17

## Context

The "Package" pillar must turn intent into `.deb` and `.rpm`. Two historical
approaches: **convert** an existing package to another format (`alien`), or
**render** each format from a neutral description (`FPM`, `nfpm`, `EPM`). `alien`
silently drops maintainer scripts and dependencies — conversion is lossy at the
edges. `FPM` renders natively but needs Ruby + tar + host tools and is
non-deterministic.

## Decision

`pack` builds each format **natively from one TOML manifest**, in **pure Rust**, with
**no shelled-out host tools** (no `dpkg-deb`, `rpmbuild`, `ar`, `tar` binaries):
`.deb` via the `ar`/`tar`/`flate2` crates, `.rpm` via the `rpm` crate. Scripts and
relationships (`depends`/`conflicts`/`provides`/`replaces`) are expressed once and
rendered natively per format. A `Backend` enum keeps `Native` (implemented) separate
from `Docker` (a documented stub): **native-first, Docker only when native genuinely
can't** (charter principle 8). Builds stage into a temp dir; entries are sorted for
determinism.

## Consequences

- Good: correct scripts/deps on every format; deterministic, reproducible output;
  zero host dependencies — runs identically on a laptop and in CI.
- Good: `pack` is a single embeddable crate — `cargo add pack`. This is the moat
  over `nfpm` (Go, not embeddable) and `FPM` (Ruby): we also *publish* what we build.
- Bad: we don't yet cover every field (triggers, conffiles, epoch nuances).
- Bad: the Docker fallback is a stub.

## Alternatives considered

- **Conversion (`alien`).** Lossy — rejected outright as the primitive.
- **Shell out to `dpkg-deb`/`rpmbuild`.** Reintroduces host-tool dependency and
  non-determinism — the exact thing `nfpm` was created to escape.

## Future improvements

`--from <staging-dir>` (checkinstall's value without mutating the host); `.apk`/arch
output; the real Docker backend for builds that truly need a foreign toolchain;
conffiles/triggers; `arx pack` already exposes this on the CLI.
