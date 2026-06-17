//! `debrepo` — a lightweight, pure-Rust Debian/apt repository generator.
//!
//! It parses `.deb` packages and emits `Packages`/`Packages.gz` per
//! architecture/component plus a single `Release` index covering the whole
//! distribution. It is **signing-agnostic** and **atomic**:
//!
//! 1. [`stage_dist`] builds the full `dists/<dist>` tree into a staging
//!    directory and returns the `Release` text.
//! 2. The caller signs `release_text` into `InRelease`/`Release.gpg` *inside the
//!    staging dir* (so signatures are part of the atomic unit).
//! 3. [`commit_dist`] swaps staging into place with a directory rename.
//!
//! [`build_dist`] is a convenience that stages and commits in one call (no
//! signing), used for unsigned repos and tests.
//!
//! `by-hash` index copies are written and `Acquire-By-Hash: yes` is set, so
//! clients never see a `Hash Sum mismatch` across a publish.

pub mod deb;
pub mod statedir;

pub use statedir::StateInfo;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// Human-facing repository identity written into the `Release` index.
#[derive(Debug, Clone)]
pub struct ReleaseMeta {
    pub origin: String,
    pub label: String,
    pub description: String,
    /// Suite/codename (e.g. `stable`). Both `Suite` and `Codename` use this.
    pub suite: String,
}

impl ReleaseMeta {
    pub fn new(
        origin: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
        suite: impl Into<String>,
    ) -> Self {
        Self {
            origin: origin.into(),
            label: label.into(),
            description: description.into(),
            suite: suite.into(),
        }
    }
}

/// A distribution staged on disk but not yet swapped into place.
#[derive(Debug, Clone)]
pub struct StagedDist {
    /// Exact `Release` contents (sign this for InRelease/Release.gpg).
    pub release_text: String,
    /// Staging directory; write signatures here before [`commit_dist`].
    pub staging_dir: PathBuf,
    /// Final location the staging dir is swapped to on commit.
    pub final_dir: PathBuf,
    pub packages: usize,
    pub components: Vec<String>,
    pub architectures: Vec<String>,
}

/// Result of a committed [`build_dist`].
#[derive(Debug, Clone)]
pub struct DistBuild {
    pub release_text: String,
    pub packages: usize,
    pub components: Vec<String>,
    pub architectures: Vec<String>,
}

/// Checksums and size for one generated index file.
struct IndexFile {
    /// Path relative to the dist directory, e.g. `main/binary-amd64/Packages`.
    rel: String,
    size: u64,
    md5: String,
    sha1: String,
    sha256: String,
}

fn hex_md5(data: &[u8]) -> String {
    hex::encode(Md5::digest(data))
}
fn hex_sha1(data: &[u8]) -> String {
    hex::encode(Sha1::digest(data))
}
fn hex_sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Build a single package's `Packages` stanza.
fn package_stanza(control: &deb::Control, filename: &str, deb_bytes: &[u8]) -> String {
    let mut out = String::new();
    for (key, value) in control.fields() {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(value);
        out.push('\n');
    }
    out.push_str(&format!("Filename: {filename}\n"));
    out.push_str(&format!("Size: {}\n", deb_bytes.len()));
    out.push_str(&format!("MD5sum: {}\n", hex_md5(deb_bytes)));
    out.push_str(&format!("SHA1: {}\n", hex_sha1(deb_bytes)));
    out.push_str(&format!("SHA256: {}\n", hex_sha256(deb_bytes)));
    out
}

fn gzip(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(data).context("gzip write")?;
    encoder.finish().context("gzip finish")
}

