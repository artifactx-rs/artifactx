//! Native, pure-Rust `.rpm` builder, layered on the `rpm` crate (the same one
//! createrepo_rs uses).
//!
//! The `rpm` crate handles the binary header/payload format; our job is to map
//! the shared [`Manifest`] onto its [`PackageBuilder`] API and write the result.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rpm::{BuildConfig, Dependency, FileMode, FileOptions, PackageBuilder};

use crate::manifest::Manifest;

/// Build an `.rpm` for `manifest`, writing it into `out_dir`.
///
/// Returns the path of the written package, named
/// `{name}-{version}-1.{arch}.rpm` using the rpm architecture spelling.
pub fn build_rpm(manifest: &Manifest, out_dir: &Path) -> Result<PathBuf> {
    let payload = crate::expand_payload(manifest)?;
    let arch = rpm_arch(&manifest.arch)?;

    // rpm wants a one-line summary; reuse the first line of the description.
    let summary = manifest
        .description
        .lines()
        .next()
        .unwrap_or("")
        .to_string();

    // Deterministic build: feed the shared epoch to the rpm crate so BUILDTIME,
    // payload file mtimes, and signature timestamp are all clamped — no wall-clock
    // or source-file-mtime leakage. (ADR-0012 §1.)
    let source_date = crate::resolve_source_epoch();

    let mut builder = PackageBuilder::new(
        &manifest.name,
        &manifest.version,
        &manifest.license,
        arch,
        &summary,
    );
    builder
        .using_config(BuildConfig::default().source_date(source_date))
        .release("1")
        .description(manifest.description.clone())
        .packager(manifest.maintainer.clone());

    if let Some(group) = manifest.rpm_group() {
        builder.group(group.to_string());
    }

    // Files are read from their host source paths and installed at their expanded destinations.
    for entry in &payload.files {
        let dest = format!("/{}", entry.rel);
        let mut options = FileOptions::new(dest.clone()).mode(FileMode::regular(entry.mode as u16));
        if entry.config {
            options = options.config().noreplace();
        }
        builder
            .with_file(&entry.source, options)
            .with_context(|| format!("adding file {} -> {}", entry.source, dest))?;
    }

    // Dependencies are passed through verbatim; `Dependency::any` is an
    // unversioned requirement, matching the PoC manifest's plain string deps.
    for dep in &manifest.depends {
        builder.requires(Dependency::any(dep.clone()));
    }

    for c in &manifest.conflicts {
        builder.conflicts(Dependency::any(c.clone()));
    }
    for p in &manifest.provides {
        builder.provides(Dependency::any(p.clone()));
    }
    for r in &manifest.replaces {
        builder.obsoletes(Dependency::any(r.clone()));
    }

    // Maintainer scripts, when present, are embedded as scriptlets.
    if let Some(path) = &manifest.scripts.preinst {
        builder.pre_install_script(read_script(path)?);
    }
    if let Some(path) = &manifest.scripts.postinst {
        builder.post_install_script(read_script(path)?);
    }
    if let Some(path) = &manifest.scripts.prerm {
        builder.pre_uninstall_script(read_script(path)?);
    }
    if let Some(path) = &manifest.scripts.postrm {
        builder.post_uninstall_script(read_script(path)?);
    }

    let package = builder.build().context("building rpm package")?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output dir {}", out_dir.display()))?;
    let out_path = out_dir.join(format!(
        "{}-{}-1.{}.rpm",
        manifest.name, manifest.version, arch
    ));
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
fn rpm_arch(arch: &str) -> Result<&'static str> {
    match arch {
        "x86_64" | "amd64" => Ok("x86_64"),
        "aarch64" | "arm64" => Ok("aarch64"),
        "i686" | "i386" | "x86" => Ok("i686"),
        "armv7hl" | "armhf" | "armv7" => Ok("armv7hl"),
        "ppc64le" | "ppc64el" => Ok("ppc64le"),
        "s390x" => Ok("s390x"),
        "riscv64" => Ok("riscv64"),
        "noarch" | "all" => Ok("noarch"),
        other => bail!(
            "unknown architecture {:?} — accepted: x86_64/amd64, aarch64/arm64, \
             i686/i386/x86, armv7hl/armhf/armv7, ppc64le/ppc64el, s390x, riscv64, noarch/all",
            other
        ),
    }
}
