//! ArtifactX (`arx`) entry point.

mod cache;
mod cli;
mod compose;
mod config;
mod cutover;
mod export;
mod hooks;
mod import;
mod mirror;
mod observability;
mod oidc;
mod pool;
mod scope;
mod server;
mod signing;
mod yum;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use pgp::composed::SignedSecretKey;

use crate::cli::{Cli, Command, KeyAction};
use crate::config::Config;

/// Full version string: package version + git sha + build time + rustc, stamped
/// at build time by `build.rs` (vergen). Shown by `arx --version`.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("VERGEN_GIT_SHA"),
    ", built ",
    env!("VERGEN_BUILD_TIMESTAMP"),
    ", rustc ",
    env!("VERGEN_RUSTC_SEMVER"),
    ")"
);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    observability::init_tracing(cli.log_format.into());

    match cli.command {
        Command::Init(args) => cmd_init(&args).await,
        Command::Key(args) => cmd_key(&args),
        Command::Add(args) => cmd_add(&args),
        Command::Cache(args) => cmd_cache(&args),
        Command::Pack(args) => cmd_pack(&args),
        Command::Publish(args) => cmd_publish(&args).await,
        Command::Rollback(args) => cmd_rollback(&args),
        Command::History(args) => cmd_history(&args),
        Command::Push(args) => cmd_push(&args).await,
        Command::PublishDir(args) => cmd_publish_dir(&args),
        Command::Rm(args) => {
            let apt_pool_root = selected_apt_pool_root(&args.root, args.apt, args.yum)?;
            let yum_base = selected_yum_base(&args.root, args.apt, args.yum)?;
            let removed = pool::remove(
                &apt_pool_root,
                &args.name,
                args.version.as_deref(),
                &yum_base,
                args.apt,
                args.yum,
            )?;
            if removed.is_empty() {
                println!("No packages matched {}.", args.name);
            } else {
                for e in &removed {
                    println!("Removed {} {} ({})", e.name, e.version, e.path.display());
                }
                println!(
                    "\nRemoved {} file(s). Run `arx publish` to update metadata.",
                    removed.len()
                );
            }
            Ok(())
        }
        Command::Search(args) => cmd_search(&args),
        Command::Mirror(args) => cmd_mirror(args).await,
        Command::Import(args) => cmd_import(args).await,
        Command::Promote(args) => cmd_promote(&args),
        Command::Gc(args) => {
            let apt_pool_root = selected_apt_pool_root(&args.root, args.apt, args.yum)?;
            let yum_base = selected_yum_base(&args.root, args.apt, args.yum)?;
            let report = pool::gc(
                &args.root,
                pool::GcOptions {
                    name: args.name.as_deref(),
                    name_prefix: args.name_prefix.as_deref(),
                    keep: args.keep,
                    keep_within_days: args.keep_within,
                    grace_days: args.grace,
                    apt_pool_root: &apt_pool_root,
                    yum_base: &yum_base,
                    apt: args.apt,
                    yum: args.yum,
                    dry_run: args.dry_run,
                    retain_rollback_states: !args.ignore_rollback_states,
                },
            )?;
            for e in &report.pruned {
                let tag = if report.dry_run {
                    "[dry-run] would prune"
                } else {
                    "Pruned"
                };
                println!("{tag} {} {} ({})", e.name, e.version, e.path.display());
            }
            if report.pruned.is_empty() && report.retained_for_rollback == 0 && report.deferred == 0
            {
                println!(
                    "Nothing to prune (every package has <= {} version(s)).",
                    args.keep
                );
            } else if !report.pruned.is_empty() && !report.dry_run {
                println!(
                    "\nPruned {} file(s) ({}). Run `arx publish` to update metadata.",
                    report.pruned.len(),
                    human_bytes(report.bytes_freed)
                );
            } else if !report.pruned.is_empty() && report.dry_run {
                println!("\nWould free {}.", human_bytes(report.bytes_freed));
            }
            if report.deferred > 0 {
                println!(
                    "Deferred {} file(s) within grace period ({} days).",
                    report.deferred, args.grace
                );
            }
            if report.retained_for_rollback > 0 {
                println!(
                    "Kept {} older file(s) pinned by retained rollback states.",
                    report.retained_for_rollback
                );
            }
            Ok(())
        }
        Command::Serve(args) => cmd_serve(&args).await,
        Command::Watch(args) => cmd_watch(&args).await,
        Command::Compose(args) => {
            compose::generate(&args.root, &args.out, &args.addr)?;
            tracing::info!(root = %args.root.display(), "wrote Dockerfile + docker-compose.yml");
            Ok(())
        }
        Command::Export(args) => cmd_export(&args),
        Command::Cutover(args) => cmd_cutover(&args),
    }
}

fn cmd_search(args: &cli::SearchArgs) -> Result<()> {
    let apt_pool_root = selected_apt_pool_root(&args.root, args.apt, args.yum)?;
    let yum_base = selected_yum_base(&args.root, args.apt, args.yum)?;
    let entries = pool::search(
        &apt_pool_root,
        &yum_base,
        pool::SearchOptions {
            query: args.query.as_deref(),
            name_prefix: args.name_prefix.as_deref(),
            version: args.version.as_deref(),
            arch: args.arch.as_deref(),
            scope: args.scope.as_deref(),
            apt: args.apt,
            yum: args.yum,
        },
    )?;
    let infos: Vec<_> = entries.iter().map(pool::Entry::info).collect();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&infos)?);
    } else if infos.is_empty() {
        println!("No packages matched.");
    } else {
        for info in infos {
            let kind = match info.kind {
                pool::Kind::Apt => "apt",
                pool::Kind::Yum => "yum",
            };
            println!(
                "{}\t{}\t{}\t{}\t{}",
                info.name, info.version, info.arch, info.scope, kind
            );
        }
    }
    Ok(())
}

