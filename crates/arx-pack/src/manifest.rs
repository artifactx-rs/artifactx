//! The packaging manifest: a single TOML document describing one package and
//! the files it installs. The same manifest drives both the `.deb` and `.rpm`
//! builders, so packagers describe intent once and target both ecosystems.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// Options used when deriving a package manifest from `Cargo.toml`.
///
/// These do not run Cargo. They only select the already-built binary path that
/// `arx pack` should read when `[package.metadata.arx]` does not specify
/// explicit `files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoManifestOptions {
    /// Override Cargo's target directory (`cargo build --target-dir ...`).
    pub target_dir: Option<PathBuf>,
    /// Target triple passed to Cargo (`cargo build --target ...`).
    pub target: Option<String>,
    /// Cargo profile name. Defaults to `release`.
    pub profile: String,
}

impl Default for CargoManifestOptions {
    fn default() -> Self {
        Self {
            target_dir: None,
            target: None,
            profile: "release".to_string(),
        }
    }
}

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

    /// Directories to recursively expand into installable files.
    #[serde(default)]
    pub dirs: Vec<DirEntry>,

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
        parse_octal_mode(&self.mode, "file")
    }
}

/// A directory tree to recursively stage into the package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    /// Directory path on the build host to traverse.
    pub source: String,
    /// Absolute install directory inside the target system.
    pub dest: String,
    /// Unix permission bits for regular files found under [`source`](Self::source).
    #[serde(default = "default_dir_file_mode")]
    pub file_mode: String,
    /// Unix permission bits for directory entries created under [`dest`](Self::dest).
    #[serde(default = "default_dir_mode")]
    pub dir_mode: String,
}

impl DirEntry {
    /// Parse [`file_mode`](Self::file_mode) as octal permission bits.
    pub fn file_mode_bits(&self) -> Result<u32> {
        parse_octal_mode(&self.file_mode, "directory file")
    }

    /// Parse [`dir_mode`](Self::dir_mode) as octal permission bits.
    pub fn dir_mode_bits(&self) -> Result<u32> {
        parse_octal_mode(&self.dir_mode, "directory")
    }
}

fn parse_octal_mode(mode: &str, label: &str) -> Result<u32> {
    let trimmed = mode.trim();
    let digits = trimmed.strip_prefix("0o").unwrap_or(trimmed);
    u32::from_str_radix(digits, 8).with_context(|| format!("invalid octal {label} mode {mode:?}"))
}

fn default_dir_file_mode() -> String {
    "0644".to_string()
}

