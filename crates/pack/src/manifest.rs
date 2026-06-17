//! The packaging manifest: a single TOML document describing one package and
//! the files it installs. The same manifest drives both the `.deb` and `.rpm`
//! builders, so packagers describe intent once and target both ecosystems.

use anyhow::{Context, Result};
use serde::Deserialize;

/// A complete package description parsed from TOML.
///
/// `arch` is accepted in either ecosystem's spelling (e.g. `amd64` or
/// `x86_64`); the builders normalise it to the convention each format expects.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// Package name, e.g. `hello`.
    pub name: String,
    /// Upstream version, e.g. `1.2.3`.
    pub version: String,
    /// Architecture as written by the packager (`amd64`, `x86_64`, `all`, ...).
    pub arch: String,
    /// `Name <email>` maintainer string.
    pub maintainer: String,
    /// One-line summary plus optional extended description.
    pub description: String,
    /// SPDX-ish license expression, e.g. `MIT`.
    pub license: String,

    /// deb `Section` (e.g. `utils`). Reused as the rpm `Group` when set.
    #[serde(default)]
    pub section: Option<String>,
    /// rpm `Group`. Falls back to [`section`](Self::section) when unset.
    #[serde(default)]
    pub group: Option<String>,

    /// Runtime dependencies, in each format's native dependency syntax.
    #[serde(default)]
    pub depends: Vec<String>,

    /// Files to install, with host source, install destination, and mode.
    #[serde(default)]
    pub files: Vec<FileEntry>,

    /// Optional maintainer scripts run by the package manager.
    #[serde(default)]
    pub scripts: Scripts,
}

/// A single file to stage into the package.
#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    /// Path on the build host to read the file contents from.
    pub source: String,
    /// Absolute install path inside the target system, e.g. `/usr/bin/hello`.
    pub dest: String,
    /// Unix permission bits. In TOML write this as a string (`"0755"`) so the
    /// leading zero and octal intent survive; see [`FileEntry::mode_bits`].
    pub mode: String,
}

impl FileEntry {
    /// Parse [`mode`](Self::mode) as octal permission bits.
    pub fn mode_bits(&self) -> Result<u32> {
        let trimmed = self.mode.trim();
        let digits = trimmed.strip_prefix("0o").unwrap_or(trimmed);
        u32::from_str_radix(digits, 8)
            .with_context(|| format!("invalid octal file mode {:?}", self.mode))
    }
}

/// Optional maintainer scripts. Each is a host path to a script file whose
/// contents are embedded into the package.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Scripts {
    #[serde(default)]
    pub preinst: Option<String>,
    #[serde(default)]
    pub postinst: Option<String>,
    #[serde(default)]
    pub prerm: Option<String>,
    #[serde(default)]
    pub postrm: Option<String>,
}

impl Manifest {
    /// Parse a manifest from a TOML string.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        toml::from_str(s).context("parsing package manifest TOML")
    }

    /// The rpm `Group`, preferring an explicit [`group`](Self::group) and
    /// falling back to the deb [`section`](Self::section).
    pub fn rpm_group(&self) -> Option<&str> {
        self.group
            .as_deref()
            .or(self.section.as_deref())
    }
}
