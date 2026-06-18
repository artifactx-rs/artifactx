//! The packaging manifest: a single TOML document describing one package and
//! the files it installs. The same manifest drives both the `.deb` and `.rpm`
//! builders, so packagers describe intent once and target both ecosystems.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// A complete package description parsed from TOML.
///
/// `arch` is accepted in either ecosystem's spelling (e.g. `amd64` or
/// `x86_64`); the builders normalise it to the convention each format expects.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// `<workspace-root>/target/release/<bin-name>` → `/usr/bin/<bin-name>` (so a
    /// built CLI just works, in and out of workspaces — ADR-0012 §3).
    ///
    /// This reads the TOML document from the caller (who knows the real path of
    /// the Cargo.toml on disk) so it does *not* walk up itself; workspace-root
    /// discovery uses `crate_root` (std::env::current_dir() or the manifest path's
    /// parent). For the simple case use [`from_cargo_toml_at`].
    pub fn from_cargo_toml(cargo_toml: &str) -> Result<Self> {
        Self::from_cargo_toml_at(cargo_toml, &std::env::current_dir().unwrap_or_default())
    }

    /// Like [`from_cargo_toml`], but the workspace-root walk starts from `crate_root`
    /// (the directory containing the `Cargo.toml` being parsed). Callers that know
    /// the file path should use this version to get correct target-dir resolution.
    pub fn from_cargo_toml_at(cargo_toml: &str, crate_root: &Path) -> Result<Self> {
        let doc: toml::Value = toml::from_str(cargo_toml).context("parsing Cargo.toml")?;
        let pkg = doc
            .get("package")
            .and_then(|v| v.as_table())
            .ok_or_else(|| anyhow!("Cargo.toml has no [package] (a workspace root has none — run `arx pack` in a crate)"))?;

        // --- identity fields, possibly inherited ---

        let name = pkg
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("[package] has no literal name"))?
            .to_string();

        let version = resolve_string(pkg, "version", &doc)
            .ok_or_else(|| anyhow!(
                "[package].version must be a literal string or {{ workspace = true }} \
                 (workspace-inherited versions need a [workspace.package.version])"
            ))?
            .to_string();

        let description = resolve_string(pkg, "description", &doc)
            .unwrap_or_else(|| name.clone());

        let license = resolve_string(pkg, "license", &doc).unwrap_or_default();

        // Workspace-root discovery: walk up from the crate root to find the nearest
        // Cargo.toml with a [workspace] table. Used for target-dir and inherited fields.
        let ws_root = find_workspace_root(crate_root);

        // Default binary name: if exactly one [[bin]] exists use its name; otherwise
        // fall back to the package name. (ADR-0012 §3, [[bin]].name resolution.)
        let bin_name = resolve_bin_name(&doc, &name);

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
            let target = resolve_target_dir(&ws_root);
            let base = target.join("release");
            // Use binary name, not package name, for the compiled artifact path.
            let src = base
                .join(&bin_name)
                .to_string_lossy()
                .into_owned();
            vec![FileEntry {
                source: src,
                dest: format!("/usr/bin/{bin_name}"),
                mode: "0755".to_string(),
            }]
        } else {
            arx.files
        };

        // Maintainer: explicit `[package.metadata.arx].maintainer` wins; then the
        // first `authors` entry (resolving {workspace=true}); then a fallback.
        let author = resolve_authors(pkg, &doc);

        Ok(Manifest {
            name: name.clone(),
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

// --- cargo workspace helpers (ADR-0012 §3) ---

/// Resolve a `[package]` field that may be a literal string or
/// `{ workspace = true }` (pulling from `[workspace.package]` in the doc).
fn resolve_string(pkg: &toml::Table, key: &str, doc: &toml::Value) -> Option<String> {
    match pkg.get(key)? {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Table(t) if t.get("workspace").and_then(|v| v.as_bool()).unwrap_or(false) => {
            doc.get("workspace")?
                .get("package")?
                .get(key)?
                .as_str()
                .map(str::to_string)
        }
        _ => None,
    }
}

/// Resolve the first author string, handling `authors.workspace = true`.
fn resolve_authors(pkg: &toml::Table, doc: &toml::Value) -> Option<String> {
    let authors = pkg.get("authors")?;
    // `authors = ["name <email>"]` — the common literal form.
    if let Some(arr) = authors.as_array() {
        return arr.first().and_then(|v| v.as_str()).map(str::to_string);
    }
    // `authors = { workspace = true }` — the inherited form.
    if let Some(t) = authors.as_table() {
        if t.get("workspace").and_then(|v| v.as_bool()).unwrap_or(false) {
            return doc
                .get("workspace")?
                .get("package")?
                .get("authors")?
                .as_array()?
                .first()
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
    }
    None
}

/// Resolve the binary name for the default asset. Uses `[[bin]].name` when exactly
/// one bin target exists; falls back to the package name (0 or >1 bins → guess the
/// package name, which may be wrong for multi-bin crates — they need explicit `files`).
fn resolve_bin_name(doc: &toml::Value, package_name: &str) -> String {
    let bins = doc.get("bin").and_then(|v| v.as_array());
    match bins.map(|a| a.len()) {
        Some(1) => {
            bins.unwrap()
                .first()
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| package_name.to_string())
        }
        _ => package_name.to_string(),
    }
}

/// Walk up from `start` to find the nearest `Cargo.toml` whose `[workspace]` table
/// includes `members`. Returns `None` when no workspace is found (standalone crate).
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let cargo = cur.join("Cargo.toml");
        if cargo.exists() {
            if let Ok(text) = std::fs::read_to_string(&cargo) {
                if let Ok(doc) = text.parse::<toml::Value>() {
                    if doc.get("workspace").and_then(|v| v.get("members")).is_some() {
                        return Some(cur);
                    }
                }
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Resolve the `target/` directory, respecting workspace root and env var.
/// Order: `CARGO_TARGET_DIR` → `.cargo/config.toml` → `<workspace-root>/target`
/// → `<crate>/target`. (ADR-0012 §3.)
fn resolve_target_dir(ws_root: &Option<PathBuf>) -> PathBuf {
    // 1. CARGO_TARGET_DIR env var.
    if let Ok(d) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(d);
    }
    // 2. .cargo/config.toml [build] target-dir, searched upward from cwd.
    if let Some(td) = config_target_dir() {
        return td;
    }
    // 3. Workspace root's target dir.
    if let Some(root) = ws_root {
        return root.join("target");
    }
    // 4. Fallback: relative to cwd.
    PathBuf::from("target")
}

/// Read `.cargo/config.toml` and extract `[build].target-dir` if present.
fn config_target_dir() -> Option<PathBuf> {
    let mut cur = std::env::current_dir().ok()?;
    loop {
        let config = cur.join(".cargo/config.toml");
        if config.exists() {
            if let Ok(text) = std::fs::read_to_string(&config) {
                if let Ok(doc) = text.parse::<toml::Value>() {
                    if let Some(v) = doc.get("build").and_then(|b| b.get("target-dir")).and_then(|v| v.as_str()) {
                        return Some(PathBuf::from(v));
                    }
                }
            }
            break;
        }
        if !cur.pop() {
            break;
        }
    }
    None
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
