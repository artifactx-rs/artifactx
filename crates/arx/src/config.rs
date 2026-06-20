//! `arx.toml` repository configuration.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::scope;

/// Default config file name living at the repository root.
pub const CONFIG_FILE: &str = "arx.toml";

/// Top-level repository configuration, persisted as `arx.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Human-facing repository identity (used in apt `Release: Origin`/`Label`).
    #[serde(default)]
    pub repo: RepoMeta,
    /// PGP signing configuration.
    #[serde(default)]
    pub signing: Signing,
    /// Built-in HTTP server defaults.
    #[serde(default)]
    pub server: Server,
    /// apt (Debian) repository settings.
    #[serde(default)]
    pub apt: Apt,
    /// yum (RPM) repository settings.
    #[serde(default)]
    pub yum: Yum,
    /// OIDC (GitHub Actions keyless push) settings.
    #[serde(default)]
    pub oidc: OidcConfig,
}

/// OIDC configuration for keyless push authentication. (ADR-0014.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    /// Enable OIDC JWT validation on the serve side.
    #[serde(default)]
    pub enabled: bool,
    /// Expected audience in the OIDC JWT (defaults to `"arx"`).
    #[serde(default = "default_oidc_audience")]
    pub audience: String,
    /// Repository glob patterns allowed to push, e.g. `["myorg/*"]`.
    #[serde(default)]
    pub allowed_repos: Vec<String>,
}

fn default_oidc_audience() -> String {
    "arx".to_string()
}

impl Default for OidcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            audience: "arx".to_string(),
            allowed_repos: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoMeta {
    pub origin: String,
    pub label: String,
    pub description: String,
}

impl Default for RepoMeta {
    fn default() -> Self {
        Self {
            origin: "ArtifactX".into(),
            label: "ArtifactX".into(),
            description: "Signed package repository managed by ArtifactX".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signing {
    /// Whether to sign generated metadata.
    pub enabled: bool,
    /// Whether the private key is passphrase-encrypted at rest.
    #[serde(default)]
    pub encrypted: bool,
    /// Directory for key storage, relative to the repo root. `arx init` creates
    /// keys here; `private_key`/`public_key` default paths are inside this dir.
    #[serde(default = "default_keys_dir")]
    pub keys_dir: String,
    /// Armored private key path, relative to the repo root.
    pub private_key: String,
    /// Armored public key path, relative to the repo root.
    pub public_key: String,
    /// UID baked into a freshly generated key.
    pub user_id: String,
}

fn default_keys_dir() -> String {
    "keys".into()
}

impl Default for Signing {
    fn default() -> Self {
        Self {
            enabled: true,
            encrypted: false,
            keys_dir: "keys".into(),
            private_key: "keys/private.asc".into(),
            public_key: "keys/public.asc".into(),
            user_id: "ArtifactX Repository Signing <signing@artifactx.local>".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub addr: String,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:8080".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Apt {
    /// Default distribution (suite/codename) for `arx add`.
    pub dist: String,
    /// Default component.
    pub component: String,
    /// Days until the published `Release` expires (`Valid-Until`). `0` omits the
    /// field (no expiry). `arx init` writes `7` for new repos (secure-by-default
    /// against repository freeze); the serde default stays `0` so existing repos
    /// and programmatic callers are never surprised by silent expiry.
    #[serde(default)]
    pub valid_days: u32,
    /// Fail the publish if any package is skipped (unreadable/colliding) instead
    /// of warning and proceeding. The source of truth for the `push`/server path;
    /// the CLI `--strict` flag also forces it on. Default `false` (forgiving).
    #[serde(default)]
    pub strict: bool,
    /// Custom pool subdirectory under `apt/`. Default `"pool"`.
    #[serde(default = "default_pool_dir")]
    pub pool_dir: String,
}

fn default_pool_dir() -> String {
    "pool".into()
}

impl Default for Apt {
    fn default() -> Self {
        Self {
            dist: "stable".into(),
            component: "main".into(),
            valid_days: 0,
            strict: false,
            pool_dir: "pool".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Yum {
    /// Default repo name for `arx add`.
    pub repo: String,
    /// Base directory under repo root for yum packages. Default `"yum"`.
    #[serde(default = "default_yum_base")]
    pub base_dir: String,
}

fn default_yum_base() -> String {
    "yum".into()
}

impl Default for Yum {
    fn default() -> Self {
        Self {
            repo: "myrepo".into(),
            base_dir: "yum".into(),
        }
    }
}

impl Config {
    /// Load `arx.toml` from a repository root directory.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(CONFIG_FILE);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    /// Persist `arx.toml` to a repository root directory.
    pub fn save(&self, root: &Path) -> Result<()> {
        let path = root.join(CONFIG_FILE);
        let text = toml::to_string_pretty(self).context("serializing config")?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Absolute path to the armored private key for a given repo root after
    /// validating the configured path stays repo-relative.
    pub fn private_key_path(&self, root: &Path) -> Result<PathBuf> {
        Ok(root.join(scope::validate_repo_relative_path(
            &self.signing.private_key,
            "signing private key",
        )?))
    }

    /// Absolute path to the armored public key for a given repo root after
    /// validating the configured path stays repo-relative.
    pub fn public_key_path(&self, root: &Path) -> Result<PathBuf> {
        Ok(root.join(scope::validate_repo_relative_path(
            &self.signing.public_key,
            "signing public key",
        )?))
    }

    /// Absolute path to the key storage directory after validating the
    /// configured path stays repo-relative.
    pub fn keys_dir(&self, root: &Path) -> Result<PathBuf> {
        Ok(root.join(scope::validate_repo_relative_path(
            &self.signing.keys_dir,
            "signing keys dir",
        )?))
    }

    /// Absolute path to the apt pool root after validating `pool_dir` is a
    /// single logical repository name, not a filesystem path.
    pub fn checked_apt_pool_root(&self, root: &Path) -> Result<PathBuf> {
        let pool_dir = scope::validate_scope_name(&self.apt.pool_dir, "apt pool dir")?;
        Ok(root.join("apt").join(pool_dir))
    }

    /// Absolute path to the yum base directory after validating `base_dir` is
    /// a single logical repository name, not a filesystem path.
    pub fn checked_yum_base(&self, root: &Path) -> Result<PathBuf> {
        let base_dir = scope::validate_scope_name(&self.yum.base_dir, "yum base dir")?;
        Ok(root.join(base_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_config() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.repo.origin, cfg.repo.origin);
        assert_eq!(back.signing.private_key, "keys/private.asc");
        assert_eq!(back.server.addr, "127.0.0.1:8080");
        assert_eq!(back.apt.dist, "stable");
        assert_eq!(back.yum.repo, "myrepo");
    }

    #[test]
    fn partial_config_uses_defaults() {
        // Only override one section; the rest should fall back to defaults.
        let text = r#"
[server]
addr = "127.0.0.1:9000"
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.server.addr, "127.0.0.1:9000");
        assert_eq!(cfg.apt.component, "main");
        assert!(cfg.signing.enabled);
    }
}
