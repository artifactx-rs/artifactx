//! yum/dnf repository generation.
//!
//! Reuses the building blocks from `createrepo_rs` (RPM parsing + repodata XML
//! dumping) and replicates the orchestration that its binary performs in
//! `src/main.rs`, then PGP-signs `repomd.xml` into `repomd.xml.asc`.

use std::path::Path;

use anyhow::{bail, Context, Result};
use createrepo_rs::compression::gzip_compress;
use createrepo_rs::pool::{Job, ProcessingResult, WorkerPool};
use createrepo_rs::types::{Repomd, RepomdRecord};
use createrepo_rs::walk::DirectoryWalker;
use createrepo_rs::xml::dump;
use sha2::{Digest, Sha256};

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
    let compressed = gzip_compress(xml, GZIP_LEVEL)
        .with_context(|| format!("gzip {record_type}.xml"))?;
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
    let rpms = DirectoryWalker::new(dir)
        .with_context(|| format!("scanning {}", dir.display()))?
        .collect();

    // Fast path: nothing changed since last publish → skip entirely.
    if incremental && !rpms.is_empty() {
        let manifest = arx_debrepo::manifest::FileManifest::load(dir).unwrap_or_default();
        let mut all_match = !manifest.files.is_empty(); // must have a manifest to trust
        let mut on_disk = std::collections::HashSet::new();
        for rpm in &rpms {
            if let Some(fname) = rpm.file_name().and_then(|n| n.to_str()) {
                on_disk.insert(fname.to_string());
                if all_match {
                    let (mtime, size) = stat_mtime_size(rpm);
                    all_match = mtime
                        .zip(size)
                        .is_some_and(|(m, s)| manifest.lookup(fname, m, s).is_some());
                }
            }
        }
        if all_match && on_disk.len() == manifest.files.len() {
            // Everything unchanged → nothing to do. (Still clean stale manifest
            // entries from deleted files.)
            let mut fresh = manifest;
            fresh.retain(&on_disk);
            let _ = fresh.save(dir);
            return Ok(rpms.len());
        }
    }

    // Parse RPMs via createrepo_rs's worker pool, which yields fully-populated
    // `types::Package` values (the conversion the library performs internally).
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let (pool, receiver) = WorkerPool::new(workers);
    let mut submitted = 0usize;
    for rpm in &rpms {
        if pool.submit(Job::ProcessPackage(rpm.clone())) {
            submitted += 1;
        }
    }

    let mut packages = Vec::with_capacity(submitted);
    for _ in 0..submitted {
        match receiver.recv().context("worker pool channel closed early")? {
            ProcessingResult::Success(_, pkg) => packages.push(pkg),
            ProcessingResult::Error(path, err) => bail!("processing {}: {err}", path.display()),
        }
    }
    pool.join();

    // Build into a staging dir, then atomically flip `repodata` (a symlink) to a
    // new immutable state — mirrors the apt side for rollback.
    let repodata = dir.join(".repodata.staging");
    if repodata.exists() {
        std::fs::remove_dir_all(&repodata).ok();
    }
    std::fs::create_dir_all(&repodata)
        .with_context(|| format!("creating {}", repodata.display()))?;

    let revision = now_unix();

    let primary_xml = dump::primary::dump_primary_xml(packages.as_slice(), true)
        .context("generating primary.xml")?;
    let filelists_xml = dump::filelists::dump_filelists_xml(packages.as_slice(), false, true)
        .context("generating filelists.xml")?;
    let other_xml =
        dump::other::dump_other_xml(packages.as_slice(), true).context("generating other.xml")?;

    let records = vec![
        write_stream(&repodata, "primary", &primary_xml, revision)?,
        write_stream(&repodata, "filelists", &filelists_xml, revision)?,
        write_stream(&repodata, "other", &other_xml, revision)?,
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
        let repomd_bytes = std::fs::read(&repomd_path).context("re-reading repomd.xml")?;
        let armored = signing::detached_sign(key, passphrase, &repomd_bytes)?;
        std::fs::write(repodata.join("repomd.xml.asc"), armored)
            .context("writing repomd.xml.asc")?;
    }

    // Atomic flip: `<arch>/repodata` → `.states/repodata/<id>`.
    arx_debrepo::statedir::commit(&repodata, &dir.join("repodata"), arx_debrepo::DEFAULT_KEEP_STATES)
        .context("committing repodata state")?;

    // Save the file manifest for the NEXT incremental publish.
    if incremental {
        let mut m = arx_debrepo::manifest::FileManifest::default();
        for rpm in &rpms {
            if let Some(fname) = rpm.file_name().and_then(|n| n.to_str()) {
                let (mtime, size) = stat_mtime_size(rpm);
                if let (Some(mt), Some(sz)) = (mtime, size) {
                    m.insert(
                        fname.to_string(),
                        arx_debrepo::manifest::CachedPackage {
                            mtime: mt,
                            size: sz,
                            sha256: String::new(),  // yum side: not used for cache lookups
                            stanza: String::new(),   // yum side: not used for cache lookups
                        },
                    );
                }
            }
        }
        let _ = m.save(dir);
    }

    Ok(packages.len())
}
