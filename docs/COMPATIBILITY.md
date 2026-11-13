# Compatibility policy

ArtifactX is an upstream dependency (`arx-pack`, `arx-debrepo`) and a CLI people
run in production. Compatibility is a release gate, not a preference.

## Surfaces

| Surface | Promise |
| --- | --- |
| Rust crates (`arx-pack`, `arx-debrepo`) | Semver for public items; downstream source compatibility; public structs/enums are additive-only or `#[non_exhaustive]` |
| CLI (`arx`) | Flags and subcommands are not removed or renamed without a deprecation cycle; behavior changes are documented in release notes |
| Configuration (`arx.toml`) | Existing keys keep their meaning; new keys are optional with defaults |
| Repository layout (`dists/`, `pool/`, `repodata/`) | apt/dnf clients need no changes; generated metadata stays valid |
| Internal state (`.arx-cache/`) | May change, but must never require manual migration; a one-time full operation after upgrade is acceptable and must be documented |
| HTTP API | Same rules as the CLI; breaking changes require an API version change |

## Public Rust API

- Adding a public field to a struct that downstream crates can construct is
  **source-breaking**, even with `#[serde(default)]`. Serde only covers
  deserialization; struct literals fail to compile.
- Public structs that may grow are `#[non_exhaustive]` from the start, or use
  private fields with constructors/builders. Marking them `#[non_exhaustive]`
  later is itself breaking.
- Same rule for enums: `#[non_exhaustive]` before downstream users exist.
- The same applies to public functions: adding a parameter is breaking; add a
  new function or an options struct instead.

## Breaking changes

A breaking change requires all of:

1. An ADR under `docs/adr/` with motivation and migration path.
2. A version that communicates it (0.x: minor bump; 1.x: major bump). Never in a
   patch release.
3. A `Breaking` section in the release notes.
4. A deprecation cycle when feasible (one minor release with a warning first).

If a change cannot satisfy these, do not ship it.

## Release gate

Run and record all of the following before creating a tag:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- **Downstream compile probe**: a throwaway crate that constructs the public API
  the previous release supported (at minimum `arx_pack::Scripts { preinst,
  postinst, prerm, postrm }`).
- **Client e2e**: real apt/dnf against a generated repository. RPM version
  floors must be proven by dependency resolution (`dnf install` fails on an old
  point release, succeeds on the required one), not only by `rpm -qpR`.
- **Public API diff** against the previous tag: every removed or changed item is
  intentional and documented.
- **Registry state**: check the crates.io sparse index independently. A missing
  GitHub tag or Release does not mean the crate was never published; the weekly
  `crates.yml` schedule publishes the current Cargo version without a tag.

## Evidence in the pull request

State:

- surfaces touched (crate API, CLI, config, repository layout, state, HTTP API);
- compatibility verdict for each surface;
- the downstream probe and client e2e commands with their results;
- migration notes for any observable behavior change.

## Past exceptions

- `publish-dir` no-op state moved to `.arx-cache/v2/publish-dir/<DIST>/state.json`
  in 0.3.8. The first run after upgrading performs one full publish; the old file
  is ignored. This is a documented one-time migration, not data loss.
