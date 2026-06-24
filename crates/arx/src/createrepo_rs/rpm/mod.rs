use rpm as rpm_crate;
use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RpmError {
    #[error("Failed to open RPM: {0}")]
    Open(String),
    #[error("Failed to read RPM metadata: {0}")]
    Metadata(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Helper struct to represent file type from file entries
#[derive(Debug, Clone)]
pub struct FileTypeInfo {
    pub path: String,
    pub file_type: String,
    pub digest: Option<String>,
    pub size: i64,
}

/// RPM Dependency information extracted from the package
#[derive(Debug, Clone)]
pub struct DependencyInfo {
    pub name: String,
    pub flags: String,
    pub epoch: Option<i32>,
    pub version: Option<String>,
    pub release: Option<String>,
    pub pre: bool,
}

/// Changelog entry from the RPM
#[derive(Debug, Clone)]
pub struct ChangelogInfo {
    pub author: String,
    pub date: i64,
    pub content: String,
}

/// Lightweight manifest entry — just the fields needed for `--dump-manifest`.
pub struct ManifestEntry {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub signed: bool,
}

pub struct PackageFile {
    pub path: String,
    pub file_type: Option<String>,
    pub digest: Option<String>,
}

pub struct Package {
    pub name: String,
    pub arch: String,
    pub version: String,
    pub epoch: Option<String>,
    pub release: String,
    pub location: String,
    pub time_file: i64,
    pub time_build: i64,
    pub size: i64,
    pub file_size: i64,
    pub sha256: String,
    pub files: Vec<PackageFile>,
    // New fields for full metadata extraction
    pub summary: Option<String>,
    pub description: Option<String>,
    pub packager: Option<String>,
    pub url: Option<String>,
    pub license: Option<String>,
    pub vendor: Option<String>,
    pub group: Option<String>,
    pub buildhost: Option<String>,
    pub sourcerpm: Option<String>,
    pub provides: Vec<DependencyInfo>,
    pub requires: Vec<DependencyInfo>,
    pub conflicts: Vec<DependencyInfo>,
    pub obsoletes: Vec<DependencyInfo>,
    pub suggests: Vec<DependencyInfo>,
    pub enhances: Vec<DependencyInfo>,
    pub recommends: Vec<DependencyInfo>,
    pub supplements: Vec<DependencyInfo>,
    pub changelogs: Vec<ChangelogInfo>,
}

pub struct RpmReader {
    path: std::path::PathBuf,
}

/// Parse an RPM dependency version string into (epoch, version, release).
///
/// RPM dependency versions follow the format `[epoch:]version[-release]`.
/// Examples:
/// - `"1.2.3"` → `(None, "1.2.3", None)`
/// - `"1.2.3-4"` → `(None, "1.2.3", "4")`
/// - `"0:1.2.3-4"` → `(Some(0), "1.2.3", "4")`
/// - `"5:3.14"` → `(Some(5), "3.14", None)`
#[must_use]
pub fn parse_dep_version(raw: &str) -> (Option<i32>, Option<String>, Option<String>) {
    if raw.is_empty() {
        return (None, None, None);
    }

    let (epoch, rest) = if let Some(colon_pos) = raw.find(':') {
        let epoch_str = &raw[..colon_pos];
        let epoch_val = epoch_str.parse::<i32>().ok();
        (epoch_val, &raw[colon_pos + 1..])
    } else {
        (None, raw)
    };

    let (version, release) = match rest.rfind('-') {
        Some(pos) => (
            Some(rest[..pos].to_string()),
            Some(rest[pos + 1..].to_string()),
        ),
        None if !rest.is_empty() => (Some(rest.to_string()), None),
        None => (None, None),
    };

    (epoch, version, release)
}

fn convert_dependency_flags(flags: rpm_crate::DependencyFlags) -> String {
    // C version: cr_flag_to_str(flags & 0xf)
    //  0 -> ""
    //  2 -> "LT" (LESS)
    //  4 -> "GT" (GREATER)
    //  8 -> "EQ" (EQUAL)
    // 10 -> "LE" (LESS | EQUAL)
    // 12 -> "GE" (GREATER | EQUAL)
    // default -> ""
    let bits = flags.bits() as u8 & 0x0f;
    match bits {
        2 => "LT",
        4 => "GT",
        8 => "EQ",
        10 => "LE",
        12 => "GE",
        _ => "",
    }
    .to_string()
}

fn convert_file_mode(mode: rpm_crate::FileMode) -> String {
    match mode.file_type() {
        rpm_crate::FileType::Dir => "dir".to_string(),
        rpm_crate::FileType::Regular => "file".to_string(),
        rpm_crate::FileType::SymbolicLink => "symlink".to_string(),
        rpm_crate::FileType::Other => "unknown".to_string(),
        _ => "unknown".to_string(),
    }
}

fn process_deps(deps: Vec<rpm_crate::Dependency>) -> Vec<DependencyInfo> {
    deps.into_iter()
        .map(|dep| {
            let flags_str = convert_dependency_flags(dep.flags);
            let (epoch, version, release) = parse_dep_version(&dep.version);
            DependencyInfo {
                name: dep.name,
                flags: flags_str,
                epoch,
                version,
                release,
                pre: false,
            }
        })
        .collect()
}

impl RpmReader {
    pub fn open(path: &Path) -> Result<Self, RpmError> {
        if !path.exists() {
            return Err(RpmError::Open(format!(
                "File does not exist: {}",
                path.display()
            )));
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    pub fn read_package(&mut self) -> Result<Package, RpmError> {
        let metadata = rpm_crate::PackageMetadata::open(&self.path)
            .map_err(|e| RpmError::Open(e.to_string()))?;

        let name = metadata
            .get_name()
            .map_err(|e| RpmError::Metadata(e.to_string()))?
            .to_string();
        let version = metadata
            .get_version()
            .map_err(|e| RpmError::Metadata(e.to_string()))?
            .to_string();
        let release = metadata
            .get_release()
            .map_err(|e| RpmError::Metadata(e.to_string()))?
            .to_string();
        let arch = metadata
            .get_arch()
            .map_err(|e| RpmError::Metadata(e.to_string()))?
            .to_string();

        let epoch = metadata.get_epoch().map(|e| e.to_string()).ok();
        let time_build = metadata.get_build_time().map_or(0, |t| t as i64);

        let file_meta = std::fs::metadata(&self.path).ok();
        let file_size = file_meta.as_ref().map_or(0, |m| m.len() as i64);
        let time_file = file_meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(time_build, |d| d.as_secs() as i64);

        let location = self.path.file_name().map_or_else(
            || self.path.to_string_lossy().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );

        let file_entries = metadata.get_file_entries().unwrap_or_default();
        let files: Vec<PackageFile> = file_entries
            .into_iter()
            .map(|entry| PackageFile {
                path: entry.path().to_string_lossy().into_owned(),
                file_type: Some(convert_file_mode(entry.mode())),
                digest: entry.digest().map(|d| d.to_string()),
            })
            .collect();

        let sha256 = compute_sha256(&self.path)?;

        let summary = metadata
            .get_summary()
            .map(std::string::ToString::to_string)
            .ok();
        let description = metadata
            .get_description()
            .map(std::string::ToString::to_string)
            .ok();
        let packager = metadata
            .get_packager()
            .map(std::string::ToString::to_string)
            .ok();
        let url = metadata
            .get_url()
            .map(std::string::ToString::to_string)
            .ok();
        let license = metadata
            .get_license()
            .map(std::string::ToString::to_string)
            .ok();
        let vendor = metadata
            .get_vendor()
            .map(std::string::ToString::to_string)
            .ok();
        let group = metadata
            .get_group()
            .map(std::string::ToString::to_string)
            .ok();
        let buildhost = metadata
            .get_build_host()
            .map(std::string::ToString::to_string)
            .ok();
        let sourcerpm = metadata
            .get_source_rpm()
            .map(std::string::ToString::to_string)
            .ok();

        let provides = process_deps(metadata.get_provides().unwrap_or_default());
        let requires = process_deps(metadata.get_requires().unwrap_or_default());
        let conflicts = process_deps(metadata.get_conflicts().unwrap_or_default());
        let obsoletes = process_deps(metadata.get_obsoletes().unwrap_or_default());
        let suggests = process_deps(metadata.get_suggests().unwrap_or_default());
        let enhances = process_deps(metadata.get_enhances().unwrap_or_default());
        let recommends = process_deps(metadata.get_recommends().unwrap_or_default());
        let supplements = process_deps(metadata.get_supplements().unwrap_or_default());

        let changelogs = metadata
            .get_changelog_entries()
            .unwrap_or_default()
            .into_iter()
            .map(|entry| ChangelogInfo {
                author: entry.name,
                date: entry.timestamp as i64,
                content: entry.description,
            })
            .collect();

        Ok(Package {
            name,
            arch,
            version,
            epoch,
            release,
            location,
            time_file,
            time_build,
            size: file_size,
            file_size,
            sha256,
            files,
            summary,
            description,
            packager,
            url,
            license,
            vendor,
            group,
            buildhost,
            sourcerpm,
            provides,
            requires,
            conflicts,
            obsoletes,
            suggests,
            enhances,
            recommends,
            supplements,
            changelogs,
        })
    }

    pub fn checksum(&self) -> Result<String, RpmError> {
        compute_sha256(&self.path)
    }

    /// Check whether this RPM is cryptographically signed.
    ///
    /// Returns true if the signature header contains a PGP, RSA, or DSA
    /// signature entry.
    #[must_use]
    pub fn is_signed(&self) -> bool {
        let Ok(metadata) = rpm_crate::PackageMetadata::open(&self.path) else {
            return false;
        };
        use rpm_crate::IndexSignatureTag;
        metadata
            .signature
            .entry_is_present(IndexSignatureTag::RPMSIGTAG_PGP)
            || metadata
                .signature
                .entry_is_present(IndexSignatureTag::RPMSIGTAG_RSA)
            || metadata
                .signature
                .entry_is_present(IndexSignatureTag::RPMSIGTAG_DSA)
    }

    /// Read only manifest fields (name, version, arch, signed).
    ///
    /// Much faster than [`read_package`](Self::read_package) — skips file
    /// lists, dependencies, changelogs, and checksum computation.
    pub fn read_manifest_entry(&mut self) -> Result<ManifestEntry, RpmError> {
        let metadata = rpm_crate::PackageMetadata::open(&self.path)
            .map_err(|e| RpmError::Open(e.to_string()))?;

        let name = metadata
            .get_name()
            .map_err(|e| RpmError::Metadata(e.to_string()))?
            .to_string();
        let version = metadata
            .get_version()
            .map_err(|e| RpmError::Metadata(e.to_string()))?
            .to_string();
        let arch = metadata
            .get_arch()
            .map_err(|e| RpmError::Metadata(e.to_string()))?
            .to_string();

        use rpm_crate::IndexSignatureTag;
        let signed = metadata
            .signature
            .entry_is_present(IndexSignatureTag::RPMSIGTAG_PGP)
            || metadata
                .signature
                .entry_is_present(IndexSignatureTag::RPMSIGTAG_RSA)
            || metadata
                .signature
                .entry_is_present(IndexSignatureTag::RPMSIGTAG_DSA);

        Ok(ManifestEntry {
            name,
            version,
            arch,
            signed,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn compute_sha256(path: &Path) -> Result<String, RpmError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 65536];
    loop {
        let bytes_read = std::io::Read::read(&mut file, &mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let result = hasher.finalize();
    Ok(format!("{result:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dep_version_empty() {
        assert_eq!(parse_dep_version(""), (None, None, None));
    }

    #[test]
    fn test_parse_dep_version_no_release() {
        assert_eq!(
            parse_dep_version("1.2.3"),
            (None, Some("1.2.3".into()), None)
        );
    }

    #[test]
    fn test_parse_dep_version_with_release() {
        assert_eq!(
            parse_dep_version("1.2.3-4"),
            (None, Some("1.2.3".into()), Some("4".into()))
        );
    }

    #[test]
    fn test_parse_dep_version_with_epoch() {
        assert_eq!(
            parse_dep_version("0:1.2.3-4"),
            (Some(0), Some("1.2.3".into()), Some("4".into()))
        );
    }

    #[test]
    fn test_parse_dep_version_epoch_only() {
        assert_eq!(
            parse_dep_version("5:3.14"),
            (Some(5), Some("3.14".into()), None)
        );
    }

    #[test]
    fn test_parse_dep_version_multiple_hyphens() {
        assert_eq!(
            parse_dep_version("1.2.3-rc1-5"),
            (None, Some("1.2.3-rc1".into()), Some("5".into()))
        );
    }

    #[test]
    fn test_parse_dep_version_hyphen_in_epoch() {
        assert_eq!(
            parse_dep_version("3:1.2.3-4"),
            (Some(3), Some("1.2.3".into()), Some("4".into()))
        );
    }
}
