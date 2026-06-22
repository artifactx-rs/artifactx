//! Preflighted publish/export/live cutover workflow.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use pgp::composed::SignedSecretKey;
use walkdir::WalkDir;

use crate::config::Config;
use crate::{export, hooks, publish_apt, publish_yum};

#[derive(Debug, Clone)]
pub struct CutoverOptions {
    pub root: PathBuf,
    pub apt_live: Option<PathBuf>,
    pub yum_flat_live: Option<PathBuf>,
    pub staging_dir: Option<PathBuf>,
    pub repo: Option<String>,
    pub arch: Vec<String>,
    pub dry_run: bool,
    pub no_publish: bool,
    pub require_signed_rpms: bool,
}

#[derive(Debug)]
pub struct CutoverReport {
    pub staging_root: PathBuf,
    pub lines: Vec<String>,
}

pub fn run(
    opts: &CutoverOptions,
    cfg: &Config,
    key: Option<&SignedSecretKey>,
    passphrase: &str,
) -> Result<CutoverReport> {
    if opts.apt_live.is_none() && opts.yum_flat_live.is_none() {
        bail!("nothing to cut over; pass --apt-live and/or --yum-flat-live");
    }

    let formats = formats(opts);
    let mut lines = Vec::new();
    let _lock = crate::PublishLock::acquire(&opts.root)?;

    if !opts.no_publish {
        hooks::run(
            &opts.root,
            cfg,
            hooks::HookEvent::PrePublish,
            &hooks::HookContext::new().with("ARX_FORMATS", formats.clone()),
        )?;
        let mut published = Vec::new();
        if opts.apt_live.is_some() {
            published
                .push(publish_apt(&opts.root, cfg, key, passphrase, cfg.apt.strict, true)?.summary);
        }
        if opts.yum_flat_live.is_some() {
            published.push(publish_yum(&opts.root, cfg, key, passphrase, true)?);
        }
        let summary = published.join("; ");
        hooks::run(
            &opts.root,
            cfg,
            hooks::HookEvent::PostPublish,
            &hooks::HookContext::new()
                .with("ARX_FORMATS", formats.clone())
                .with("ARX_SUMMARY", summary.clone()),
        )?;
        lines.push(format!("publish: {summary}"));
    }

    let staging_root = opts
        .staging_dir
        .clone()
        .unwrap_or_else(|| default_staging_dir(opts));
    std::fs::create_dir_all(&staging_root)
        .with_context(|| format!("creating {}", staging_root.display()))?;
    let cutover_id = format!(
        "cutover-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        std::process::id()
    );
    let version_root = staging_root.join(cutover_id);
    if version_root.exists() {
        bail!("{} already exists", version_root.display());
    }
    std::fs::create_dir(&version_root)
        .with_context(|| format!("creating {}", version_root.display()))?;

    hooks::run(
        &opts.root,
        cfg,
        hooks::HookEvent::PreExport,
        &hooks::HookContext::new().with("ARX_FORMATS", formats.clone()),
    )?;
    if opts.apt_live.is_some() {
        let out = version_root.join("deb");
        export::export_apt(&opts.root, cfg, &out)?;
        validate_apt_export(cfg, &out)?;
        lines.push(format!("apt staging: {}", out.display()));
    }
    if opts.yum_flat_live.is_some() {
        let out = version_root.join("repo");
        let repo = opts.repo.as_deref().unwrap_or(&cfg.yum.repo);
        let report =
            export::export_yum_flat(&opts.root, cfg, &out, repo, &opts.arch, key, passphrase)?;
        validate_yum_flat_export(&out, cfg.signing.enabled && key.is_some())?;
        let unsigned = rpm_signature_report(&out)?;
        if opts.require_signed_rpms && !unsigned.is_empty() {
            bail!(
                "unsigned RPM payload(s) block cutover with --require-signed-rpms: {}",
                unsigned.join(", ")
            );
        }
        if unsigned.is_empty() {
            lines.push("rpm payload signatures: all scanned RPMs are signed".to_string());
        } else {
            lines.push(format!(
                "rpm payload signatures: {} unsigned RPM(s) found (repository metadata is still signed)",
                unsigned.len()
            ));
        }
        lines.push(format!(
            "yum staging: {} (copied {} rpm(s), indexed {} rpm(s), arches: {})",
            report.path.display(),
            report.copied_rpms,
            report.indexed_rpms,
            if report.arches.is_empty() {
                "none".to_string()
            } else {
                report.arches.join(",")
            }
        ));
    }
    let export_summary = lines.join("\n");
    hooks::run(
        &opts.root,
        cfg,
        hooks::HookEvent::PostExport,
        &hooks::HookContext::new()
            .with("ARX_FORMATS", formats)
            .with("ARX_SUMMARY", export_summary),
    )?;

    if opts.dry_run {
        lines.push("dry-run: live pointers were not changed".to_string());
        return Ok(CutoverReport {
            staging_root: version_root,
            lines,
        });
    }

    if let Some(live) = &opts.apt_live {
        let target = version_root.join("deb");
        let previous = switch_live(live, &target)?;
        lines.push(cutover_line("apt live", live, &target, previous.as_deref()));
    }
    if let Some(live) = &opts.yum_flat_live {
        let target = version_root.join("repo");
        let previous = switch_live(live, &target)?;
        lines.push(cutover_line("yum live", live, &target, previous.as_deref()));
    }

    Ok(CutoverReport {
        staging_root: version_root,
        lines,
    })
}

