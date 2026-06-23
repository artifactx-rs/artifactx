//! `arx-debrepo` — a lightweight, pure-Rust Debian/apt repository generator.
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
pub mod manifest;
pub mod statedir;

pub use deb::Control;
pub use manifest::{CachedPackage, FileManifest};
pub use statedir::StateInfo;

use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    /// Suite (e.g. `stable`).
    pub suite: String,
    /// Codename (often the same as `suite`, but upstream repos may differ).
    pub codename: String,
    /// Days until the `Release` expires (`Valid-Until`). `0` omits the field
    /// (no expiry), preserving the original behavior; a positive value protects
    /// clients against repository freeze/replay (a MITM serving stale metadata).
    /// Republishing refreshes the window.
    pub valid_days: u32,
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
            codename: String::new(),
            valid_days: 0,
        }
    }

    pub fn with_codename(mut self, codename: impl Into<String>) -> Self {
        self.codename = codename.into();
        self
    }

    /// Set the `Valid-Until` window (days). `0` means no expiry.
    pub fn with_valid_days(mut self, days: u32) -> Self {
        self.valid_days = days;
        self
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
    /// Packages left out of the index because they could not be read/parsed or
    /// collided with an already-indexed package. Empty on a clean stage. The
    /// caller decides whether to warn-and-proceed or fail (`--strict`).
    pub skipped: Vec<SkippedDeb>,
}

