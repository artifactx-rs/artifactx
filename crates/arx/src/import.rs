//! Repository import: download packages from an upstream apt/yum repo into
//! the local pool — the migration path from aptly/Nexus/reprepro.

use std::io::Read;
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
    filename: String,
    size: Option<u64>,
    sha256: Option<String>,
}

#[derive(Debug)]
struct YumPackageEntry {
    href: String,
    size: Option<u64>,
    checksum: Option<Checksum>,
}

#[derive(Debug)]
struct Checksum {
    kind: String,
    value: String,
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
                filename,
                size,
                sha256,
            });
        }
    }
    entries
}

fn attr(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(rest) = tag.split_once(&needle).map(|(_, r)| r) {
            return rest.split(quote).next().map(str::to_string);
        }
    }
    None
}

fn repomd_primary_location(xml: &str) -> Result<String> {
    for block in xml.split("<data ").skip(1) {
        let header = block.split('>').next().unwrap_or(block);
        if attr(header, "type").as_deref() != Some("primary") {
            continue;
        }
        let data = block.split("</data>").next().unwrap_or(block);
        if let Some(location_tag) = data
            .split("<location")
            .nth(1)
            .and_then(|s| s.split('>').next())
        {
            if let Some(href) = attr(location_tag, "href") {
                return Ok(href);
            }
        }
    }
    bail!("could not find primary.xml location in repomd.xml")
}

fn parse_yum_package_entries(xml: &str) -> Vec<YumPackageEntry> {
    let mut entries = Vec::new();
    for block in xml.split("<package").skip(1) {
        let package = block.split("</package>").next().unwrap_or(block);
        let Some(location_tag) = package
            .split("<location")
            .nth(1)
            .and_then(|s| s.split('>').next())
        else {
            continue;
        };
        let Some(href) = attr(location_tag, "href") else {
            continue;
        };
        let size = package
            .split("<size")
            .nth(1)
            .and_then(|s| s.split('>').next())
            .and_then(|tag| attr(tag, "package"))
            .and_then(|s| s.parse::<u64>().ok());
        let checksum = package.split("<checksum").nth(1).and_then(|s| {
            let tag = s.split('>').next().unwrap_or(s);
            let body = s.split_once('>')?.1.split("</checksum>").next()?;
            Some(Checksum {
                kind: attr(tag, "type")?.to_ascii_lowercase(),
                value: body.trim().to_ascii_lowercase(),
            })
        });
        entries.push(YumPackageEntry {
            href,
            size,
            checksum,
        });
    }
    entries
}

fn resolve_repo_url(base: &str, location: &str) -> Result<String> {
    if reqwest::Url::parse(location).is_ok() {
        return Ok(location.to_string());
    }
    let base = format!("{}/", base.trim_end_matches('/'));
    let url = reqwest::Url::parse(&base)
        .with_context(|| format!("parsing upstream base URL {base}"))?
        .join(location.trim_start_matches('/'))
        .with_context(|| format!("resolving repository location {location}"))?;
    Ok(url.to_string())
}