fn default_dir_mode() -> String {
    "0755".to_string()
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
        Self::from_cargo_toml_at_with_options(
            cargo_toml,
            &std::env::current_dir().unwrap_or_default(),
            &CargoManifestOptions::default(),
        )
    }

    /// Like [`from_cargo_toml`], but the workspace-root walk starts from `crate_root`
    /// (the directory containing the `Cargo.toml` being parsed). Callers that know
    /// the file path should use this version to get correct target-dir resolution.
    pub fn from_cargo_toml_at(cargo_toml: &str, crate_root: &Path) -> Result<Self> {
        Self::from_cargo_toml_at_with_options(
            cargo_toml,
            crate_root,
            &CargoManifestOptions::default(),
        )
    }

    /// Like [`from_cargo_toml_at`], with explicit Cargo output selection options.
    pub fn from_cargo_toml_at_with_options(
        cargo_toml: &str,
        crate_root: &Path,
        options: &CargoManifestOptions,
    ) -> Result<Self> {
        let doc: toml::Value = toml::from_str(cargo_toml).context("parsing Cargo.toml")?;
        let pkg = doc
            .get("package")
            .and_then(|v| v.as_table())
            .ok_or_else(|| anyhow!("Cargo.toml has no [package] (a workspace root has none — run `arx pack` in a crate or pass a workspace member Cargo.toml path)"))?;

        // Workspace-root discovery: walk up from the crate root to find the nearest
        // Cargo.toml with a [workspace] table. Used for target-dir and inherited fields.
        let ws_root = find_workspace_root(crate_root);
        let ws_doc = workspace_doc(ws_root.as_deref());

        // --- identity fields, possibly inherited ---

        let name = pkg
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("[package] has no literal name"))?
            .to_string();

        let version = resolve_string(pkg, "version", &doc, ws_doc.as_ref())
            .ok_or_else(|| {
                anyhow!(
                    "[package].version must be a literal string or {{ workspace = true }} \
                 (workspace-inherited versions need a [workspace.package.version])"
                )
            })?
            .to_string();

        let description = resolve_string(pkg, "description", &doc, ws_doc.as_ref())
            .unwrap_or_else(|| name.clone());

        let license = resolve_string(pkg, "license", &doc, ws_doc.as_ref()).unwrap_or_default();

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
            // Default binary name: if exactly one [[bin]] exists use its name;
            // otherwise fall back to the package name. Multiple bins are a hard
            // error unless the user supplies explicit files.
            let bin_name = resolve_bin_name(&doc, &name)?;
            let target = resolve_target_dir(crate_root, &ws_root, options.target_dir.as_deref());
            let base = cargo_binary_dir(&target, options)?;
            // Use binary name, not package name, for the compiled artifact path.
            let src = base.join(&bin_name).to_string_lossy().into_owned();
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
        let author = resolve_authors(pkg, &doc, ws_doc.as_ref());

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
            dirs: arx.dirs,
            scripts: arx.scripts,
        })
    }

    /// The rpm `Group`, preferring an explicit [`group`](Self::group) and
    /// falling back to the deb [`section`](Self::section).
    pub fn rpm_group(&self) -> Option<&str> {
        self.group.as_deref().or(self.section.as_deref())
    }
}

// --- cargo workspace helpers (ADR-0012 §3) ---

/// Resolve a `[package]` field that may be a literal string or
/// `{ workspace = true }` (pulling from `[workspace.package]` in the doc).
fn resolve_string(
    pkg: &toml::Table,
    key: &str,
    doc: &toml::Value,
    ws_doc: Option<&toml::Value>,
) -> Option<String> {
    match pkg.get(key)? {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Table(t)
            if t.get("workspace")
                .and_then(|v| v.as_bool())
                .unwrap_or(false) =>
        {
            workspace_package_string(doc, key)
                .or_else(|| ws_doc.and_then(|doc| workspace_package_string(doc, key)))
        }
        _ => None,
    }
}

/// Resolve the first author string, handling `authors.workspace = true`.
fn resolve_authors(
    pkg: &toml::Table,
    doc: &toml::Value,
    ws_doc: Option<&toml::Value>,
) -> Option<String> {
    let authors = pkg.get("authors")?;
    // `authors = ["name <email>"]` — the common literal form.
    if let Some(arr) = authors.as_array() {
        return arr.first().and_then(|v| v.as_str()).map(str::to_string);
    }
    // `authors = { workspace = true }` — the inherited form.
    if let Some(t) = authors.as_table() {
        if t.get("workspace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return workspace_package_authors(doc)
                .or_else(|| ws_doc.and_then(workspace_package_authors));
        }
    }
    None
}

fn workspace_package_string(doc: &toml::Value, key: &str) -> Option<String> {
    doc.get("workspace")?
        .get("package")?
        .get(key)?
        .as_str()
        .map(str::to_string)
}

