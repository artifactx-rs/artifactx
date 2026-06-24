//! yum/dnf repository generation.
//!
//! Reuses the building blocks from `createrepo_rs` (RPM parsing + repodata XML
//! dumping) and replicates the orchestration that its binary performs in
//! `src/main.rs`, then PGP-signs `repomd.xml` into `repomd.xml.asc`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::createrepo_rs::compression::gzip_compress;
use crate::createrepo_rs::pool::{Job, ProcessingResult, WorkerPool};
use crate::createrepo_rs::types::{Package, Repomd, RepomdRecord};
use crate::createrepo_rs::walk::DirectoryWalker;
use crate::createrepo_rs::xml::dump;
use crate::signing;

/// gzip level matching createrepo's default.
const GZIP_LEVEL: i32 = 6;
const CHECKSUM_TYPE: &str = "sha256";

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn stat_mtime_size(path: &Path) -> (Option<u64>, Option<u64>) {
    std::fs::metadata(path)
        .ok()
        .map(|m| {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            (mtime, Some(m.len()))
        })
        .unwrap_or((None, None))
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One metadata stream (primary/filelists/other): write the gzipped XML and
/// return the repomd record describing it.
fn write_stream(
    repodata: &Path,
    record_type: &str,
    xml: &[u8],
    pretty_revision: i64,
) -> Result<RepomdRecord> {
    let open_checksum = sha256_hex(xml);
    let filename = format!("{CHECKSUM_TYPE}-{record_type}.xml.gz");
    let compressed =
        gzip_compress(xml, GZIP_LEVEL).with_context(|| format!("gzip {record_type}.xml"))?;
    let checksum = sha256_hex(&compressed);
    std::fs::write(repodata.join(&filename), &compressed)
        .with_context(|| format!("writing {filename}"))?;

    Ok(RepomdRecord {
        record_type: record_type.to_string(),
        location: format!("repodata/{filename}"),
        checksum: Some(checksum),
        timestamp: Some(pretty_revision),
        size: Some(compressed.len() as i64),
        open_size: Some(xml.len() as i64),
        open_checksum: Some(open_checksum),
        checksum_type: Some(CHECKSUM_TYPE.to_string()),
    })
}

/// Build repodata for a single `<dir>` containing `.rpm` files, writing into
/// `<dir>/repodata/`. If `key` is provided, also writes `repomd.xml.asc`.
///
/// When `incremental` is true, (mtime, size) of every `.rpm` is compared against
/// `.arx-manifest.toml`. If nothing changed, the repodata rebuild is skipped
/// entirely — O(scan) instead of O(repo). Set `incremental = false` (or `--full`)
/// to rebuild everything.
pub fn build_repodata(
    dir: &Path,
    key: Option<&pgp::composed::SignedSecretKey>,
    passphrase: &str,
    incremental: bool,
) -> Result<usize> {
    let rpms = scan_rpms(dir)?;

    if incremental && cache_is_fresh(dir, &rpms)? {
        return Ok(rpms.len());
    }

    let repodata = prepare_staging_dir(dir)?;
    let package_count = if incremental {
        let metadata = load_or_build_incremental_metadata(dir, &rpms)?;
        write_repodata_from_fragments(&repodata, metadata.as_slice(), key, passphrase)?;
        metadata.len()
    } else {
        let packages = parse_rpms(&rpms)?;
        write_repodata(&repodata, packages.as_slice(), key, passphrase)?;
        packages.len()
    };
    commit_repodata(dir, &repodata)?;

    Ok(package_count)
}

fn scan_rpms(dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(DirectoryWalker::new(dir)
        .with_context(|| format!("scanning {}", dir.display()))?
        .collect())
}

fn cache_is_fresh(dir: &Path, rpms: &[PathBuf]) -> Result<bool> {
    if rpms.is_empty() {
        return Ok(false);
    }

    let manifest = arx_debrepo::manifest::FileManifest::load(dir).unwrap_or_default();
    let mut all_match = !manifest.files.is_empty(); // must have a manifest to trust
    let mut on_disk = HashSet::new();
    for rpm in rpms {
        if let Some(fname) = rpm.file_name().and_then(|n| n.to_str()) {
            on_disk.insert(fname.to_string());
            if all_match {
                let (mtime, size) = stat_mtime_size(rpm);
                all_match = mtime.zip(size).is_some_and(|(m, s)| {
                    manifest.lookup(fname, m, s).is_some_and(|cached| {
                        !cached.stanza.is_empty()
                            && !cached.contents.is_empty()
                            && !cached.other.is_empty()
                    })
                });
            }
        }
    }

    if all_match && on_disk.len() == manifest.files.len() {
        // Everything unchanged → nothing to do. Still clean stale manifest
        // entries from deleted files.
        let mut fresh = manifest;
        fresh.retain(&on_disk);
        let _ = fresh.save(dir);
        return Ok(true);
    }

    Ok(false)
}

fn parse_rpms(rpms: &[PathBuf]) -> Result<Vec<Package>> {
    Ok(parse_rpms_with_paths(rpms)?
        .into_iter()
        .map(|(_, pkg)| pkg)
        .collect())
}

fn parse_rpms_with_paths(rpms: &[PathBuf]) -> Result<Vec<(PathBuf, Package)>> {
    // Parse RPMs via createrepo_rs's worker pool, which yields fully-populated
    // `types::Package` values (the conversion the library performs internally).
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let (pool, receiver) = WorkerPool::new(workers);
    let mut submitted = 0usize;
    for rpm in rpms {
        if pool.submit(Job::ProcessPackage(rpm.clone())) {
            submitted += 1;
        }
    }

    let mut packages = Vec::with_capacity(submitted);
    for _ in 0..submitted {
        match receiver
            .recv()
            .context("worker pool channel closed early")?
        {
            ProcessingResult::Success(path, pkg) => packages.push((path, pkg)),
            ProcessingResult::Error(path, err) => bail!("processing {}: {err}", path.display()),
        }
    }
    pool.join();
    Ok(packages)
}

fn prepare_staging_dir(dir: &Path) -> Result<PathBuf> {
    // Build into a staging dir, then atomically flip `repodata` (a symlink) to a
    // new immutable state — mirrors the apt side for rollback.
    let repodata = dir.join(".repodata.staging");
    if repodata.exists() {
        std::fs::remove_dir_all(&repodata).ok();
    }
    std::fs::create_dir_all(&repodata)
        .with_context(|| format!("creating {}", repodata.display()))?;
    Ok(repodata)
}

fn write_repodata(
    repodata: &Path,
    packages: &[Package],
    key: Option<&pgp::composed::SignedSecretKey>,
    passphrase: &str,
) -> Result<()> {
    let primary_xml =
        dump::primary::dump_primary_xml(packages, true).context("generating primary.xml")?;
    let filelists_xml = dump::filelists::dump_filelists_xml(packages, false, true)
        .context("generating filelists.xml")?;
    let other_xml = dump::other::dump_other_xml(packages, true).context("generating other.xml")?;

    write_repodata_xml(
        repodata,
        &primary_xml,
        &filelists_xml,
        &other_xml,
        key,
        passphrase,
    )
}

#[derive(Debug, Clone)]
struct YumPackageMetadata {
    filename: String,
    mtime: u64,
    size: u64,
    primary: String,
    filelists: String,
    other: String,
}

fn write_repodata_from_fragments(
    repodata: &Path,
    packages: &[YumPackageMetadata],
    key: Option<&pgp::composed::SignedSecretKey>,
    passphrase: &str,
) -> Result<()> {
    let primary_xml = render_xml_stream(
        "metadata",
        "http://linux.duke.edu/metadata/common",
        Some("http://linux.duke.edu/metadata/rpm"),
        packages.len(),
        packages.iter().map(|p| p.primary.as_str()),
    );
    let filelists_xml = render_xml_stream(
        "filelists",
        "http://linux.duke.edu/metadata/filelists",
        None,
        packages.len(),
        packages.iter().map(|p| p.filelists.as_str()),
    );
    let other_xml = render_xml_stream(
        "otherdata",
        "http://linux.duke.edu/metadata/other",
        None,
        packages.len(),
        packages.iter().map(|p| p.other.as_str()),
    );

    write_repodata_xml(
        repodata,
        primary_xml.as_bytes(),
        filelists_xml.as_bytes(),
        other_xml.as_bytes(),
        key,
        passphrase,
    )
}

fn write_repodata_xml(
    repodata: &Path,
    primary_xml: &[u8],
    filelists_xml: &[u8],
    other_xml: &[u8],
    key: Option<&pgp::composed::SignedSecretKey>,
    passphrase: &str,
) -> Result<()> {
    let revision = now_unix();

    let records = vec![
        write_stream(repodata, "primary", primary_xml, revision)?,
        write_stream(repodata, "filelists", filelists_xml, revision)?,
        write_stream(repodata, "other", other_xml, revision)?,
    ];

    let repomd = Repomd {
        revision: Some(revision.to_string()),
        records,
        distro_tags: Vec::new(),
        content_tags: Vec::new(),
        repo_tags: Vec::new(),
    };

    let repomd_path = repodata.join("repomd.xml");
    dump::repomd::dump_repomd(&repomd, &repomd_path, true).context("writing repomd.xml")?;

    if let Some(key) = key {
        sign_repomd(repodata, &repomd_path, key, passphrase)?;
    }
    Ok(())
}

fn sign_repomd(
    repodata: &Path,
    repomd_path: &Path,
    key: &pgp::composed::SignedSecretKey,
    passphrase: &str,
) -> Result<()> {
    let repomd_bytes = std::fs::read(repomd_path).context("re-reading repomd.xml")?;
    let armored = signing::detached_sign(key, passphrase, &repomd_bytes)?;
    std::fs::write(repodata.join("repomd.xml.asc"), armored).context("writing repomd.xml.asc")?;
    Ok(())
}

fn commit_repodata(dir: &Path, repodata: &Path) -> Result<()> {
    // Atomic flip: `<arch>/repodata` → `.states/repodata/<id>`.
    arx_debrepo::statedir::commit(
        repodata,
        &dir.join("repodata"),
        arx_debrepo::DEFAULT_KEEP_STATES,
    )
    .context("committing repodata state")?;
    Ok(())
}

fn load_or_build_incremental_metadata(
    dir: &Path,
    rpms: &[PathBuf],
) -> Result<Vec<YumPackageMetadata>> {
    let manifest = arx_debrepo::manifest::FileManifest::load(dir).unwrap_or_default();
    let mut on_disk = HashSet::new();
    let mut pending = Vec::new();
    let mut metadata_by_name: HashMap<String, YumPackageMetadata> = HashMap::new();

    for rpm in rpms {
        let fname = match rpm.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        on_disk.insert(fname.clone());
        let (mtime, size) = stat_mtime_size(rpm);
        let cached = mtime
            .zip(size)
            .and_then(|(m, s)| manifest.lookup(&fname, m, s));
        if let (Some(mtime), Some(size), Some(cached)) = (mtime, size, cached) {
            if !cached.stanza.is_empty() && !cached.contents.is_empty() && !cached.other.is_empty()
            {
                metadata_by_name.insert(
                    fname.clone(),
                    YumPackageMetadata {
                        filename: fname,
                        mtime,
                        size,
                        primary: cached.stanza.clone(),
                        filelists: cached.contents.clone(),
                        other: cached.other.clone(),
                    },
                );
                continue;
            }
        }
        pending.push(rpm.clone());
    }

    for (path, package) in parse_rpms_with_paths(&pending)? {
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("rpm path has no file name: {}", path.display()))?
            .to_string();
        let (mtime, size) = stat_mtime_size(&path);
        let (mtime, size) = mtime
            .zip(size)
            .ok_or_else(|| anyhow!("stat failed for {}", path.display()))?;
        let fragments = package_fragments(&package)?;
        metadata_by_name.insert(
            fname.clone(),
            YumPackageMetadata {
                filename: fname,
                mtime,
                size,
                primary: fragments.primary,
                filelists: fragments.filelists,
                other: fragments.other,
            },
        );
    }

    let mut ordered = Vec::with_capacity(rpms.len());
    for rpm in rpms {
        if let Some(fname) = rpm.file_name().and_then(|n| n.to_str()) {
            let meta = metadata_by_name
                .remove(fname)
                .ok_or_else(|| anyhow!("missing yum metadata for {}", rpm.display()))?;
            ordered.push(meta);
        }
    }

    save_yum_manifest(dir, &ordered, &on_disk);
    Ok(ordered)
}

