//! Pool inspection and maintenance, shared by the CLI (`arx rm`/`arx gc`) and
//! the HTTP API. Functions here **return data**; printing/serialising is the
//! caller's job. Operations touch the pool only — run `arx publish` (or push,
//! which republishes) afterwards to regenerate metadata.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Serialize;

/// Which repository format a pool entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Apt,
    Yum,
}

/// One package file in the pool, with parsed identity.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub version: String,
    pub arch: String,
    /// rpm epoch as a string (yum only; e.g. `"0"`/`"1"`). `None` for apt, where
    /// the epoch is embedded in the Debian version string and parsed there.
    pub epoch: Option<String>,
    /// rpm release (yum only); empty for apt (embedded in `version`).
    pub release: String,
    /// apt component or yum repo name — the grouping scope.
    pub scope: String,
    pub kind: Kind,
    pub mtime: SystemTime,
}

impl Entry {
    fn group_key(&self) -> (Kind, String, String, String) {
        (self.kind, self.scope.clone(), self.name.clone(), self.arch.clone())
    }

    /// A serialisable, path-free view for the HTTP API.
    pub fn info(&self) -> PackageInfo {
        PackageInfo {
            name: self.name.clone(),
            version: self.version.clone(),
            arch: self.arch.clone(),
            scope: self.scope.clone(),
            kind: self.kind,
        }
    }
}

/// Public, serialisable description of a pooled package.
#[derive(Debug, Clone, Serialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub scope: String,
    pub kind: Kind,
}

/// Compare two same-group entries by package version, returned as an ascending
/// `Ordering` (`Less` ⇒ `a` is the older/smaller version). Uses dpkg semantics
/// for apt and rpm EVR semantics for yum — tested comparators, never a
/// hand-roll, because deleting the wrong version is data loss (ADR-0011 #3).
/// Returns `None` if either version is unparseable so the caller can fall back
/// to mtime for *that* pair only.
fn version_order(a: &Entry, b: &Entry) -> Option<Ordering> {
    match a.kind {
        Kind::Apt => {
            let va = debversion::Version::from_str(&a.version).ok()?;
            let vb = debversion::Version::from_str(&b.version).ok()?;
            Some(va.cmp(&vb))
        }
        Kind::Yum => {
            // rpm epoch is a string here; empty is treated as "0" by the crate.
            let ea = rpm_version::Evr::new(
                a.epoch.clone().unwrap_or_default(),
                a.version.clone(),
                a.release.clone(),
            );
            let eb = rpm_version::Evr::new(
                b.epoch.clone().unwrap_or_default(),
                b.version.clone(),
                b.release.clone(),
            );
            Some(ea.cmp(&eb))
        }
    }
}

fn mtime_of(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn scan_apt(root: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    let pool = root.join("apt/pool");
    if !pool.is_dir() {
        return Ok(out);
    }
    for comp in std::fs::read_dir(&pool)? {
        let comp = comp?;
        if !comp.path().is_dir() {
            continue;
        }
        let scope = comp.file_name().to_string_lossy().into_owned();
        for entry in walkdir::WalkDir::new(comp.path()).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "deb").unwrap_or(false) {
                let control = arx_debrepo::deb::read_control(p)
                    .with_context(|| format!("reading {}", p.display()))?;
                out.push(Entry {
                    name: control.package()?.to_string(),
                    version: control.version()?.to_string(),
                    arch: control.architecture()?.to_string(),
                    epoch: None,
                    release: String::new(),
                    scope: scope.clone(),
                    kind: Kind::Apt,
                    mtime: mtime_of(p),
                    path: p.to_path_buf(),
                });
            }
        }
    }
    Ok(out)
}

fn scan_yum(root: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    let yum = root.join("yum");
    if !yum.is_dir() {
        return Ok(out);
    }
    for repo in std::fs::read_dir(&yum)? {
        let repo = repo?;
        if !repo.path().is_dir() {
            continue;
        }
        let scope = repo.file_name().to_string_lossy().into_owned();
        for entry in walkdir::WalkDir::new(repo.path()).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "rpm").unwrap_or(false) {
                let mut reader = createrepo_rs::rpm::RpmReader::open(p)
                    .with_context(|| format!("opening {}", p.display()))?;
                let pkg = reader
                    .read_package()
                    .with_context(|| format!("reading {}", p.display()))?;
                out.push(Entry {
                    name: pkg.name,
                    version: pkg.version,
                    arch: pkg.arch,
                    epoch: pkg.epoch,
                    release: pkg.release,
                    scope: scope.clone(),
                    kind: Kind::Yum,
                    mtime: mtime_of(p),
                    path: p.to_path_buf(),
                });
            }
        }
    }
    Ok(out)
}

