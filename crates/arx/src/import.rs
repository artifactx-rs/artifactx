//! Repository import: download packages from an upstream apt/yum repo into
//! the local pool — the migration path from aptly/Nexus/reprepro.

use std::path::Path;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::scope;

/// Import parameters bundled to keep clippy's `too_many_arguments` happy.
pub struct ImportOpts<'a> {
    pub root: &'a Path,
    pub cfg: &'a Config,
    pub base_url: &'a str,
    pub dist: &'a str,
    pub component: &'a str,
    pub arch: &'a str,
    pub match_name: Option<&'a str>,
    pub limit: Option<usize>,
}

#[derive(Debug)]
struct AptPackageEntry {
    package: String,
    filename: String,
    size: Option<u64>,
    sha256: Option<String>,
}

fn parse_apt_package_entries(text: &str, match_name: Option<&str>) -> Vec<AptPackageEntry> {
    let mut entries = Vec::new();
    for paragraph in text.split("\n\n") {
        let mut package = None;
        let mut filename = None;
        let mut size = None;
        let mut sha256 = None;

        for line in paragraph.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }
            if let Some(p) = line.strip_prefix("Package: ") {
                package = Some(p.trim().to_string());
            } else if let Some(f) = line.strip_prefix("Filename: ") {
                filename = Some(f.trim().to_string());
            } else if let Some(s) = line.strip_prefix("Size: ") {
                size = s.trim().parse::<u64>().ok();
            } else if let Some(s) = line.strip_prefix("SHA256: ") {
                sha256 = Some(s.trim().to_ascii_lowercase());
            }
        }

        let Some(package) = package else {
            continue;
        };
        if match_name.is_some_and(|m| !package.starts_with(m)) {
            continue;
        }
        if let Some(filename) = filename {
            entries.push(AptPackageEntry {
                package,
                filename,
                size,
                sha256,
            });
        }
    }
    entries
}

fn apt_package_url(base: &str, filename: &str) -> Result<String> {
    if reqwest::Url::parse(filename).is_ok() {
        return Ok(filename.to_string());
    }
    let base = format!("{}/", base.trim_end_matches('/'));
    let url = reqwest::Url::parse(&base)
        .with_context(|| format!("parsing upstream base URL {base}"))?
        .join(filename.trim_start_matches('/'))
        .with_context(|| format!("resolving package filename {filename}"))?;
    Ok(url.to_string())
}

/// Import packages from an upstream apt repository.
pub fn import_apt(opts: &ImportOpts) -> Result<usize> {
    let ImportOpts {
        root,
        cfg,
        base_url,
        dist,
        component,
        arch,
        match_name,
        limit,
    } = *opts;
    let dist = scope::validate_scope_name(dist, "apt dist")?;
    let component = scope::validate_scope_name(component, "apt component")?;
    let arch = scope::validate_scope_name(arch, "apt arch")?;
    let base = base_url.trim_end_matches('/');
    let packages_gz = format!("{base}/dists/{dist}/{component}/binary-{arch}/Packages.gz");
    let packages_plain = format!("{base}/dists/{dist}/{component}/binary-{arch}/Packages");
    let client = reqwest::blocking::Client::new();

    let text = match client.get(&packages_gz).send() {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.bytes().context("reading Packages.gz")?;
            let xml = createrepo_rs::compression::gzip_decompress(&body)
                .context("decompressing Packages.gz")?;
            String::from_utf8_lossy(&xml).into_owned()
        }
        _ => client
            .get(&packages_plain)
            .send()
            .with_context(|| format!("fetching {packages_plain}"))?
            .error_for_status()
            .context("upstream returned error")?
            .text()
            .context("reading Packages")?,
    };

    let entries = parse_apt_package_entries(&text, match_name);
    if entries.is_empty() {
        bail!("no packages found");
    }

    let dir = cfg.checked_apt_pool_root(root)?.join(component);
    std::fs::create_dir_all(&dir)?;
    let mut imported = 0usize;
    for entry in &entries {
        if let Some(n) = limit {
            if imported >= n {
                break;
            }
        }
        let url = apt_package_url(base, &entry.filename)?;
        let name = Path::new(&entry.filename)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| entry.filename.clone());
        let dest = dir.join(&name);
        if dest.exists() {
            tracing::info!(%name, "already exists, skipping");
            continue;
        }
        let resp = client
            .get(&url)
            .send()
            .with_context(|| format!("downloading {url}"))?
            .error_for_status()?;
        let body = resp.bytes().context("reading body")?;
        if let Some(expected_size) = entry.size {
            if body.len() as u64 != expected_size {
                bail!(
                    "{}: size mismatch for package {} (expected {}, got {})",
                    entry.filename,
                    entry.package,
                    expected_size,
                    body.len()
                );
            }
        }
        if let Some(expected_sha256) = &entry.sha256 {
            let actual = hex::encode(Sha256::digest(&body));
            if actual != *expected_sha256 {
                bail!(
                    "{}: sha256 mismatch for package {} (expected {}, got {})",
                    entry.filename,
                    entry.package,
                    expected_sha256,
                    actual
                );
            }
        }
        std::fs::write(&dest, &body).with_context(|| format!("writing {}", dest.display()))?;
        println!("imported {name}");
        imported += 1;
    }
    Ok(imported)
}