#[derive(Debug)]
struct YumXmlFragments {
    primary: String,
    filelists: String,
    other: String,
}

fn package_fragments(package: &Package) -> Result<YumXmlFragments> {
    let one = std::slice::from_ref(package);
    Ok(YumXmlFragments {
        primary: xml_body(
            dump::primary::dump_primary_xml(one, true).context("generating cached primary.xml")?,
            "metadata",
        )?,
        filelists: xml_body(
            dump::filelists::dump_filelists_xml(one, false, true)
                .context("generating cached filelists.xml")?,
            "filelists",
        )?,
        other: xml_body(
            dump::other::dump_other_xml(one, true).context("generating cached other.xml")?,
            "otherdata",
        )?,
    })
}

fn xml_body(xml: Vec<u8>, root: &str) -> Result<String> {
    let text = String::from_utf8(xml).context("yum metadata XML was not UTF-8")?;
    let decl_end = text
        .find("?>")
        .ok_or_else(|| anyhow!("missing XML declaration for {root}"))?
        + 2;
    let root_start = text[decl_end..]
        .find('>')
        .ok_or_else(|| anyhow!("missing XML root for {root}"))?
        + decl_end
        + 1;
    let closing = format!("</{root}>");
    let root_end = text
        .rfind(&closing)
        .ok_or_else(|| anyhow!("missing XML closing tag for {root}"))?;
    Ok(text[root_start..root_end].to_string())
}

