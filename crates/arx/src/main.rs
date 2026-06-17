//! ArtifactX (`arx`) entry point.

mod cli;
mod compose;
mod config;
mod observability;
mod server;
mod signing;
mod yum;

use std::path::Path;

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
        Command::Publish(args) => cmd_publish(&args).await,
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

fn cmd_add(args: &cli::AddArgs) -> Result<()> {
    let root = &args.root;
    let cfg = Config::load(root).unwrap_or_default();
    let component = args.component.as_deref().unwrap_or(&cfg.apt.component);
    let repo = args.repo.as_deref().unwrap_or(&cfg.yum.repo);

    for pkg in &args.packages {
        let ext = pkg.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "deb" => {
                let dest_dir = root.join("apt/pool").join(component);
                std::fs::create_dir_all(&dest_dir)?;
                let dest = dest_dir.join(pkg.file_name().unwrap());
                std::fs::copy(pkg, &dest)
                    .with_context(|| format!("copying {}", pkg.display()))?;
                tracing::info!(file = %dest.display(), "added deb");
                println!("Added {}", dest.display());
            }
            "rpm" => {
                let mut reader = createrepo_rs::rpm::RpmReader::open(pkg)
                    .with_context(|| format!("opening {}", pkg.display()))?;
                let meta = reader
                    .read_package()
                    .with_context(|| format!("reading {}", pkg.display()))?;
                let dest_dir = root.join("yum").join(repo).join(&meta.arch);
                std::fs::create_dir_all(&dest_dir)?;
                let dest = dest_dir.join(pkg.file_name().unwrap());
                std::fs::copy(pkg, &dest)
                    .with_context(|| format!("copying {}", pkg.display()))?;
                tracing::info!(file = %dest.display(), arch = %meta.arch, "added rpm");
                println!("Added {}", dest.display());
            }
            other => bail!("{}: unsupported package type .{other}", pkg.display()),
        }
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

    // CPU-bound generation runs on a blocking thread.
    let summary = tokio::task::spawn_blocking(move || -> Result<String> {
        let mut lines = Vec::new();
        if do_apt {
            lines.push(publish_apt(&root, &cfg, key.as_ref(), &passphrase)?);
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

fn publish_apt(
    root: &Path,
    cfg: &Config,
    key: Option<&SignedSecretKey>,
    passphrase: &str,
) -> Result<String> {
    let apt_root = root.join("apt");
    let start = std::time::Instant::now();

    let meta = debrepo::ReleaseMeta::new(
        cfg.repo.origin.as_str(),
        cfg.repo.label.as_str(),
        cfg.repo.description.as_str(),
        cfg.apt.dist.as_str(),
    );

    // Stage the whole dist (all components/arches) into a fresh directory.
    let staged = debrepo::stage_dist(&apt_root, &cfg.apt.dist, &meta)?;

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
    // Atomic swap into place — clients never see a half-written dist.
    debrepo::commit_dist(&staged)?;

    metrics::histogram!("arx_publish_apt_seconds").record(start.elapsed().as_secs_f64());
    Ok(format!("apt: indexed {packages} package(s) across {components} component(s)"))
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
    let addr = args.addr.clone().unwrap_or(cfg.server.addr);
    let handle = observability::init_metrics()?;
    server::serve(args.root.clone(), addr, handle).await
}
