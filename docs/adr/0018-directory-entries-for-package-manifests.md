# ADR-0018: Directory entries for package manifests

- Status: **Proposed**
- Date: 2026-06-22
- Target: v0.2.0 planning candidate
- Related: [GitHub issue #14](https://github.com/artifactx-rs/artifactx/issues/14)

## Context

ArtifactX `pack` currently installs explicit regular files through `[[files]]`:

```toml
[[files]]
source = "build/hello"
dest = "/usr/bin/hello"
mode = "0755"
```

This is simple and deterministic, but it is tedious for packages that need to
ship a directory-shaped payload: static assets, documentation trees, config
examples, service units, or other non-binary resources.

The current shared validation step deliberately rejects directory sources before
any builder sees them. That keeps `.deb`, `.rpm`, and `.apk` behavior aligned and
avoids accidental traversal of symlinks, FIFOs, devices, or host-specific files.

Thanks to **@daamien** for raising this gap in issue #14 and for connecting it to
the `rpm` crate's `with_dir()` ergonomics. The proposal is useful because it
points at a real packaging workflow while still fitting ArtifactX's manifest-first
model. Follow-up discussion, ADR refinement, and pull requests from @daamien or
other contributors are very welcome.

## Decision

Add a directory-entry design to the v0.2.0 planning backlog, but do not rush it
as an rpm-only helper.

The preferred shape is a separate `[[dirs]]` section rather than overloading
`[[files]]`:

```toml
[[dirs]]
source = "assets"
dest = "/usr/share/my-package/assets"
file_mode = "0644"
dir_mode = "0755"
```

Implementation requirements before acceptance:

1. **Shared semantics:** directory expansion must feed the same normalized staged
   file list for `.deb`, `.rpm`, and `.apk`.
2. **Determinism:** traversal order must be stable and sorted; package output
   must not depend on host filesystem ordering.
3. **Explicit modes:** files and directories need predictable mode defaults or
   overrides. The manifest should avoid silently copying host-specific mode bits
   unless that behavior is explicitly designed.
4. **Safe source handling:** symlinks, devices, FIFOs, sockets, and other special
   files remain rejected unless a future ADR explicitly adds support for them.
5. **Conflict policy:** if a `[[files]]` entry and an expanded `[[dirs]]` entry
   target the same destination, the builder must fail loudly instead of choosing
   one implicitly.
6. **Documentation and tests:** add manifest examples and regression tests that
   prove sorted expansion, mode handling, special-file rejection, and parity
   across package formats.

## Consequences

- Good: users can package assets/config/docs without manually enumerating every
  file.
- Good: aligns ArtifactX with a familiar rpm-packaging ergonomic while preserving
  a cross-format manifest contract.
- Good: keeps the feature inside the Package pillar and makes `arx pack` more
  useful for real applications.
- Bad / cost: introduces path traversal, mode, and conflict semantics that must
  be specified carefully.
- Bad / cost: increases the manifest surface during a feature-freeze period, so
  it should be handled as a v0.2.0 planning item rather than an immediate patch.

## Alternatives considered

- **Use `rpm::PackageBuilder::with_dir()` directly.** Rejected: this would make
  rpm behavior richer than deb/apk and break the shared-manifest promise.
- **Allow directories in `[[files]]`.** Rejected for now: it makes a single table
  mean both one file and recursive expansion, which hides important mode and
  conflict semantics.
- **Require users to pre-expand directories in scripts.** Rejected as the only
  answer: it is workable today, but it pushes deterministic packaging behavior
  out of ArtifactX and into every user's build glue.

## Future improvements

- Consider include/exclude globs after the basic recursive directory contract is
  proven.
- Consider per-entry ownership metadata if ArtifactX later supports ownership
  beyond mode bits.
- Revisit symlink support only with an explicit security and reproducibility
  model.
