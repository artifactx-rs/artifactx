//! ArtifactX (`arx`) entry point.

mod cli;
mod compose;
mod config;
mod observability;
mod pool;
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
        Command::Pack(args) => cmd_pack(&args),
        Command::Publish(args) => cmd_publish(&args).await,
        Command::Rollback(args) => cmd_rollback(&args),
        Command::History(args) => cmd_history(&args),
        Command::Push(args) => cmd_push(&args).await,
        Command::Rm(args) => {
            let removed =
                pool::remove(&args.root, &args.name, args.version.as_deref(), args.apt, args.yum)?;
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
        Command::Gc(args) => {
            let report = pool::gc(&args.root, args.keep, args.apt, args.yum, args.dry_run)?;
            for e in &report.pruned {
                let tag = if report.dry_run { "[dry-run] would prune" } else { "Pruned" };
                println!("{tag} {} {} ({})", e.name, e.version, e.path.display());
            }
            if report.pruned.is_empty() && report.retained_for_rollback == 0 {
                println!("Nothing to prune (every package has <= {} version(s)).", args.keep);
            } else if !report.pruned.is_empty() && !report.dry_run {
                println!("\nPruned {} file(s). Run `arx publish` to update metadata.", report.pruned.len());
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
        Command::Compose(args) => {
            compose::generate(&args.root, &args.addr)?;
            tracing::info!(root = %args.root.display(), "wrote Dockerfile + docker-compose.yml");
            Ok(())
        }
    }
}

/// Load the signing key referenced by config, if signing is enabled.
fn load_key(root: &Path, cfg: &Config) -> Result<Option<SignedSecretKey>> {
    if !cfg.signing.enabled {
        return Ok(None);
    }
    let path = cfg.private_key_path(root);
    if !path.exists() {
        bail!(
            "signing enabled but no key at {}; run `arx key generate`",
            path.display()
        );
    }
    let armored = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
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
    for dir in ["apt/pool", "apt/dists", "yum", "keys"] {
        std::fs::create_dir_all(root.join(dir))
            .with_context(|| format!("creating {}", root.join(dir).display()))?;
    }

    let mut cfg = Config::default();
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
    std::fs::create_dir_all(cfg.private_key_path(root).parent().unwrap()).ok();
    std::fs::write(cfg.private_key_path(root), &key.private_armored)
        .context("writing private key")?;
    std::fs::write(cfg.public_key_path(root), &key.public_armored)
        .context("writing public key")?;
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
            println!("Wrote {}", cfg.public_key_path(root).display());
        }
        KeyAction::Import { file } => {
            let armored = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            let key = signing::load_secret_key(&armored)?;
            // An imported key may be encrypted; the passphrase unlocks it to
            // derive the public key.
            let passphrase =
                resolve_passphrase(args.passphrase_file.as_deref())?.unwrap_or_default();
            std::fs::create_dir_all(cfg.private_key_path(root).parent().unwrap()).ok();
            std::fs::write(cfg.private_key_path(root), &armored).context("writing private key")?;
            let public = signing::public_from_secret(&key, &passphrase)?;
            std::fs::write(cfg.public_key_path(root), public).context("writing public key")?;
            cfg.signing.enabled = true;
            cfg.signing.encrypted = !passphrase.is_empty();
            cfg.save(root)?;
            println!("Imported key, wrote {}", cfg.public_key_path(root).display());
        }
        KeyAction::Export => {
            let path = cfg.public_key_path(root);
            let pubkey = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            print!("{pubkey}");
        }
    }
    Ok(())
}