fn cmd_export(args: &cli::ExportArgs) -> Result<()> {
    if args.apt_out.is_none() && args.yum_flat_out.is_none() {
        bail!("nothing to export; pass --apt-out and/or --yum-flat-out");
    }

    let cfg = Config::load(&args.root).context("loading config; run `arx init` first")?;
    let formats = export_formats(args);
    hooks::run(
        &args.root,
        &cfg,
        hooks::HookEvent::PreExport,
        &hooks::HookContext::new().with("ARX_FORMATS", formats.clone()),
    )?;
    let key = if args.yum_flat_out.is_some() {
        load_key(&args.root, &cfg)?
    } else {
        None
    };
    let passphrase = if args.yum_flat_out.is_some() && cfg.signing.enabled && cfg.signing.encrypted
    {
        match resolve_passphrase(args.passphrase_file.as_deref())? {
            Some(p) => p,
            None => bail!(
                "signing key is encrypted; provide --passphrase-file or set ARX_KEY_PASSPHRASE"
            ),
        }
    } else {
        String::new()
    };

    let mut lines = Vec::new();
    if let Some(out) = &args.apt_out {
        let path = export::export_apt(&args.root, &cfg, out)?;
        lines.push(format!("apt export: {}", path.display()));
    }
    if let Some(out) = &args.yum_flat_out {
        let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);
        let report = export::export_yum_flat(
            &args.root,
            &cfg,
            out,
            repo,
            &args.arch,
            key.as_ref(),
            &passphrase,
        )?;
        lines.push(format!(
            "yum flat export: {} (copied {} rpm(s), indexed {} rpm(s), arches: {})",
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
    let summary = lines.join("\n");
    hooks::run(
        &args.root,
        &cfg,
        hooks::HookEvent::PostExport,
        &hooks::HookContext::new()
            .with("ARX_FORMATS", formats)
            .with("ARX_SUMMARY", summary.clone()),
    )?;
    println!("{summary}");
    Ok(())
}

fn export_formats(args: &cli::ExportArgs) -> String {
    let mut formats = Vec::new();
    if args.apt_out.is_some() {
        formats.push("apt");
    }
    if args.yum_flat_out.is_some() {
        formats.push("yum");
    }
    formats.join(",")
}

fn cmd_cutover(args: &cli::CutoverArgs) -> Result<()> {
    let cfg = Config::load(&args.root).context("loading config; run `arx init` first")?;
    let needs_key = !args.no_publish || args.yum_flat_live.is_some();
    let key = if needs_key {
        load_key(&args.root, &cfg)?
    } else {
        None
    };
    let passphrase = if needs_key && cfg.signing.enabled && cfg.signing.encrypted {
        match resolve_passphrase(args.passphrase_file.as_deref())? {
            Some(p) => p,
            None => bail!(
                "signing key is encrypted; provide --passphrase-file or set ARX_KEY_PASSPHRASE"
            ),
        }
    } else {
        String::new()
    };
    let report = cutover::run(
        &cutover::CutoverOptions {
            root: args.root.clone(),
            apt_live: args.apt_live.clone(),
            yum_flat_live: args.yum_flat_live.clone(),
            staging_dir: args.staging_dir.clone(),
            repo: args.repo.clone(),
            arch: args.arch.clone(),
            dry_run: args.dry_run,
            no_publish: args.no_publish,
            full: false,
            require_signed_rpms: args.require_signed_rpms,
        },
        &cfg,
        key.as_ref(),
        &passphrase,
    )?;
    println!("cutover staging: {}", report.staging_root.display());
    for line in report.lines {
        println!("{line}");
    }
    Ok(())
}

/// Load the signing key referenced by config, if signing is enabled.
fn load_key(root: &Path, cfg: &Config) -> Result<Option<SignedSecretKey>> {
    if !cfg.signing.enabled {
        return Ok(None);
    }
    let path = cfg.private_key_path(root)?;
    if !path.exists() {
        bail!(
            "signing enabled but no key at {}; run `arx key generate`",
            path.display()
        );
    }
    let armored =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(Some(signing::load_secret_key(&armored)?))
}

/// Resolve a key passphrase from a file (preferred) or the `ARX_KEY_PASSPHRASE`
/// env var. Returns `None` when neither is set (key stays unencrypted). No
/// interactive prompt — keeping the 5-minute quickstart frictionless.
fn resolve_passphrase(file: Option<&Path>) -> Result<Option<String>> {
    if let Some(path) = file {
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("reading passphrase file {}", path.display()))?;
        return Ok(Some(s.trim_end_matches(['\n', '\r']).to_string()));
    }
    match std::env::var("ARX_KEY_PASSPHRASE") {
        Ok(s) if !s.is_empty() => Ok(Some(s)),
        _ => Ok(None),
    }
}

fn warn_if_unencrypted(passphrase: &str) {
    if passphrase.is_empty() {
        tracing::warn!(
            "signing key stored UNENCRYPTED; set ARX_KEY_PASSPHRASE or --passphrase-file to encrypt it"
        );
    }
}

async fn cmd_init(args: &cli::InitArgs) -> Result<()> {
    let root = &args.path;

    let mut cfg = Config::default();
    // Custom key dir: before key generation, update the config.
    if let Some(ref kd) = args.key_dir {
        cfg.signing.keys_dir = kd.clone();
        cfg.signing.private_key = format!("{kd}/private.asc");
        cfg.signing.public_key = format!("{kd}/public.asc");
    }
    // Custom pool dir.
    if let Some(ref pd) = args.pool_dir {
        cfg.apt.pool_dir = pd.clone();
    }

    // Create directory structure using config-driven paths.
    let pool = cfg.checked_apt_pool_root(root)?;
    let keys = cfg.keys_dir(root)?;
    let yum = cfg.checked_yum_base(root)?;
    for dir in [
        pool.as_path(),
        &root.join("apt/dists"),
        yum.as_path(),
        keys.as_path(),
    ] {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }

    // New repos are secure-by-default: expire the apt Release 7 days out so a
    // stale-metadata (freeze/replay) attack has only a small window. Republishing
    // refreshes it. (Serde default stays 0 — existing repos are untouched.)
    cfg.apt.valid_days = 7;
    if args.no_key {
        cfg.signing.enabled = false;
    } else {
        let passphrase = resolve_passphrase(args.passphrase_file.as_deref())?.unwrap_or_default();
        generate_and_store_key(root, &cfg, &passphrase)?;
        cfg.signing.encrypted = !passphrase.is_empty();
        warn_if_unencrypted(&passphrase);
    }
    cfg.save(root)?;
    tracing::info!(root = %root.display(), "initialized repository");
    println!("Initialized arx repository at {}", root.display());
    Ok(())
}

fn generate_and_store_key(root: &Path, cfg: &Config, passphrase: &str) -> Result<()> {
    tracing::info!("generating RSA-2048 signing key...");
    let key = signing::generate_key(&cfg.signing.user_id, passphrase)?;
    let private_key = cfg.private_key_path(root)?;
    let public_key = cfg.public_key_path(root)?;
    std::fs::create_dir_all(private_key.parent().unwrap()).ok();
    write_private_key(&private_key, key.private_armored.as_bytes())?;
    std::fs::write(&public_key, &key.public_armored).context("writing public key")?;
    Ok(())
}

fn write_private_key(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("writing private key {}", path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("writing private key {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("syncing private key {}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restricting private key {}", path.display()))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
            .with_context(|| format!("writing private key {}", path.display()))
    }
}

#[cfg(unix)]
fn restrict_private_key(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting private key {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_private_key(_path: &Path) -> Result<()> {
    Ok(())
}

fn cmd_key(args: &cli::KeyArgs) -> Result<()> {
    let root = &args.root;
    let mut cfg = Config::load(root).unwrap_or_default();
    match &args.action {
        KeyAction::Generate => {
            let passphrase =
                resolve_passphrase(args.passphrase_file.as_deref())?.unwrap_or_default();
            generate_and_store_key(root, &cfg, &passphrase)?;
            cfg.signing.encrypted = !passphrase.is_empty();
            cfg.save(root)?;
            warn_if_unencrypted(&passphrase);
            println!("Wrote {}", cfg.public_key_path(root)?.display());
        }
        KeyAction::Rotate => {
            let passphrase =
                resolve_passphrase(args.passphrase_file.as_deref())?.unwrap_or_default();
            // Back up the current key.
            let priv_path = cfg.private_key_path(root)?;
            let old = format!("{}.old", priv_path.display());
            if priv_path.exists() {
                std::fs::copy(&priv_path, &old)
                    .with_context(|| format!("backing up {} → {}", priv_path.display(), old))?;
                restrict_private_key(std::path::Path::new(&old))?;
                println!("Backed up old key to {old}");
            }
            // Generate and store the new key.
            generate_and_store_key(root, &cfg, &passphrase)?;
            cfg.signing.encrypted = !passphrase.is_empty();
            cfg.save(root)?;
            warn_if_unencrypted(&passphrase);
            println!(
                "Key rotated — new public key at {}",
                cfg.public_key_path(root)?.display()
            );
            println!("Old key backed up to {old}");
        }
        KeyAction::Revoke => {
            let old = format!("{}.old", cfg.private_key_path(root)?.display());
            if std::path::Path::new(&old).exists() {
                std::fs::remove_file(&old).with_context(|| format!("removing old key {old}"))?;
                println!("Revoked old key ({old} deleted)");
            } else {
                println!("No old key found at {old} — nothing to revoke");
            }
        }
        KeyAction::Import { file } => {
            let armored = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            let key = signing::load_secret_key(&armored)?;
            // An imported key may be encrypted; the passphrase unlocks it to
            // derive the public key.
            let passphrase =
                resolve_passphrase(args.passphrase_file.as_deref())?.unwrap_or_default();
            let private_key = cfg.private_key_path(root)?;
            let public_key = cfg.public_key_path(root)?;
            std::fs::create_dir_all(private_key.parent().unwrap()).ok();
            write_private_key(&private_key, armored.as_bytes())?;
            let public = signing::public_from_secret(&key, &passphrase)?;
            std::fs::write(&public_key, public).context("writing public key")?;
            cfg.signing.enabled = true;
            cfg.signing.encrypted = !passphrase.is_empty();
            cfg.save(root)?;
            println!("Imported key, wrote {}", public_key.display());
        }
        KeyAction::Export => {
            let path = cfg.public_key_path(root)?;
            let pubkey = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            print!("{pubkey}");
        }
    }
    Ok(())
}

/// Copy one `.deb`/`.rpm` into the pool, returning its destination path.
/// `.deb` goes to the configured apt pool under `<component>`;
/// `.rpm` goes to the configured yum base under `<repo>/<arch>`.
fn add_to_pool(
    root: &Path,
    cfg: &Config,
    pkg: &Path,
    component: &str,
    repo: &str,
    cache: &mut cache::PackageFileCache,
) -> Result<(PathBuf, cache::CacheDecision)> {
    let component = scope::validate_scope_name(component, "apt component")?;
    let repo = scope::validate_scope_name(repo, "yum repo")?;
    let ext = pkg.extension().and_then(|e| e.to_str()).unwrap_or("");
    let dest_dir = match ext {
        "deb" => cfg.checked_apt_pool_root(root)?.join(component),
        "rpm" => {
            let mut reader = createrepo_rs::rpm::RpmReader::open(pkg)
                .with_context(|| format!("opening {}", pkg.display()))?;
            let arch = reader
                .read_package()
                .with_context(|| format!("reading {}", pkg.display()))?
                .arch;
            let arch = scope::validate_scope_name(&arch, "yum arch")?;
            cfg.checked_yum_base(root)?.join(repo).join(arch)
        }
        other => bail!("{}: unsupported package type .{other}", pkg.display()),
    };
    std::fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(pkg.file_name().unwrap());
    let decision = copy_package_with_cache(pkg, &dest, cache)?;
    Ok((dest, decision))
}

fn copy_package_with_cache(
    source: &Path,
    dest: &Path,
    cache: &mut cache::PackageFileCache,
) -> Result<cache::CacheDecision> {
    let source_fp = cache::fingerprint(source)?;

    if let Ok(dest_fp) = cache::fingerprint(dest) {
        if let Some(entry) = cache.get(dest) {
            if entry.matches_source(source_fp) && entry.matches_dest(dest_fp) {
                return Ok(cache::CacheDecision::Hit);
            }
        }

        let source_hash = cache::content_digest_file(source)?;
        let dest_matches = cache
            .get(dest)
            .filter(|entry| entry.matches_dest(dest_fp))
            .is_some_and(|entry| entry.content_digest == source_hash)
            || cache::content_digest_file(dest)? == source_hash;

        if dest_matches {
            cache.update(source_fp, dest, dest_fp, source_hash);
            return Ok(cache::CacheDecision::Hit);
        }
    }

    std::fs::copy(source, dest).with_context(|| format!("copying {}", source.display()))?;
    let dest_fp = cache::fingerprint(dest)?;
    let source_hash = cache::content_digest_file(source)?;
    cache.update(source_fp, dest, dest_fp, source_hash);
    Ok(cache::CacheDecision::Miss)
}

fn is_supported_package_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("deb" | "rpm")
    )
}

fn expand_add_inputs(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut packages = Vec::new();

    for input in inputs {
        if input.is_dir() {
            let before = packages.len();
            for entry in walkdir::WalkDir::new(input).follow_links(false) {
                let entry = entry.with_context(|| format!("walking {}", input.display()))?;
                let path = entry.path();
                if entry.file_type().is_file() && is_supported_package_path(path) {
                    packages.push(path.to_path_buf());
                }
            }
            if packages.len() == before {
                bail!(
                    "{}: directory contains no supported package files (.deb or .rpm)",
                    input.display()
                );
            }
        } else {
            packages.push(input.clone());
        }
    }

    packages.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    packages.dedup();

    Ok(packages)
}

fn cmd_add(args: &cli::AddArgs) -> Result<()> {
    let root = &args.root;
    let cfg = Config::load(root).unwrap_or_default();
    let component = args.component.as_deref().unwrap_or(&cfg.apt.component);
    let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);

    let mut cache = cache::PackageFileCache::load(root);
    let mut cache_dirty = false;
    for pkg in expand_add_inputs(&args.packages)? {
        let (dest, decision) = add_to_pool(root, &cfg, &pkg, component, repo, &mut cache)?;
        cache_dirty = true;
        match decision {
            cache::CacheDecision::Hit => tracing::info!(file = %dest.display(), "add cache hit"),
            cache::CacheDecision::Miss => tracing::info!(file = %dest.display(), "added"),
        }
        println!("Added {}", dest.display());
    }
    if cache_dirty {
        if let Err(err) = cache.save(root) {
            tracing::warn!(error = %err, "failed to save package cache");
        }
    }
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PublishDirState {
    version: u32,
    source_dir: String,
    recursive: bool,
    packages: Vec<PublishDirEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PublishDirEntry {
    path: String,
    size: u64,
    modified_ns: u128,
    changed_ns: u128,
    digest: String,
}

#[derive(Debug)]
struct PublishDirSource {
    path: PathBuf,
    entry: PublishDirEntry,
}

fn cmd_publish_dir(args: &cli::PublishDirArgs) -> Result<()> {
    let root = args.root.clone();
    let cfg = Config::load(&root).context("loading config; run `arx init` first")?;
    let component = args.component.as_deref().unwrap_or(&cfg.apt.component);
    let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);
    let state_file = publish_dir_state_file(args);
    let sources = collect_publish_dir_sources(&args.dir, args.recursive, false)?;

    if sources.is_empty() {
        println!(
            "publish-dir: no source packages found in {}",
            args.dir.display()
        );
        return Ok(());
    }

    let previous = load_publish_dir_state(&state_file)?;
    if !args.force
        && publish_dir_fingerprints_match(previous.as_ref(), &sources, &args.dir, args.recursive)
        && publish_dir_outputs_ok(&root, &cfg, args, &sources)?
    {
        println!(
            "publish-dir: fast no-op: {} source package(s) unchanged",
            sources.len()
        );
        return Ok(());
    }

    let sources = collect_publish_dir_sources(&args.dir, args.recursive, true)?;
    if !args.force
        && publish_dir_state_matches(previous.as_ref(), &sources, &args.dir, args.recursive)
        && publish_dir_outputs_ok(&root, &cfg, args, &sources)?
    {
        save_publish_dir_state(&state_file, &args.dir, args.recursive, &sources)?;
        println!(
            "publish-dir: verified no-op: {} source package(s) unchanged",
            sources.len()
        );
        return Ok(());
    }

    let mut package_cache = cache::PackageFileCache::load(&root);
    let mut cache_dirty = false;
    for source in &sources {
        let (dest, decision) = add_to_pool(
            &root,
            &cfg,
            &source.path,
            component,
            repo,
            &mut package_cache,
        )?;
        cache_dirty = true;
        match decision {
            cache::CacheDecision::Hit => tracing::info!(file = %dest.display(), "add cache hit"),
            cache::CacheDecision::Miss => tracing::info!(file = %dest.display(), "added"),
        }
        println!("Added {}", dest.display());
    }
    if cache_dirty {
        if let Err(err) = package_cache.save(&root) {
            tracing::warn!(error = %err, "failed to save package cache");
        }
    }

    publish_dir_publish(&root, &cfg, args)?;
    if args.dry_run {
        println!("publish-dir: dry-run: source-directory state was not updated");
        return Ok(());
    }
    save_publish_dir_state(&state_file, &args.dir, args.recursive, &sources)?;
    println!(
        "publish-dir: ok: {} source package(s); state={}",
        sources.len(),
        state_file.display()
    );

    if let Some(sync_cmd) = args
        .sync_cmd
        .as_deref()
        .filter(|cmd| !cmd.trim().is_empty())
    {
        run_publish_dir_sync(sync_cmd, &root, &args.dir, sources.len())?;
        println!("publish-dir: sync requested");
    }

    Ok(())
}

fn publish_dir_publish(root: &Path, cfg: &Config, args: &cli::PublishDirArgs) -> Result<()> {
    let key = load_key(root, cfg)?;
    let passphrase = publish_passphrase(cfg, args.passphrase_file.as_deref())?;

    if args.apt_live.is_some() || args.yum_flat_live.is_some() {
        if args.apt && args.apt_live.is_none() {
            bail!("--apt requires --apt-live when publish-dir cutover options are used");
        }
        if args.yum && args.yum_flat_live.is_none() {
            bail!("--yum requires --yum-flat-live when publish-dir cutover options are used");
        }
        let report = cutover::run(
            &cutover::CutoverOptions {
                root: root.to_path_buf(),
                apt_live: args.apt_live.clone(),
                yum_flat_live: args.yum_flat_live.clone(),
                staging_dir: args.staging_dir.clone(),
                repo: args.repo.clone(),
                arch: args.arch.clone(),
                dry_run: args.dry_run,
                no_publish: false,
                full: args.full,
                require_signed_rpms: args.require_signed_rpms,
            },
            cfg,
            key.as_ref(),
            &passphrase,
        )?;
        println!("publish-dir staging: {}", report.staging_root.display());
        for line in report.lines {
            println!("{line}");
        }
        return Ok(());
    }

    let _lock = PublishLock::acquire(root)?;
    let publish_apt_selected = args.apt || !args.yum;
    let publish_yum_selected = args.yum || !args.apt;
    let formats = publish_formats(publish_apt_selected, publish_yum_selected);
    hooks::run(
        root,
        cfg,
        hooks::HookEvent::PrePublish,
        &hooks::HookContext::new().with("ARX_FORMATS", formats.clone()),
    )?;
    let incremental = !args.full;
    let mut lines = Vec::new();
    if publish_apt_selected {
        lines.push(
            publish_apt(
                root,
                cfg,
                key.as_ref(),
                &passphrase,
                cfg.apt.strict,
                incremental,
            )?
            .summary,
        );
    }
    if publish_yum_selected {
        lines.push(publish_yum(
            root,
            cfg,
            key.as_ref(),
            &passphrase,
            incremental,
        )?);
    }
    let summary = lines.join("; ");
    hooks::run(
        root,
        cfg,
        hooks::HookEvent::PostPublish,
        &hooks::HookContext::new()
            .with("ARX_FORMATS", formats)
            .with("ARX_SUMMARY", summary.clone()),
    )?;
    println!("Published: {summary}");
    Ok(())
}

fn publish_passphrase(cfg: &Config, passphrase_file: Option<&Path>) -> Result<String> {
    if cfg.signing.enabled && cfg.signing.encrypted {
        match resolve_passphrase(passphrase_file)? {
            Some(p) => Ok(p),
            None => bail!(
                "signing key is encrypted; provide --passphrase-file or set ARX_KEY_PASSPHRASE"
            ),
        }
    } else {
        Ok(String::new())
    }
}

fn collect_publish_dir_sources(
    dir: &Path,
    recursive: bool,
    include_digest: bool,
) -> Result<Vec<PublishDirSource>> {
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    let mut sources = Vec::new();
    let mut walker = walkdir::WalkDir::new(dir).min_depth(1).follow_links(false);
    if !recursive {
        walker = walker.max_depth(1);
    }
    for entry in walker {
        let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
        let path = entry.path();
        if !entry.file_type().is_file() || !is_supported_package_path(path) {
            continue;
        }
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let fp = cache::fingerprint(path)?;
        let digest = if include_digest {
            cache::content_digest_file(path)?
        } else {
            String::new()
        };
        sources.push(PublishDirSource {
            path: path.to_path_buf(),
            entry: PublishDirEntry {
                path: rel,
                size: fp.size,
                modified_ns: fp.modified_ns,
                changed_ns: fp.changed_ns,
                digest,
            },
        });
    }
    sources.sort_by(|a, b| a.entry.path.cmp(&b.entry.path));
    Ok(sources)
}

fn publish_dir_state_file(args: &cli::PublishDirArgs) -> PathBuf {
    args.state_file.clone().unwrap_or_else(|| {
        cache::cache_dir(&args.root)
            .join("publish-dir")
            .join("state.json")
    })
}

fn load_publish_dir_state(path: &Path) -> Result<Option<PublishDirState>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .with_context(|| format!("parsing {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

fn save_publish_dir_state(
    path: &Path,
    dir: &Path,
    recursive: bool,
    sources: &[PublishDirSource],
) -> Result<()> {
    let state = PublishDirState {
        version: 1,
        source_dir: dir.to_string_lossy().to_string(),
        recursive,
        packages: sources.iter().map(|source| source.entry.clone()).collect(),
    };
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(&state).context("serializing publish-dir state")?;
    std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

fn publish_dir_fingerprints_match(
    previous: Option<&PublishDirState>,
    sources: &[PublishDirSource],
    dir: &Path,
    recursive: bool,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    previous.version == 1
        && previous.source_dir == dir.to_string_lossy()
        && previous.recursive == recursive
        && previous.packages.len() == sources.len()
        && previous.packages.iter().zip(sources).all(|(old, new)| {
            old.path == new.entry.path
                && old.size == new.entry.size
                && old.modified_ns == new.entry.modified_ns
                && old.changed_ns == new.entry.changed_ns
        })
}

fn publish_dir_state_matches(
    previous: Option<&PublishDirState>,
    sources: &[PublishDirSource],
    dir: &Path,
    recursive: bool,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    previous.version == 1
        && previous.source_dir == dir.to_string_lossy()
        && previous.recursive == recursive
        && previous
            .packages
            .iter()
            .eq(sources.iter().map(|source| &source.entry))
}

fn publish_dir_outputs_ok(
    root: &Path,
    cfg: &Config,
    args: &cli::PublishDirArgs,
    sources: &[PublishDirSource],
) -> Result<bool> {
    let has_deb = sources
        .iter()
        .any(|source| source.path.extension().and_then(|e| e.to_str()) == Some("deb"));
    let has_rpm = sources
        .iter()
        .any(|source| source.path.extension().and_then(|e| e.to_str()) == Some("rpm"));

    if let Some(live) = &args.apt_live {
        if !apt_layout_ok(live, cfg) {
            return Ok(false);
        }
    } else if args.apt || (!args.yum && has_deb) {
        let apt_root = root.join("apt");
        if !apt_layout_ok(&apt_root, cfg) {
            return Ok(false);
        }
    }

    if let Some(live) = &args.yum_flat_live {
        if !yum_flat_layout_ok(live) {
            return Ok(false);
        }
    } else if args.yum || (!args.apt && has_rpm) {
        let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);
        let base = cfg.checked_yum_base(root)?.join(repo);
        if !yum_pool_layout_ok(&base) {
            return Ok(false);
        }
    }

    Ok(true)
}

fn apt_layout_ok(root: &Path, cfg: &Config) -> bool {
    root.join("dists")
        .join(&cfg.apt.dist)
        .join("Release")
        .is_file()
        && root
            .join("dists")
            .join(&cfg.apt.dist)
            .join(&cfg.apt.component)
            .join("binary-amd64")
            .join("Packages.gz")
            .is_file()
}

fn yum_flat_layout_ok(root: &Path) -> bool {
    let repomd = root.join("repodata/repomd.xml");
    repomd.is_file()
        && std::fs::read_to_string(repomd)
            .map(|text| text.contains(".xml.gz") && !text.contains(".xml.xz"))
            .unwrap_or(false)
}

fn yum_pool_layout_ok(repo_root: &Path) -> bool {
    walkdir::WalkDir::new(repo_root)
        .min_depth(2)
        .max_depth(3)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .strip_prefix(repo_root)
                    .map(|rel| rel.ends_with("repodata/repomd.xml"))
                    .unwrap_or(false)
        })
}

fn run_publish_dir_sync(
    sync_cmd: &str,
    root: &Path,
    dir: &Path,
    package_count: usize,
) -> Result<()> {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(sync_cmd)
        .current_dir(root)
        .env("ARX_ROOT", root)
        .env("ARX_SOURCE_DIR", dir)
        .env("ARX_PACKAGE_COUNT", package_count.to_string())
        .status()
        .with_context(|| format!("running --sync-cmd: {sync_cmd}"))?;
    if !status.success() {
        bail!("--sync-cmd failed with status {status}: {sync_cmd}");
    }
    Ok(())
}

fn cmd_cache(args: &cli::CacheArgs) -> Result<()> {
    match args.action {
        cli::CacheAction::Status => {
            let path = cache::package_cache_path(&args.root);
            let cache = cache::PackageFileCache::load(&args.root);
            println!("cache version: {}", cache.version);
            println!("cache path: {}", path.display());
            println!("package entries: {}", cache.len());
            println!("exists: {}", path.exists());
        }
        cli::CacheAction::Rebuild => {
            let cfg = Config::load(&args.root).unwrap_or_default();
            let paths = collect_pool_package_paths(&args.root, &cfg)?;
            let count = paths.len();
            let cache = cache::rebuild_from_paths_with_jobs(&args.root, paths, args.jobs)?;
            println!(
                "rebuilt cache v{} at {} with {} package file(s)",
                cache.version,
                cache::package_cache_path(&args.root).display(),
                count
            );
        }
        cli::CacheAction::Clear => {
            cache::PackageFileCache::clear(&args.root)?;
            println!(
                "cleared {}",
                cache::package_cache_path(&args.root).display()
            );
        }
    }
    Ok(())
}

fn collect_pool_package_paths(root: &Path, cfg: &Config) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let roots = [
        cfg.checked_apt_pool_root(root)?,
        cfg.checked_yum_base(root)?,
    ];
    for base in roots {
        if !base.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&base).follow_links(false) {
            let entry = entry.with_context(|| format!("walking {}", base.display()))?;
            let path = entry.path();
            if entry.file_type().is_file() && is_supported_package_path(path) {
                paths.push(path.to_path_buf());
            }
        }
    }
    paths.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    paths.dedup();
    Ok(paths)
}

