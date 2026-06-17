//! Pool maintenance: remove packages (`arx rm`) and prune old ones (`arx gc`).
//!
//! These operate on the **pool only** (the source of truth). After changing the
//! pool, run `arx publish` to regenerate metadata — keeping the two steps
//! explicit and predictable, the way `aptly` separates repo edits from publish.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

/// Which repository format a pool entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    /// Retention grouping key: same package across versions.
    fn group_key(&self) -> (Kind, String, String, String) {
        (self.kind, self.scope.clone(), self.name.clone(), self.arch.clone())
    }
}

fn mtime_of(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Scan `apt/pool/<component>/*.deb`.
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

/// Scan `yum/<repo>/<arch>/*.rpm`.
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

/// Scan the pool(s) selected by `apt`/`yum` (both when neither is set).
fn scan(root: &Path, apt: bool, yum: bool) -> Result<Vec<Entry>> {
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

/// Remove packages matching `name` (and optional exact `version`) from the pool.
pub fn remove(
    root: &Path,
    name: &str,
    version: Option<&str>,
    apt: bool,
    yum: bool,
) -> Result<usize> {
    let matches: Vec<Entry> = scan(root, apt, yum)?
        .into_iter()
        .filter(|e| e.name == name && version.is_none_or(|v| e.version == v))
        .collect();

    if matches.is_empty() {
        println!("No packages matched {name}{}.", version.map(|v| format!(" {v}")).unwrap_or_default());
        return Ok(0);
    }
    for e in &matches {
        std::fs::remove_file(&e.path)
            .with_context(|| format!("removing {}", e.path.display()))?;
        println!("Removed {} {} ({})", e.name, e.version, e.path.display());
    }
    println!(
        "\nRemoved {} file(s). Run `arx publish` to update repository metadata.",
        matches.len()
    );
    Ok(matches.len())
}

/// Keep the `keep` most recently added files per package (name+arch+scope),
/// deleting older ones. `dry_run` reports without deleting.
///
/// Retention is by recency (file mtime), not semantic version ordering — simple
/// and safe; semver-aware retention is a planned enhancement.
pub fn gc(root: &Path, keep: usize, apt: bool, yum: bool, dry_run: bool) -> Result<usize> {
    use std::collections::BTreeMap;

    let entries = scan(root, apt, yum)?;
    let mut groups: BTreeMap<(Kind, String, String, String), Vec<Entry>> = BTreeMap::new();
    for e in entries {
        groups.entry(e.group_key()).or_default().push(e);
    }

    let mut removed = 0usize;
    for (_, mut versions) in groups {
        if versions.len() <= keep {
            continue;
        }
        // Newest first; keep the first `keep`.
        versions.sort_by_key(|e| std::cmp::Reverse(e.mtime));
        for e in versions.into_iter().skip(keep) {
            if dry_run {
                println!("[dry-run] would prune {} {} ({})", e.name, e.version, e.path.display());
            } else {
                std::fs::remove_file(&e.path)
                    .with_context(|| format!("removing {}", e.path.display()))?;
                println!("Pruned {} {} ({})", e.name, e.version, e.path.display());
            }
            removed += 1;
        }
    }

    if removed == 0 {
        println!("Nothing to prune (every package has <= {keep} version(s)).");
    } else if dry_run {
        println!("\n[dry-run] {removed} file(s) would be pruned.");
    } else {
        println!("\nPruned {removed} file(s). Run `arx publish` to update repository metadata.");
    }
    Ok(removed)
}