fn render_xml_stream<'a>(
    root: &str,
    namespace: &str,
    rpm_namespace: Option<&str>,
    package_count: usize,
    fragments: impl Iterator<Item = &'a str>,
) -> String {
    let mut out = String::new();
    out.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
"#,
    );
    out.push('<');
    out.push_str(root);
    out.push_str(r#" xmlns=""#);
    out.push_str(namespace);
    out.push('"');
    if let Some(rpm_namespace) = rpm_namespace {
        out.push_str(r#" xmlns:rpm=""#);
        out.push_str(rpm_namespace);
        out.push('"');
    }
    out.push_str(r#" packages=""#);
    out.push_str(&package_count.to_string());
    out.push_str(r#"">"#);
    for fragment in fragments {
        out.push_str(fragment);
    }
    out.push_str("</");
    out.push_str(root);
    out.push('>');
    out
}

fn save_yum_manifest(dir: &Path, packages: &[YumPackageMetadata], keep: &HashSet<String>) {
    // Save the file manifest for the NEXT incremental publish.
    let mut manifest = arx_debrepo::manifest::FileManifest::default();
    for package in packages {
        manifest.insert(
            package.filename.clone(),
            arx_debrepo::manifest::CachedPackage {
                mtime: package.mtime,
                size: package.size,
                sha256: String::new(), // yum side: not used for cache lookups
                stanza: package.primary.clone(),
                package: String::new(),
                version: String::new(),
                architecture: String::new(),
                contents: package.filelists.clone(),
                other: package.other.clone(),
            },
        );
    }
    manifest.retain(keep);
    let _ = manifest.save(dir);
}