fn load_pack_manifest(path: Option<&Path>) -> Result<arx_pack::Manifest> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => {
            let cargo = Path::new("Cargo.toml");
            if !cargo.exists() {
                bail!("no manifest given and no ./Cargo.toml here — pass a manifest path or run in a crate root");
            }
            cargo.to_path_buf()
        }
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if path.file_name().map(|n| n == "Cargo.toml").unwrap_or(false) {
        let crate_root = path.parent().expect("Cargo.toml has a parent directory");
        arx_pack::Manifest::from_cargo_toml_at(&text, crate_root)
            .with_context(|| format!("from {}", path.display()))
    } else {
        arx_pack::Manifest::from_toml_str(&text)
    }
}

async fn cmd_mirror(args: cli::MirrorArgs) -> Result<()> {
    tokio::task::spawn_blocking(move || cmd_mirror_blocking(&args))
        .await
        .context("mirror task panicked")?
}

fn cmd_mirror_blocking(args: &cli::MirrorArgs) -> Result<()> {
    let cfg = Config::load(&args.root).unwrap_or_default();
    let dist = args.dist.as_deref().unwrap_or(&cfg.apt.dist);
    let comp = args.component.as_deref().unwrap_or(&cfg.apt.component);
    scope::validate_scope_name(comp, "apt component")?;

    let (downloaded, removed, total) = mirror::mirror_apt(
        &args.root, &cfg, &args.url, dist, comp, &args.arch, args.prune,
    )?;

    println!(
        "Mirror sync complete: {downloaded} downloaded, {removed} pruned, {total} total upstream"
    );
    if args.publish {
        let key = load_key(&args.root, &cfg)?;
        let passphrase = resolve_passphrase(None)?.unwrap_or_default();
        let _lock = PublishLock::acquire(&args.root)?;
        let apt = publish_apt(&args.root, &cfg, key.as_ref(), &passphrase, false, true)?;
        println!("Published: {}", apt.summary);
    }
    Ok(())
}

