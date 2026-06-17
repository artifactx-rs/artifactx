//! Repository import: download packages from an upstream apt/yum repo into
//! the local arx pool — the migration path from aptly/Nexus/reprepro.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;

/// Import packages from an upstream apt repository.
/// `base_url` is e.g. `http://example.com/apt`, `dist` is e.g. `stable`,
/// `component` is e.g. `main`, `arch` is e.g. `amd64`.
pub fn import_apt(
    root: &Path,
    cfg: &Config,
    base_url: &str,
    dist: &str,
    component: &str,
    arch: &str,
) -> Result<usize> {
    let packages_url = format!(
        "{}/dists/{dist}/{component}/binary-{arch}/Packages",
        base_url.trim_end_matches('/')
    );
    let client = reqwest::blocking::Client::new();
    let text = client
        .get(&packages_url)
        .send()
        .with_context(|| format!("fetching {packages_url}"))?
        .error_for_status()
        .context("upstream returned error")?
        .text()
        .context("reading Packages")?;

    let entries = parse_packages_entries(&text);
    if entries.is_empty() {
        bail!("no packages found in upstream Packages file");
    }

    let dir = cfg.apt_pool_root(root).join(component);
    std::fs::create_dir_all(&dir)?;

    let mut imported = 0usize;
    for file in &entries {
        let url = format!("{}/{}", base_url.trim_end_matches('/'), file);
        let filename = Path::new(file)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| file.clone());
        let dest = dir.join(&filename);
        if dest.exists() {
            tracing::info!(%filename, "already exists, skipping");
            continue;
        }
        let resp = client
            .get(&url)
            .send()
            .with_context(|| format!("downloading {url}"))?
            .error_for_status()?;
        let body = resp.bytes().context("reading response body")?;
        std::fs::write(&dest, &body)
            .with_context(|| format!("writing {}", dest.display()))?;
        println!("imported {filename}");
        imported += 1;
    }
    Ok(imported)
}

/// Parse `Filename:` lines from a Packages index to get the file URLs.
fn parse_packages_entries(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("Filename: "))
        .map(|v| v.trim().to_string())
        .collect()
}

/// Import packages from an upstream yum/dnf repository.
/// `base_url` is e.g. `http://example.com/yum/myrepo/x86_64`.
pub fn import_yum(
    root: &Path,
    cfg: &Config,
    base_url: &str,
    repo: &str,
) -> Result<usize> {
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

    // Find primary.xml.gz location.
    let primary_location = xml
        .lines()
        .find_map(|line| {
            let s = line.trim();
            if s.contains("primary") && s.contains("location") {
                // <location href="repodata/..."> — crude but works.
                s.split('"')
                    .nth(1)
                    .map(str::to_string)
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

    let xml = createrepo_rs::compression::gzip_decompress(&gz)
        .context("decompressing primary.xml.gz")?;
    let text = String::from_utf8_lossy(&xml);

    // Extract location hrefs and download each .rpm.
    let hrefs = extract_hrefs(&text);
    if hrefs.is_empty() {
        bail!("no packages found in primary.xml");
    }

    let dir = cfg.yum_base(root).join(repo);
    std::fs::create_dir_all(&dir)?;

    let mut imported = 0usize;
    for href in &hrefs {
        let url = if href.starts_with("http") {
            href.clone()
        } else {
            format!("{base}/{href}")
        };
        let filename = Path::new(href)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| href.clone());
        let dest = dir.join(&filename);
        if dest.exists() {
            tracing::info!(%filename, "already exists, skipping");
            continue;
        }
        let resp = client.get(&url).send().context("downloading rpm")?.error_for_status()?;
        let body = resp.bytes().context("reading rpm body")?;
        std::fs::write(&dest, &body)?;
        println!("imported {filename}");
        imported += 1;
    }
    Ok(imported)
}

fn extract_hrefs(xml: &str) -> Vec<String> {
    xml.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next().map(|v| v.to_string()))
        .collect()
}
