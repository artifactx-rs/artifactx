//! Repository mirroring: keep a local pool in sync with an upstream apt/yum
//! repo — incremental fetch, package diff, optional auto-publish.
//! Builds on the import infrastructure (ADR: mirror = import + diff).

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Mirror state persisted between syncs: maps upstream filename → (mtime, size, sha256).
/// After each sync, updated to reflect the current upstream state.
#[derive(Debug, Default, Serialize, Deserialize)]
struct MirrorState {
    files: std::collections::HashMap<String, FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
    size: u64,
    sha256: String,
}

/// Sync apt packages from upstream. Returns (downloaded, removed, total).
pub fn mirror_apt(
    root: &Path, cfg: &Config, base_url: &str,
    dist: &str, component: &str, arch: &str,
    prune: bool,
) -> Result<(usize, usize, usize)> {
    let base = base_url.trim_end_matches('/');
    let packages_gz = format!("{base}/dists/{dist}/{component}/binary-{arch}/Packages.gz");
    let packages_plain = format!("{base}/dists/{dist}/{component}/binary-{arch}/Packages");
    let client = reqwest::blocking::Client::new();

    let text = match client.get(&packages_gz).send() {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.bytes().context("reading Packages.gz")?;
            let xml = createrepo_rs::compression::gzip_decompress(&body)
                .context("decompressing")?;
            String::from_utf8_lossy(&xml).into_owned()
        }
        _ => client.get(&packages_plain).send()
            .context("fetching Packages")?.error_for_status()?.text()?,
    };

    // Parse upstream Packages index: split by paragraph (blank line separator),
    // extract Filename + Size + SHA256 from each stanza.
    let mut upstream: Vec<(String, u64, String)> = Vec::new();
    for stanza in text.split("\n\n") {
        let mut file = String::new();
        let mut size: u64 = 0;
        let mut sha = String::new();
        for line in stanza.lines() {
            if let Some(f) = line.strip_prefix("Filename: ") { file = f.trim().to_string(); }
            if let Some(s) = line.strip_prefix("Size: ") { size = s.trim().parse().unwrap_or(0); }
            if let Some(s) = line.strip_prefix("SHA256: ") { sha = s.trim().to_string(); }
        }
        if !file.is_empty() && !sha.is_empty() {
            upstream.push((file, size, sha));
        }
    }

    // Load local state
    let pool_dir = cfg.apt_pool_root(root).join(component);
    std::fs::create_dir_all(&pool_dir)?;
    let state_path = pool_dir.join(".arx-mirror.toml");
    let state: MirrorState = if state_path.exists() {
        let s = std::fs::read_to_string(&state_path).context("reading mirror state")?;
        toml::from_str(&s).unwrap_or_default()
    } else {
        MirrorState::default()
    };

    let mut downloaded = 0usize;
    let mut removed = 0usize;
    let mut next = MirrorState::default();
    let mut upstream_names: HashSet<String> = HashSet::new();

    for (filename, size, sha256) in &upstream {
        let name = Path::new(filename).file_name()
            .and_then(|n| n.to_str()).unwrap_or(filename);
        upstream_names.insert(name.to_string());
        next.files.insert(name.to_string(), FileEntry { size: *size, sha256: sha256.clone() });

        let dest = pool_dir.join(name);
        let need_download = match state.files.get(name) {
            Some(e) => e.sha256 != *sha256 || e.size != *size,
            None => true,
        };
        if need_download && !dest.exists() {
            let url = format!("{base}/{filename}");
            let resp = client.get(&url).send()
                .with_context(|| format!("downloading {url}"))?;
            let body = resp.bytes()?;
            std::fs::write(&dest, &body)?;
            println!("synced {name}");
            downloaded += 1;
        } else if !need_download {
            // Already current — nothing to do.
        }
    }

    // Prune: remove local files not in upstream.
    if prune {
        for entry in std::fs::read_dir(&pool_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".arx-") { continue; }
            if !upstream_names.contains(&name) && entry.path().is_file() {
                std::fs::remove_file(entry.path())?;
                println!("pruned {name}");
                removed += 1;
            }
        }
    }

    // Save new mirror state
    let toml = toml::to_string_pretty(&next).context("serializing mirror state")?;
    std::fs::write(&state_path, toml).context("writing mirror state")?;

    Ok((downloaded, removed, upstream.len()))
}