/// List packages in the pool(s) selected by `apt`/`yum` (both when neither set).
pub fn list(root: &Path, apt: bool, yum: bool) -> Result<Vec<Entry>> {
    let do_apt = apt || !yum;
    let do_yum = yum || !apt;
    let mut entries = Vec::new();
    if do_apt {
        entries.extend(scan_apt(root)?);
    }
    if do_yum {
        entries.extend(scan_yum(root)?);
    }
    Ok(entries)
}

/// Remove packages matching `name` (and optional exact `version`). Returns the
/// removed entries; does not print or republish.
pub fn remove(
    root: &Path,
    name: &str,
    version: Option<&str>,
    apt: bool,
    yum: bool,
) -> Result<Vec<Entry>> {
    let matches: Vec<Entry> = list(root, apt, yum)?
        .into_iter()
        .filter(|e| e.name == name && version.is_none_or(|v| e.version == v))
        .collect();
    for e in &matches {
        std::fs::remove_file(&e.path)
            .with_context(|| format!("removing {}", e.path.display()))?;
    }
    Ok(matches)
}

/// Result of a `gc` pass.
pub struct GcReport {
    pub pruned: Vec<Entry>,
    pub dry_run: bool,
    /// Files that *would* have been pruned but are pinned by a retained rollback
    /// state (kept so `arx rollback` stays valid).
    pub retained_for_rollback: usize,
    /// Files eligible but within the grace period (deferred).
    pub deferred: usize,
    /// Total bytes freed (or would-be-freed in dry-run).
    pub bytes_freed: u64,
}

/// Pool-relative `Filename:` paths referenced by any retained apt published state
/// (`apt/dists/.states/**/Packages`). Such files must not be pruned, or a
/// rolled-back index would 404.
fn referenced_apt_files(root: &Path) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let states = root.join("apt/dists/.states");
    if !states.is_dir() {
        return set;
    }
    for entry in walkdir::WalkDir::new(&states).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() && p.file_name().map(|n| n == "Packages").unwrap_or(false) {
            if let Ok(text) = std::fs::read_to_string(p) {
                for line in text.lines() {
                    if let Some(v) = line.strip_prefix("Filename: ") {
                        set.insert(v.trim().to_string());
                    }
                }
            }
        }
    }
    set
}

/// Absolute `.rpm` paths referenced by any retained yum state's `primary.xml`
/// (`yum/<repo>/<arch>/.states/repodata/**/sha256-primary.xml.gz`).
fn referenced_yum_files(root: &Path) -> std::collections::HashSet<PathBuf> {
    let mut set = std::collections::HashSet::new();
    let yum = root.join("yum");
    if !yum.is_dir() {
        return set;
    }
    for entry in walkdir::WalkDir::new(&yum).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        let is_primary = p
            .file_name()
            .map(|n| n.to_string_lossy().ends_with("primary.xml.gz"))
            .unwrap_or(false);
        if !is_primary || !p.components().any(|c| c.as_os_str() == ".states") {
            continue;
        }
        // arch dir = the directory whose child is `.states`.
        let arch_dir = p
            .ancestors()
            .find(|a| a.file_name().map(|n| n == ".states").unwrap_or(false))
            .and_then(|a| a.parent());
        let Some(arch_dir) = arch_dir else { continue };
        if let Ok(gz) = std::fs::read(p) {
            if let Ok(xml) = createrepo_rs::compression::gzip_decompress(&gz) {
                for href in extract_hrefs(&String::from_utf8_lossy(&xml)) {
                    set.insert(arch_dir.join(href));
                }
            }
        }
    }
    set
}

/// Pull `href="..."` values out of a primary.xml body (cheap, parser-free).
fn extract_hrefs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in xml.split("href=\"").skip(1) {
        if let Some(end) = part.find('"') {
            out.push(part[..end].to_string());
        }
    }
    out
}