fn workspace_package_authors(doc: &toml::Value) -> Option<String> {
    doc.get("workspace")?
        .get("package")?
        .get("authors")?
        .as_array()?
        .first()
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Resolve the binary name for the default asset. Uses `[[bin]].name` when exactly
/// one bin target exists; falls back to the package name when no explicit bin is
/// declared. Multiple bins need explicit files so the default cannot choose the
/// wrong executable silently.
fn resolve_bin_name(doc: &toml::Value, package_name: &str) -> Result<String> {
    let bins = doc.get("bin").and_then(|v| v.as_array());
    match bins.map(|a| a.len()) {
        Some(1) => Ok(bins
            .unwrap()
            .first()
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| package_name.to_string())),
        Some(n) if n > 1 => {
            let names = bins
                .unwrap()
                .iter()
                .filter_map(|bin| bin.get("name").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "Cargo.toml declares multiple [[bin]] targets ({names}); add explicit [package.metadata.arx] files or use a standalone pack manifest"
            );
        }
        _ => Ok(package_name.to_string()),
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
                    if doc
                        .get("workspace")
                        .and_then(|v| v.get("members"))
                        .is_some()
                    {
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

fn workspace_doc(ws_root: Option<&Path>) -> Option<toml::Value> {
    let root = ws_root?;
    let text = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    text.parse::<toml::Value>().ok()
}

/// Resolve the `target/` directory, respecting workspace root and env var.
/// Order: explicit `--target-dir` → `CARGO_TARGET_DIR`/`CARGO_BUILD_TARGET_DIR`
/// → `.cargo/config.toml` → `<workspace-root>/target` → `<crate>/target`.
fn resolve_target_dir(
    crate_root: &Path,
    ws_root: &Option<PathBuf>,
    explicit_target_dir: Option<&Path>,
) -> PathBuf {
    // 1. CLI `--target-dir` equivalent. Cargo documents command-line target-dir
    // as overriding configured build.target-dir.
    if let Some(d) = explicit_target_dir {
        return d.to_path_buf();
    }
    // 2. Cargo-recognised env vars.
    if let Ok(d) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(d);
    }
    if let Ok(d) = std::env::var("CARGO_BUILD_TARGET_DIR") {
        return PathBuf::from(d);
    }
    // 3. .cargo/config.toml [build] target-dir, searched upward from the crate.
    if let Some(td) = config_target_dir(crate_root) {
        return td;
    }
    // 4. Workspace root's target dir.
    if let Some(root) = ws_root {
        return root.join("target");
    }
    // 5. Fallback: relative to the selected crate root.
    crate_root.join("target")
}

/// Read `.cargo/config.toml` and extract `[build].target-dir` if present.
fn config_target_dir(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let config = cur.join(".cargo/config.toml");
        if config.exists() {
            if let Ok(text) = std::fs::read_to_string(&config) {
                if let Ok(doc) = text.parse::<toml::Value>() {
                    if let Some(v) = doc
                        .get("build")
                        .and_then(|b| b.get("target-dir"))
                        .and_then(|v| v.as_str())
                    {
                        let path = PathBuf::from(v);
                        return Some(if path.is_absolute() {
                            path
                        } else {
                            cur.join(path)
                        });
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

fn cargo_binary_dir(target_dir: &Path, options: &CargoManifestOptions) -> Result<PathBuf> {
    let profile_dir = profile_output_dir(&options.profile)?;
    let mut dir = target_dir.to_path_buf();
    if let Some(target) = options.target.as_deref() {
        dir.push(validate_cargo_path_component(target, "target triple")?);
    }
    dir.push(profile_dir);
    Ok(dir)
}

fn profile_output_dir(profile: &str) -> Result<&str> {
    let profile = validate_cargo_path_component(profile, "profile")?;
    Ok(match profile {
        // Cargo's dev profile writes binary artifacts under target/debug.
        "dev" => "debug",
        other => other,
    })
}

fn validate_cargo_path_component<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
    {
        anyhow::bail!("invalid Cargo {label} {value:?}: expected one path component");
    }
    Ok(value)
}

/// The `[package.metadata.arx]` table in a `Cargo.toml`: packaging fields that
/// aren't expressible in `[package]`.
#[derive(Debug, Clone, Default, Deserialize)]
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
    dirs: Vec<DirEntry>,
    scripts: Scripts,
}
