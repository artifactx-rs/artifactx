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