async fn cmd_import(args: cli::ImportArgs) -> Result<()> {
    tokio::task::spawn_blocking(move || cmd_import_blocking(&args))
        .await
        .context("import task panicked")?
}

fn cmd_import_blocking(args: &cli::ImportArgs) -> Result<()> {
    let cfg = Config::load(&args.root).unwrap_or_default();
    let do_apt = args.apt || !args.yum;
    let do_yum = args.yum || !args.apt;
    let mut total = 0usize;

    if do_apt {
        let dist = args.dist.as_deref().unwrap_or(&cfg.apt.dist);
        let comp = args.component.as_deref().unwrap_or(&cfg.apt.component);
        scope::validate_scope_name(comp, "apt component")?;
        let n = import::import_apt(&import::ImportOpts {
            root: &args.root,
            cfg: &cfg,
            base_url: &args.url,
            dist,
            component: comp,
            arch: &args.arch,
            match_name: args.match_name.as_deref(),
            limit: args.limit,
        })?;
        total += n;
    }
    if do_yum {
        let repo = args.component.as_deref().unwrap_or(&cfg.yum.repo);
        scope::validate_scope_name(repo, "yum repo")?;
        let n = import::import_yum(&args.root, &cfg, &args.url, repo, args.limit, args.strict)?;
        total += n;
    }

    if args.publish {
        let cfg = Config::load(&args.root).unwrap_or(cfg);
        let key = load_key(&args.root, &cfg)?;
        let passphrase = resolve_passphrase(None)?.unwrap_or_default();
        let _lock = PublishLock::acquire(&args.root)?;
        let mut published = Vec::new();
        if do_apt {
            published.push(
                publish_apt(
                    &args.root,
                    &cfg,
                    key.as_ref(),
                    &passphrase,
                    cfg.apt.strict,
                    true,
                )?
                .summary,
            );
        }
        if do_yum {
            published.push(publish_yum(
                &args.root,
                &cfg,
                key.as_ref(),
                &passphrase,
                true,
            )?);
        }
        println!(
            "Imported {total} package(s). Published: {}",
            published.join("; ")
        );
    } else {
        println!("Imported {total} package(s). Run `arx publish` to update metadata.");
    }
    Ok(())
}