/// Copy one `.deb`/`.rpm` into the pool, returning its destination path.
/// `.deb` goes to `apt/pool/<component>`; `.rpm` to `yum/<repo>/<arch>`.
fn add_to_pool(root: &Path, pkg: &Path, component: &str, repo: &str) -> Result<PathBuf> {
    let ext = pkg.extension().and_then(|e| e.to_str()).unwrap_or("");
    let dest_dir = match ext {
        "deb" => root.join("apt/pool").join(component),
        "rpm" => {
            let mut reader = createrepo_rs::rpm::RpmReader::open(pkg)
                .with_context(|| format!("opening {}", pkg.display()))?;
            let arch = reader
                .read_package()
                .with_context(|| format!("reading {}", pkg.display()))?
                .arch;
            root.join("yum").join(repo).join(arch)
        }
        other => bail!("{}: unsupported package type .{other}", pkg.display()),
    };
    std::fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(pkg.file_name().unwrap());
    std::fs::copy(pkg, &dest).with_context(|| format!("copying {}", pkg.display()))?;
    Ok(dest)
}

fn cmd_add(args: &cli::AddArgs) -> Result<()> {
    let root = &args.root;
    let cfg = Config::load(root).unwrap_or_default();
    let component = args.component.as_deref().unwrap_or(&cfg.apt.component);
    let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);

    for pkg in &args.packages {
        let dest = add_to_pool(root, pkg, component, repo)?;
        tracing::info!(file = %dest.display(), "added");
        println!("Added {}", dest.display());
    }
    Ok(())
}

fn load_pack_manifest(path: Option<&Path>) -> Result<pack::Manifest> {
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
        pack::Manifest::from_cargo_toml(&text)
            .with_context(|| format!("from {}", path.display()))
    } else {
        pack::Manifest::from_toml_str(&text)
    }
}

fn cmd_pack(args: &cli::PackArgs) -> Result<()> {
    let manifest = load_pack_manifest(args.manifest.as_deref())?;

    let do_deb = args.deb || !args.rpm;
    let do_rpm = args.rpm || !args.deb;
    let mut built = Vec::new();
    if do_deb {
        built.push(pack::build_deb(&manifest, &args.out).context("building .deb")?);
    }
    if do_rpm {
        built.push(pack::build_rpm(&manifest, &args.out).context("building .rpm")?);
    }
    for p in &built {
        println!("Built {}", p.display());
    }

    if args.add {
        let cfg = Config::load(&args.root).unwrap_or_default();
        let component = args.component.as_deref().unwrap_or(&cfg.apt.component);
        let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);
        for p in &built {
            let dest = add_to_pool(&args.root, p, component, repo)?;
            println!("Added {}", dest.display());
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

    // Hold an exclusive lock for the whole publish.
    let _lock = PublishLock::acquire(&root)?;

    // both flags off means publish both.
    let do_apt = args.apt || !args.yum;
    let do_yum = args.yum || !args.apt;
    // CLI flag OR config opt-in: any skipped package becomes a hard error.
    let strict = args.strict || cfg.apt.strict;

    // CPU-bound generation runs on a blocking thread.
    let summary = tokio::task::spawn_blocking(move || -> Result<String> {
        let mut lines = Vec::new();
        if do_apt {
            lines.push(publish_apt(&root, &cfg, key.as_ref(), &passphrase, strict)?.summary);
        }
        if do_yum {
            lines.push(publish_yum(&root, key.as_ref(), &passphrase)?);
        }
        Ok(lines.join("\n"))
    })
    .await
    .context("publish task panicked")??;

    println!("{summary}");
    Ok(())
}

/// Print a loud, human-visible summary of skipped packages to stderr so a
/// forgiving publish can't silently drop a package behind a green exit code.
fn report_skipped(skipped: &[debrepo::SkippedDeb]) {
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
) -> Result<AptPublish> {
    let apt_root = root.join("apt");
    let start = std::time::Instant::now();

    let meta = debrepo::ReleaseMeta::new(
        cfg.repo.origin.as_str(),
        cfg.repo.label.as_str(),
        cfg.repo.description.as_str(),
        cfg.apt.dist.as_str(),
    )
    .with_valid_days(cfg.apt.valid_days);

    // Stage the whole dist (all components/arches) into a fresh directory.
    let staged = debrepo::stage_dist(&apt_root, &cfg.apt.dist, &meta)?;

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
    debrepo::commit_dist(&staged, debrepo::DEFAULT_KEEP_STATES)?;

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

fn publish_yum(root: &Path, key: Option<&SignedSecretKey>, passphrase: &str) -> Result<String> {
    let yum_root = root.join("yum");
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
                    let n = yum::build_repodata(&arch_path, key, passphrase)?;
                    total += n;
                    repos += 1;
                }
            }
        }
    }
    Ok(format!("yum: indexed {total} package(s) across {repos} repo/arch dir(s)"))
}