fn formats(opts: &CutoverOptions) -> String {
    let mut out = Vec::new();
    if opts.apt_live.is_some() {
        out.push("apt");
    }
    if opts.yum_flat_live.is_some() {
        out.push("yum");
    }
    out.join(",")
}

fn default_staging_dir(opts: &CutoverOptions) -> PathBuf {
    let live = opts
        .apt_live
        .as_ref()
        .or(opts.yum_flat_live.as_ref())
        .expect("validated at entry");
    live.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".arx-cutovers")
}

fn validate_apt_export(cfg: &Config, out: &Path) -> Result<()> {
    let dist = &cfg.apt.dist;
    let component = &cfg.apt.component;
    let release = out.join("dists").join(dist).join("Release");
    if !release.is_file() {
        bail!("apt preflight failed: missing {}", release.display());
    }
    let packages_gz = out
        .join("dists")
        .join(dist)
        .join(component)
        .join("binary-amd64")
        .join("Packages.gz");
    if !packages_gz.is_file() {
        bail!("apt preflight failed: missing {}", packages_gz.display());
    }
    Ok(())
}

fn validate_yum_flat_export(out: &Path, expect_repo_signature: bool) -> Result<()> {
    let repodata = out.join("repodata");
    let repomd = repodata.join("repomd.xml");
    if !repomd.is_file() {
        bail!("yum preflight failed: missing {}", repomd.display());
    }
    let repomd_text = std::fs::read_to_string(&repomd).context("reading repomd.xml")?;
    if !repomd_text.contains(".xml.gz") {
        bail!("yum preflight failed: gzip metadata is required for older clients");
    }
    if repomd_text.contains(".xml.xz") {
        bail!("yum preflight failed: xz-only metadata is not allowed for gzip-only clients");
    }
    if !repodata.join("sha256-primary.xml.gz").is_file() {
        bail!("yum preflight failed: missing gzip primary metadata");
    }
    if expect_repo_signature && !repodata.join("repomd.xml.asc").is_file() {
        bail!("yum preflight failed: missing repomd.xml.asc repository metadata signature");
    }
    Ok(())
}

fn rpm_signature_report(out: &Path) -> Result<Vec<String>> {
    let mut unsigned = Vec::new();
    for entry in WalkDir::new(out).into_iter().filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("rpm") {
            continue;
        }
        let mut reader = createrepo_rs::rpm::RpmReader::open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        if !reader.is_signed() {
            let mut package = reader
                .read_package()
                .context("reading rpm package metadata")?;
            if package.release.is_empty() {
                package.release = "-".to_string();
            }
            unsigned.push(format!(
                "{}-{}-{}.{}",
                package.name, package.version, package.release, package.arch
            ));
        }
    }
    unsigned.sort();
    Ok(unsigned)
}

fn cutover_line(label: &str, live: &Path, target: &Path, previous: Option<&Path>) -> String {
    match previous {
        Some(prev) => format!(
            "{label}: {} -> {} (previous: {})",
            live.display(),
            target.display(),
            prev.display()
        ),
        None => format!("{label}: {} -> {}", live.display(), target.display()),
    }
}

#[cfg(unix)]
fn switch_live(live: &Path, target: &Path) -> Result<Option<PathBuf>> {
    use std::os::unix::fs::symlink;

    let parent = live.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let previous = match std::fs::symlink_metadata(live) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Some(std::fs::read_link(live).with_context(|| format!("reading {}", live.display()))?)
        }
        Ok(_) => bail!(
            "{} exists and is not a symlink; move it aside once, then rerun cutover",
            live.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("stat {}", live.display())),
    };

    let name = live.file_name().and_then(|n| n.to_str()).unwrap_or("live");
    let tmp = parent.join(format!(".{name}.next-{}", std::process::id()));
    if tmp.exists() {
        std::fs::remove_file(&tmp).with_context(|| format!("removing {}", tmp.display()))?;
    }
    symlink(target, &tmp)
        .with_context(|| format!("linking {} -> {}", tmp.display(), target.display()))?;
    std::fs::rename(&tmp, live)
        .with_context(|| format!("renaming {} to {}", tmp.display(), live.display()))?;

    if let Some(prev) = &previous {
        let rollback = live.with_extension("previous");
        let rollback_tmp = parent.join(format!(".{name}.previous-{}", std::process::id()));
        if rollback_tmp.exists() {
            std::fs::remove_file(&rollback_tmp)
                .with_context(|| format!("removing {}", rollback_tmp.display()))?;
        }
        symlink(prev, &rollback_tmp)
            .with_context(|| format!("linking rollback pointer {}", rollback_tmp.display()))?;
        std::fs::rename(&rollback_tmp, &rollback).with_context(|| {
            format!(
                "renaming {} to {}",
                rollback_tmp.display(),
                rollback.display()
            )
        })?;
    }

    Ok(previous)
}

#[cfg(not(unix))]
fn switch_live(_live: &Path, _target: &Path) -> Result<Option<PathBuf>> {
    bail!("cutover live switching currently requires Unix symlinks")
}