fn basename_from_location(location: &str) -> String {
    if let Ok(url) = reqwest::Url::parse(location) {
        if let Some(name) = url.path_segments().and_then(|mut s| s.next_back()) {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    Path::new(location)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| location.to_string())
}

fn decompress_by_location(location: &str, body: &[u8]) -> Result<String> {
    let bytes = if location.ends_with(".gz") {
        createrepo_rs::compression::gzip_decompress(body).context("decompressing gzip metadata")?
    } else if location.ends_with(".xz") {
        let mut out = Vec::new();
        xz2::read::XzDecoder::new(body)
            .read_to_end(&mut out)
            .context("decompressing xz metadata")?;
        out
    } else if location.ends_with(".bz2") {
        let mut out = Vec::new();
        bzip2::read::BzDecoder::new(body)
            .read_to_end(&mut out)
            .context("decompressing bzip2 metadata")?;
        out
    } else if location.ends_with(".zck") {
        bail!("zchunk metadata is not supported yet: {location}")
    } else {
        body.to_vec()
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn fetch_text(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let body = client
        .get(url)
        .send()
        .with_context(|| format!("fetching {url}"))?
        .error_for_status()
        .with_context(|| format!("upstream returned error for {url}"))?
        .bytes()
        .with_context(|| format!("reading {url}"))?;
    decompress_by_location(url, &body)
}

fn fetch_first_existing(
    client: &reqwest::blocking::Client,
    candidates: &[String],
) -> Result<(String, String)> {
    let mut misses = Vec::new();
    for url in candidates {
        match client.get(url).send() {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.bytes().with_context(|| format!("reading {url}"))?;
                let text = decompress_by_location(url, &body)?;
                return Ok((url.clone(), text));
            }
            Ok(resp) => misses.push(format!("{url} -> {}", resp.status())),
            Err(e) => misses.push(format!("{url} -> {e}")),
        }
    }
    bail!(
        "none of the metadata candidates were available: {}",
        misses.join(", ")
    )
}

fn verify_size(location: &str, expected: Option<u64>, actual: usize) -> Result<()> {
    if let Some(expected) = expected {
        if actual as u64 != expected {
            bail!(
                "{location}: size mismatch (expected {}, got {})",
                expected,
                actual
            );
        }
    }
    Ok(())
}

fn verify_sha256(location: &str, expected: Option<&str>, body: &[u8]) -> Result<()> {
    if let Some(expected) = expected {
        let actual = hex::encode(Sha256::digest(body));
        if actual != expected {
            bail!(
                "{location}: sha256 mismatch (expected {}, got {})",
                expected,
                actual
            );
        }
    }
    Ok(())
}

fn verify_checksum(location: &str, checksum: Option<&Checksum>, body: &[u8]) -> Result<()> {
    if let Some(checksum) = checksum {
        match checksum.kind.as_str() {
            "sha256" => verify_sha256(location, Some(&checksum.value), body)?,
            other => {
                tracing::warn!(%location, kind = %other, "unsupported upstream checksum type, skipping validation")
            }
        }
    }
    Ok(())
}

fn check_existing_package(
    dest: &Path,
    location: &str,
    body: &[u8],
    size: Option<u64>,
    checksum: Option<&Checksum>,
    sha256: Option<&str>,
) -> Result<bool> {
    if !dest.exists() {
        return Ok(false);
    }
    let existing = std::fs::read(dest).with_context(|| format!("reading {}", dest.display()))?;
    verify_size(location, size, existing.len())?;
    verify_sha256(location, sha256, &existing)?;
    verify_checksum(location, checksum, &existing)?;
    if existing == body {
        tracing::info!(file = %dest.display(), "already exists with identical content, skipping");
        return Ok(true);
    }
    bail!(
        "{} already exists but differs from upstream package {}",
        dest.display(),
        location
    );
}

fn write_verified_package(
    dest: &Path,
    location: &str,
    body: &[u8],
    size: Option<u64>,
    checksum: Option<&Checksum>,
    sha256: Option<&str>,
) -> Result<bool> {
    verify_size(location, size, body.len())?;
    verify_sha256(location, sha256, body)?;
    verify_checksum(location, checksum, body)?;

    if check_existing_package(dest, location, body, size, checksum, sha256)? {
        return Ok(false);
    }

    std::fs::write(dest, body).with_context(|| format!("writing {}", dest.display()))?;
    Ok(true)
}

fn rpm_arch_from_file(path: &Path) -> Result<String> {
    let mut reader = createrepo_rs::rpm::RpmReader::open(path)
        .with_context(|| format!("opening rpm {}", path.display()))?;
    let package = reader
        .read_package()
        .with_context(|| format!("reading rpm metadata from {}", path.display()))?;
    Ok(scope::validate_scope_name(&package.arch, "yum arch")?.to_string())
}

fn write_verified_rpm_to_arch_dir(
    repo_dir: &Path,
    name: &str,
    location: &str,
    body: &[u8],
    size: Option<u64>,
    checksum: Option<&Checksum>,
) -> Result<bool> {
    verify_size(location, size, body.len())?;
    verify_checksum(location, checksum, body)?;

    let tmp = repo_dir.join(format!(".incoming-{name}"));
    if tmp.exists() {
        std::fs::remove_file(&tmp).with_context(|| format!("removing stale {}", tmp.display()))?;
    }
    std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;

    let result = (|| {
        let arch = rpm_arch_from_file(&tmp)?;
        let arch_dir = repo_dir.join(arch);
        std::fs::create_dir_all(&arch_dir)
            .with_context(|| format!("creating {}", arch_dir.display()))?;
        let dest = arch_dir.join(name);
        if check_existing_package(&dest, location, body, size, checksum, None)? {
            return Ok(false);
        }
        std::fs::rename(&tmp, &dest)
            .with_context(|| format!("moving {} to {}", tmp.display(), dest.display()))?;
        Ok(true)
    })();

    if result.is_err() && tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
    let prefix = format!("{base}/dists/{dist}/{component}/binary-{arch}/Packages");
    let client = reqwest::blocking::Client::new();
    let (_metadata_url, text) = fetch_first_existing(
        &client,
        &[
            format!("{prefix}.gz"),
            format!("{prefix}.xz"),
            format!("{prefix}.bz2"),
            prefix,
        ],
    )?;

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
        let url = resolve_repo_url(base, &entry.filename)?;
        let name = basename_from_location(&entry.filename);
        let dest = dir.join(&name);
        let body = client
            .get(&url)
            .send()
            .with_context(|| format!("downloading {url}"))?
            .error_for_status()?
            .bytes()
            .context("reading body")?;
        if write_verified_package(
            &dest,
            &entry.filename,
            &body,
            entry.size,
            None,
            entry.sha256.as_deref(),
        )? {
            println!("imported {name}");
            imported += 1;
        }
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
    let client = reqwest::blocking::Client::new();
    let repomd_url = format!("{base}/repodata/repomd.xml");
    let repomd = fetch_text(&client, &repomd_url)?;
    let primary_location = repomd_primary_location(&repomd)?;
    let primary_url = resolve_repo_url(base, &primary_location)?;
    let primary = fetch_text(&client, &primary_url)?;
    let entries = parse_yum_package_entries(&primary);
    if entries.is_empty() {
        bail!("no packages found");
    }

    let repo_dir = cfg.checked_yum_base(root)?.join(repo);
    std::fs::create_dir_all(&repo_dir)?;
    let mut imported = 0usize;
    for entry in &entries {
        if let Some(n) = limit {
            if imported >= n {
                break;
            }
        }
        let url = resolve_repo_url(base, &entry.href)?;
        let name = basename_from_location(&entry.href);
        let result = (|| -> Result<bool> {
            let body = client
                .get(&url)
                .send()
                .with_context(|| format!("downloading {url}"))?
                .error_for_status()?
                .bytes()
                .context("reading rpm body")?;
            write_verified_rpm_to_arch_dir(
                &repo_dir,
                &name,
                &entry.href,
                &body,
                entry.size,
                entry.checksum.as_ref(),
            )
        })();

        match result {
            Ok(true) => {
                println!("imported {name}");
                imported += 1;
            }
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(
                    location = %entry.href,
                    url = %url,
                    error = %err,
                    "skipping invalid yum package entry"
                );
            }
        }
    }
    Ok(imported)
}
