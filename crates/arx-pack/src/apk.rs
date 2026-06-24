//! Native, pure-Rust `.apk` (Alpine Linux) builder.
//!
//! An `.apk` is a gzip-compressed tar archive containing a `.PKGINFO` metadata
//! file and the installed files at their destination paths. The format is
//! simpler than `.deb`/`.rpm` — no control archive, no cpio payload.
//!
//! Reference: https://wiki.alpinelinux.org/wiki/Apk_spec

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::manifest::Manifest;

/// Build an `.apk` for `manifest`, writing it into `out_dir`.
pub fn build_apk(manifest: &Manifest, out_dir: &Path) -> Result<PathBuf> {
    let payload = crate::expand_payload(manifest)?;
    let arch = apk_arch(&manifest.arch)?;

    let staged: Vec<StagedFile> = payload
        .files
        .iter()
        .map(|entry| StagedFile {
            rel: entry.rel.clone(),
            mode: entry.mode,
            data: entry.data.clone(),
        })
        .collect();
    let staged_dirs: Vec<StagedDir> = payload
        .dirs
        .iter()
        .map(|entry| StagedDir {
            rel: entry.rel.clone(),
            mode: entry.mode,
        })
        .collect();

    // Build the tar.gz in memory.
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.mode(tar::HeaderMode::Deterministic);

    // .PKGINFO — the only mandatory metadata file.
    let pkinfo = render_pkinfo(manifest, arch);
    append_text(
        &mut tar,
        ".PKGINFO",
        &pkinfo,
        0o644,
        crate::resolve_source_epoch(),
    )?;

    // Maintainer scripts.
    if let Some(path) = &manifest.scripts.postinst {
        let body =
            std::fs::read_to_string(path).with_context(|| format!("reading postinst {path}"))?;
        append_text(
            &mut tar,
            ".post-install",
            &body,
            0o755,
            crate::resolve_source_epoch(),
        )?;
    }
    if let Some(path) = &manifest.scripts.preinst {
        let body =
            std::fs::read_to_string(path).with_context(|| format!("reading preinst {path}"))?;
        append_text(
            &mut tar,
            ".pre-install",
            &body,
            0o755,
            crate::resolve_source_epoch(),
        )?;
    }

    // Payload directories.
    for dir in &staged_dirs {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Directory);
        h.set_mode(dir.mode);
        h.set_size(0);
        h.set_mtime(crate::resolve_source_epoch() as u64);
        h.set_cksum();
        tar.append_data(&mut h, format!(".{}", dir.rel), std::io::empty())
            .with_context(|| format!("appending dir {}", dir.rel))?;
    }

    // Payload files.
    for f in &staged {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Regular);
        h.set_mode(f.mode);
        h.set_size(f.data.len() as u64);
        h.set_mtime(crate::resolve_source_epoch() as u64);
        h.set_cksum();
        tar.append_data(&mut h, format!(".{}", f.rel), f.data.as_slice())
            .with_context(|| format!("appending file {}", f.rel))?;
    }

    let gz = tar.into_inner().context("finishing apk tar")?;
    let body = gz.finish().context("finishing apk gzip")?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output dir {}", out_dir.display()))?;
    // APK naming: <name>-<version>-r0.<arch>.apk
    let filename = format!("{}-{}-r0.{}.apk", manifest.name, manifest.version, arch);
    let out_path = out_dir.join(&filename);
    std::fs::write(&out_path, &body).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(out_path)
}

struct StagedFile {
    rel: String,
    mode: u32,
    data: Vec<u8>,
}

struct StagedDir {
    rel: String,
    mode: u32,
}

fn render_pkinfo(manifest: &Manifest, arch: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("pkgname = {}\n", manifest.name));
    s.push_str(&format!("pkgver = {}\n", manifest.version));
    s.push_str(&format!(
        "pkgdesc = {}\n",
        manifest.description.lines().next().unwrap_or("")
    ));
    s.push_str(&format!("arch = {arch}\n"));
    s.push_str(&format!("license = {}\n", manifest.license));
    s.push_str(&format!("maintainer = {}\n", manifest.maintainer));
    if !manifest.depends.is_empty() {
        s.push_str(&format!("depend = {}\n", manifest.depends.join(" ")));
    }
    s
}

fn apk_arch(arch: &str) -> Result<&'static str> {
    match arch {
        "x86_64" | "amd64" => Ok("x86_64"),
        "aarch64" | "arm64" => Ok("aarch64"),
        "armhf" | "armv7" | "armv7hl" => Ok("armhf"),
        "x86" | "i386" | "i686" => Ok("x86"),
        "ppc64le" | "ppc64el" => Ok("ppc64le"),
        "s390x" => Ok("s390x"),
        "riscv64" => Ok("riscv64"),
        "noarch" | "all" => Ok("noarch"),
        other => anyhow::bail!("unknown APK architecture {other:?}"),
    }
}

fn append_text<W: Write>(
    tar: &mut tar::Builder<W>,
    name: &str,
    body: &str,
    mode: u32,
    epoch: u32,
) -> Result<()> {
    let bytes = body.as_bytes();
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(mode);
    h.set_size(bytes.len() as u64);
    h.set_mtime(epoch as u64);
    h.set_cksum();
    tar.append_data(&mut h, name, bytes)
        .with_context(|| format!("appending {name}"))?;
    Ok(())
}