/// List component directories under `<apt_root>/pool/`.
fn discover_components(apt_root: &Path) -> Result<Vec<String>> {
    let pool = apt_root.join("pool");
    let mut components = Vec::new();
    if pool.is_dir() {
        for entry in std::fs::read_dir(&pool).with_context(|| format!("reading {}", pool.display()))? {
            let entry = entry?;
            if entry.path().is_dir() {
                components.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    components.sort();
    Ok(components)
}

/// Collect `.deb` paths under `<apt_root>/pool/<component>`, sorted.
fn debs_in(apt_root: &Path, component: &str) -> Vec<PathBuf> {
    let pool = apt_root.join("pool").join(component);
    let mut debs = Vec::new();
    if pool.is_dir() {
        for entry in walkdir::WalkDir::new(&pool).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "deb").unwrap_or(false) {
                debs.push(p.to_path_buf());
            }
        }
    }
    debs.sort();
    debs
}

/// Write an index file plus its `by-hash/SHA256/<sha>` copy, and record it.
fn write_index(
    comp_dir: &Path,
    component: &str,
    arch: &str,
    name: &str,
    bytes: &[u8],
    index_files: &mut Vec<IndexFile>,
) -> Result<()> {
    let arch_dir = comp_dir.join(format!("binary-{arch}"));
    std::fs::create_dir_all(&arch_dir)
        .with_context(|| format!("creating {}", arch_dir.display()))?;
    std::fs::write(arch_dir.join(name), bytes)
        .with_context(|| format!("writing {name}"))?;

    let sha256 = hex_sha256(bytes);
    let by_hash = arch_dir.join("by-hash").join("SHA256");
    std::fs::create_dir_all(&by_hash)
        .with_context(|| format!("creating {}", by_hash.display()))?;
    std::fs::write(by_hash.join(&sha256), bytes).context("writing by-hash copy")?;

    index_files.push(IndexFile {
        rel: format!("{component}/binary-{arch}/{name}"),
        size: bytes.len() as u64,
        md5: hex_md5(bytes),
        sha1: hex_sha1(bytes),
        sha256,
    });
    Ok(())
}

/// Build the entire `dists/<dist>` tree into a fresh **staging** directory under
/// `<apt_root>/dists/.<dist>.staging`, without touching the live dist.
///
/// Sign `release_text` into the returned `staging_dir`, then call [`commit_dist`].
pub fn stage_dist(apt_root: &Path, dist: &str, meta: &ReleaseMeta) -> Result<StagedDist> {
    let dists = apt_root.join("dists");
    let final_dir = dists.join(dist);
    let staging_dir = dists.join(format!(".{dist}.staging"));

    // Start from a clean staging dir.
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir)
            .with_context(|| format!("clearing {}", staging_dir.display()))?;
    }
    std::fs::create_dir_all(&staging_dir)
        .with_context(|| format!("creating {}", staging_dir.display()))?;

    let components = discover_components(apt_root)?;
    let mut index_files: Vec<IndexFile> = Vec::new();
    let mut all_arches: BTreeSet<String> = BTreeSet::new();
    let mut total = 0usize;

    for component in &components {
        // arch -> accumulated Packages text; plus Architecture: all stanzas.
        let mut by_arch: BTreeMap<String, String> = BTreeMap::new();
        let mut all_stanzas: Vec<String> = Vec::new();

        for deb_path in debs_in(apt_root, component) {
            let control = deb::read_control(&deb_path)
                .with_context(|| format!("inspecting {}", deb_path.display()))?;
            let arch = control.architecture()?.to_string();
            let deb_bytes = std::fs::read(&deb_path)
                .with_context(|| format!("reading {}", deb_path.display()))?;
            let rel_filename = format!(
                "pool/{component}/{}",
                deb_path.file_name().unwrap().to_string_lossy()
            );
            let stanza = package_stanza(&control, &rel_filename, &deb_bytes);
            if arch == "all" {
                all_stanzas.push(stanza);
            } else {
                let buf = by_arch.entry(arch).or_default();
                buf.push_str(&stanza);
                buf.push('\n');
            }
            total += 1;
        }

        let concrete: Vec<String> = by_arch.keys().cloned().collect();
        let target_arches: Vec<String> = if concrete.is_empty() && !all_stanzas.is_empty() {
            vec!["all".to_string()]
        } else {
            concrete
        };

        let comp_dir = staging_dir.join(component);
        for arch in &target_arches {
            let buf = by_arch.entry(arch.clone()).or_default();
            for stanza in &all_stanzas {
                buf.push_str(stanza);
                buf.push('\n');
            }
            let plain = buf.as_bytes();
            write_index(&comp_dir, component, arch, "Packages", plain, &mut index_files)?;
            let gz = gzip(plain)?;
            write_index(&comp_dir, component, arch, "Packages.gz", &gz, &mut index_files)?;
            all_arches.insert(arch.clone());
        }
    }

    let arches: Vec<String> = all_arches.into_iter().collect();
    let release_text = render_release(meta, &components, &arches, &index_files);
    std::fs::write(staging_dir.join("Release"), release_text.as_bytes())
        .context("writing Release")?;

    Ok(StagedDist {
        release_text,
        staging_dir,
        final_dir,
        packages: total,
        components,
        architectures: arches,
    })
}

