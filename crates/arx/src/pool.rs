//! Pool inspection and maintenance, shared by the CLI (`arx rm`/`arx gc`) and
//! the HTTP API. Functions here **return data**; printing/serialising is the
//! caller's job. Operations touch the pool only — run `arx publish` (or push,
//! which republishes) afterwards to regenerate metadata.

use std::path::{Path, PathBuf};
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
                let control = debrepo::deb::read_control(p)
                    .with_context(|| format!("reading {}", p.display()))?;
                out.push(Entry {
                    name: control.package()?.to_string(),
                    version: control.version()?.to_string(),
                    arch: control.architecture()?.to_string(),
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

/// Keep the `keep` most recently added files per package; prune older ones.
/// Returns the pruned entries (deleted unless `dry_run`). Retention is by
/// recency (mtime); semver-aware ordering is a planned enhancement. Files pinned
/// by a retained rollback state are never pruned.
pub fn gc(root: &Path, keep: usize, apt: bool, yum: bool, dry_run: bool) -> Result<GcReport> {
    use std::collections::BTreeMap;

    let referenced = referenced_apt_files(root);
    let apt_root = root.join("apt");

    let mut groups: BTreeMap<(Kind, String, String, String), Vec<Entry>> = BTreeMap::new();
    for e in list(root, apt, yum)? {
        groups.entry(e.group_key()).or_default().push(e);
    }

    let mut pruned = Vec::new();
    let mut retained_for_rollback = 0usize;
    for (_, mut versions) in groups {
        if versions.len() <= keep {
            continue;
        }
        versions.sort_by_key(|e| std::cmp::Reverse(e.mtime));
        for e in versions.into_iter().skip(keep) {
            // Keep files a retained rollback state still points at.
            if e.kind == Kind::Apt {
                if let Ok(rel) = e.path.strip_prefix(&apt_root) {
                    if referenced.contains(rel.to_string_lossy().as_ref()) {
                        retained_for_rollback += 1;
                        continue;
                    }
                }
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
    })
}
