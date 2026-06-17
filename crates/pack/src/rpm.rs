//! Native, pure-Rust `.rpm` builder, layered on the `rpm` crate (the same one
//! createrepo_rs uses).
//!
//! The `rpm` crate handles the binary header/payload format; our job is to map
//! the shared [`Manifest`] onto its [`PackageBuilder`] API and write the result.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rpm::{Dependency, FileMode, FileOptions, PackageBuilder};

use crate::manifest::Manifest;

/// Build an `.rpm` for `manifest`, writing it into `out_dir`.
///
/// Returns the path of the written package, named
/// `{name}-{version}-1.{arch}.rpm` using the rpm architecture spelling.
pub fn build_rpm(manifest: &Manifest, out_dir: &Path) -> Result<PathBuf> {
    let arch = rpm_arch(&manifest.arch);

    // rpm wants a one-line summary; reuse the first line of the description.
    let summary = manifest.description.lines().next().unwrap_or("").to_string();

    let mut builder = PackageBuilder::new(
        &manifest.name,
        &manifest.version,
        &manifest.license,
        arch,
        &summary,
    )
    .release("1")
    .description(manifest.description.clone())
    .packager(manifest.maintainer.clone());

    if let Some(group) = manifest.rpm_group() {
        builder = builder.group(group.to_string());
    }

    // Files are read from their host source paths and installed at `dest`.
    for entry in &manifest.files {
        let mode = entry.mode_bits()?;
        let options = FileOptions::new(entry.dest.clone()).mode(FileMode::regular(mode as u16));
        builder = builder
            .with_file(&entry.source, options)
            .with_context(|| format!("adding file {} -> {}", entry.source, entry.dest))?;
    }

    // Dependencies are passed through verbatim; `Dependency::any` is an
    // unversioned requirement, matching the PoC manifest's plain string deps.
    for dep in &manifest.depends {
        builder = builder.requires(Dependency::any(dep.clone()));
    }

    // Maintainer scripts, when present, are embedded as scriptlets.
    if let Some(path) = &manifest.scripts.preinst {
        builder = builder.pre_install_script(read_script(path)?);
    }
    if let Some(path) = &manifest.scripts.postinst {
        builder = builder.post_install_script(read_script(path)?);
    }
    if let Some(path) = &manifest.scripts.prerm {
        builder = builder.pre_uninstall_script(read_script(path)?);
    }
    if let Some(path) = &manifest.scripts.postrm {
        builder = builder.post_uninstall_script(read_script(path)?);
    }

    let package = builder.build().context("building rpm package")?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output dir {}", out_dir.display()))?;
    let out_path = out_dir.join(format!("{}-{}-1.{}.rpm", manifest.name, manifest.version, arch));
    package
        .write_file(&out_path)
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(out_path)
}

/// Read a maintainer script file into a string for embedding as a scriptlet.
fn read_script(path: &str) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading maintainer script {path}"))
}

/// Map a manifest architecture onto the rpm spelling.
///
/// rpm uses the GNU names (`x86_64`/`aarch64`) and `noarch`; we also accept the
/// Debian spellings so a single manifest can feed both builders.
fn rpm_arch(arch: &str) -> &'static str {
    match arch {
        "x86_64" | "amd64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        "i686" | "i386" | "x86" => "i686",
        "armv7hl" | "armhf" | "armv7" => "armv7hl",
        "ppc64le" | "ppc64el" => "ppc64le",
        "s390x" => "s390x",
        "riscv64" => "riscv64",
        "noarch" | "all" => "noarch",
        // Unknown: default to x86_64 for the PoC rather than failing the build.
        _ => "x86_64",
    }
}
