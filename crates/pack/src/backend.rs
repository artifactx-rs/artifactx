//! Build backend selection.
//!
//! # Philosophy: native-first, Docker as a fallback
//!
//! `pack` is built around a deliberate ordering of preferences:
//!
//! 1. **Prefer the native host build.** Building `.deb` and `.rpm` in pure Rust
//!    needs no `dpkg-deb`, no `rpmbuild`, no root, and no container runtime. It
//!    is fast, dependency-light, and works identically on a developer laptop and
//!    in CI. This is the default and the common case.
//!
//! 2. **Fall back to Docker only when native genuinely can't do it.** Some
//!    packages legitimately need a foreign toolchain — compiling against a
//!    specific distro's libraries, running `%build` scriptlets, or producing
//!    arch-specific binaries the host can't. For those cases the intent is to
//!    shell out to a clean, pinned container image. We do *not* reach for Docker
//!    for anything the native path already handles.
//!
//! 3. **Keep build-environment hygiene non-negotiable.** Whether native or
//!    containerised, a build should be clean (no leftover state), isolated (no
//!    bleed from the host or between builds), and reproducible (sorted entries,
//!    deterministic modes and timestamps). The native builders stage into a
//!    fresh `tempfile` directory and emit deterministic archives for this
//!    reason; the Docker path, once implemented, must use a fresh container per
//!    build and never mount more of the host than required.
//!
//! The Docker backend is intentionally a documented stub in this PoC: the
//! interface is fixed so callers can target it today, but invoking it returns a
//! clear "not yet implemented" error rather than a half-working build.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::manifest::Manifest;
use crate::{build_deb, build_rpm};

/// Which output format to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Deb,
    Rpm,
}

/// How a build is executed.
///
/// [`Native`](Backend::Native) is fully implemented. [`Docker`](Backend::Docker)
/// is the documented fallback path for builds the native packagers cannot do;
/// see the [module docs](self) for the native-first philosophy.
#[derive(Debug, Clone, Default)]
pub enum Backend {
    /// Pure-Rust, on-host build. No toolchain or container runtime required.
    #[default]
    Native,
    /// Containerised build in a pinned image. Not yet implemented.
    Docker {
        /// The container image to build inside, e.g. `debian:bookworm`.
        image: String,
    },
}

impl Backend {
    /// Build `manifest` in `format`, writing the package into `out_dir`.
    ///
    /// Returns the path of the written package. The [`Native`](Backend::Native)
    /// backend dispatches to [`build_deb`]/[`build_rpm`]; the
    /// [`Docker`](Backend::Docker) backend is a stub that returns an error.
    pub fn build(&self, manifest: &Manifest, format: Format, out_dir: &Path) -> Result<PathBuf> {
        match self {
            Backend::Native => match format {
                Format::Deb => build_deb(manifest, out_dir),
                Format::Rpm => build_rpm(manifest, out_dir),
            },
            Backend::Docker { image } => {
                // Stub: the interface is final, the implementation is deferred.
                // When built out, this must spin up a fresh container from
                // `image`, build inside it with the host source files mounted
                // read-only, and copy the resulting artifact back to `out_dir`.
                bail!(
                    "Docker backend (image {image:?}) is not yet implemented; \
                     use Backend::Native for the {format:?} build. The native \
                     path covers the common case — Docker is reserved for builds \
                     that genuinely need a foreign toolchain."
                )
            }
        }
    }
}
