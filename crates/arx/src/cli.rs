//! Command-line interface definition (clap derive).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// ArtifactX (arx) — cross-platform package repository manager.
#[derive(Debug, Parser)]
#[command(name = "arx", version = crate::VERSION, about)]
pub struct Cli {
    /// Log output format.
    #[arg(long, value_enum, default_value_t = LogFormat::Text, global = true)]
    pub log_format: LogFormat,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scaffold a new repository (directories, arx.toml, signing key).
    Init(InitArgs),
    /// Manage the signing key.
    Key(KeyArgs),
    /// Add one or more `.deb`/`.rpm` packages into the repository.
    Add(AddArgs),
    /// Generate and sign repository metadata.
    Publish(PublishArgs),
    /// Roll a target back to its previous published state.
    Rollback(RollbackArgs),
    /// List retained published states (all targets, or one).
    History(HistoryArgs),
    /// Build a `.deb`/`.rpm` from a manifest (the Package pillar).
    Pack(PackArgs),
    /// Push a package to a running `arx serve` (uploads + publishes remotely).
    Push(PushArgs),
    /// Remove a package from the pool (then run `publish`).
    Rm(RmArgs),
    /// Prune old package versions from the pool (then run `publish`).
    Gc(GcArgs),
    /// Serve the repository over HTTP.
    Serve(ServeArgs),
    /// Generate docker-compose.yml + Dockerfile.
    Compose(ComposeArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Repository root to create (defaults to current directory).
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Skip generating a signing key.
    #[arg(long)]
    pub no_key: bool,
    /// Encrypt the signing key with the passphrase in this file (else
    /// `ARX_KEY_PASSPHRASE`; if neither, the key is stored unencrypted).
    #[arg(long)]
    pub passphrase_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct KeyArgs {
    #[command(subcommand)]
    pub action: KeyAction,
    /// Repository root.
    #[arg(long, default_value = ".", global = true)]
    pub root: PathBuf,
    /// Passphrase file to encrypt (generate) or unlock (import) the key with;
    /// falls back to `ARX_KEY_PASSPHRASE`.
    #[arg(long, global = true)]
    pub passphrase_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum KeyAction {
    /// Generate a new signing key (overwrites existing).
    Generate,
    /// Import an existing armored private key.
    Import {
        /// Path to an armored private key file.
        file: PathBuf,
    },
    /// Print the armored public key to stdout.
    Export,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Package files (`.deb` or `.rpm`).
    #[arg(required = true)]
    pub packages: Vec<PathBuf>,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// apt distribution/suite (overrides config).
    #[arg(long)]
    pub dist: Option<String>,
    /// apt component (overrides config).
    #[arg(long)]
    pub component: Option<String>,
    /// yum repo name (overrides config).
    #[arg(long)]
    pub repo: Option<String>,
}

#[derive(Debug, Args)]
pub struct PublishArgs {
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Only publish the apt repository.
    #[arg(long)]
    pub apt: bool,
    /// Only publish the yum repository.
    #[arg(long)]
    pub yum: bool,
    /// Passphrase file to unlock an encrypted signing key; falls back to
    /// `ARX_KEY_PASSPHRASE`.
    #[arg(long)]
    pub passphrase_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct PackArgs {
    /// Manifest path. A `Cargo.toml` is read via `[package]` +
    /// `[package.metadata.arx]`; any other `.toml` is a standalone manifest.
    /// Omit to read `./Cargo.toml`.
    pub manifest: Option<PathBuf>,
    /// Output directory for built packages.
    #[arg(long, default_value = "dist")]
    pub out: PathBuf,
    /// Build only the `.deb` (default: build both).
    #[arg(long)]
    pub deb: bool,
    /// Build only the `.rpm` (default: build both).
    #[arg(long)]
    pub rpm: bool,
    /// Also add the built packages into the repository pool.
    #[arg(long)]
    pub add: bool,
    /// Repository root (used with `--add`).
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// apt component for `--add` (config default if unset).
    #[arg(long)]
    pub component: Option<String>,
    /// yum repo for `--add` (config default if unset).
    #[arg(long)]
    pub repo: Option<String>,
}

#[derive(Debug, Args)]
pub struct PushArgs {
    /// Package files (`.deb` / `.rpm`) to upload.
    #[arg(required = true)]
    pub packages: Vec<PathBuf>,
    /// Base URL of the running server, e.g. `https://repo.example.com`.
    #[arg(long)]
    pub url: String,
    /// Bearer token; falls back to `ARX_SERVE_TOKEN`.
    #[arg(long)]
    pub token: Option<String>,
    /// apt component for `.deb` uploads (server default if unset).
    #[arg(long)]
    pub component: Option<String>,
    /// yum repo for `.rpm` uploads (server default if unset).
    #[arg(long)]
    pub repo: Option<String>,
}

#[derive(Debug, Args)]
pub struct RmArgs {
    /// Package name to remove.
    pub name: String,
    /// Only remove this exact version (else all versions of the name).
    #[arg(long)]
    pub version: Option<String>,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Restrict to the apt pool.
    #[arg(long)]
    pub apt: bool,
    /// Restrict to the yum pool.
    #[arg(long)]
    pub yum: bool,
}

#[derive(Debug, Args)]
pub struct GcArgs {
    /// Keep this many most-recently-added versions per package.
    #[arg(long, default_value_t = 3)]
    pub keep: usize,
    /// Show what would be pruned without deleting.
    #[arg(long)]
    pub dry_run: bool,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Restrict to the apt pool.
    #[arg(long)]
    pub apt: bool,
    /// Restrict to the yum pool.
    #[arg(long)]
    pub yum: bool,
}

#[derive(Debug, Args)]
pub struct RollbackArgs {
    /// Target: an apt dist (e.g. `stable`) or a yum `repo/arch` (e.g.
    /// `myrepo/x86_64`). Defaults to the configured apt dist.
    pub dist: Option<String>,
    /// Roll back to this specific state id (default: the previous state).
    #[arg(long)]
    pub to: Option<String>,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(Debug, Args)]
pub struct HistoryArgs {
    /// Target to inspect (apt dist or yum `repo/arch`); omit to list all.
    pub dist: Option<String>,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Repository root (web root).
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Listen address (overrides config).
    #[arg(long)]
    pub addr: Option<String>,
}

#[derive(Debug, Args)]
pub struct ComposeArgs {
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Listen address baked into the compose file.
    #[arg(long, default_value = "0.0.0.0:8080")]
    pub addr: String,
}

impl From<LogFormat> for crate::observability::LogFormat {
    fn from(f: LogFormat) -> Self {
        match f {
            LogFormat::Text => crate::observability::LogFormat::Text,
            LogFormat::Json => crate::observability::LogFormat::Json,
        }
    }
}
