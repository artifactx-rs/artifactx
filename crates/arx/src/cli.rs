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
    /// Inspect or rebuild the persistent acceleration cache.
    Cache(CacheArgs),
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
    /// Ingest a drop directory, publish, and optionally switch live public repos.
    PublishDir(PublishDirArgs),
    /// Remove a package from the pool (then run `publish`).
    Rm(RmArgs),
    /// Search packages in the local apt/yum pools.
    Search(SearchArgs),
    /// Import packages from an existing apt/yum repository (migration path).
    Import(ImportArgs),
    /// Prune old package versions from the pool (then run `publish`).
    Gc(GcArgs),
    /// Promote packages between components (apt) or repos (yum).
    Promote(PromoteArgs),
    /// Serve the repository over HTTP.
    Serve(ServeArgs),
    /// Mirror an upstream apt/yum repository (sync + keep up-to-date).
    Mirror(MirrorArgs),
    /// Watch a directory for new packages (auto-add + publish).
    Watch(WatchArgs),
    /// Generate docker-compose.yml + Dockerfile.
    Compose(ComposeArgs),
    /// Export published metadata into legacy-compatible public layouts.
    Export(ExportArgs),
    /// Publish, export, preflight, and atomically switch live repository pointers.
    Cutover(CutoverArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Repository root to create (defaults to current directory).
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Skip generating a signing key.
    #[arg(long)]
    pub no_key: bool,
    /// Custom directory for signing keys (default `"keys"`).
    #[arg(long)]
    pub key_dir: Option<String>,
    /// Custom apt pool subdirectory name (default `"pool"`).
    #[arg(long)]
    pub pool_dir: Option<String>,
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
    /// Rotate the signing key: backs up the current key, generates a new one.
    /// Clients must re-trust the new public key.
    Rotate,
    /// Revoke the old key (delete the backup created by `rotate`).
    Revoke,
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
pub struct CacheArgs {
    #[command(subcommand)]
    pub action: CacheAction,
    /// Repository root.
    #[arg(long, default_value = ".", global = true)]
    pub root: PathBuf,
    /// Worker threads for cache rebuild/hash work (0 = available CPU parallelism).
    #[arg(long, default_value_t = 0, global = true)]
    pub jobs: usize,
}

#[derive(Debug, Subcommand)]
pub enum CacheAction {
    /// Show cache version, path, and entry count.
    Status,
    /// Rebuild package file cache from the current apt/yum pools.
    Rebuild,
    /// Delete the persistent cache.
    Clear,
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
    /// Rebuild all metadata from scratch, ignoring the incremental cache
    /// (`.arx-manifest.toml`). Use after `init`, after deleting pool files, or
    /// when the cache is suspected to be stale.
    #[arg(long)]
    pub full: bool,
    /// Fail if any package is unreadable or collides, instead of skipping it and
    /// publishing the rest. Also settable as `[apt].strict` in `arx.toml`.
    #[arg(long)]
    pub strict: bool,
    /// Also export the apt public layout and atomically switch this live symlink.
    #[arg(long)]
    pub apt_live: Option<PathBuf>,
    /// Also export a flat yum public layout and atomically switch this live symlink.
    #[arg(long)]
    pub yum_flat_live: Option<PathBuf>,
    /// Directory that receives versioned cutover exports. Defaults near the first live path.
    #[arg(long)]
    pub staging_dir: Option<PathBuf>,
    /// Yum repo name to export when `--yum-flat-live` is set (defaults to `[yum].repo`).
    #[arg(long)]
    pub repo: Option<String>,
    /// Limit yum export to one or more architectures when `--yum-flat-live` is set.
    #[arg(long)]
    pub arch: Vec<String>,
    /// Validate staged export without switching live pointers.
    #[arg(long)]
    pub dry_run: bool,
    /// Fail if any staged RPM payload is unsigned. Repository metadata signing is checked separately.
    #[arg(long)]
    pub require_signed_rpms: bool,
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
    /// Build an Alpine Linux `.apk` package.
    #[arg(long)]
    pub apk: bool,
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
    /// Bearer token; falls back to `ARX_SERVE_TOKEN`, then GitHub OIDC.
    #[arg(long)]
    pub token: Option<String>,
    /// OIDC audience claim (defaults to `"arx"`). Only used when auto-detecting
    /// GitHub Actions OIDC; ignored with explicit `--token`.
    #[arg(long)]
    pub oidc_audience: Option<String>,
    /// apt component for `.deb` uploads (server default if unset).
    #[arg(long)]
    pub component: Option<String>,
    /// yum repo for `.rpm` uploads (server default if unset).
    #[arg(long)]
    pub repo: Option<String>,
}

#[derive(Debug, Args)]
pub struct PublishDirArgs {
    /// Directory containing `.deb`/`.rpm` packages to ingest.
    pub dir: PathBuf,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// apt component for `.deb` packages (config default if unset).
    #[arg(long)]
    pub component: Option<String>,
    /// yum repo name for `.rpm` packages and flat yum export (config default if unset).
    #[arg(long)]
    pub repo: Option<String>,
    /// Recurse below the drop directory instead of scanning only direct children.
    #[arg(long)]
    pub recursive: bool,
    /// State file for no-op detection (defaults under `.arx-cache/` in the repo root).
    #[arg(long)]
    pub state_file: Option<PathBuf>,
    /// Ignore cached source-directory state and publish even if inputs look unchanged.
    #[arg(long)]
    pub force: bool,
    /// Rebuild all publish metadata from scratch, ignoring incremental metadata caches.
    #[arg(long)]
    pub full: bool,
    /// Publish only apt metadata. Requires `--apt-live` when live cutover is requested.
    #[arg(long)]
    pub apt: bool,
    /// Publish only yum metadata. Requires `--yum-flat-live` when live cutover is requested.
    #[arg(long)]
    pub yum: bool,
    /// Also export the apt public layout and atomically switch this live symlink.
    #[arg(long)]
    pub apt_live: Option<PathBuf>,
    /// Also export a flat yum public layout and atomically switch this live symlink.
    #[arg(long)]
    pub yum_flat_live: Option<PathBuf>,
    /// Directory that receives versioned cutover exports. Defaults near the first live path.
    #[arg(long)]
    pub staging_dir: Option<PathBuf>,
    /// Limit yum export to one or more architectures when `--yum-flat-live` is set.
    #[arg(long)]
    pub arch: Vec<String>,
    /// Validate staged export without switching live pointers.
    #[arg(long)]
    pub dry_run: bool,
    /// Fail if any staged RPM payload is unsigned. Repository metadata signing is checked separately.
    #[arg(long)]
    pub require_signed_rpms: bool,
    /// Sign unsigned RPM payloads before ingest using the system RPM signing backend.
    ///
    /// This runs `rpm --addsign <rpm>` for unsigned RPMs and verifies each
    /// payload is signed before continuing. No RPM signing is attempted unless
    /// this option or `--rpm-sign-cmd` is set.
    #[arg(long, conflicts_with = "rpm_sign_cmd")]
    pub sign_rpms: bool,
    /// Optional shell command used to sign unsigned RPM payloads before ingesting them.
    ///
    /// Prefer `--sign-rpms` for the default RPM signing backend. This escape hatch
    /// is for environments that use a custom signer. The command is skipped for
    /// already-signed RPMs and receives `ARX_RPM_PATH`/`ARX_PACKAGE_PATH` plus
    /// repository context in its environment.
    #[arg(long)]
    pub rpm_sign_cmd: Option<String>,
    /// Optional shell command to run after a successful non-no-op publish.
    #[arg(long)]
    pub sync_cmd: Option<String>,
    /// Passphrase file to unlock an encrypted signing key; falls back to
    /// `ARX_KEY_PASSPHRASE`.
    #[arg(long)]
    pub passphrase_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct MirrorArgs {
    /// Base URL of the upstream repository.
    pub url: String,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// apt distribution (default: config default).
    #[arg(long)]
    pub dist: Option<String>,
    /// apt component (default: config default).
    #[arg(long)]
    pub component: Option<String>,
    /// Architecture filter (default: amd64).
    #[arg(long, default_value = "amd64")]
    pub arch: String,
    /// Prune local packages no longer in upstream.
    #[arg(long)]
    pub prune: bool,
    /// Auto-publish after sync.
    #[arg(long)]
    pub publish: bool,
    /// Operate on apt pool.
    #[arg(long)]
    pub apt: bool,
    /// Operate on yum pool.
    #[arg(long)]
    pub yum: bool,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Base URL of the upstream repository.
    pub url: String,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// apt distribution (default: config default).
    #[arg(long)]
    pub dist: Option<String>,
    /// apt component or yum repo name.
    #[arg(long)]
    pub component: Option<String>,
    /// Architecture filter for apt (default: amd64).
    #[arg(long, default_value = "amd64")]
    pub arch: String,
    /// Limit the number of packages to import (default: unlimited).
    #[arg(long)]
    pub limit: Option<usize>,
    /// Only import packages matching this name prefix (e.g. "clickhouse").
    #[arg(long)]
    pub match_name: Option<String>,
    /// Import from an apt repo.
    #[arg(long)]
    pub apt: bool,
    /// Import from a yum repo.
    #[arg(long)]
    pub yum: bool,
    /// Fail if any upstream metadata entry is invalid or cannot be downloaded.
    #[arg(long)]
    pub strict: bool,
    /// Publish repository metadata immediately after a successful import.
    #[arg(long)]
    pub publish: bool,
}

#[derive(Debug, Args)]
pub struct PromoteArgs {
    /// Package name to promote.
    pub name: String,
    /// Source apt component (or yum repo).
    #[arg(long)]
    pub from: String,
    /// Destination apt component (or yum repo).
    #[arg(long)]
    pub to: String,
    /// Specific version to promote (all if unset).
    #[arg(long)]
    pub version: Option<String>,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Operate on the apt pool.
    #[arg(long)]
    pub apt: bool,
    /// Operate on the yum pool.
    #[arg(long)]
    pub yum: bool,
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
pub struct SearchArgs {
    /// Match package names containing this text.
    pub query: Option<String>,
    /// Match package names starting with this prefix.
    #[arg(long)]
    pub name_prefix: Option<String>,
    /// Match this exact package version.
    #[arg(long)]
    pub version: Option<String>,
    /// Match this exact architecture.
    #[arg(long)]
    pub arch: Option<String>,
    /// Match this exact apt component or yum repo name.
    #[arg(long)]
    pub scope: Option<String>,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Restrict to the apt pool.
    #[arg(long)]
    pub apt: bool,
    /// Restrict to the yum pool.
    #[arg(long)]
    pub yum: bool,
    /// Emit JSON instead of tab-separated text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct GcArgs {
    /// Only prune this package name.
    pub name: Option<String>,
    /// Only prune packages whose names start with this prefix.
    #[arg(long)]
    pub name_prefix: Option<String>,
    /// Keep this many newest versions per package.
    #[arg(long, default_value_t = 3)]
    pub keep: usize,
    /// Additionally protect files newer than this many days from pruning.
    #[arg(long, default_value_t = 0)]
    pub keep_within: u32,
    /// Grace period in days: files eligible for pruning are deferred
    /// (not deleted) until they're older than this window.
    #[arg(long, default_value_t = 0)]
    pub grace: u32,
    /// Show what would be pruned without deleting.
    #[arg(long)]
    pub dry_run: bool,
    /// Allow pruning files referenced by retained rollback states.
    ///
    /// By default, ArtifactX keeps files needed by rollback history so old
    /// states do not 404. Use this only after intentionally discarding that
    /// rollback safety net.
    #[arg(long)]
    pub ignore_rollback_states: bool,
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
pub struct WatchArgs {
    /// Directory to watch for new `.deb`/`.rpm` files.
    #[arg(default_value = ".")]
    pub dir: PathBuf,
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Poll interval in seconds.
    #[arg(long, default_value_t = 10)]
    pub interval: u64,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Export apt layout (`dists/` + `pool/`) to this fresh directory.
    #[arg(long)]
    pub apt_out: Option<PathBuf>,
    /// Export a flat yum repo (`*.rpm` + `repodata/`) to this fresh directory.
    #[arg(long)]
    pub yum_flat_out: Option<PathBuf>,
    /// Yum repo name to export (defaults to `[yum].repo`).
    #[arg(long)]
    pub repo: Option<String>,
    /// Limit yum export to one or more architectures (default: all arch dirs).
    #[arg(long)]
    pub arch: Vec<String>,
    /// Passphrase file to unlock an encrypted signing key for exported yum metadata.
    #[arg(long)]
    pub passphrase_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct CutoverArgs {
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Live apt path to switch to the staged export. Must be absent or a symlink.
    #[arg(long)]
    pub apt_live: Option<PathBuf>,
    /// Live flat yum path to switch to the staged export. Must be absent or a symlink.
    #[arg(long)]
    pub yum_flat_live: Option<PathBuf>,
    /// Directory that receives versioned cutover exports. Defaults near the first live path.
    #[arg(long)]
    pub staging_dir: Option<PathBuf>,
    /// Yum repo name to export (defaults to `[yum].repo`).
    #[arg(long)]
    pub repo: Option<String>,
    /// Limit yum export to one or more architectures (default: all arch dirs).
    #[arg(long)]
    pub arch: Vec<String>,
    /// Validate and leave the staged export in place without switching live pointers.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip the publish step and cut over the currently published metadata.
    #[arg(long)]
    pub no_publish: bool,
    /// Fail if any staged RPM payload is unsigned. Repository metadata signing is checked separately.
    #[arg(long)]
    pub require_signed_rpms: bool,
    /// Passphrase file to unlock an encrypted signing key.
    #[arg(long)]
    pub passphrase_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ComposeArgs {
    /// Repository root.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Output directory for generated files.
    #[arg(long, default_value = ".")]
    pub out: PathBuf,
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
