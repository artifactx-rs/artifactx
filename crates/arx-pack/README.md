# arx-pack

**Pure-Rust packager: build `.deb`, `.rpm`, and `.apk` from a single TOML manifest — no native toolchain required for the common case.**

`arx-pack` is the *Package* pillar of ArtifactX. You describe a package once — its
metadata and the files it installs — and `arx-pack` emits Debian `.deb`, RPM
`.rpm`, and Alpine `.apk` packages. No `dpkg-deb`, no `rpmbuild`, no root, and no container runtime
needed for ordinary "stage these files at these paths with this metadata"
packaging. The same code runs identically on a developer laptop and in CI.

> Status: **proof of concept.** The native `.deb`, `.rpm`, and `.apk` builders
> are implemented and tested. The Docker backend is a documented stub (see below).

## Philosophy: native-first, Docker as a fallback, hygiene always

1. **Prefer the native host build.** Building `.deb`, `.rpm`, and `.apk` in pure Rust is
   fast, dependency-light, and toolchain-free. This is the default and the
   common case.
2. **Fall back to Docker only when native genuinely can't do it.** Some packages
   legitimately need a foreign toolchain — compiling against a specific distro's
   libraries, running build scriptlets, producing arch-specific binaries the
   host can't. Those, and only those, are what the Docker backend is reserved
   for. We do not reach for a container for anything the native path already
   handles.
3. **Keep build-environment hygiene non-negotiable.** Native or containerised, a
   build should be **clean** (no leftover state), **isolated** (no bleed from the
   host or between builds), and **reproducible** (sorted entries, deterministic
   modes and timestamps). The native builders stage into a fresh temporary
   directory and emit deterministic archives for exactly this reason.

## Manifest

```toml
name = "hello"
version = "1.2.3"
arch = "amd64"            # accepts deb (amd64) or rpm (x86_64) spellings
maintainer = "Jane Dev <jane@example.com>"
description = """A friendly greeter
A longer paragraph describing the package."""
license = "MIT"
section = "utils"          # deb Section; reused as rpm Group if `group` unset
# group = "Applications/System"
depends   = ["libc6"]
conflicts = ["hello-old"]  # deb Conflicts / rpm Conflicts
provides  = ["greeter"]    # virtual package / capability
replaces  = ["hello-old"]  # deb Replaces / rpm Obsoletes
# config_files = ["/etc/hello/extra.toml"] # optional for files expanded from [[dirs]]

[[files]]
source = "build/hello"     # path on the build host
dest   = "/usr/bin/hello"  # install path inside the target system
mode   = "0755"            # octal, as a string so the leading zero survives

[[files]]
source = "config/hello.toml"
dest   = "/etc/hello/config.toml"
mode   = "0644"
config = true              # same as listing this dest in config_files

[[dirs]]
source = "assets"          # recursively include this host directory
dest   = "/usr/share/hello/assets"
file_mode = "0644"         # optional; defaults to 0644
dir_mode  = "0755"         # optional; defaults to 0755

# Optional maintainer scripts (host paths, embedded into the package):
# [scripts]
# postinst = "scripts/postinst.sh"
# prerm    = "scripts/prerm.sh"
```

Configuration files are ordinary payload files with package-manager policy
attached. Mark an explicit `[[files]]` entry with `config = true`, or list an
absolute installed path in `config_files` when the file comes from `[[dirs]]`.
Every `config_files` entry must match an installed regular file so typos fail
before any package is written. The marker maps to Debian `conffiles` and RPM
`%config(noreplace)`; APK output currently carries the file as normal payload
because this packager has no Alpine-specific backup marker yet.

## From a Cargo project (zero config)

In a Rust crate, identity is derived from `Cargo.toml` and packaging details from
`[package.metadata.arx]` — no separate manifest, no repeated name/version:

```toml
# Cargo.toml
[package]
name = "greeter"
version = "1.2.3"
description = "A friendly greeter"
license = "MIT"
authors = ["Jane Dev <jane@example.com>"]

[package.metadata.arx]
section = "utils"
depends = ["libc6"]
# files = [...]  # optional; default is target/release/<name> → /usr/bin/<name>
```

```bash
cargo build --release
arx pack            # reads ./Cargo.toml → .deb + .rpm + .apk in dist/
```

`pack` does not run `cargo build` for you. It reads the binary that already
exists under Cargo's output directory. The default lookup is
`target/release/<bin-name>`; for workspaces it walks up to the workspace
`target/` directory, and a single `[[bin]].name` overrides the package name for
the binary path. Multiple binaries require explicit `files` so `pack` cannot
guess the wrong executable.

