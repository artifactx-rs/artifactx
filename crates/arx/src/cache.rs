//! Persistent repository cache v2.
//!
//! The cache is an acceleration structure only: package files and generated
//! repository metadata remain authoritative. Missing, corrupt, or stale cache
//! state must degrade to regular filesystem/hash checks rather than changing
//! command behavior.

use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub const CACHE_VERSION: u32 = 2;
const CACHE_DIR: &str = ".arx-cache/v2";
const PACKAGE_CACHE_FILE: &str = "package-files.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageFileCache {
    pub version: u32,
    pub entries: BTreeMap<String, PackageFileEntry>,
}

impl Default for PackageFileCache {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageFileEntry {
    pub source_size: u64,
    pub source_modified_ns: u128,
    #[serde(default)]
    pub source_changed_ns: u128,
    pub dest_size: u64,
    pub dest_modified_ns: u128,
    #[serde(default)]
    pub dest_changed_ns: u128,
    pub content_digest: String,
}

#[derive(Debug, Clone, Copy)]
pub struct FileFingerprint {
    pub size: u64,
    pub modified_ns: u128,
    pub changed_ns: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheDecision {
    Hit,
    Miss,
}

pub fn package_cache_path(root: &Path) -> PathBuf {
    cache_dir(root).join(PACKAGE_CACHE_FILE)
}

pub fn cache_dir(root: &Path) -> PathBuf {
    if let Some(base) = env::var_os("ARX_CACHE_DIR") {
        return PathBuf::from(base).join(root_cache_key(root)).join("v2");
    }
    root.join(CACHE_DIR)
}

impl PackageFileCache {
    pub fn load(root: &Path) -> Self {
        match Self::try_load(root) {
            Ok(cache) if cache.version == CACHE_VERSION => cache,
            Ok(_) => Self::default(),
            Err(err) => {
                tracing::debug!(error = %err, "ignoring package cache");
                Self::default()
            }
        }
    }

    pub fn try_load(root: &Path) -> Result<Self> {
        let path = package_cache_path(root);
        let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = package_cache_path(root);
        let dir = path.parent().expect("cache path has parent");
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(self).context("serializing package cache")?;
        std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;
        Ok(())
    }

    pub fn clear(root: &Path) -> Result<()> {
        let path = package_cache_path(root);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, dest: &Path) -> Option<&PackageFileEntry> {
        self.entries.get(&entry_key(dest))
    }

    pub fn update(
        &mut self,
        source_fp: FileFingerprint,
        dest: &Path,
        dest_fp: FileFingerprint,
        content_digest: String,
    ) {
        self.entries.insert(
            entry_key(dest),
            PackageFileEntry {
                source_size: source_fp.size,
                source_modified_ns: source_fp.modified_ns,
                source_changed_ns: source_fp.changed_ns,
                dest_size: dest_fp.size,
                dest_modified_ns: dest_fp.modified_ns,
                dest_changed_ns: dest_fp.changed_ns,
                content_digest,
            },
        );
    }
}

impl PackageFileEntry {
    pub fn matches_source(&self, fp: FileFingerprint) -> bool {
        self.source_size == fp.size
            && self.source_modified_ns == fp.modified_ns
            && self.source_changed_ns == fp.changed_ns
    }

    pub fn matches_dest(&self, fp: FileFingerprint) -> bool {
        self.dest_size == fp.size
            && self.dest_modified_ns == fp.modified_ns
            && self.dest_changed_ns == fp.changed_ns
    }
}

pub fn fingerprint(path: &Path) -> Result<FileFingerprint> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let modified = meta
        .modified()
        .with_context(|| format!("reading mtime for {}", path.display()))?;
    Ok(FileFingerprint {
        size: meta.len(),
        modified_ns: system_time_ns(modified),
        changed_ns: changed_time_ns(&meta),
    })
}

pub fn content_digest_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("reading {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn rebuild_from_paths_with_jobs(
    root: &Path,
    paths: impl IntoIterator<Item = PathBuf>,
    jobs: usize,
) -> Result<PackageFileCache> {
    let paths: Vec<PathBuf> = paths.into_iter().filter(|path| path.is_file()).collect();
    if paths.is_empty() {
        let cache = PackageFileCache::default();
        cache.save(root)?;
        return Ok(cache);
    }

    let jobs = effective_jobs(jobs).min(paths.len());
    let entries = hash_paths_parallel(&paths, jobs)?;
    let mut cache = PackageFileCache::default();
    for (path, fp, hash) in entries {
        cache.update(fp, &path, fp, hash);
    }
    cache.save(root)?;
    Ok(cache)
}

fn hash_paths_parallel(
    paths: &[PathBuf],
    jobs: usize,
) -> Result<Vec<(PathBuf, FileFingerprint, String)>> {
    let jobs = effective_jobs(jobs);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .context("building cache rebuild thread pool")?;

    pool.install(|| {
        paths
            .par_iter()
            .map(|path| {
                let fp = fingerprint(path)?;
                let digest = content_digest_file(path)?;
                Ok((path.clone(), fp, digest))
            })
            .collect()
    })
}

fn effective_jobs(jobs: usize) -> usize {
    if jobs > 0 {
        return jobs;
    }
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1)
}

pub fn entry_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn root_cache_key(root: &Path) -> String {
    let stable_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let digest = blake3::hash(stable_root.to_string_lossy().as_bytes());
    digest.to_hex()[..32].to_string()
}

fn system_time_ns(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(unix)]
fn changed_time_ns(meta: &std::fs::Metadata) -> u128 {
    (meta.ctime() as u128) * 1_000_000_000 + meta.ctime_nsec() as u128
}

#[cfg(not(unix))]
fn changed_time_ns(_meta: &std::fs::Metadata) -> u128 {
    0
}