/// Keep the `keep` newest *versions* per package; prune older ones. Additionally,
/// `keep_within_days` (when > 0) protects files younger than that many days from
/// pruning regardless of version count — so `--keep 3 --keep-within 90d` means
/// "keep at least 3 versions, and also keep anything from the last 90 days".
/// Returns the pruned entries (deleted unless `dry_run`). Ordering is
/// version-aware (dpkg for apt, rpm EVR for yum) so re-uploading an old file
/// can't evict a newer version; mtime is only a per-pair tiebreaker when a
/// version is unparseable. Files pinned by a retained rollback state are never
/// pruned.
pub fn gc(
    root: &Path,
    keep: usize,
    keep_within_days: u32,
    grace_days: u32,
    apt: bool,
    yum: bool,
    dry_run: bool,
) -> Result<GcReport> {
    use std::collections::BTreeMap;

    let referenced = referenced_apt_files(root);
    let referenced_rpm = referenced_yum_files(root);
    let apt_root = root.join("apt");

    let mut groups: BTreeMap<(Kind, String, String, String), Vec<Entry>> = BTreeMap::new();
    for e in list(root, apt, yum)? {
        groups.entry(e.group_key()).or_default().push(e);
    }

    let keep_within_secs = (keep_within_days as u64).saturating_mul(86400);
    let grace_secs = (grace_days as u64).saturating_mul(86400);
    let time_cutoff = if keep_within_days > 0 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub(keep_within_secs))
            .ok()
    } else {
        None
    };
    let grace_cutoff = if grace_days > 0 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub(grace_secs))
            .ok()
    } else {
        None
    };

    let mut pruned = Vec::new();
    let mut retained_for_rollback = 0usize;
    let mut deferred = 0usize;
    let mut bytes_freed: u64 = 0;
    for (_, mut versions) in groups {
        if versions.len() <= keep && keep_within_days == 0 {
            continue;
        }
        // Newest version first; per-pair fall back to mtime when a version is
        // unparseable. Then keep the first `keep`, prune the rest (the oldest).
        versions.sort_by(|a, b| {
            version_order(a, b)
                .unwrap_or_else(|| a.mtime.cmp(&b.mtime))
                .reverse()
        });
        for e in versions.into_iter().skip(keep) {
            // Protect files younger than the time cutoff.
            if let Some(cutoff) = time_cutoff {
                let mtime_secs = e
                    .mtime
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if mtime_secs >= cutoff {
                    continue; // still within the keep-within window
                }
            }
            // Keep files a retained rollback state still points at.
            let pinned = match e.kind {
                Kind::Apt => e
                    .path
                    .strip_prefix(&apt_root)
                    .map(|rel| referenced.contains(rel.to_string_lossy().as_ref()))
                    .unwrap_or(false),
                Kind::Yum => referenced_rpm.contains(&e.path),
            };
            if pinned {
                retained_for_rollback += 1;
                continue;
            }
            // Grace period: defer files that are eligible but too young to delete.
            if let Some(gc) = grace_cutoff {
                let mtime_secs = e
                    .mtime
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if mtime_secs >= gc {
                    deferred += 1;
                    continue;
                }
            }
            // Track file size for bytes-freed reporting.
            if let Ok(meta) = std::fs::metadata(&e.path) {
                bytes_freed += meta.len();
            }
            if !dry_run {
                std::fs::remove_file(&e.path)
                    .with_context(|| format!("removing {}", e.path.display()))?;
            }
            pruned.push(e);
        }
    }
    Ok(GcReport {
        pruned,
        dry_run,
        retained_for_rollback,
        deferred,
        bytes_freed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apt(v: &str) -> Entry {
        Entry {
            path: PathBuf::from(format!("/pool/{v}.deb")),
            name: "pkg".into(),
            version: v.into(),
            arch: "amd64".into(),
            epoch: None,
            release: String::new(),
            scope: "main".into(),
            kind: Kind::Apt,
            mtime: SystemTime::UNIX_EPOCH,
        }
    }

    fn yum(v: &str, epoch: Option<&str>, release: &str) -> Entry {
        Entry {
            path: PathBuf::from(format!("/pool/{v}.rpm")),
            name: "pkg".into(),
            version: v.into(),
            arch: "x86_64".into(),
            epoch: epoch.map(str::to_string),
            release: release.into(),
            scope: "repo".into(),
            kind: Kind::Yum,
            mtime: SystemTime::UNIX_EPOCH,
        }
    }

    // --- dpkg (apt) version semantics ---

    #[test]
    fn apt_epoch_dominates_upstream() {
        // 2:1.0-1 is newer than 1:9.9-1 despite the smaller upstream version.
        assert_eq!(
            version_order(&apt("2:1.0-1"), &apt("1:9.9-1")),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn apt_tilde_is_older_than_release() {
        // The tilde sorts before everything: a pre-release precedes the release.
        assert_eq!(
            version_order(&apt("1.0~rc1"), &apt("1.0")),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn apt_revision_breaks_ties() {
        assert_eq!(
            version_order(&apt("1.0-2"), &apt("1.0-1")),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn apt_unparseable_version_yields_none() {
        // None => the caller falls back to mtime for this pair (no data loss).
        assert_eq!(version_order(&apt(""), &apt("1.0")), None);
    }

    // --- rpm (yum) EVR semantics ---

    #[test]
    fn yum_epoch_dominates() {
        assert_eq!(
            version_order(&yum("1.0", Some("1"), "1"), &yum("9.9", Some("0"), "1")),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn yum_tilde_is_prerelease() {
        assert_eq!(
            version_order(&yum("1.0~beta", None, "1"), &yum("1.0", None, "1")),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn yum_release_breaks_ties() {
        assert_eq!(
            version_order(&yum("1.0", None, "2"), &yum("1.0", None, "1")),
            Some(Ordering::Greater)
        );
    }
}
