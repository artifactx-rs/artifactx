//! Compatibility exports for production cutovers.
//!
//! ArtifactX's native yum layout is `<base>/<repo>/<arch>/...`, while many
//! existing yum repos expose a flat public directory (`/repo/*.rpm` +
//! `/repo/repodata`). This module builds immutable export directories that keep
//! those legacy public contracts stable without changing the internal pool.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use pgp::composed::SignedSecretKey;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::{scope, yum};

#[derive(Debug, Clone)]
pub struct YumExportReport {
    pub path: PathBuf,
    pub copied_rpms: usize,
    pub indexed_rpms: usize,
    pub arches: Vec<String>,
}

pub fn export_apt(root: &Path, cfg: &Config, out: &Path) -> Result<PathBuf> {
    let apt_root = root.join("apt");
    let dists = apt_root.join("dists");
    let pool = cfg.checked_apt_pool_root(root)?;
    if !dists.is_dir() {
        bail!(
            "{} does not exist; run `arx publish --apt` first",
            dists.display()
        );
    }
    if !pool.is_dir() {
        bail!(
            "{} does not exist; add/import .deb packages first",
            pool.display()
        );
    }

    let staging = staging_path(out);
    prepare_staging(out, &staging)?;
    let result = (|| -> Result<()> {
        copy_tree(&dists, &staging.join("dists"))?;
        let pool_name = Path::new(&cfg.apt.pool_dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("pool");
        copy_tree(&pool, &staging.join(pool_name))?;
        Ok(())
    })();
    commit_staging(result, &staging, out)?;
    Ok(out.to_path_buf())
}

pub fn export_yum_flat(
    root: &Path,
    cfg: &Config,
    out: &Path,
    repo: &str,
    arch_filters: &[String],
    key: Option<&SignedSecretKey>,
    passphrase: &str,
) -> Result<YumExportReport> {
    let repo = scope::validate_scope_name(repo, "yum repo")?;
    let yum_base = cfg.checked_yum_base(root)?;
    let repo_root = yum_base.join(repo);
    if !repo_root.is_dir() {
        bail!(
            "{} does not exist; add/import .rpm packages first",
            repo_root.display()
        );
    }

    let staging = staging_path(out);
    prepare_staging(out, &staging)?;
    let result = (|| -> Result<YumExportReport> {
        let mut copied = 0usize;
        let mut arches = Vec::new();
        for arch_dir in selected_arch_dirs(&repo_root, arch_filters)? {
            let arch = arch_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            arches.push(arch);
            for rpm in rpms_directly_under(&arch_dir)? {
                copy_flat_rpm(&rpm, &staging)?;
                copied += 1;
            }
        }
        let indexed = yum::build_repodata(&staging, key, passphrase, false)?;
        materialize_symlink_dir(&staging.join("repodata"))?;
        remove_if_exists(&staging.join(".states"))?;
        remove_if_exists(&staging.join(".arx-manifest.toml"))?;
        Ok(YumExportReport {
            path: out.to_path_buf(),
            copied_rpms: copied,
            indexed_rpms: indexed,
            arches,
        })
    })();
    let report = commit_staging(result, &staging, out)?;
    Ok(report)
}

fn selected_arch_dirs(repo_root: &Path, arch_filters: &[String]) -> Result<Vec<PathBuf>> {
    let filters: BTreeSet<String> = arch_filters
        .iter()
        .map(|a| scope::validate_scope_name(a, "yum arch").map_err(anyhow::Error::from))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(str::to_string)
        .collect();

    let mut dirs = Vec::new();
    for entry in
        std::fs::read_dir(repo_root).with_context(|| format!("reading {}", repo_root.display()))?
    {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let Some(arch) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if filters.is_empty() || filters.contains(arch) {
            dirs.push(path);
        }
    }
    dirs.sort();
    if !filters.is_empty() && dirs.len() != filters.len() {
        let found: BTreeSet<String> = dirs
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
            .collect();
        let missing: Vec<_> = filters.difference(&found).cloned().collect();
        bail!(
            "missing yum arch dir(s) under {}: {}",
            repo_root.display(),
            missing.join(", ")
        );
    }
    Ok(dirs)
}

fn rpms_directly_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut rpms = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rpm") {
            rpms.push(path);
        }
    }
    rpms.sort();
    Ok(rpms)
}

fn copy_flat_rpm(src: &Path, dest_dir: &Path) -> Result<()> {
    let name = src
        .file_name()
        .with_context(|| format!("{} has no file name", src.display()))?;
    let dest = dest_dir.join(name);
    if dest.exists() {
        if sha256_file(src)? == sha256_file(&dest)? {
            return Ok(());
        }
        bail!(
            "flat yum export filename collision: {} and {} differ",
            src.display(),
            dest.display()
        );
    }
    std::fs::copy(src, &dest)
        .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn staging_path(out: &Path) -> PathBuf {
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    let name = out.file_name().and_then(|n| n.to_str()).unwrap_or("export");
    parent.join(format!(".{name}.staging-{}", std::process::id()))
}

fn prepare_staging(out: &Path, staging: &Path) -> Result<()> {
    if out.exists() {
        bail!(
            "{} already exists; choose a fresh versioned export path",
            out.display()
        );
    }
    if staging.exists() {
        std::fs::remove_dir_all(staging)
            .with_context(|| format!("removing stale {}", staging.display()))?;
    }
    std::fs::create_dir_all(staging).with_context(|| format!("creating {}", staging.display()))?;
    Ok(())
}

fn commit_staging<T>(result: Result<T>, staging: &Path, out: &Path) -> Result<T> {
    match result {
        Ok(value) => {
            std::fs::rename(staging, out)
                .with_context(|| format!("renaming {} to {}", staging.display(), out.display()))?;
            Ok(value)
        }
        Err(err) => {
            if staging.exists() {
                let _ = std::fs::remove_dir_all(staging);
            }
            Err(err)
        }
    }
}

fn copy_tree(src: &Path, dest: &Path) -> Result<()> {
    if should_skip_export_entry(src) {
        return Ok(());
    }
    if src.is_dir() {
        std::fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
        for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_tree(&child_src, &child_dest)?;
        }
        Ok(())
    } else if src.is_file() {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::copy(src, dest)
            .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
        Ok(())
    } else {
        // Follow symlinks for repo state pointers; ignore broken/non-file entries.
        let meta = std::fs::metadata(src).with_context(|| format!("stat {}", src.display()))?;
        if meta.is_dir() {
            copy_tree(src, dest)
        } else if meta.is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::copy(src, dest)
                .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
            Ok(())
        } else {
            Ok(())
        }
    }
}

fn should_skip_export_entry(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name == ".states" || name == ".arx-manifest.toml" || name.ends_with(".staging")
}

fn materialize_symlink_dir(path: &Path) -> Result<()> {
    let meta =
        std::fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if !meta.file_type().is_symlink() {
        return Ok(());
    }
    let tmp = path.with_file_name(format!(
        ".{}.materialized-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repodata"),
        std::process::id()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).with_context(|| format!("removing {}", tmp.display()))?;
    }
    copy_tree(path, &tmp)?;
    std::fs::remove_file(path).with_context(|| format!("removing symlink {}", path.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            std::fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))?
        }
        Ok(_) => {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("stat {}", path.display())),
    }
    Ok(())
}
