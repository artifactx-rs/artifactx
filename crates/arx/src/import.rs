//! Repository import: download packages from an upstream apt/yum repo into
//! the local pool — the migration path from aptly/Nexus/reprepro.

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::config::Config;

/// Import packages from an upstream apt repository.
pub fn import_apt(
    root: &Path, cfg: &Config, base_url: &str,
    dist: &str, component: &str, arch: &str,
    limit: Option<usize>,
) -> Result<usize> {
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
        _ => client.get(&packages_plain).send()
            .with_context(|| format!("fetching {packages_plain}"))?
            .error_for_status().context("upstream returned error")?
            .text().context("reading Packages")?,
    };

    let filenames: Vec<String> = text.lines()
        .filter_map(|l| l.strip_prefix("Filename: "))
        .map(|v| v.trim().to_string())
        .collect();
    if filenames.is_empty() { bail!("no packages found"); }

    let dir = cfg.apt_pool_root(root).join(component);
    std::fs::create_dir_all(&dir)?;
    let mut imported = 0usize;
    for file in &filenames {
        if let Some(n) = limit { if imported >= n { break; } }
        let url = format!("{base}/{file}");
        let name = Path::new(file).file_name()
            .map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| file.clone());
        let dest = dir.join(&name);
        if dest.exists() { tracing::info!(%name, "already exists, skipping"); continue; }
        let resp = client.get(&url).send()
            .with_context(|| format!("downloading {url}"))?.error_for_status()?;
        let body = resp.bytes().context("reading body")?;
        std::fs::write(&dest, &body)
            .with_context(|| format!("writing {}", dest.display()))?;
        println!("imported {name}");
        imported += 1;
    }
    Ok(imported)
}

/// Import packages from an upstream yum/dnf repository.
pub fn import_yum(
    root: &Path, cfg: &Config, base_url: &str, repo: &str,
    limit: Option<usize>,
) -> Result<usize> {
    let base = base_url.trim_end_matches('/');
    let repomd_url = format!("{base}/repodata/repomd.xml");
    let client = reqwest::blocking::Client::new();
    let xml = client.get(&repomd_url).send()
        .with_context(|| format!("fetching {repomd_url}"))?
        .error_for_status()?.text().context("reading repomd.xml")?;

    let primary_location = xml.lines().find_map(|line| {
        let s = line.trim();
        if s.contains("primary") && s.contains("location") {
            s.split('"').nth(1).map(str::to_string)
        } else { None }
    }).context("could not find primary.xml location in repomd.xml")?;

    let primary_url = if primary_location.starts_with("http") {
        primary_location
    } else {
        format!("{base}/{primary_location}")
    };
    let gz = client.get(&primary_url).send().context("fetching primary.xml.gz")?
        .error_for_status()?.bytes().context("reading primary.xml.gz")?;
    let xml = createrepo_rs::compression::gzip_decompress(&gz)
        .context("decompressing primary.xml.gz")?;
    let text = String::from_utf8_lossy(&xml);
    let hrefs: Vec<String> = text.split("href=\"").skip(1)
        .filter_map(|s| s.split('"').next().map(|v| v.to_string())).collect();
    if hrefs.is_empty() { bail!("no packages found"); }

    let dir = cfg.yum_base(root).join(repo);
    std::fs::create_dir_all(&dir)?;
    let mut imported = 0usize;
    for href in &hrefs {
        if let Some(n) = limit { if imported >= n { break; } }
        let url = if href.starts_with("http") { href.clone() } else { format!("{base}/{href}") };
        let name = Path::new(href).file_name()
            .map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| href.clone());
        let dest = dir.join(&name);
        if dest.exists() { tracing::info!(%name, "already exists, skipping"); continue; }
        let resp = client.get(&url).send().context("downloading rpm")?.error_for_status()?;
        let body = resp.bytes().context("reading rpm body")?;
        std::fs::write(&dest, &body)?;
        println!("imported {name}");
        imported += 1;
    }
    Ok(imported)
}