fn cmd_promote(args: &cli::PromoteArgs) -> Result<()> {
    let cfg = Config::load(&args.root).unwrap_or_default();
    let from = scope::validate_scope_name(&args.from, "source scope")?;
    let to = scope::validate_scope_name(&args.to, "destination scope")?;
    let do_apt = args.apt || !args.yum;
    let do_yum = args.yum || !args.apt;
    let mut moved = 0usize;

    if do_apt {
        let src = cfg.checked_apt_pool_root(&args.root)?.join(from);
        let dst = cfg.checked_apt_pool_root(&args.root)?.join(to);
        if src.is_dir() {
            for entry in walkdir::WalkDir::new(&src)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let p = entry.path();
                if p.is_file() && p.extension().map(|e| e == "deb").unwrap_or(false) {
                    let ctrl = arx_debrepo::deb::read_control(p)
                        .with_context(|| format!("reading {}", p.display()))?;
                    let name = ctrl.package()?;
                    let version = ctrl.version()?;
                    if name == args.name && args.version.as_deref().is_none_or(|v| version == v) {
                        let dest = dst.join(p.file_name().unwrap());
                        std::fs::create_dir_all(&dst)?;
                        std::fs::rename(p, &dest)
                            .with_context(|| format!("promoting {}", p.display()))?;
                        println!("Promoted {name} {version} {from} → {to}");
                        moved += 1;
                    }
                }
            }
        }
    }
    if do_yum {
        let yum_base = cfg.checked_yum_base(&args.root)?;
        for entry in walkdir::WalkDir::new(&yum_base)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "rpm").unwrap_or(false) {
                let mut reader = createrepo_rs::rpm::RpmReader::open(p)
                    .with_context(|| format!("opening {}", p.display()))?;
                let pkg = reader.read_package()?;
                if pkg.name == args.name
                    && args.version.as_deref().is_none_or(|v| pkg.version == v)
                    && p.parent()
                        .and_then(|arch_dir| arch_dir.parent())
                        .and_then(|repo_dir| repo_dir.file_name())
                        .map(|n| n.to_string_lossy())
                        == Some(from.into())
                {
                    let arch = scope::validate_scope_name(&pkg.arch, "yum arch")?;
                    let dst = yum_base.join(to).join(arch);
                    std::fs::create_dir_all(&dst)?;
                    std::fs::rename(p, dst.join(p.file_name().unwrap()))?;
                    println!("Promoted {} {} {} → {}", pkg.name, pkg.version, from, to);
                    moved += 1;
                }
            }
        }
    }

    if moved == 0 {
        println!("No packages matched '{}'.", args.name);
    } else {
        println!("Promoted {moved} package(s). Run `arx publish` to update metadata.");
    }
    Ok(())
}

