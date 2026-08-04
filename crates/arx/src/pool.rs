//! Pool inspection and maintenance, shared by the CLI (`arx rm`/`arx gc`) and
//! the HTTP API. Functions here **return data**; printing/serialising is the
//! caller's job. Operations touch the pool only — run `arx publish` (or push,
//! which republishes) afterwards to regenerate metadata.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::time::{Instant, SystemTime};

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
        (
            self.kind,
            self.scope.clone(),
            self.name.clone(),
            self.arch.clone(),
        )
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

#[derive(Debug, Default)]
struct PoolScanStats {
    scopes_seen: usize,
    scopes_skipped: usize,
    files_seen: usize,
    files_skipped_by_hint: usize,
    cache_hits: usize,
    cache_misses: usize,
    packages_parsed: usize,
}

#[derive(Debug, Default)]
struct SearchScanReport {
    apt: PoolScanStats,
    yum: PoolScanStats,
}

#[derive(Debug, Clone, Copy)]
struct SearchScanFilter<'a> {
    query: Option<&'a str>,
    name_prefix: Option<&'a str>,
    arch: Option<&'a str>,
    scope: Option<&'a str>,
}

impl<'a> From<&'a SearchOptions<'a>> for SearchScanFilter<'a> {
    fn from(options: &'a SearchOptions<'a>) -> Self {
        Self {
            query: options.query,
            name_prefix: options.name_prefix,
            arch: options.arch,
            scope: options.scope,
        }
    }
}

fn file_name_has_apt_arch(file_name: &str, arch: &str) -> bool {
    file_name
        .strip_suffix(".deb")
        .and_then(|stem| stem.strip_suffix(arch))
        .is_some_and(|prefix| prefix.ends_with('_'))
}

fn file_name_has_rpm_arch(file_name: &str, arch: &str) -> bool {
    file_name
        .strip_suffix(".rpm")
        .and_then(|stem| stem.strip_suffix(arch))
        .is_some_and(|prefix| prefix.ends_with('.'))
}

impl SearchScanFilter<'_> {
    fn matches_scope(self, scope: &str) -> bool {
        self.scope.is_none_or(|wanted| wanted == scope)
    }

    fn matches_name_hint(self, file_name: &str) -> bool {
        self.query.is_none_or(|query| file_name.contains(query))
            && self
                .name_prefix
                .is_none_or(|prefix| file_name.starts_with(prefix))
    }

    fn matches_apt_path_hint(self, file_name: &str) -> bool {
        self.matches_name_hint(file_name)
            && self
                .arch
                .is_none_or(|arch| file_name_has_apt_arch(file_name, arch))
    }

    fn matches_yum_path_hint(self, file_name: &str, arch_scope: Option<&str>) -> bool {
        self.matches_name_hint(file_name)
            && self.arch.is_none_or(|arch| {
                arch_scope == Some(arch) || file_name_has_rpm_arch(file_name, arch)
            })
    }
}

fn stat_mtime_size(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((mtime, meta.len()))
}

fn cached_apt_entry(
    path: &Path,
    scope: &str,
    manifest: &arx_debrepo::manifest::FileManifest,
) -> Option<Entry> {
    let fname = path.file_name()?.to_str()?;
    let (mtime, size) = stat_mtime_size(path)?;
    let cached = manifest.lookup(fname, mtime, size)?;
    if cached.package.is_empty() || cached.version.is_empty() || cached.architecture.is_empty() {
        return None;
    }
    Some(Entry {
        name: cached.package.clone(),
        version: cached.version.clone(),
        arch: cached.architecture.clone(),
        epoch: None,
        release: String::new(),
        scope: scope.to_string(),
        kind: Kind::Apt,
        mtime: mtime_of(path),
        path: path.to_path_buf(),
    })
}

fn cached_yum_entry(
    path: &Path,
    scope: &str,
    manifest: &arx_debrepo::manifest::FileManifest,
) -> Option<Entry> {
    let fname = path.file_name()?.to_str()?;
    let (mtime, size) = stat_mtime_size(path)?;
    let cached = manifest.lookup(fname, mtime, size)?;
    if cached.stanza.is_empty() {
        return None;
    }
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata xmlns="http://linux.duke.edu/metadata/common" xmlns:rpm="http://linux.duke.edu/metadata/rpm" packages="1">{}</metadata>"#,
        cached.stanza
    );
    let mut packages = crate::createrepo_rs::xml::parse::parse_primary_xml(xml.as_bytes()).ok()?;
    let pkg = packages.pop()?;
    Some(Entry {
        name: pkg.name,
        version: pkg.version,
        arch: pkg.arch,
        epoch: pkg.epoch.map(|epoch| epoch.to_string()),
        release: pkg.release,
        scope: scope.to_string(),
        kind: Kind::Yum,
        mtime: mtime_of(path),
        path: path.to_path_buf(),
    })
}

