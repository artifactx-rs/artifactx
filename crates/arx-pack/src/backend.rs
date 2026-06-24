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
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::manifest::Manifest;
use crate::{build_apk, build_deb, build_rpm};

/// Which output format to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Deb,
    Rpm,
    Apk,
}

/// How a build is executed.
#[derive(Debug, Clone, Default)]
pub enum Backend {
    /// Pure-Rust, on-host build. No toolchain or container runtime required.
    #[default]
    Native,
    /// Containerised build. Spins up a fresh container from `image`, mounts the
    /// host's `arx` binary and source files, runs `arx pack` inside, and copies
    /// the resulting artifacts back. Requires `docker` on PATH.
    Docker { image: String },
}

impl Backend {
    /// Build `manifest` in `format`, writing the package into `out_dir`.
    pub fn build(&self, manifest: &Manifest, format: Format, out_dir: &Path) -> Result<PathBuf> {
        match self {
            Backend::Native => match format {
                Format::Deb => build_deb(manifest, out_dir),
                Format::Rpm => build_rpm(manifest, out_dir),
                Format::Apk => build_apk(manifest, out_dir),
            },
            Backend::Docker { image } => docker_build(manifest, format, out_dir, image),
        }
    }
}

/// Real Docker backend: mount host arx + source files, build inside container.
fn docker_build(
    manifest: &Manifest,
    format: Format,
    out_dir: &Path,
    image: &str,
) -> Result<PathBuf> {
    // Find the host's arx binary.
    let arx_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("arx"));
    if !arx_bin.exists() {
        bail!(
            "Docker backend requires the arx binary at {} — build arx first",
            arx_bin.display()
        );
    }

    // Build a self-contained context directory with the manifest + source files.
    let context = tempfile::tempdir().context("creating Docker build context")?;
    let manifest_path = context.path().join("manifest.toml");

    // Copy source files and directory payloads into the context directory, adjusting paths.
    let mut adjusted = manifest.clone();
    for entry in adjusted.files.iter_mut() {
        let src = Path::new(&entry.source);
        let name = src.file_name().context("source has no filename")?;
        let dest = context.path().join(name);
        std::fs::copy(src, &dest)
            .with_context(|| format!("copying {} into Docker context", entry.source))?;
        entry.source = dest.to_string_lossy().into_owned();
    }
    for (idx, entry) in adjusted.dirs.iter_mut().enumerate() {
        let src = Path::new(&entry.source);
        let dest = context.path().join(format!("dir-{idx}"));
        copy_dir_tree(src, &dest)
            .with_context(|| format!("copying {} into Docker context", entry.source))?;
        entry.source = dest.to_string_lossy().into_owned();
    }
    // Write the adjusted manifest.
    let manifest_toml = toml::to_string_pretty(&adjusted).context("serialising manifest")?;
    std::fs::write(&manifest_path, &manifest_toml).context("writing manifest.toml")?;

    if format == Format::Apk {
        anyhow::bail!("Docker backend does not yet support .apk builds; use Backend::Native");
    }
    let fmt_flag = match format {
        Format::Deb => "--deb",
        Format::Rpm => "--rpm",
        Format::Apk => unreachable!(),
    };
    let container_out = "/build/out";

    // Run: docker run --rm -v <context>:/build -v <arx>:/arx <image> /arx pack manifest.toml --out <container_out>
    let status = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/build", context.path().display()),
            "-v",
            &format!("{}:/arx", arx_bin.display()),
            "-w",
            "/build",
            image,
            "/arx",
            "pack",
            "manifest.toml",
            fmt_flag,
            "--out",
            container_out,
        ])
        .status()
        .context("running docker — is Docker installed and running?")?;

    if !status.success() {
        bail!("Docker build failed (exit {status})");
    }

    // Find the built artifact in the context output dir and copy it out.
    let container_out_dir = context.path().join("out");
    let mut found: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&container_out_dir)
        .with_context(|| format!("reading Docker output dir {}", container_out_dir.display()))?
    {
        let entry = entry?;
        let p = entry.path();
        if p.is_file() {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if (format == Format::Deb && ext == "deb") || (format == Format::Rpm && ext == "rpm") {
                found = Some(p);
                break;
            }
        }
    }
    let artifact = found.context("no .deb/.rpm found in Docker build output")?;
    let name = artifact.file_name().unwrap().to_string_lossy().into_owned();
    let dest = out_dir.join(&name);
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    std::fs::copy(&artifact, &dest).with_context(|| format!("copying {name} from container"))?;

    Ok(dest)
}

fn copy_dir_tree(src: &Path, dest: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(src)
        .with_context(|| format!("stat-ing source directory {}", src.display()))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        bail!("source directory {} is a symbolic link", src.display());
    }
    if !ft.is_dir() {
        bail!("source directory {} is not a directory", src.display());
    }

    std::fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut children = std::fs::read_dir(src)
        .with_context(|| format!("reading source directory {}", src.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("reading entries in {}", src.display()))?;
    children.sort_by_key(|entry| entry.path());

    for child in children {
        let child_src = child.path();
        let child_dest = dest.join(child.file_name());
        let child_meta = std::fs::symlink_metadata(&child_src)
            .with_context(|| format!("stat-ing source path {}", child_src.display()))?;
        let child_ft = child_meta.file_type();
        if child_ft.is_symlink() {
            bail!("source path {} is a symbolic link", child_src.display());
        }
        if child_ft.is_dir() {
            copy_dir_tree(&child_src, &child_dest)?;
        } else if child_ft.is_file() {
            std::fs::copy(&child_src, &child_dest).with_context(|| {
                format!(
                    "copying {} to {}",
                    child_src.display(),
                    child_dest.display()
                )
            })?;
        } else {
            bail!(
                "source path {} is not a regular file or directory",
                child_src.display()
            );
        }
    }
    Ok(())
}