Use the same output selectors you used for `cargo build` when packaging custom
builds:

```bash
cargo build --profile dev --target x86_64-unknown-linux-gnu --target-dir build/target
arx pack --profile dev --target x86_64-unknown-linux-gnu --target-dir build/target
```

For reproducible timestamps, package builders use `SOURCE_DATE_EPOCH` when set
and otherwise default to epoch `0`. The CLI also accepts `--source-date <epoch>`
to override the environment for one `arx pack` invocation.

`Manifest::from_cargo_toml(&str)` exposes the same mapping as a library call.

### Existing Cargo packaging metadata

`arx pack` also understands the useful, pure-metadata subset of common Rust
packaging tables before applying the ArtifactX overlay:

- `[package.metadata.deb]`: maintainer, section, relationships, `assets`, and
  `conf-files`.
- `[package.metadata.generate-rpm]`: summary/license, relationships, and
  `assets` including `config = true` asset markers.
- legacy `[package.metadata.rpm]`: summary/group, relationships, `files`, and
  `targets`; `files` entries can carry `config = true`.

`[package.metadata.arx]` wins when it supplies the same field, so projects can
reuse existing cargo-deb / cargo-generate-rpm / cargo-rpm metadata and add only
ArtifactX-specific cross-format behavior. Rendering still happens inside
ArtifactX: `pack` does not require `cargo-deb`, `cargo-generate-rpm`,
`cargo-rpm`, `rpmbuild`, or `dpkg-deb`.

Compat asset sources are interpreted relative to the crate root; sources under
`target/release/` are rewritten to the selected Cargo output directory, so the
same `--profile`, `--target`, and `--target-dir` selectors above apply. The
cargo-deb `$auto` dependency sentinel is ignored because ArtifactX does not run
host dependency scanners.

## Usage

```rust
use arx_pack::{Manifest, Backend, Format};
use std::path::Path;

let manifest = Manifest::from_toml_str(toml_str)?;

// Direct, format-specific builders:
let deb = arx_pack::build_deb(&manifest, Path::new("dist"))?; // dist/hello_1.2.3_amd64.deb
let rpm = arx_pack::build_rpm(&manifest, Path::new("dist"))?; // dist/hello-1.2.3-1.x86_64.rpm

// Or via the backend abstraction:
let backend = Backend::Native;
let deb = backend.build(&manifest, Format::Deb, Path::new("dist"))?;
# Ok::<(), anyhow::Error>(())
```

## Public API

| Item | Description |
| --- | --- |
| `Manifest::from_toml_str(&str) -> Result<Manifest>` | Parse a manifest from TOML. |
| `Manifest::from_cargo_toml_at_with_options(&str, &Path, &CargoManifestOptions) -> Result<Manifest>` | Derive a manifest from `Cargo.toml` using explicit Cargo output selectors. |
| `arx_pack::build_deb(&Manifest, out_dir: &Path) -> Result<PathBuf>` | Build a `.deb`, return its path. |
| `arx_pack::build_rpm(&Manifest, out_dir: &Path) -> Result<PathBuf>` | Build a `.rpm`, return its path. |
| `arx_pack::build_apk(&Manifest, out_dir: &Path) -> Result<PathBuf>` | Build a `.apk`, return its path. |
| `Backend::{Native, Docker { image }}` | Build backend. `Native` is the default; `Docker` shells out to a configured container image when requested. |
| `Backend::build(&Manifest, Format, out_dir) -> Result<PathBuf>` | Dispatch a build through a backend. |
| `Format::{Deb, Rpm, Apk}` | Output format selector. |

Errors are `anyhow::Result`. Directory entries are expanded once through the
shared manifest path, sorted deterministically, and reject symlinks, special
files, and duplicate destinations before any backend builds. The crate is
designed to be embeddable — `cargo add arx-pack` and call the builders directly
from another Rust tool.

## How it works

- **`.deb`** is assembled in pure Rust (`ar` + `tar` + `flate2`): a
  `debian-binary` member (`2.0\n`), a `control.tar.gz` (RFC822 `control`,
  `md5sums`, optional maintainer scripts), and a `data.tar.gz` of the installed
  files at their destination paths. Entries are sorted and headers are
  deterministic.
- **`.rpm`** is assembled via the [`rpm`](https://crates.io/crates/rpm) crate
  (the same one `createrepo_rs` uses), mapping the manifest onto its
  `PackageBuilder`.

## License

MIT OR Apache-2.0
