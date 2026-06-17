//! `pack` — a pure-Rust packager that builds `.deb` and `.rpm` artifacts from a
//! single TOML manifest, with **no native toolchain required** for the common
//! case (no `dpkg-deb`, no `rpmbuild`, no container runtime).
//!
//! # Why
//!
//! Most packaging needs are "take these files, put them at these paths, attach
//! this metadata". That doesn't require a foreign toolchain — it requires
//! correctly assembling two well-specified archive formats. `pack` does exactly
//! that in pure Rust, so the same code runs on a laptop and in CI, fast and
//! dependency-light.
//!
//! See [`backend`] for the native-first / Docker-fallback philosophy and build
//! hygiene guarantees.
//!
//! # Example
//!
//! ```no_run
//! use pack::Manifest;
//! use std::path::Path;
//!
//! let manifest = Manifest::from_toml_str(r#"
//!     name = "hello"
//!     version = "1.0.0"
//!     arch = "amd64"
//!     maintainer = "Jane Dev <jane@example.com>"
//!     description = "A friendly greeter"
//!     license = "MIT"
//!
//!     [[files]]
//!     source = "build/hello"
//!     dest = "/usr/bin/hello"
//!     mode = "0755"
//! "#).unwrap();
//!
//! let deb = pack::build_deb(&manifest, Path::new("dist")).unwrap();
//! let rpm = pack::build_rpm(&manifest, Path::new("dist")).unwrap();
//! ```

mod apk;
mod backend;
mod deb;
mod manifest;
mod rpm;

pub use apk::build_apk;
pub use backend::{Backend, Format};
pub use deb::build_deb;
pub use manifest::{FileEntry, Manifest, Scripts};
pub use rpm::build_rpm;

/// Resolve the reproducibility epoch for deterministic builds.
///
/// Read the `SOURCE_DATE_EPOCH` env var (the reproducible-builds standard); if
/// unset or unparseable, default to `0`. Both `.deb` (tar/ar/gzip mtimes) and
/// `.rpm` (`source_date` → BUILDTIME + payload mtime + signature timestamp) feed
/// from this single value so the two formats share one deterministic clock.
pub fn resolve_source_epoch() -> u32 {
    if let Ok(val) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(epoch) = val.trim().parse::<u32>() {
            return epoch;
        }
    }
    0
}

/// Validate that every `files[].source` is a regular file, rejecting symlinks,
/// directories, devices, and FIFOs before either builder reads them. This is the
/// **shared pre-staging gate** (ADR-0012 §2). Uses `symlink_metadata` so symlinks
/// are distinguishable (not silently followed) and their type is named in the error.
pub fn validate_sources(manifest: &Manifest) -> Result<(), anyhow::Error> {
    use anyhow::{bail, Context};
    for entry in &manifest.files {
        let meta = std::fs::symlink_metadata(&entry.source)
            .with_context(|| format!("stat-ing source file {}", entry.source))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            bail!(
                "source {:?} is a symbolic link, not a regular file — \
                 symlink sources are not supported (copy the target content or use a regular file)",
                entry.source
            );
        }
        if ft.is_dir() {
            bail!("source {:?} is a directory, not a regular file", entry.source);
        }
        if !ft.is_file() {
            bail!(
                "source {:?} is not a regular file (type: {:?})",
                entry.source,
                ft
            );
        }
    }
    Ok(())
}
