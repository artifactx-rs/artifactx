# Pack reference

`arx pack` builds native package artifacts from either an ArtifactX TOML manifest
or a Rust `Cargo.toml`. It is the package-build side of ArtifactX; repository
indexing and serving are still handled by `arx add`, `arx publish`, and
`arx serve`.

## Supported outputs

By default, `arx pack` emits the four archive formats into `dist/`. The Nix and
Guix definitions are opt-in (`--nix` / `--guix`): their `$out` mapping rejects
some manifests the archives accept, so leaving them out of the default set keeps
existing `arx pack` invocations unchanged.

| Format | Output example | Native builder | Repository indexing in ArtifactX |
| --- | --- | --- | --- |
| Debian | `hello_1.2.3_amd64.deb` | yes | `arx add` / apt publish |
| RPM | `hello-1.2.3-1.x86_64.rpm` | yes | `arx add` / yum publish |
| Alpine APK | `hello-1.2.3-r0.x86_64.apk` | yes | artifact output only |
| Arch Linux | `hello-1.2.3-1-x86_64.pkg.tar.zst` | yes | artifact output only |
| Nix | `hello-1.2.3-x86_64-linux.nix` | yes | artifact output only |
| Guix | `hello-1.2.3-x86_64-linux.scm` | yes | artifact output only |

Use output selectors when a build only needs one or two formats:

```sh
arx pack ./arx-pack.toml --out dist --deb
arx pack ./Cargo.toml --out dist --rpm --apk
arx pack ./Cargo.toml --out dist --arch-pkg
arx pack ./Cargo.toml --out dist --nix --guix
```

`arx pack --add` follows the same repository boundary as `arx add`: it adds
built `.deb` and `.rpm` files to the configured apt/yum pools and leaves `.apk`,
`.pkg.tar.zst`, `.nix`, and `.scm` files in the output directory for downstream
handling.

## Manifest inputs

A standalone manifest describes package identity, relationships, installed files,
recursive directory payloads, config-file intent, and maintainer scripts.

`depends` entries may carry an rpm version constraint in dnf spelling, for
example `depends = ["postgresql-18 >= 18.7"]`; the rpm builder emits
`Requires: postgresql-18 >= 18.7`. Entries without a comparison stay unversioned
requirements. The deb builder writes the same strings verbatim into `Depends:`,
so a manifest that targets both formats must use entries that are valid for
each (`libc6` is unversioned in both; `libc6 (>= 2.31)` is deb-only).

```toml
name = "hello"
version = "1.2.3"
arch = "amd64"
maintainer = "Jane Dev <jane@example.com>"
description = "A friendly greeter"
license = "MIT"
depends = ["libc6"]
config_files = ["/etc/hello/hello.toml"]

[[files]]
source = "target/release/hello"
dest = "/usr/bin/hello"
mode = "0755"

[[files]]
source = "config/hello.toml"
dest = "/etc/hello/hello.toml"
mode = "0644"
config = true

[[dirs]]
source = "assets"
dest = "/usr/share/hello/assets"
file_mode = "0644"
dir_mode = "0755"
```

Directory entries are expanded deterministically before any backend builds. The
shared expansion path rejects symlinks, special files, and duplicate destination
paths so every output stays aligned.

## Nix and Guix definitions

`--nix` and `--guix` render the same payload as a self-contained package
definition instead of an archive. Neither ecosystem is invoked: ArtifactX writes
the file, and `nix`/`guix` are only needed when someone builds it later.

- The payload is embedded as base64 and written by an install script inside the
  definition, so the file needs no host path, URL, or adjacent payload
  directory. The cost is size: a definition is roughly 4/3 the payload.
- Installed paths move under the build prefix: `/usr/bin/hello` becomes
  `$out/bin/hello` (the `usr/` segment is what `$out` already means), and every
  other path keeps its location below `$out` (`/etc/app.conf` becomes
  `$out/etc/app.conf`). Two destinations that collide after that mapping fail
  the build rather than overwriting each other, and a payload file installed at
  exactly `/usr` cannot be represented (it would become `$out` itself).
- Directory modes are applied after the payload is written, deepest directory
  first, so `dir_mode = "0555"` works even though the ecosystem builders run
  unprivileged.
- The Nix output is a `{ lib, stdenv }:` function returning
  `stdenv.mkDerivation`; the Guix output is a `(package …)` form using
  `trivial-build-system`. Both take the description, license, and architecture
  from the manifest; the architecture becomes `meta.platforms` /
  `supported-systems` (Guix uses its own system names, and `--guix` refuses
  `s390x`, which Guix does not support).
