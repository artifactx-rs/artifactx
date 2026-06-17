//! Native, pure-Rust `.deb` builder.
//!
//! A `.deb` is an `ar` archive of exactly three members, in order:
//!   1. `debian-binary` — the literal text `2.0\n`.
//!   2. `control.tar.gz` — the package metadata: an RFC822 `control` file plus
//!      `md5sums`, and any maintainer scripts.
//!   3. `data.tar.gz` — the installed files, laid out at their destination paths.
//!
//! We assemble the data tree in a temporary staging directory so file modes are
//! set deterministically, then build both tarballs in memory with entries sorted
//! for reproducible output.

use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::manifest::Manifest;

/// Build a `.deb` for `manifest`, writing it into `out_dir`.
///
/// Returns the path of the written package, named `{name}_{version}_{arch}.deb`
/// using the Debian architecture spelling.
pub fn build_deb(manifest: &Manifest, out_dir: &Path) -> Result<PathBuf> {
    let arch = deb_arch(&manifest.arch);

    // Stage the payload on disk first. tempfile gives us an isolated, auto-removed
    // directory so a build leaves nothing behind even if it fails midway.
    let staging = tempfile::tempdir().context("creating staging directory")?;
    let mut staged: Vec<StagedFile> = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        let mode = entry.mode_bits()?;
        let data = std::fs::read(&entry.source)
            .with_context(|| format!("reading source file {}", entry.source))?;
        // Destination is absolute in the manifest; inside a tar it must be relative.
        let rel = entry
            .dest
            .strip_prefix('/')
            .unwrap_or(&entry.dest)
            .to_string();
        if rel.is_empty() {
            bail!("file dest {:?} resolves to an empty path", entry.dest);
        }
        staged.push(StagedFile { rel, mode, data });
    }
    // Sort by install path for reproducible archive ordering.
    staged.sort_by(|a, b| a.rel.cmp(&b.rel));

    let data_tar = build_data_tar(&staged).context("building data.tar.gz")?;
    let md5sums = md5sums(&staged);
    let control = render_control(manifest, arch, installed_size(&staged));
    let control_tar = build_control_tar(manifest, &control, &md5sums)
        .context("building control.tar.gz")?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output dir {}", out_dir.display()))?;
    let out_path = out_dir.join(format!("{}_{}_{}.deb", manifest.name, manifest.version, arch));
    let file = std::fs::File::create(&out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;

    // ar members must appear in this exact order for a valid .deb.
    let mut builder = ar::Builder::new(file);
    append_ar(&mut builder, "debian-binary", b"2.0\n")?;
    append_ar(&mut builder, "control.tar.gz", &control_tar)?;
    append_ar(&mut builder, "data.tar.gz", &data_tar)?;
    builder.into_inner().context("finalising ar archive")?;

    // `staging` is dropped (and deleted) here; keep it alive until now so any
    // future on-disk staging strategy stays valid.
    drop(staging);
    Ok(out_path)
}

/// A file staged for inclusion, with its in-archive relative path.
struct StagedFile {
    rel: String,
    mode: u32,
    data: Vec<u8>,
}

/// Map a manifest architecture onto the Debian spelling.
///
/// Debian uses `amd64`/`arm64`/`i386`; we also accept the GNU/rpm spellings so a
/// single manifest can feed both builders.
fn deb_arch(arch: &str) -> &'static str {
    match arch {
        "amd64" | "x86_64" => "amd64",
        "arm64" | "aarch64" => "arm64",
        "i386" | "i686" | "x86" => "i386",
        "armhf" | "armv7" | "armv7hl" => "armhf",
        "ppc64el" | "ppc64le" => "ppc64el",
        "s390x" => "s390x",
        "riscv64" => "riscv64",
        "all" | "noarch" => "all",
        // Unknown: default to amd64 for the PoC rather than failing the build.
        _ => "amd64",
    }
}

/// Sum of file sizes in KiB, rounded up — the value of the deb `Installed-Size`.
fn installed_size(staged: &[StagedFile]) -> u64 {
    let bytes: u64 = staged.iter().map(|f| f.data.len() as u64).sum();
    bytes.div_ceil(1024)
}