/// Default number of published dist states retained for rollback.
pub const DEFAULT_KEEP_STATES: usize = 5;

/// Commit a staged dist into an immutable state dir and atomically flip
/// `dists/<dist>` (a symlink) to it. Old states beyond `keep` are pruned.
pub fn commit_dist(staged: &StagedDist, keep: usize) -> Result<()> {
    statedir::commit(&staged.staging_dir, &staged.final_dir, keep)?;
    Ok(())
}

/// List retained states for an apt `dist`, oldest first.
pub fn list_states(apt_root: &Path, dist: &str) -> Result<Vec<StateInfo>> {
    statedir::list(&apt_root.join("dists").join(dist))
}

/// Roll an apt `dist` back to a previous state (or `to`). Returns the new id.
pub fn rollback(apt_root: &Path, dist: &str, to: Option<&str>) -> Result<String> {
    statedir::rollback(&apt_root.join("dists").join(dist), to)
}

/// Convenience: stage and commit in one step (no signing). For tests / unsigned repos.
pub fn build_dist(apt_root: &Path, dist: &str, meta: &ReleaseMeta) -> Result<DistBuild> {
    let staged = stage_dist(apt_root, dist, meta)?;
    let out = DistBuild {
        release_text: staged.release_text.clone(),
        packages: staged.packages,
        components: staged.components.clone(),
        architectures: staged.architectures.clone(),
    };
    commit_dist(&staged, DEFAULT_KEEP_STATES)?;
    Ok(out)
}

fn render_release(
    meta: &ReleaseMeta,
    components: &[String],
    arches: &[String],
    index_files: &[IndexFile],
) -> String {
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S UTC").to_string();

    let mut out = String::new();
    out.push_str(&format!("Origin: {}\n", meta.origin));
    out.push_str(&format!("Label: {}\n", meta.label));
    out.push_str(&format!("Suite: {}\n", meta.suite));
    out.push_str(&format!("Codename: {}\n", meta.suite));
    out.push_str(&format!("Components: {}\n", components.join(" ")));
    out.push_str(&format!("Architectures: {}\n", arches.join(" ")));
    out.push_str(&format!("Date: {date}\n"));
    out.push_str("Acquire-By-Hash: yes\n");
    out.push_str(&format!("Description: {}\n", meta.description));

    out.push_str("MD5Sum:\n");
    for f in index_files {
        out.push_str(&format!(" {} {} {}\n", f.md5, f.size, f.rel));
    }
    out.push_str("SHA1:\n");
    for f in index_files {
        out.push_str(&format!(" {} {} {}\n", f.sha1, f.size, f.rel));
    }
    out.push_str("SHA256:\n");
    for f in index_files {
        out.push_str(&format!(" {} {} {}\n", f.sha256, f.size, f.rel));
    }
    out
}