- The Nix definition sets `dontFixup = true`: stdenv's fixup phase would
  otherwise rewrite shebangs, move `sbin`/`lib64`, strip binaries, and change
  modes, so the payload would stop being the same artifact the archive builders
  produce. Nix store paths stay read-only either way.
- SPDX licenses are mapped through a fixed table (`MIT` → `lib.licenses.mit` /
  `expat`, `Apache-2.0` → `asl20` / `asl2.0`, GPL/LGPL/AGPL
  `-only`/`-or-later` variants, and a few permissive licenses). Guix exports
  these names unprefixed (`expat`, `gpl3+`); the `license:` spelling some
  modules use is an import-time prefix, not a different name. An id or a
  `WITH` exception outside that table stays visible in a comment and the field
  is omitted (Nix) or `#f` (Guix) — a wrong license claim is worse than a gap.
- `depends`, `conflicts`, `provides`, `replaces`, `config_files`, and maintainer
  script names have no correct automatic translation: deb/rpm names are not Nix
  attributes or Guix variables, and neither ecosystem has install-time
  maintainer hooks. They are recorded verbatim in the file header for the
  maintainer to wire up. See
  [ADR-0030](../adr/0030-nix-guix-packaging-outputs.md).

Check a definition with the real toolchain before publishing it:

```sh
nix-instantiate --parse dist/hello-1.2.3-x86_64-linux.nix
nix-build -E 'let pkgs = import <nixpkgs> {}; in import ./dist/hello-1.2.3-x86_64-linux.nix { inherit (pkgs) lib stdenv; }'
guix build --dry-run -f dist/hello-1.2.3-x86_64-linux.scm
```

## Cargo.toml mode

When `MANIFEST` is omitted, `arx pack` reads `./Cargo.toml`. Passing a path named
`Cargo.toml` uses the same mode.

```sh
cargo build --release
arx pack Cargo.toml --out dist
```

Cargo mode derives package identity from `[package]` and overlays packaging
fields from `[package.metadata.arx]`. It also reads useful metadata from common
Rust packaging tables before applying the ArtifactX overlay:

- `[package.metadata.deb]`
- `[package.metadata.generate-rpm]`
- legacy `[package.metadata.rpm]`

`[package.metadata.arx]` wins when schemas overlap. Rendering still happens
inside ArtifactX; `cargo-deb`, `cargo-generate-rpm`, `cargo-rpm`, `rpmbuild`, and
`dpkg-deb` are not invoked.

`arx pack` does not run `cargo build`. Build first, then pass the same output
selectors to `pack` if the binary is not in Cargo's default release directory:
```sh
cargo build --profile dev --target x86_64-unknown-linux-gnu --target-dir build/target
arx pack Cargo.toml \
  --profile dev \
  --target x86_64-unknown-linux-gnu \
  --target-dir build/target \
  --out dist
```

To build packages for a different architecture than what the manifest declares,
pass `--arch`. This overrides `[package.metadata.arx].arch`, compat metadata,
and the `amd64` default, so a single `Cargo.toml` can produce both `amd64` and
`arm64` packages without editing the source tree:

```sh
arx pack Cargo.toml --target aarch64-unknown-linux-musl --arch arm64 --deb --rpm --out dist/arm64
arx pack Cargo.toml --target x86_64-unknown-linux-musl  --arch amd64 --deb --rpm --out dist/amd64
```

Native `[package.metadata.arx].files` sources that begin with `target/release/`
are resolved through the same `--target-dir`, `--target`, and `--profile`
selection as compat assets, so a `source = "target/release/foo"` entry picks up
the correct cross-compiled binary rather than a CWD-relative path.

Cargo lookup defaults to profile `release`, the selected crate or workspace
`target/` directory, and the package name unless a single `[[bin]].name` is
present. Multiple binaries require explicit file mappings so `pack` cannot guess
the wrong executable.

## Config-file semantics

Mark config files either with `config = true` on an explicit `[[files]]` entry or
by listing absolute installed paths in top-level `config_files` when files come
from `[[dirs]]`. Every listed path must match an installed regular file.

| Format | Config behavior |
| --- | --- |
| `.deb` | Writes Debian `conffiles`. |
| `.rpm` | Marks files as `%config(noreplace)`. |
| `.apk` | Keeps files as ordinary payload until Alpine-specific backup semantics are designed. |
| `.pkg.tar.zst` | Writes Arch `backup = ...` entries in `.PKGINFO`. |
| `.nix` / `.scm` | No conffiles mechanism; config paths are listed in the file header. |

