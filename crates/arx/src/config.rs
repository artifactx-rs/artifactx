//! `arx.toml` repository configuration.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default config file name living at the repository root.
pub const CONFIG_FILE: &str = "arx.toml";

/// Top-level repository configuration, persisted as `arx.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
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
            description: "Repository managed by arx".into(),
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
    /// Armored private key path, relative to the repo root.
    pub private_key: String,
    /// Armored public key path, relative to the repo root.
    pub public_key: String,
    /// UID baked into a freshly generated key.
    pub user_id: String,
}

impl Default for Signing {
    fn default() -> Self {
        Self {
            enabled: true,
            encrypted: false,
            private_key: "keys/private.asc".into(),
            public_key: "keys/public.asc".into(),
            user_id: "ArtifactX <arx@localhost>".into(),
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
            addr: "0.0.0.0:8080".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Apt {
    /// Default distribution (suite/codename) for `arx add`.
    pub dist: String,
    /// Default component.
    pub component: String,
}

impl Default for Apt {
    fn default() -> Self {
        Self {
            dist: "stable".into(),
            component: "main".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Yum {
    /// Default repo name for `arx add`.
    pub repo: String,
}

impl Default for Yum {
    fn default() -> Self {
        Self {
            repo: "myrepo".into(),
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

    /// Absolute path to the armored private key for a given repo root.
    pub fn private_key_path(&self, root: &Path) -> PathBuf {
        root.join(&self.signing.private_key)
    }

    /// Absolute path to the armored public key for a given repo root.
    pub fn public_key_path(&self, root: &Path) -> PathBuf {
        root.join(&self.signing.public_key)
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
