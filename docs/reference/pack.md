# Pack reference

`arx pack` builds native package artifacts from either an ArtifactX TOML manifest
or a Rust `Cargo.toml`. It is the package-build side of ArtifactX; repository
indexing and serving are still handled by `arx add`, `arx publish`, and
`arx serve`.

## Supported outputs

By default, `arx pack` emits every supported package artifact into `dist/`:

| Format | Output example | Native builder | Repository indexing in ArtifactX |
| --- | --- | --- | --- |
| Debian | `hello_1.2.3_amd64.deb` | yes | `arx add` / apt publish |
| RPM | `hello-1.2.3-1.x86_64.rpm` | yes | `arx add` / yum publish |
| Alpine APK | `hello-1.2.3-r0.x86_64.apk` | yes | artifact output only |
| Arch Linux | `hello-1.2.3-1-x86_64.pkg.tar.zst` | yes | artifact output only |

Use output selectors when a build only needs one or two formats:

```sh
arx pack ./arx-pack.toml --out dist --deb
arx pack ./Cargo.toml --out dist --rpm --apk
arx pack ./Cargo.toml --out dist --arch-pkg
```

`arx pack --add` follows the same repository boundary as `arx add`: it adds
built `.deb` and `.rpm` files to the configured apt/yum pools and leaves `.apk`
and `.pkg.tar.zst` files in the output directory for downstream handling.

## Manifest inputs

A standalone manifest describes package identity, relationships, installed files,
recursive directory payloads, config-file intent, and maintainer scripts.

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
paths so `.deb`, `.rpm`, `.apk`, and `.pkg.tar.zst` stay aligned.

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

| Manifest script | Debian | RPM | APK | Arch |
| --- | --- | --- | --- | --- |
| `preinst` | `preinst` | pre-install script | pre-install script | `.INSTALL` `pre_install()` |
| `postinst` | `postinst` | post-install script | post-install script | `.INSTALL` `post_install()` |
| `prerm` | `prerm` | pre-uninstall script | pre-deinstall script | `.INSTALL` `pre_remove()` |
| `postrm` | `postrm` | post-uninstall script | post-deinstall script | `.INSTALL` `post_remove()` |

Keep scripts portable. `arx pack` embeds them; it does not run package-manager
integration tests for every target distribution during a normal build.

## Reproducibility

Package builders use deterministic ordering, modes, and timestamps. Timestamp
selection is:

1. `--source-date <EPOCH>` for this invocation;
2. `SOURCE_DATE_EPOCH` from the environment;
3. epoch `0` when neither is set.

This controls archive metadata and package metadata that carry build timestamps.

## Docker backend boundary

Native build is the default and the common path. The Docker backend is an
explicit fallback for packages that need a pinned foreign build environment. It
runs `arx pack` inside the configured image and collects the requested artifact
suffix, including `.pkg.tar.zst` for Arch output.

Do not use Docker to paper over missing manifest metadata. If the native path can
stage the files and render the package, prefer native.

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
- No promise that generated packages are accepted by every distro policy. Use
  real target-system smoke tests before publishing packages to users.

## Related design records and issues

- [ADR-0005: pack manifest native](../adr/0005-pack-manifest-native.md)
- [ADR-0010: Cargo.toml-driven packaging](../adr/0010-cargo-toml-driven-packaging.md)
- [ADR-0012: pack product-readiness](../adr/0012-pack-product-readiness.md)
- [ADR-0018: directory entries for package manifests](../adr/0018-directory-entries-for-package-manifests.md)
- [#103: Cargo.toml-driven pack](https://github.com/artifactx-rs/artifactx/issues/103)
- [#30: pack documentation completeness](https://github.com/artifactx-rs/artifactx/issues/30)