## Maintainer scripts

Manifest script paths are embedded into the package:

```toml
[scripts]
preinst = "scripts/preinst.sh"
postinst = "scripts/postinst.sh"
prerm = "scripts/prerm.sh"
postrm = "scripts/postrm.sh"
```

Per-format mapping:

| Manifest script | Debian | RPM | APK | Arch | Nix / Guix |
| --- | --- | --- | --- | --- | --- |
| `preinst` | `preinst` | pre-install script | pre-install script | `.INSTALL` `pre_install()` | header comment only |
| `postinst` | `postinst` | post-install script | post-install script | `.INSTALL` `post_install()` | header comment only |
| `prerm` | `prerm` | pre-uninstall script | pre-deinstall script | `.INSTALL` `pre_remove()` | header comment only |
| `postrm` | `postrm` | post-uninstall script | post-deinstall script | `.INSTALL` `post_remove()` | header comment only |

Keep scripts portable. `arx pack` embeds them; it does not run package-manager
integration tests for every target distribution during a normal build. Nix and
Guix have no install-time maintainer-script equivalent, so those entries stay in
the definition header instead of being translated into build phases.

## Reproducibility

Package builders use deterministic ordering, modes, and timestamps. Timestamp
selection is:

1. `--source-date <EPOCH>` for this invocation;
2. `SOURCE_DATE_EPOCH` from the environment;
3. epoch `0` when neither is set.

This controls archive metadata and package metadata that carry build timestamps.
Nix and Guix definitions carry no timestamp of their own; their output is a pure
function of the manifest, the payload bytes, and the architecture.

## Docker backend boundary

Native build is the only backend exposed by the `arx pack` CLI. The
`arx-pack` library also exposes a Docker backend API for callers that need a
pinned foreign build environment, but there is currently no CLI option or
manifest field that selects a Docker image.

Do not use Docker to paper over missing manifest metadata. If the native path
can stage the files and render the package, prefer native.

## Limits and non-goals

These limits are intentional until a follow-up design changes them:

- No inline package payload signing in `arx pack`. ArtifactX signs apt/yum
  repository metadata; package-level signing belongs in the package build
  pipeline.
- No automatic dependency detection. Declare dependencies explicitly. The
  cargo-deb `$auto` sentinel is skipped rather than guessed from host tools.
- No symlink following or special-file packaging. Sources must resolve to regular
  files or deterministic directory trees of regular files.
- No source packages. `arx pack` emits binary package artifacts only.
- No automatic `cargo build`. Build your binaries before running `arx pack`.
- No `.apk` or Arch repository indexing in ArtifactX yet. Those formats are
  package artifacts, not `arx add` / publish repository formats.
- No Nix or Guix repository indexing, and no `nix`/`guix` invocation by
  ArtifactX. The definitions are artifacts; validating them is the consumer's
  step.
- No translation of deb/rpm dependency names into Nix attributes or Guix
  variables. That mapping is not derivable without the ecosystem's index, so it
  stays a maintainer decision (ADR-0030).
- No promise that generated packages are accepted by every distro policy. Use
  real target-system smoke tests before publishing packages to users.

## Related design records and issues

- [#131: Cargo.toml pack mode cannot override package arch per target build](https://github.com/artifactx-rs/artifactx/issues/131)
- [#132: Native arx.files sources ignore --target while compat assets rewrite target/release](https://github.com/artifactx-rs/artifactx/issues/132)
- [#156: pack: add Nix and Guix packaging outputs](https://github.com/artifactx-rs/artifactx/issues/156)
- [ADR-0005: pack manifest native](../adr/0005-pack-manifest-native.md)
- [ADR-0010: Cargo.toml-driven packaging](../adr/0010-cargo-toml-driven-packaging.md)
- [ADR-0012: pack product-readiness](../adr/0012-pack-product-readiness.md)
- [ADR-0018: directory entries for package manifests](../adr/0018-directory-entries-for-package-manifests.md)
- [ADR-0030: Nix and Guix packaging outputs](../adr/0030-nix-guix-packaging-outputs.md)
- [#103: Cargo.toml-driven pack](https://github.com/artifactx-rs/artifactx/issues/103)
- [#30: pack documentation completeness](https://github.com/artifactx-rs/artifactx/issues/30)