async fn cmd_serve(args: &cli::ServeArgs) -> Result<()> {
    let cfg = Config::load(&args.root).unwrap_or_default();
    let addr = args.addr.clone().unwrap_or_else(|| cfg.server.addr.clone());
    let handle = observability::init_metrics()?;
    // Optional bearer-token auth; unset means public reads, writes disabled.
    let token = std::env::var("ARX_SERVE_TOKEN").ok().filter(|s| !s.is_empty());
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
fn target_link(root: &Path, target: &str) -> PathBuf {
    match target.split_once('/') {
        Some((repo, arch)) => root.join("yum").join(repo).join(arch).join("repodata"),
        None => root.join("apt/dists").join(target),
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
fn all_targets(root: &Path) -> Vec<String> {
    let mut targets = Vec::new();
    for dist in visible_children(&root.join("apt/dists")) {
        targets.push(dist);
    }
    let yum = root.join("yum");
    for repo in visible_children(&yum) {
        for arch in visible_children(&yum.join(&repo)) {
            targets.push(format!("{repo}/{arch}"));
        }
    }
    targets
}

fn print_states(target: &str, link: &Path) -> Result<()> {
    let states = debrepo::statedir::list(link)?;
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
    let target = args.dist.clone().unwrap_or(cfg.apt.dist);
    let link = target_link(&args.root, &target);
    let id = debrepo::statedir::rollback(&link, args.to.as_deref())?;
    println!("Rolled back '{target}' to state {id}.");
    println!("(The next `arx publish` regenerates metadata from the current pool.)");
    Ok(())
}

fn cmd_history(args: &cli::HistoryArgs) -> Result<()> {
    match &args.dist {
        Some(target) => print_states(target, &target_link(&args.root, target)),
        None => {
            let targets = all_targets(&args.root);
            if targets.is_empty() {
                println!("No published states yet — run `arx publish`.");
                return Ok(());
            }
            for t in &targets {
                print_states(t, &target_link(&args.root, t))?;
            }
            Ok(())
        }
    }
}

async fn cmd_push(args: &cli::PushArgs) -> Result<()> {
    let token = args
        .token
        .clone()
        .or_else(|| std::env::var("ARX_SERVE_TOKEN").ok().filter(|s| !s.is_empty()))
        .context("no token: pass --token or set ARX_SERVE_TOKEN")?;
    let endpoint = format!("{}/api/v1/packages", args.url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    for pkg in &args.packages {
        let filename = pkg
            .file_name()
            .and_then(|f| f.to_str())
            .context("package path has no filename")?
            .to_string();
        let body = std::fs::read(pkg).with_context(|| format!("reading {}", pkg.display()))?;
        let mut req = client
            .post(&endpoint)
            .bearer_auth(&token)
            .header("X-Arx-Filename", &filename)
            .body(body);
        if let Some(c) = &args.component {
            req = req.header("X-Arx-Component", c);
        }
        if let Some(r) = &args.repo {
            req = req.header("X-Arx-Repo", r);
        }
        let resp = req.send().await.with_context(|| format!("POST {endpoint}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("push {filename} failed ({status}): {}", text.trim());
        }
        println!("✓ pushed {filename}");
        tracing::debug!(%filename, response = %text.trim(), "push ok");
    }
    Ok(())
}
