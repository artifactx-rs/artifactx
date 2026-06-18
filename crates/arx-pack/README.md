# arx-pack

**Pure-Rust packager: build `.deb` and `.rpm` from a single TOML manifest — no native toolchain required for the common case.**

`arx-pack` is the *Package* pillar of ArtifactX. You describe a package once — its
metadata and the files it installs — and `arx-pack` emits both a Debian `.deb` and
an RPM `.rpm`. No `dpkg-deb`, no `rpmbuild`, no root, and no container runtime
needed for ordinary "stage these files at these paths with this metadata"
packaging. The same code runs identically on a developer laptop and in CI.

> Status: **proof of concept.** The native `.deb` and `.rpm` builders are
> implemented and tested. The Docker backend is a documented stub (see below).

## Philosophy: native-first, Docker as a fallback, hygiene always

1. **Prefer the native host build.** Building `.deb` and `.rpm` in pure Rust is
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

[[files]]
source = "build/hello"     # path on the build host
dest   = "/usr/bin/hello"  # install path inside the target system
mode   = "0755"            # octal, as a string so the leading zero survives

# Optional maintainer scripts (host paths, embedded into the package):
# [scripts]
# postinst = "scripts/postinst.sh"
# prerm    = "scripts/prerm.sh"
```

## From a Cargo project (zero config)

In a Rust crate, identity is derived from `Cargo.toml` and packaging details from
`[package.metadata.arx]` — no separate manifest, no repeated name/version:

```toml
# Cargo.toml
[package]
name = "greeter"
version = "0.3.0"
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
arx pack            # reads ./Cargo.toml → greeter_0.3.0_amd64.deb + .rpm
```

`Manifest::from_cargo_toml(&str)` exposes the same mapping as a library call.

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
| `arx_pack::build_deb(&Manifest, out_dir: &Path) -> Result<PathBuf>` | Build a `.deb`, return its path. |
| `arx_pack::build_rpm(&Manifest, out_dir: &Path) -> Result<PathBuf>` | Build a `.rpm`, return its path. |
| `Backend::{Native, Docker { image }}` | Build backend. `Native` is implemented; `Docker` is a stub. |
| `Backend::build(&Manifest, Format, out_dir) -> Result<PathBuf>` | Dispatch a build through a backend. |
| `Format::{Deb, Rpm}` | Output format selector. |

Errors are `anyhow::Result`. The crate is designed to be embeddable —
`cargo add arx-pack` and call the builders directly from another Rust tool.

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
