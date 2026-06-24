//! `arx-pack` — a pure-Rust packager that builds `.deb`, `.rpm`, and `.apk`
//! artifacts from a single TOML manifest, with **no native toolchain required** for the common
//! case (no `dpkg-deb`, no `rpmbuild`, no container runtime).
//!
//! # Why
//!
//! Most packaging needs are "take these files, put them at these paths, attach
//! this metadata". That doesn't require a foreign toolchain — it requires
//! correctly assembling two well-specified archive formats. `arx-pack` does exactly
//! that in pure Rust, so the same code runs on a laptop and in CI, fast and
//! dependency-light.
//!
//! See [`backend`] for the native-first / Docker-fallback philosophy and build
//! hygiene guarantees.
//!
//! # Example
//!
//! ```no_run
//! use arx_pack::Manifest;
//! use std::path::Path;
//!
//! let manifest = Manifest::from_toml_str(r#"
//!     name = "hello"
//!     version = "1.0.0"
//!     arch = "amd64"
//!     maintainer = "Jane Dev <jane@example.com>"
//!     description = "A friendly greeter"
//!     license = "MIT"
//!
//!     [[files]]
//!     source = "build/hello"
//!     dest = "/usr/bin/hello"
//!     mode = "0755"
//! "#).unwrap();
//!
//! let deb = arx_pack::build_deb(&manifest, Path::new("dist")).unwrap();
//! let rpm = arx_pack::build_rpm(&manifest, Path::new("dist")).unwrap();
//! let apk = arx_pack::build_apk(&manifest, Path::new("dist")).unwrap();
//! ```

mod apk;
mod backend;
mod deb;
mod manifest;
mod rpm;

pub use apk::build_apk;
pub use backend::{Backend, Format};
pub use deb::build_deb;
pub use manifest::{CargoManifestOptions, DirEntry, FileEntry, Manifest, Scripts};
pub use rpm::build_rpm;

/// Resolve the reproducibility epoch for deterministic builds.
///
/// Read the `SOURCE_DATE_EPOCH` env var (the reproducible-builds standard); if
/// unset or unparseable, default to `0`. Both `.deb` (tar/ar/gzip mtimes) and
/// `.rpm` (`source_date` → BUILDTIME + payload mtime + signature timestamp) feed
/// from this single value so the two formats share one deterministic clock.
pub fn resolve_source_epoch() -> u32 {
    if let Ok(val) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(epoch) = val.trim().parse::<u32>() {
            return epoch;
        }
    }
    0
}

/// Validate that every source is safe to stage, rejecting symlinks, devices,
/// FIFOs, and duplicate destinations before any builder reads payload content.
/// This is the shared pre-staging gate (ADR-0012 §2, ADR-0018). Uses
/// `symlink_metadata` so symlinks are distinguishable and never followed.
pub fn validate_sources(manifest: &Manifest) -> Result<(), anyhow::Error> {
    expand_payload(manifest).map(|_| ())
}

#[derive(Debug)]
pub(crate) struct ExpandedPayload {
    pub files: Vec<ExpandedFile>,
    pub dirs: Vec<ExpandedDir>,
}