/// Import packages from an upstream yum/dnf repository.
pub fn import_yum(
    root: &Path,
    cfg: &Config,
    base_url: &str,
    repo: &str,
    limit: Option<usize>,
) -> Result<usize> {
    let repo = scope::validate_scope_name(repo, "yum repo")?;
    let base = base_url.trim_end_matches('/');
    let repomd_url = format!("{base}/repodata/repomd.xml");
    let client = reqwest::blocking::Client::new();
    let xml = client
        .get(&repomd_url)
        .send()
        .with_context(|| format!("fetching {repomd_url}"))?
        .error_for_status()?
        .text()
        .context("reading repomd.xml")?;

    let primary_location = xml
        .lines()
        .find_map(|line| {
            let s = line.trim();
            if s.contains("primary") && s.contains("location") {
                s.split('"').nth(1).map(str::to_string)
            } else {
                None
            }
        })
        .context("could not find primary.xml location in repomd.xml")?;

    let primary_url = if primary_location.starts_with("http") {
        primary_location
    } else {
        format!("{base}/{primary_location}")
    };
    let gz = client
        .get(&primary_url)
        .send()
        .context("fetching primary.xml.gz")?
        .error_for_status()?
        .bytes()
        .context("reading primary.xml.gz")?;
    let xml =
        createrepo_rs::compression::gzip_decompress(&gz).context("decompressing primary.xml.gz")?;
    let text = String::from_utf8_lossy(&xml);
    let hrefs: Vec<String> = text
        .split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next().map(|v| v.to_string()))
        .collect();
    if hrefs.is_empty() {
        bail!("no packages found");
    }

    let dir = cfg.checked_yum_base(root)?.join(repo);
    std::fs::create_dir_all(&dir)?;
    let mut imported = 0usize;
    for href in &hrefs {
        if let Some(n) = limit {
            if imported >= n {
                break;
            }
        }
        let url = if href.starts_with("http") {
            href.clone()
        } else {
            format!("{base}/{href}")
        };
        let name = Path::new(href)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| href.clone());
        let dest = dir.join(&name);
        if dest.exists() {
            tracing::info!(%name, "already exists, skipping");
            continue;
        }
        let resp = client
            .get(&url)
            .send()
            .context("downloading rpm")?
            .error_for_status()?;
        let body = resp.bytes().context("reading rpm body")?;
        std::fs::write(&dest, &body)?;
        println!("imported {name}");
        imported += 1;
    }
    Ok(imported)
}