/// A package omitted from the staged index, with a human-readable reason.
#[derive(Debug, Clone)]
pub struct SkippedDeb {
    pub path: PathBuf,
    pub reason: String,
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

/// Read a `.deb`'s control + raw bytes, validating the fields the index needs
/// (Package/Version/Architecture). Returns a human reason on any failure so the
/// caller can skip-and-record instead of aborting the whole publish.
/// Return (mtime_secs, file_size) for a deb path, or `(None, None)` on stat error.
fn stat_mtime_size(path: &Path) -> (Option<u64>, Option<u64>) {
    std::fs::metadata(path)
        .ok()
        .map(|m| {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            (mtime, Some(m.len()))
        })
        .unwrap_or((None, None))
}

/// Build a single package's `Packages` stanza.
fn package_stanza(control: &deb::Control, filename: &str, deb_bytes: &[u8]) -> String {
    let mut out = String::new();
    for (key, value) in control.fields() {
        // Some vendor packages carry relationship fields such as `Provides:`
        // with an empty value. Aptly omits those from generated indices, and
        // apt can reject an empty `Provides:` in Packages metadata even when it
        // tolerates other blank fields in the package control file.
        if value.trim().is_empty() {
            continue;
        }
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

/// List component directories under `<apt_root>/<pool_subdir>/`.
fn discover_components(apt_root: &Path, pool_subdir: &str) -> Result<Vec<String>> {
    let pool = apt_root.join(pool_subdir);
    let mut components = Vec::new();
    if pool.is_dir() {
        for entry in
            std::fs::read_dir(&pool).with_context(|| format!("reading {}", pool.display()))?
        {
            let entry = entry?;
            if entry.path().is_dir() {
                components.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    components.sort();
    Ok(components)
}

/// Collect `.deb` paths under `<apt_root>/<pool_subdir>/<component>`, sorted.
fn debs_in(apt_root: &Path, pool_subdir: &str, component: &str) -> Vec<PathBuf> {
    let pool = apt_root.join(pool_subdir).join(component);
    let mut debs = Vec::new();
    if pool.is_dir() {
        for entry in walkdir::WalkDir::new(&pool)
            .into_iter()
            .filter_map(|e| e.ok())
        {
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
    std::fs::write(arch_dir.join(name), bytes).with_context(|| format!("writing {name}"))?;

    let sha256 = hex_sha256(bytes);
    let by_hash = arch_dir.join("by-hash").join("SHA256");
    std::fs::create_dir_all(&by_hash).with_context(|| format!("creating {}", by_hash.display()))?;
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
///
/// When `incremental` is true, (mtime, size) of each `.deb` is compared against a
/// cached manifest (`.arx-manifest.toml` per pool component). A match reuses the
/// cached `Packages` stanza + SHA256 without re-opening the file — O(changes).
/// Set `incremental = false` (or pass `--full`) to rebuild everything from scratch.
pub fn stage_dist(
    apt_root: &Path,
    pool_subdir: &str,
    dist: &str,
    meta: &ReleaseMeta,
    incremental: bool,
) -> Result<StagedDist> {
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

    let components = discover_components(apt_root, pool_subdir)?;
    let mut index_files: Vec<IndexFile> = Vec::new();
    let mut all_arches: BTreeSet<String> = BTreeSet::new();
    let mut total = 0usize;
    let mut skipped: Vec<SkippedDeb> = Vec::new();
    // Contents-<arch> data: arch → accumulated file\tpackage lines.
    let mut contents: BTreeMap<String, String> = BTreeMap::new();
    // (Package, Version, Architecture) -> (sha256, source path) already indexed.
    // Lets identical double-adds be idempotent and flags genuine collisions
    // (same identity, different bytes). Keyed across the whole dist; iteration is
    // `debs_in`'s sorted order (deb.rs sort) so "first wins" is deterministic.
    let mut seen: HashMap<(String, String, String), (String, PathBuf)> = HashMap::new();

    for component in &components {
        let mut by_arch: BTreeMap<String, String> = BTreeMap::new();
        let mut all_stanzas: Vec<String> = Vec::new();
        let pool_comp = apt_root.join(pool_subdir).join(component);
        let comp_manifest = if incremental {
            manifest::FileManifest::load(&pool_comp).unwrap_or_default()
        } else {
            manifest::FileManifest::default()
        };
        let mut next_manifest = manifest::FileManifest::default();
        // Track filenames actually on disk so we don't leave stale manifest entries.
        let mut on_disk: std::collections::HashSet<String> = std::collections::HashSet::new();

        for deb_path in debs_in(apt_root, pool_subdir, component) {
            let fname = deb_path.file_name().unwrap().to_string_lossy().to_string();
            on_disk.insert(fname.clone());

            // Stat for cache lookup.
            let (mtime, fsize) = stat_mtime_size(&deb_path);

            // Fast path: (mtime, size) match → reuse cached stanza, checksum,
            // identity, and Contents-* lines without reopening control.tar or
            // data.tar. Older manifests that do not contain identity fall back
            // to the safe parse path and refresh themselves on this publish.
            let cached = if incremental {
                mtime
                    .zip(fsize)
                    .and_then(|(m, s)| comp_manifest.lookup(&fname, m, s))
                    .cloned()
            } else {
                None
            };
            let cached_identity = cached.as_ref().filter(|c| {
                !c.package.is_empty() && !c.version.is_empty() && !c.architecture.is_empty()
            });

            let (name, version, arch, sha, stanza, cached_contents) =
                if let Some(c) = cached_identity {
                    (
                        c.package.clone(),
                        c.version.clone(),
                        c.architecture.clone(),
                        c.sha256.clone(),
                        c.stanza.clone(),
                        Some(c.contents.clone()),
                    )
                } else {
                    // Cache miss (or an older incomplete manifest): parse the
                    // control section. On a true miss we also read the full file
                    // body for checksums and build a new stanza.
                    let control = match deb::read_control(&deb_path) {
                        Ok(c) => c,
                        Err(reason) => {
                            skipped.push(SkippedDeb {
                                path: deb_path.clone(),
                                reason: format!("{reason:#}"),
                            });
                            continue;
                        }
                    };
                    let name = match control.package() {
                        Ok(n) => n.to_string(),
                        Err(e) => {
                            skipped.push(SkippedDeb {
                                path: deb_path.clone(),
                                reason: e.to_string(),
                            });
                            continue;
                        }
                    };
                    let version = match control.version() {
                        Ok(v) => v.to_string(),
                        Err(e) => {
                            skipped.push(SkippedDeb {
                                path: deb_path.clone(),
                                reason: e.to_string(),
                            });
                            continue;
                        }
                    };
                    let arch = match control.architecture() {
                        Ok(a) => a.to_string(),
                        Err(e) => {
                            skipped.push(SkippedDeb {
                                path: deb_path.clone(),
                                reason: e.to_string(),
                            });
                            continue;
                        }
                    };

                    if let Some(ref c) = cached {
                        (
                            name,
                            version,
                            arch,
                            c.sha256.clone(),
                            c.stanza.clone(),
                            None,
                        )
                    } else {
                        let deb_bytes = match std::fs::read(&deb_path) {
                            Ok(b) => b,
                            Err(e) => {
                                skipped.push(SkippedDeb {
                                    path: deb_path.clone(),
                                    reason: format!("reading: {e}"),
                                });
                                continue;
                            }
                        };
                        let sha = hex_sha256(&deb_bytes);
                        let rel = format!("{pool_subdir}/{component}/{fname}");
                        let stanza = package_stanza(&control, &rel, &deb_bytes);
                        (name, version, arch, sha, stanza, None)
                    }
                };

            // Dedup (shared, fast-path and slow-path).
            let key = (name.clone(), version.clone(), arch.clone());
            if let Some((prev_sha, prev_path)) = seen.get(&key) {
                if prev_sha != &sha {
                    let reason = format!(
                        "collision: {name} {version} {arch} already indexed from {} with different contents",
                        prev_path.display()
                    );
                    skipped.push(SkippedDeb {
                        path: deb_path.clone(),
                        reason,
                    });
                }
                continue;
            }
            seen.insert(key, (sha.clone(), deb_path.clone()));

            // Save Contents arch before arch is moved by the entry() call below.
            let contents_arch = if arch == "all" {
                "all".to_string()
            } else {
                arch.clone()
            };

            if arch == "all" {
                all_stanzas.push(stanza.clone());
            } else {
                let buf = by_arch.entry(arch.clone()).or_default();
                buf.push_str(&stanza);
                buf.push('\n');
            }
            total += 1;

            let package_contents = if let Some(lines) = cached_contents {
                lines
            } else {
                // Accumulate Contents-<arch> data for apt-file support.
                // Failures here are not fatal — we silently skip a .deb whose
                // data.tar can't be read. The caller can't log (no tracing in
                // the MIT/Apache lib), but the data is returned in `skipped`.
                match deb::read_data_paths(&deb_path) {
                    Ok(paths) => paths
                        .into_iter()
                        .map(|fp| format!("{}\t{name}\n", fp.trim_start_matches('/')))
                        .collect(),
                    Err(_) => String::new(),
                }
            };
            if !package_contents.is_empty() {
                contents
                    .entry(contents_arch)
                    .or_default()
                    .push_str(&package_contents);
            }

            // Store every indexed file in the next manifest, including cache
            // hits, so a hot publish stays hot across repeated publishes.
            if incremental {
                if let (Some(m), Some(sz)) = (mtime, fsize) {
                    next_manifest.insert(
                        fname.clone(),
                        manifest::CachedPackage {
                            mtime: m,
                            size: sz,
                            sha256: sha.clone(),
                            stanza,
                            package: name,
                            version,
                            architecture: arch,
                            contents: package_contents,
                        },
                    );
                }
            }
        }

        // Save updated manifest (prune stale entries).
        if incremental {
            next_manifest.retain(&on_disk);
            let _ = next_manifest.save(&pool_comp);
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
            write_index(
                &comp_dir,
                component,
                arch,
                "Packages",
                plain,
                &mut index_files,
            )?;
            let gz = gzip(plain)?;
            write_index(
                &comp_dir,
                component,
                arch,
                "Packages.gz",
                &gz,
                &mut index_files,
            )?;
            all_arches.insert(arch.clone());
        }
    }

    // Build Contents-<arch> files (apt-file support). Architecture:all package
    // paths are folded into every concrete arch, like Packages. Contents files
    // live at dists/<dist>/ directly (not under a component), so we emit them
    // here without using write_index.
    let all_contents = contents.remove("all").unwrap_or_default();
    for arch in all_arches.iter() {
        let mut cbody = contents.remove(arch).unwrap_or_default();
        if !all_contents.is_empty() {
            cbody.push_str(&all_contents);
        }
        if !cbody.is_empty() {
            let plain = cbody.as_bytes();
            let name = format!("Contents-{arch}");
            let name_gz = format!("Contents-{arch}.gz");
            std::fs::write(staging_dir.join(&name), plain).context("writing Contents")?;
            let gz = gzip(plain)?;
            std::fs::write(staging_dir.join(&name_gz), &gz).context("writing Contents.gz")?;
            let sha256 = hex_sha256(plain);
            // Also write by-hash copies.
            let by_hash = staging_dir.join("by-hash").join("SHA256");
            std::fs::create_dir_all(&by_hash)
                .with_context(|| format!("creating {}", by_hash.display()))?;
            std::fs::write(by_hash.join(&sha256), plain).context("writing by-hash Contents")?;
            let sha_gz = hex_sha256(&gz);
            std::fs::write(by_hash.join(&sha_gz), &gz).context("writing by-hash Contents.gz")?;
            index_files.push(IndexFile {
                rel: name,
                size: plain.len() as u64,
                md5: hex_md5(plain),
                sha1: hex_sha1(plain),
                sha256,
            });
            index_files.push(IndexFile {
                rel: name_gz,
                size: gz.len() as u64,
                md5: hex_md5(&gz),
                sha1: hex_sha1(&gz),
                sha256: sha_gz,
            });
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
        skipped,
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
    let staged = stage_dist(apt_root, "pool", dist, meta, false)?;
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
    // One snapshot drives both Date and Valid-Until so they can't drift.
    const RFC822: &str = "%a, %d %b %Y %H:%M:%S UTC";
    let now = Utc::now();
    let date = now.format(RFC822).to_string();

    let mut out = String::new();
    out.push_str(&format!("Origin: {}\n", meta.origin));
    out.push_str(&format!("Label: {}\n", meta.label));
    out.push_str(&format!("Suite: {}\n", meta.suite));
    let codename = if meta.codename.is_empty() {
        &meta.suite
    } else {
        &meta.codename
    };
    out.push_str(&format!("Codename: {codename}\n"));
    out.push_str(&format!("Components: {}\n", components.join(" ")));
    out.push_str(&format!("Architectures: {}\n", arches.join(" ")));
    out.push_str(&format!("Date: {date}\n"));
    // Freeze protection: expire the index N days out (same format as Date so apt
    // parses it). Omitted entirely when valid_days == 0.
    if meta.valid_days > 0 {
        let valid_until = (now + chrono::Duration::days(meta.valid_days as i64))
            .format(RFC822)
            .to_string();
        out.push_str(&format!("Valid-Until: {valid_until}\n"));
    }
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