fn mtime_of(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn scan_apt(apt_pool_root: &Path) -> Result<Vec<Entry>> {
    let mut stats = PoolScanStats::default();
    scan_apt_with_filter(apt_pool_root, None, &mut stats)
}

fn scan_apt_with_filter(
    apt_pool_root: &Path,
    filter: Option<SearchScanFilter<'_>>,
    stats: &mut PoolScanStats,
) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    if !apt_pool_root.is_dir() {
        return Ok(out);
    }
    for comp in std::fs::read_dir(apt_pool_root)? {
        let comp = comp?;
        if !comp.path().is_dir() {
            continue;
        }
        let scope = comp.file_name().to_string_lossy().into_owned();
        stats.scopes_seen += 1;
        if filter.is_some_and(|filter| !filter.matches_scope(&scope)) {
            stats.scopes_skipped += 1;
            continue;
        }
        let manifest = arx_debrepo::manifest::FileManifest::load(&comp.path()).unwrap_or_default();
        for entry in walkdir::WalkDir::new(comp.path())
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "deb").unwrap_or(false) {
                stats.files_seen += 1;
                let file_name = p.file_name().and_then(|name| name.to_str()).unwrap_or("");
                if filter.is_some_and(|filter| !filter.matches_apt_path_hint(file_name)) {
                    stats.files_skipped_by_hint += 1;
                    continue;
                }
                if let Some(entry) = cached_apt_entry(p, &scope, &manifest) {
                    stats.cache_hits += 1;
                    out.push(entry);
                    continue;
                }
                stats.cache_misses += 1;
                let control = arx_debrepo::deb::read_control(p)
                    .with_context(|| format!("reading {}", p.display()))?;
                stats.packages_parsed += 1;
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

fn scan_yum(yum_base: &Path) -> Result<Vec<Entry>> {
    let mut stats = PoolScanStats::default();
    scan_yum_with_filter(yum_base, None, &mut stats)
}

fn scan_yum_with_filter(
    yum_base: &Path,
    filter: Option<SearchScanFilter<'_>>,
    stats: &mut PoolScanStats,
) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    if !yum_base.is_dir() {
        return Ok(out);
    }
    let mut manifests: HashMap<PathBuf, arx_debrepo::manifest::FileManifest> = HashMap::new();
    for repo in std::fs::read_dir(yum_base)? {
        let repo = repo?;
        if !repo.path().is_dir() {
            continue;
        }
        let scope = repo.file_name().to_string_lossy().into_owned();
        stats.scopes_seen += 1;
        if filter.is_some_and(|filter| !filter.matches_scope(&scope)) {
            stats.scopes_skipped += 1;
            continue;
        }
        for entry in walkdir::WalkDir::new(repo.path())
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "rpm").unwrap_or(false) {
                stats.files_seen += 1;
                let file_name = p.file_name().and_then(|name| name.to_str()).unwrap_or("");
                let arch_scope = p
                    .parent()
                    .and_then(|parent| parent.file_name())
                    .and_then(|name| name.to_str());
                if filter.is_some_and(|filter| !filter.matches_yum_path_hint(file_name, arch_scope))
                {
                    stats.files_skipped_by_hint += 1;
                    continue;
                }
                if let Some(parent) = p.parent() {
                    let manifest = manifests.entry(parent.to_path_buf()).or_insert_with(|| {
                        arx_debrepo::manifest::FileManifest::load(parent).unwrap_or_default()
                    });
                    if let Some(entry) = cached_yum_entry(p, &scope, manifest) {
                        stats.cache_hits += 1;
                        out.push(entry);
                        continue;
                    }
                }
                stats.cache_misses += 1;
                let mut reader = crate::createrepo_rs::rpm::RpmReader::open(p)
                    .with_context(|| format!("opening {}", p.display()))?;
                let pkg = reader
                    .read_package()
                    .with_context(|| format!("reading {}", p.display()))?;
                stats.packages_parsed += 1;
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
pub fn list(apt_pool_root: &Path, yum_base: &Path, apt: bool, yum: bool) -> Result<Vec<Entry>> {
    let do_apt = apt || !yum;
    let do_yum = yum || !apt;
    let mut entries = Vec::new();
    if do_apt {
        entries.extend(scan_apt(apt_pool_root)?);
    }
    if do_yum {
        entries.extend(scan_yum(yum_base)?);
    }
    Ok(entries)
}

#[derive(Debug, Default)]
pub struct SearchOptions<'a> {
    pub query: Option<&'a str>,
    pub name_prefix: Option<&'a str>,
    pub version: Option<&'a str>,
    pub arch: Option<&'a str>,
    pub scope: Option<&'a str>,
    pub apt: bool,
    pub yum: bool,
}

impl Entry {
    fn matches_search(&self, options: &SearchOptions<'_>) -> bool {
        options.query.is_none_or(|q| self.name.contains(q))
            && options
                .name_prefix
                .is_none_or(|prefix| self.name.starts_with(prefix))
            && options
                .version
                .is_none_or(|version| self.version == version)
            && options.arch.is_none_or(|arch| self.arch == arch)
            && options.scope.is_none_or(|scope| self.scope == scope)
    }
}

pub fn search(
    apt_pool_root: &Path,
    yum_base: &Path,
    options: SearchOptions<'_>,
) -> Result<Vec<Entry>> {
    let start = Instant::now();
    let mut report = SearchScanReport::default();
    let do_apt = options.apt || !options.yum;
    let do_yum = options.yum || !options.apt;
    let filter = SearchScanFilter::from(&options);
    let mut scanned = Vec::new();
    if do_apt {
        let scan_start = Instant::now();
        let apt_entries = scan_apt_with_filter(apt_pool_root, Some(filter), &mut report.apt)?;
        tracing::debug!(
            root = %apt_pool_root.display(),
            elapsed_ms = scan_start.elapsed().as_millis(),
            scopes_seen = report.apt.scopes_seen,
            scopes_skipped = report.apt.scopes_skipped,
            files_seen = report.apt.files_seen,
            files_skipped_by_hint = report.apt.files_skipped_by_hint,
            cache_hits = report.apt.cache_hits,
            cache_misses = report.apt.cache_misses,
            packages_parsed = report.apt.packages_parsed,
            entries = apt_entries.len(),
            "apt search scan completed"
        );
        scanned.extend(apt_entries);
    }
    if do_yum {
        let scan_start = Instant::now();
        let yum_entries = scan_yum_with_filter(yum_base, Some(filter), &mut report.yum)?;
        tracing::debug!(
            root = %yum_base.display(),
            elapsed_ms = scan_start.elapsed().as_millis(),
            scopes_seen = report.yum.scopes_seen,
            scopes_skipped = report.yum.scopes_skipped,
            files_seen = report.yum.files_seen,
            files_skipped_by_hint = report.yum.files_skipped_by_hint,
            cache_hits = report.yum.cache_hits,
            cache_misses = report.yum.cache_misses,
            packages_parsed = report.yum.packages_parsed,
            entries = yum_entries.len(),
            "yum search scan completed"
        );
        scanned.extend(yum_entries);
    }
    let parsed_entries = scanned.len();
    let mut entries: Vec<Entry> = scanned
        .into_iter()
        .filter(|entry| entry.matches_search(&options))
        .collect();
    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis(),
        parsed_entries,
        matched_entries = entries.len(),
        apt_files_seen = report.apt.files_seen,
        apt_files_skipped_by_hint = report.apt.files_skipped_by_hint,
        yum_files_seen = report.yum.files_seen,
        yum_files_skipped_by_hint = report.yum.files_skipped_by_hint,
        apt_cache_hits = report.apt.cache_hits,
        apt_cache_misses = report.apt.cache_misses,
        yum_cache_hits = report.yum.cache_hits,
        yum_cache_misses = report.yum.cache_misses,
        "pool search completed"
    );
    entries.sort_by(|a, b| {
        (
            a.kind,
            a.scope.as_str(),
            a.name.as_str(),
            a.arch.as_str(),
            a.version.as_str(),
        )
            .cmp(&(
                b.kind,
                b.scope.as_str(),
                b.name.as_str(),
                b.arch.as_str(),
                b.version.as_str(),
            ))
    });
    Ok(entries)
}

/// Remove packages matching `name` (and optional exact `version`). Returns the
/// removed entries; does not print or republish.
pub fn remove(
    apt_pool_root: &Path,
    name: &str,
    version: Option<&str>,
    yum_base: &Path,
    apt: bool,
    yum: bool,
) -> Result<Vec<Entry>> {
    let matches: Vec<Entry> = list(apt_pool_root, yum_base, apt, yum)?
        .into_iter()
        .filter(|e| e.name == name && version.is_none_or(|v| e.version == v))
        .collect();
    for e in &matches {
        std::fs::remove_file(&e.path).with_context(|| format!("removing {}", e.path.display()))?;
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
/// (`<apt-base>/dists/.states/**/Packages`). Such files must not be pruned, or a
/// rolled-back index would 404.
fn referenced_apt_files(apt_base: &Path) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let states = apt_base.join("dists/.states");
    if !states.is_dir() {
        return set;
    }
    for entry in walkdir::WalkDir::new(&states)
        .into_iter()
        .filter_map(|e| e.ok())
    {
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
/// (`<yum-base>/<repo>/<arch>/.states/repodata/**/sha256-primary.xml.gz`).
fn referenced_yum_files(yum_base: &Path) -> std::collections::HashSet<PathBuf> {
    let mut set = std::collections::HashSet::new();
    if !yum_base.is_dir() {
        return set;
    }
    for entry in walkdir::WalkDir::new(yum_base)
        .into_iter()
        .filter_map(|e| e.ok())
    {
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
            if let Ok(xml) = crate::createrepo_rs::compression::gzip_decompress(&gz) {
                for href in extract_hrefs(&String::from_utf8_lossy(&xml)) {
                    set.insert(normalize_path(&arch_dir.join(href)));
                }
            }
        }
    }
    set
}

/// Normalize lexical `.`/`..` components without touching the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
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
pub struct GcOptions<'a> {
    pub name: Option<&'a str>,
    pub name_prefix: Option<&'a str>,
    pub keep: usize,
    pub keep_within_days: u32,
    pub grace_days: u32,
    pub apt_pool_root: &'a Path,
    pub apt_base: &'a Path,
    pub yum_base: &'a Path,
    pub apt: bool,
    pub yum: bool,
    pub dry_run: bool,
    pub retain_rollback_states: bool,
}

pub fn gc(options: GcOptions<'_>) -> Result<GcReport> {
    use std::collections::BTreeMap;

    let referenced = if options.retain_rollback_states {
        referenced_apt_files(options.apt_base)
    } else {
        std::collections::HashSet::new()
    };
    let referenced_rpm = if options.retain_rollback_states {
        referenced_yum_files(options.yum_base)
    } else {
        std::collections::HashSet::new()
    };
    let apt_root = options.apt_base;

    let mut groups: BTreeMap<(Kind, String, String, String), Vec<Entry>> = BTreeMap::new();
    for e in list(
        options.apt_pool_root,
        options.yum_base,
        options.apt,
        options.yum,
    )? {
        if options.name.is_some_and(|name| e.name != name) {
            continue;
        }
        if options
            .name_prefix
            .is_some_and(|prefix| !e.name.starts_with(prefix))
        {
            continue;
        }
        groups.entry(e.group_key()).or_default().push(e);
    }

    let keep_within_secs = (options.keep_within_days as u64).saturating_mul(86400);
    let grace_secs = (options.grace_days as u64).saturating_mul(86400);
    let time_cutoff = if options.keep_within_days > 0 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub(keep_within_secs))
            .ok()
    } else {
        None
    };
    let grace_cutoff = if options.grace_days > 0 {
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
        if versions.len() <= options.keep && options.keep_within_days == 0 {
            continue;
        }
        // Newest version first; per-pair fall back to mtime when a version is
        // unparseable. Then keep the first `keep`, prune the rest (the oldest).
        versions.sort_by(|a, b| {
            version_order(a, b)
                .unwrap_or_else(|| a.mtime.cmp(&b.mtime))
                .reverse()
        });
        for e in versions.into_iter().skip(options.keep) {
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
                    .strip_prefix(apt_root)
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
            if !options.dry_run {
                std::fs::remove_file(&e.path)
                    .with_context(|| format!("removing {}", e.path.display()))?;
            }
            pruned.push(e);
        }
    }
    Ok(GcReport {
        pruned,
        dry_run: options.dry_run,
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

    #[test]
    fn yum_rollback_href_is_normalized_before_gc_pinning() {
        let href = Path::new("/repo/yum/myrepo/x86_64/../noarch/shared.rpm");
        assert_eq!(
            normalize_path(href),
            PathBuf::from("/repo/yum/myrepo/noarch/shared.rpm")
        );
    }
}