#[derive(Debug)]
pub(crate) struct ExpandedFile {
    pub source: String,
    pub rel: String,
    pub mode: u32,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct ExpandedDir {
    pub rel: String,
    pub mode: u32,
}

pub(crate) fn expand_payload(manifest: &Manifest) -> Result<ExpandedPayload, anyhow::Error> {
    use anyhow::{anyhow, bail, Context};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    let mut files = Vec::new();
    let mut dirs: BTreeMap<String, u32> = BTreeMap::new();
    let mut seen_files = BTreeSet::new();

    for entry in &manifest.files {
        validate_regular_source(&entry.source)?;
        let rel = dest_to_rel(&entry.dest, "file")?;
        if !seen_files.insert(rel.clone()) {
            bail!("duplicate package destination {:?}", entry.dest);
        }
        insert_parent_dirs(&mut dirs, &rel, 0o755);
        let data = std::fs::read(&entry.source)
            .with_context(|| format!("reading source file {}", entry.source))?;
        files.push(ExpandedFile {
            source: entry.source.clone(),
            rel,
            mode: entry.mode_bits()?,
            data,
        });
    }

    for entry in &manifest.dirs {
        let source = Path::new(&entry.source);
        validate_directory_source(source, &entry.source)?;
        let dest_rel = dest_to_rel(&entry.dest, "directory")?;
        let file_mode = entry.file_mode_bits()?;
        let dir_mode = entry.dir_mode_bits()?;
        dirs.entry(with_trailing_slash(dest_rel.clone()))
            .or_insert(dir_mode);

        let mut stack = vec![source.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let mut children = std::fs::read_dir(&dir)
                .with_context(|| format!("reading source directory {}", dir.display()))?
                .collect::<Result<Vec<_>, _>>()
                .with_context(|| format!("reading entries in {}", dir.display()))?;
            children.sort_by_key(|child| child.path());

            for child in children {
                let path = child.path();
                let meta = std::fs::symlink_metadata(&path)
                    .with_context(|| format!("stat-ing directory entry {}", path.display()))?;
                let ft = meta.file_type();
                if ft.is_symlink() {
                    bail!(
                        "source {:?} is a symbolic link — symlink sources are not supported in [[dirs]]",
                        path
                    );
                }
                let relative = path
                    .strip_prefix(source)
                    .with_context(|| format!("computing relative path for {}", path.display()))?;
                let rel_suffix = path_to_slash(relative)?;
                let package_rel = join_rel(&dest_rel, &rel_suffix);
                if ft.is_dir() {
                    dirs.entry(with_trailing_slash(package_rel))
                        .or_insert(dir_mode);
                    stack.push(path);
                } else if ft.is_file() {
                    if !seen_files.insert(package_rel.clone()) {
                        bail!("duplicate package destination {:?}", package_rel);
                    }
                    insert_parent_dirs(&mut dirs, &package_rel, dir_mode);
                    let data = std::fs::read(&path)
                        .with_context(|| format!("reading source file {}", path.display()))?;
                    files.push(ExpandedFile {
                        source: path.to_string_lossy().into_owned(),
                        rel: package_rel,
                        mode: file_mode,
                        data,
                    });
                } else {
                    bail!(
                        "source {:?} is not a regular file or directory (type: {:?})",
                        path,
                        ft
                    );
                }
            }
        }
    }

    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let dirs = dirs
        .into_iter()
        .map(|(rel, mode)| ExpandedDir { rel, mode })
        .collect();
    let payload = ExpandedPayload { files, dirs };

    fn validate_regular_source(source: &str) -> Result<(), anyhow::Error> {
        use anyhow::{bail, Context};
        let meta = std::fs::symlink_metadata(source)
            .with_context(|| format!("stat-ing source file {source}"))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            bail!(
                "source {:?} is a symbolic link, not a regular file — \
                 symlink sources are not supported (copy the target content or use a regular file)",
                source
            );
        }
        if ft.is_dir() {
            bail!("source {:?} is a directory, not a regular file", source);
        }
        if !ft.is_file() {
            bail!("source {:?} is not a regular file (type: {:?})", source, ft);
        }
        Ok(())
    }

    fn validate_directory_source(path: &Path, source: &str) -> Result<(), anyhow::Error> {
        let meta = std::fs::symlink_metadata(path)
            .with_context(|| format!("stat-ing source directory {source}"))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            bail!("source {:?} is a symbolic link, not a directory", source);
        }
        if !ft.is_dir() {
            bail!("source {:?} is not a directory", source);
        }
        Ok(())
    }

    fn dest_to_rel(dest: &str, label: &str) -> Result<String, anyhow::Error> {
        let rel = dest.strip_prefix('/').unwrap_or(dest).trim_matches('/');
        if rel.is_empty() {
            bail!("{label} dest {:?} resolves to an empty path", dest);
        }
        if rel
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            bail!("{label} dest {:?} contains an invalid path component", dest);
        }
        Ok(rel.to_string())
    }

    fn insert_parent_dirs(dirs: &mut BTreeMap<String, u32>, rel: &str, mode: u32) {
        let mut accum = String::new();
        let parts: Vec<&str> = rel.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            accum.push_str(part);
            accum.push('/');
            dirs.entry(accum.clone()).or_insert(mode);
        }
    }

    fn path_to_slash(path: &Path) -> Result<String, anyhow::Error> {
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::Normal(part) => parts.push(
                    part.to_str()
                        .ok_or_else(|| anyhow!("directory entry path is not UTF-8: {:?}", path))?
                        .to_string(),
                ),
                _ => bail!(
                    "directory entry path has an unsupported component: {:?}",
                    path
                ),
            }
        }
        Ok(parts.join("/"))
    }

    fn join_rel(base: &str, suffix: &str) -> String {
        if suffix.is_empty() {
            base.to_string()
        } else {
            format!("{base}/{suffix}")
        }
    }

    fn with_trailing_slash(mut rel: String) -> String {
        if !rel.ends_with('/') {
            rel.push('/');
        }
        rel
    }

    Ok(payload)
}