/// Render the RFC822 `control` file body.
fn render_control(manifest: &Manifest, arch: &str, installed_kib: u64) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Package: {}", manifest.name);
    let _ = writeln!(out, "Version: {}", manifest.version);
    let _ = writeln!(out, "Architecture: {arch}");
    let _ = writeln!(out, "Maintainer: {}", manifest.maintainer);
    let _ = writeln!(out, "Installed-Size: {installed_kib}");
    if let Some(section) = &manifest.section {
        let _ = writeln!(out, "Section: {section}");
    }
    if !manifest.depends.is_empty() {
        let _ = writeln!(out, "Depends: {}", manifest.depends.join(", "));
    }
    let _ = writeln!(out, "Priority: optional");
    // Render the description: first line is the synopsis, subsequent lines are
    // folded with a leading space per Debian policy.
    let mut lines = manifest.description.lines();
    let synopsis = lines.next().unwrap_or("");
    let _ = writeln!(out, "Description: {synopsis}");
    for line in lines {
        let line = line.trim_end();
        if line.is_empty() {
            let _ = writeln!(out, " .");
        } else {
            let _ = writeln!(out, " {line}");
        }
    }
    out
}

/// Compute the `md5sums` body: `<md5>  <relative-path>` per file.
fn md5sums(staged: &[StagedFile]) -> String {
    use md5::{Digest, Md5};
    let mut out = String::new();
    for f in staged {
        let digest = Md5::digest(&f.data);
        let _ = writeln!(out, "{:x}  {}", digest, f.rel);
    }
    out
}

/// Build `data.tar.gz` from the staged files, creating parent directory entries
/// deterministically so the archive matches across rebuilds.
fn build_data_tar(staged: &[StagedFile]) -> Result<Vec<u8>> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.mode(tar::HeaderMode::Deterministic);

    // Emit each unique parent directory once, before its files, in sorted order.
    let mut dirs: BTreeMap<String, ()> = BTreeMap::new();
    for f in staged {
        let mut accum = String::new();
        let parts: Vec<&str> = f.rel.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if part.is_empty() {
                continue;
            }
            accum.push_str(part);
            accum.push('/');
            dirs.insert(accum.clone(), ());
        }
    }
    for dir in dirs.keys() {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_mode(0o755);
        header.set_size(0);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, format!("./{dir}"), std::io::empty())
            .with_context(|| format!("appending dir {dir}"))?;
    }

    for f in staged {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(f.mode);
        header.set_size(f.data.len() as u64);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, format!("./{}", f.rel), f.data.as_slice())
            .with_context(|| format!("appending file {}", f.rel))?;
    }

    let gz = tar.into_inner().context("finishing data tar")?;
    gz.finish().context("finishing data gzip")
}

/// Build `control.tar.gz` containing `control`, `md5sums`, and any scripts.
fn build_control_tar(manifest: &Manifest, control: &str, md5sums: &str) -> Result<Vec<u8>> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.mode(tar::HeaderMode::Deterministic);

    append_text(&mut tar, "./control", control, 0o644)?;
    append_text(&mut tar, "./md5sums", md5sums, 0o644)?;

    // Maintainer scripts are mode 0755 and read from their host paths.
    let scripts = [
        ("./preinst", &manifest.scripts.preinst),
        ("./postinst", &manifest.scripts.postinst),
        ("./prerm", &manifest.scripts.prerm),
        ("./postrm", &manifest.scripts.postrm),
    ];
    for (name, path) in scripts {
        if let Some(path) = path {
            let body = std::fs::read(path)
                .with_context(|| format!("reading maintainer script {path}"))?;
            append_bytes(&mut tar, name, &body, 0o755)?;
        }
    }

    let gz = tar.into_inner().context("finishing control tar")?;
    gz.finish().context("finishing control gzip")
}

/// Append a UTF-8 text member to a tar builder.
fn append_text<W: Write>(
    tar: &mut tar::Builder<W>,
    name: &str,
    body: &str,
    mode: u32,
) -> Result<()> {
    append_bytes(tar, name, body.as_bytes(), mode)
}

/// Append a raw byte member to a tar builder with a deterministic header.
fn append_bytes<W: Write>(
    tar: &mut tar::Builder<W>,
    name: &str,
    body: &[u8],
    mode: u32,
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(mode);
    header.set_size(body.len() as u64);
    header.set_mtime(0);
    header.set_cksum();
    tar.append_data(&mut header, name, body)
        .with_context(|| format!("appending {name}"))?;
    Ok(())
}

/// Append a member to the outer `ar` archive with deterministic metadata.
fn append_ar<W: Write>(builder: &mut ar::Builder<W>, name: &str, data: &[u8]) -> Result<()> {
    let mut header = ar::Header::new(name.as_bytes().to_vec(), data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    builder
        .append(&header, data)
        .with_context(|| format!("appending ar member {name}"))?;
    Ok(())
}