fn cmd_pack(args: &cli::PackArgs) -> Result<()> {
    let manifest = load_pack_manifest(args.manifest.as_deref())?;

    // Default (no flags): build all three formats.
    let all = !args.deb && !args.rpm && !args.apk;
    let do_deb = args.deb || all;
    let do_rpm = args.rpm || all;
    let do_apk = args.apk || all;
    let mut built = Vec::new();
    if do_deb {
        built.push(arx_pack::build_deb(&manifest, &args.out).context("building .deb")?);
    }
    if do_rpm {
        built.push(arx_pack::build_rpm(&manifest, &args.out).context("building .rpm")?);
    }
    if do_apk {
        built.push(arx_pack::build_apk(&manifest, &args.out).context("building .apk")?);
    }
    for p in &built {
        println!("Built {}", p.display());
    }

    if args.add {
        let cfg = Config::load(&args.root).unwrap_or_default();
        let component = args.component.as_deref().unwrap_or(&cfg.apt.component);
        let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);
        let mut cache = cache::PackageFileCache::load(&args.root);
        let mut cache_dirty = false;
        for p in &built {
            match p.extension().and_then(|e| e.to_str()) {
                Some("deb" | "rpm") => {
                    let (dest, _) = add_to_pool(&args.root, &cfg, p, component, repo, &mut cache)?;
                    cache_dirty = true;
                    println!("Added {}", dest.display());
                }
                Some("apk") => {
                    println!(
                        "Skipped {} (.apk repositories are not supported by arx add)",
                        p.display()
                    );
                }
                _ => {}
            }
        }
        if cache_dirty {
            if let Err(err) = cache.save(&args.root) {
                tracing::warn!(error = %err, "failed to save package cache");
            }
        }
        println!("\nRun `arx publish` to update repository metadata.");
    }
    Ok(())
}

/// Exclusive publish lock, released on drop. Prevents concurrent `arx publish`
/// runs from corrupting each other's metadata.
struct PublishLock {
    path: std::path::PathBuf,
}

impl PublishLock {
    fn acquire(root: &Path) -> Result<Self> {
        let path = root.join(".arx-publish.lock");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                use std::io::Write as _;
                let _ = writeln!(f, "{}", std::process::id());
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => bail!(
                "another publish is in progress (lock {}); remove it if stale",
                path.display()
            ),
            Err(e) => Err(e).context("creating publish lock"),
        }
    }
}

impl Drop for PublishLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn cmd_publish(args: &cli::PublishArgs) -> Result<()> {
    let root = args.root.clone();
    let cfg = Config::load(&root).context("loading config; run `arx init` first")?;
    let key = load_key(&root, &cfg)?;

    // Resolve the passphrase up front if the key is encrypted.
    let passphrase = if cfg.signing.enabled && cfg.signing.encrypted {
        match resolve_passphrase(args.passphrase_file.as_deref())? {
            Some(p) => p,
            None => bail!(
                "signing key is encrypted; provide --passphrase-file or set ARX_KEY_PASSPHRASE"
            ),
        }
    } else {
        String::new()
    };

    if args.apt_live.is_some() || args.yum_flat_live.is_some() {
        if args.apt && args.apt_live.is_none() {
            bail!("--apt requires --apt-live when publish cutover options are used");
        }
        if args.yum && args.yum_flat_live.is_none() {
            bail!("--yum requires --yum-flat-live when publish cutover options are used");
        }
        let report = cutover::run(
            &cutover::CutoverOptions {
                root,
                apt_live: args.apt_live.clone(),
                yum_flat_live: args.yum_flat_live.clone(),
                staging_dir: args.staging_dir.clone(),
                repo: args.repo.clone(),
                arch: args.arch.clone(),
                dry_run: args.dry_run,
                no_publish: false,
                full: args.full,
                require_signed_rpms: args.require_signed_rpms,
            },
            &cfg,
            key.as_ref(),
            &passphrase,
        )?;
        println!("publish staging: {}", report.staging_root.display());
        for line in report.lines {
            println!("{line}");
        }
        return Ok(());
    }

    // Hold an exclusive lock for the whole publish.
    let _lock = PublishLock::acquire(&root)?;

    // both flags off means publish both.
    let do_apt = args.apt || !args.yum;
    let do_yum = args.yum || !args.apt;
    let formats = publish_formats(do_apt, do_yum);
    // CLI flag OR config opt-in: any skipped package becomes a hard error.
    let strict = args.strict || cfg.apt.strict;
    // `--full` disables the incremental cache (rebuild from scratch).
    let incremental = !args.full;

    hooks::run(
        &root,
        &cfg,
        hooks::HookEvent::PrePublish,
        &hooks::HookContext::new().with("ARX_FORMATS", formats.clone()),
    )?;

    // CPU-bound generation runs on a blocking thread.
    let publish_cfg = cfg.clone();
    let summary = tokio::task::spawn_blocking(move || -> Result<String> {
        let mut lines = Vec::new();
        if do_apt {
            lines.push(
                publish_apt(
                    &root,
                    &publish_cfg,
                    key.as_ref(),
                    &passphrase,
                    strict,
                    incremental,
                )?
                .summary,
            );
        }
        if do_yum {
            lines.push(publish_yum(
                &root,
                &publish_cfg,
                key.as_ref(),
                &passphrase,
                incremental,
            )?);
        }
        Ok(lines.join("\n"))
    })
    .await
    .context("publish task panicked")??;

    hooks::run(
        &args.root,
        &cfg,
        hooks::HookEvent::PostPublish,
        &hooks::HookContext::new()
            .with("ARX_FORMATS", formats)
            .with("ARX_SUMMARY", summary.clone()),
    )?;
    println!("{summary}");
    Ok(())
}

fn publish_formats(do_apt: bool, do_yum: bool) -> String {
    let mut formats = Vec::new();
    if do_apt {
        formats.push("apt");
    }
    if do_yum {
        formats.push("yum");
    }
    formats.join(",")
}

/// Print a loud, human-visible summary of skipped packages to stderr so a
/// forgiving publish can't silently drop a package behind a green exit code.
/// Fetch a GitHub Actions OIDC JWT. (ADR-0014.)
async fn fetch_oidc_token(
    request_url: &str,
    request_token: &str,
    audience: &str,
) -> Result<String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{request_url}&audience={audience}"))
        .bearer_auth(request_token)
        .header("Accept", "application/json")
        .send()
        .await
        .context("fetching OIDC token")?;
    #[derive(serde::Deserialize)]
    struct OidcResp {
        value: String,
    }
    let body: OidcResp = resp.json().await.context("parsing OIDC response")?;
    Ok(body.value)
}

fn selected_apt_pool_root(root: &Path, apt: bool, yum: bool) -> Result<PathBuf> {
    if apt || !yum {
        let config_path = root.join("arx.toml");
        if config_path.exists() {
            Config::load(root)?.checked_apt_pool_root(root)
        } else {
            Config::default().checked_apt_pool_root(root)
        }
    } else {
        Ok(root.join("apt/pool"))
    }
}

