//! The packaging manifest: a single TOML document describing one package and
//! the files it installs. The same manifest drives both the `.deb` and `.rpm`
//! builders, so packagers describe intent once and target both ecosystems.

use anyhow::{anyhow, Context, Result};
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
    /// Packages this one conflicts with.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// Virtual packages / capabilities this package provides.
    #[serde(default)]
    pub provides: Vec<String>,
    /// Packages this one replaces (deb `Replaces`, rpm `Obsoletes`).
    #[serde(default)]
    pub replaces: Vec<String>,

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

    /// Derive a manifest from a `Cargo.toml`: identity from `[package]`, packaging
    /// details from `[package.metadata.arx]`. With no `files`, the convention is
    /// `target/release/<name>` → `/usr/bin/<name>` (so a built CLI just works).
    pub fn from_cargo_toml(cargo_toml: &str) -> Result<Self> {
        let doc: toml::Value = toml::from_str(cargo_toml).context("parsing Cargo.toml")?;
        let pkg = doc
            .get("package")
            .and_then(|v| v.as_table())
            .ok_or_else(|| anyhow!("Cargo.toml has no [package] (a workspace root has none — run `arx pack` in a crate)"))?;
        let get = |k: &str| pkg.get(k).and_then(|v| v.as_str());

        let name = get("name")
            .ok_or_else(|| anyhow!("[package] has no literal name"))?
            .to_string();
        let version = get("version")
            .ok_or_else(|| anyhow!(
                "[package].version must be a literal string (workspace-inherited \
                 versions aren't supported here yet — use a standalone manifest)"
            ))?
            .to_string();
        let description = get("description").unwrap_or(&name).to_string();
        let license = get("license").unwrap_or("").to_string();
        let author = pkg
            .get("authors")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(String::from);

        let arx: ArxMeta = pkg
            .get("metadata")
            .and_then(|m| m.as_table())
            .and_then(|t| t.get("arx"))
            .cloned()
            .map(|v| v.try_into())
            .transpose()
            .context("parsing [package.metadata.arx]")?
            .unwrap_or_default();

        let files = if arx.files.is_empty() {
            vec![FileEntry {
                source: format!("target/release/{name}"),
                dest: format!("/usr/bin/{name}"),
                mode: "0755".to_string(),
            }]
        } else {
            arx.files
        };

        Ok(Manifest {
            name,
            version,
            arch: arx.arch.unwrap_or_else(|| "amd64".to_string()),
            maintainer: arx
                .maintainer
                .or(author)
                .unwrap_or_else(|| "Unknown <unknown@localhost>".to_string()),
            description,
            license,
            section: arx.section,
            group: None,
            depends: arx.depends,
            conflicts: arx.conflicts,
            provides: arx.provides,
            replaces: arx.replaces,
            files,
            scripts: arx.scripts,
        })
    }

    /// The rpm `Group`, preferring an explicit [`group`](Self::group) and
    /// falling back to the deb [`section`](Self::section).
    pub fn rpm_group(&self) -> Option<&str> {
        self.group
            .as_deref()
            .or(self.section.as_deref())
    }
}

/// The `[package.metadata.arx]` table in a `Cargo.toml`: packaging fields that
/// aren't expressible in `[package]`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ArxMeta {
    maintainer: Option<String>,
    arch: Option<String>,
    section: Option<String>,
    depends: Vec<String>,
    conflicts: Vec<String>,
    provides: Vec<String>,
    replaces: Vec<String>,
    files: Vec<FileEntry>,
    scripts: Scripts,
}
