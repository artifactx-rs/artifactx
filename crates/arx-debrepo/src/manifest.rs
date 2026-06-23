//! Incremental-publish file manifest: detect unchanged packages by (mtime, size)
//! so a no-op publish skips re-reading every `.deb` body (ADR-0013).
//!
//! After a successful publish we write a TOML file (`<pool-component-dir>/.arx-manifest.toml`)
//! mapping filename → {mtime, size, sha256, stanza}. On the next publish, if a
//! file's (mtime, size) still match, we reuse the cached stanza + sha256 and
//! never open the file — O(changes + scan) instead of O(repo).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const MANIFEST_FILE: &str = ".arx-manifest.toml";

/// One cached package entry in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPackage {
    pub mtime: u64,
    pub size: u64,
    pub sha256: String,
    /// Pre-built Packages stanza (control fields + Filename/Size/MD5sum/SHA1/SHA256).
    pub stanza: String,
    /// Cached Debian package identity. Older manifests and yum manifests leave
    /// these empty, which makes the publisher fall back to parsing control.tar.
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub architecture: String,
    /// Pre-built Contents-* lines for this package. Empty means no installed
    /// files, or an older manifest that should use the safe fallback path.
    #[serde(default)]
    pub contents: String,
}

/// In-memory manifest for one pool component (or one yum arch dir).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileManifest {
    #[serde(flatten)]
    pub files: HashMap<String, CachedPackage>,
}

impl FileManifest {
    /// Load the manifest from a directory, or return an empty one if no manifest
    /// exists yet (first publish, or `--full` deleted it).
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(MANIFEST_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Save the manifest to a directory.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serialising file manifest")?;
        let path = dir.join(MANIFEST_FILE);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Look up a file by its current on-disk (mtime, size). Returns `Some` with
    /// the cached entry when both match; `None` if the file changed or is new.
    pub fn lookup(&self, filename: &str, mtime: u64, size: u64) -> Option<&CachedPackage> {
        self.files
            .get(filename)
            .filter(|c| c.mtime == mtime && c.size == size)
    }

    /// Insert or replace a cache entry.
    pub fn insert(&mut self, filename: String, cached: CachedPackage) {
        self.files.insert(filename, cached);
    }

    /// Remove entries whose filename is NOT in `keep`. Call after publish so
    /// deleted packages don't leave stale entries.
    pub fn retain(&mut self, keep: &std::collections::HashSet<String>) {
        self.files.retain(|k, _| keep.contains(k));
    }
}