fn selected_yum_base(root: &Path, apt: bool, yum: bool) -> Result<PathBuf> {
    if yum || !apt {
        let config_path = root.join("arx.toml");
        if config_path.exists() {
            Config::load(root)?.checked_yum_base(root)
        } else {
            Config::default().checked_yum_base(root)
        }
    } else {
        Ok(root.join("yum"))
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats_correctly() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(1048576), "1.0 MiB");
        assert_eq!(human_bytes(1073741824), "1.0 GiB");
    }

    #[test]
    fn target_link_rejects_path_traversal() {
        let root = Path::new("/repo");
        let cfg = Config::default();
        assert!(target_link(root, &cfg, "../escape").is_err());
        assert!(target_link(root, &cfg, "C:escape").is_err());
        assert!(target_link(root, &cfg, "CON").is_err());
        assert!(target_link(root, &cfg, "repo/../../escape").is_err());
        assert_eq!(
            target_link(root, &cfg, "myrepo/x86_64").unwrap(),
            root.join("yum/myrepo/x86_64/repodata")
        );
    }

    #[test]
    fn all_targets_skips_legacy_invalid_names() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("apt/dists/stable")).unwrap();
        std::fs::create_dir_all(root.join("apt/dists/main.")).unwrap();
        std::fs::create_dir_all(root.join("yum/myrepo/x86_64")).unwrap();
        std::fs::create_dir_all(root.join("yum/CON/x86_64")).unwrap();
        std::fs::create_dir_all(root.join("yum/myrepo/C:escape")).unwrap();

        let cfg = Config::default();
        assert_eq!(all_targets(root, &cfg), vec!["stable", "myrepo/x86_64"]);
    }

    #[test]
    fn yum_history_targets_respect_configured_base_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut cfg = Config::default();
        cfg.yum.base_dir = "rpmrepos".to_string();
        std::fs::create_dir_all(root.join("rpmrepos/myrepo/x86_64")).unwrap();

        assert_eq!(
            target_link(root, &cfg, "myrepo/x86_64").unwrap(),
            root.join("rpmrepos/myrepo/x86_64/repodata")
        );
        assert_eq!(all_targets(root, &cfg), vec!["myrepo/x86_64"]);
    }
}

fn report_skipped(skipped: &[arx_debrepo::SkippedDeb]) {
    eprintln!("WARNING: skipped {} package(s):", skipped.len());
    for s in skipped {
        eprintln!("  - {}: {}", s.path.display(), s.reason);
    }
    eprintln!("  (use --strict to fail instead of skipping)");
}

/// Outcome of an apt publish: the human summary plus the reasons any packages
/// were left out (so the HTTP API can report them in its response body).
pub(crate) struct AptPublish {
    pub summary: String,
    pub skipped: Vec<String>,
}

/// A publish refused under `--strict`/`[apt].strict` because packages were
/// skipped. A typed error so the HTTP layer can map it to a 4xx (client's
/// package was rejected) rather than a generic 500.
#[derive(Debug)]
pub(crate) struct StrictSkip {
    pub reasons: Vec<String>,
}

impl std::fmt::Display for StrictSkip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "strict: refusing to publish — {} package(s) skipped (nothing committed): {}",
            self.reasons.len(),
            self.reasons.join("; ")
        )
    }
}
impl std::error::Error for StrictSkip {}

fn publish_apt(
    root: &Path,
    cfg: &Config,
    key: Option<&SignedSecretKey>,
    passphrase: &str,
    strict: bool,
    incremental: bool,
) -> Result<AptPublish> {
    let apt_root = root.join("apt");
    let start = std::time::Instant::now();
    let dist = scope::validate_scope_name(&cfg.apt.dist, "apt dist")?;
    let pool_dir = scope::validate_scope_name(&cfg.apt.pool_dir, "apt pool dir")?;

    let meta = arx_debrepo::ReleaseMeta::new(
        cfg.repo.origin.as_str(),
        cfg.repo.label.as_str(),
        cfg.repo.description.as_str(),
        cfg.repo.suite.as_deref().unwrap_or(dist),
    )
    .with_codename(cfg.repo.codename.as_deref().unwrap_or(dist))
    .with_valid_days(cfg.apt.valid_days);

    // Stage the whole dist (all components/arches) into a fresh directory.
    let staged = arx_debrepo::stage_dist(&apt_root, pool_dir, dist, &meta, incremental)?;

    // A forgiving default must still be observable: never let a skipped package
    // pass silently behind an exit-0 publish. Under strict, refuse to commit.
    let skipped_reasons: Vec<String> = staged
        .skipped
        .iter()
        .map(|s| format!("{}: {}", s.path.display(), s.reason))
        .collect();
    if !staged.skipped.is_empty() {
        metrics::counter!("arx_publish_skipped_total").increment(staged.skipped.len() as u64);
        report_skipped(&staged.skipped);
        if strict {
            return Err(StrictSkip {
                reasons: skipped_reasons,
            }
            .into());
        }
    }

    // Sign into the staging dir so signatures are part of the atomic swap.
    if let Some(key) = key {
        let inrelease = signing::clearsign(key, passphrase, &staged.release_text)?;
        std::fs::write(staged.staging_dir.join("InRelease"), inrelease)
            .context("writing InRelease")?;
        let detached = signing::detached_sign(key, passphrase, staged.release_text.as_bytes())?;
        std::fs::write(staged.staging_dir.join("Release.gpg"), detached)
            .context("writing Release.gpg")?;
    }

    let packages = staged.packages;
    let components = staged.components.len();
    let skipped = staged.skipped.len();
    // Atomic symlink flip into place — clients never see a half-written dist,
    // and the previous state is retained for rollback.
    arx_debrepo::commit_dist(&staged, arx_debrepo::DEFAULT_KEEP_STATES)?;

    metrics::histogram!("arx_publish_apt_seconds").record(start.elapsed().as_secs_f64());
    let tail = if skipped > 0 {
        format!(", {skipped} skipped")
    } else {
        String::new()
    };
    Ok(AptPublish {
        summary: format!(
            "apt: indexed {packages} package(s) across {components} component(s){tail}"
        ),
        skipped: skipped_reasons,
    })
}

fn publish_yum(
    root: &Path,
    cfg: &Config,
    key: Option<&SignedSecretKey>,
    passphrase: &str,
    incremental: bool,
) -> Result<String> {
    let yum_root = cfg.checked_yum_base(root)?;
    let mut total = 0usize;
    let mut repos = 0usize;
    if yum_root.is_dir() {
        for repo_entry in std::fs::read_dir(&yum_root)? {
            let repo_path = repo_entry?.path();
            if !repo_path.is_dir() {
                continue;
            }
            for arch_entry in std::fs::read_dir(&repo_path)? {
                let arch_path = arch_entry?.path();
                if arch_path.is_dir() {
                    let n = yum::build_repodata(&arch_path, key, passphrase, incremental)?;
                    total += n;
                    repos += 1;
                }
            }
        }
    }
    Ok(format!(
        "yum: indexed {total} package(s) across {repos} repo/arch dir(s)"
    ))
}

async fn cmd_serve(args: &cli::ServeArgs) -> Result<()> {
    let cfg = Config::load(&args.root).unwrap_or_default();
    let addr = args.addr.clone().unwrap_or_else(|| cfg.server.addr.clone());
    let handle = observability::init_metrics()?;
    // Optional bearer-token auth; unset means public reads, writes disabled.
    let token = std::env::var("ARX_SERVE_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    // Context for accepting & publishing pushes. A missing key must NOT stop
    // read-only serving — only pushes need it (they'd then publish unsigned).
    let key = match load_key(&args.root, &cfg) {
        Ok(k) => k,
        Err(_) => {
            tracing::warn!("no signing key available; pushes would publish unsigned");
            None
        }
    };
    let passphrase = resolve_passphrase(None)?.unwrap_or_default();
    let push = server::PushContext {
        cfg,
        key,
        passphrase,
    };
    server::serve(args.root.clone(), addr, handle, token, push).await
}

/// Resolve a rollback target to its versioned symlink path. A target containing
/// `/` is a yum `<repo>/<arch>` (`yum/<repo>/<arch>/repodata`); otherwise it is an
/// apt dist (`apt/dists/<dist>`).
pub(crate) fn target_link(root: &Path, cfg: &Config, target: &str) -> Result<PathBuf> {
    match target.split_once('/') {
        Some((repo, arch)) if !arch.contains('/') => {
            let repo = scope::validate_scope_name(repo, "yum repo")?;
            let arch = scope::validate_scope_name(arch, "yum arch")?;
            Ok(cfg
                .checked_yum_base(root)?
                .join(repo)
                .join(arch)
                .join("repodata"))
        }
        Some(_) => bail!("invalid rollback target {target:?}: expected <repo>/<arch>"),
        None => {
            let dist = scope::validate_scope_name(target, "apt dist")?;
            Ok(root.join("apt/dists").join(dist))
        }
    }
}

/// Child names under `dir`, excluding hidden entries (e.g. `.states`).
fn visible_children(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// Every rollback target present in the repo: apt dists and yum `repo/arch`.
fn all_targets(root: &Path, cfg: &Config) -> Vec<String> {
    let mut targets = Vec::new();
    for dist in visible_children(&root.join("apt/dists")) {
        if target_link(root, cfg, &dist).is_ok() {
            targets.push(dist);
        } else {
            tracing::warn!(target = %dist, "skipping invalid apt history target");
        }
    }
    let yum = match cfg.checked_yum_base(root) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(error = %e, "skipping yum history targets: invalid yum base dir");
            return targets;
        }
    };
    for repo in visible_children(&yum) {
        if scope::validate_scope_name(&repo, "yum repo").is_err() {
            tracing::warn!(target = %repo, "skipping invalid yum repo history target");
            continue;
        }
        for arch in visible_children(&yum.join(&repo)) {
            let target = format!("{repo}/{arch}");
            if target_link(root, cfg, &target).is_ok() {
                targets.push(target);
            } else {
                tracing::warn!(target = %target, "skipping invalid yum history target");
            }
        }
    }
    targets
}

fn print_states(target: &str, link: &Path) -> Result<()> {
    let states = arx_debrepo::statedir::list(link)?;
    if states.is_empty() {
        return Ok(());
    }
    println!("{target} (* = current):");
    for s in &states {
        println!("  {} {}", if s.current { "*" } else { " " }, s.id);
    }
    Ok(())
}

fn cmd_rollback(args: &cli::RollbackArgs) -> Result<()> {
    let cfg = Config::load(&args.root).unwrap_or_default();
    let target = args.dist.clone().unwrap_or_else(|| cfg.apt.dist.clone());
    hooks::run(
        &args.root,
        &cfg,
        hooks::HookEvent::PreRollback,
        &hooks::HookContext::new().with("ARX_TARGET", target.clone()),
    )?;
    let link = target_link(&args.root, &cfg, &target)?;
    let id = arx_debrepo::statedir::rollback(&link, args.to.as_deref())?;
    hooks::run(
        &args.root,
        &cfg,
        hooks::HookEvent::PostRollback,
        &hooks::HookContext::new()
            .with("ARX_TARGET", target.clone())
            .with("ARX_STATE", id.clone()),
    )?;
    println!("Rolled back '{target}' to state {id}.");
    println!("(The next `arx publish` regenerates metadata from the current pool.)");
    Ok(())
}

fn cmd_history(args: &cli::HistoryArgs) -> Result<()> {
    let cfg = Config::load(&args.root).unwrap_or_default();
    match &args.dist {
        Some(target) => print_states(target, &target_link(&args.root, &cfg, target)?),
        None => {
            let targets = all_targets(&args.root, &cfg);
            if targets.is_empty() {
                println!("No published states yet — run `arx publish`.");
                return Ok(());
            }
            for t in &targets {
                print_states(t, &target_link(&args.root, &cfg, t)?)?;
            }
            Ok(())
        }
    }
}

async fn cmd_watch(args: &cli::WatchArgs) -> Result<()> {
    use std::collections::HashSet;
    let cfg = Config::load(&args.root).unwrap_or_default();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let interval = std::time::Duration::from_secs(args.interval);
    println!(
        "Watching {} (polling every {}s)...",
        args.dir.display(),
        args.interval
    );
    loop {
        let dir = args.dir.clone();
        let root = args.root.clone();
        let cfg = cfg.clone();
        let mut added = 0usize;
        let mut package_cache = cache::PackageFileCache::load(&root);
        let mut cache_dirty = false;
        for entry in walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path().to_path_buf();
            if p.is_file()
                && (p.extension().map(|e| e == "deb").unwrap_or(false)
                    || p.extension().map(|e| e == "rpm").unwrap_or(false))
                && !seen.contains(&p)
            {
                seen.insert(p.clone());
                match add_to_pool(
                    &root,
                    &cfg,
                    &p,
                    &cfg.apt.component,
                    &cfg.yum.repo,
                    &mut package_cache,
                ) {
                    Ok((dest, _)) => {
                        cache_dirty = true;
                        println!("Added {}", dest.display());
                        added += 1;
                    }
                    Err(e) => eprintln!("Error adding {}: {e:#}", p.display()),
                }
            }
        }
        if cache_dirty {
            if let Err(err) = package_cache.save(&root) {
                tracing::warn!(error = %err, "failed to save package cache");
            }
        }
        if added > 0 {
            match cmd_publish_static(&root, &cfg) {
                Ok(summary) => println!("Published — {summary}"),
                Err(e) => eprintln!("Publish error: {e:#}"),
            }
        }
        tokio::time::sleep(interval).await;
    }
}

/// Synchronous publish for the watcher (no async needed).
fn cmd_publish_static(root: &Path, cfg: &Config) -> Result<String> {
    let _lock = PublishLock::acquire(root)?;
    hooks::run(
        root,
        cfg,
        hooks::HookEvent::PrePublish,
        &hooks::HookContext::new().with("ARX_FORMATS", "apt,yum"),
    )?;
    let key = load_key(root, cfg)?;
    let passphrase = resolve_passphrase(None)?.unwrap_or_default();
    let apt = publish_apt(root, cfg, key.as_ref(), &passphrase, false, true)?;
    let yum = publish_yum(root, cfg, key.as_ref(), &passphrase, true)?;
    let summary = format!("{}; {yum}", apt.summary);
    hooks::run(
        root,
        cfg,
        hooks::HookEvent::PostPublish,
        &hooks::HookContext::new()
            .with("ARX_FORMATS", "apt,yum")
            .with("ARX_SUMMARY", summary.clone()),
    )?;
    Ok(summary)
}

async fn resolve_push_token(args: &cli::PushArgs) -> Result<String> {
    // Token resolution: explicit --token → ARX_SERVE_TOKEN → GitHub OIDC.
    if let Some(token) = args.token.clone() {
        return Ok(token);
    }
    if let Some(token) = std::env::var("ARX_SERVE_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return Ok(token);
    }

    // Try GitHub Actions OIDC (ADR-0014).
    let request_url = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let request_token = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    match (request_url, request_token) {
        (Some(url), Some(rt)) => {
            let audience = args.oidc_audience.as_deref().unwrap_or("arx");
            fetch_oidc_token(&url, &rt, audience).await
        }
        _ => bail!("no token: pass --token, set ARX_SERVE_TOKEN, or run in GitHub Actions (OIDC)"),
    }
}

async fn push_one_package(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    pkg: &Path,
    component: Option<&str>,
    repo: Option<&str>,
) -> Result<()> {
    let filename = pkg
        .file_name()
        .and_then(|f| f.to_str())
        .context("package path has no filename")?
        .to_string();
    let body = std::fs::read(pkg).with_context(|| format!("reading {}", pkg.display()))?;
    let mut req = client
        .post(endpoint)
        .bearer_auth(token)
        .header("X-Arx-Filename", &filename)
        .body(body);
    if let Some(c) = component {
        req = req.header("X-Arx-Component", c);
    }
    if let Some(r) = repo {
        req = req.header("X-Arx-Repo", r);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("POST {endpoint}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("push {filename} failed ({status}): {}", text.trim());
    }
    println!("✓ pushed {filename}");
    tracing::debug!(%filename, response = %text.trim(), "push ok");
    Ok(())
}

async fn cmd_push(args: &cli::PushArgs) -> Result<()> {
    let token = resolve_push_token(args).await?;
    let endpoint = format!("{}/api/v1/packages", args.url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    for pkg in &args.packages {
        push_one_package(
            &client,
            &endpoint,
            &token,
            pkg,
            args.component.as_deref(),
            args.repo.as_deref(),
        )
        .await?;
    }
    Ok(())
}
