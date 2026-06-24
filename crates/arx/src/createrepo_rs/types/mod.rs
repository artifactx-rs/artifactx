//! Idiomatic Rust data structures for package metadata.
//!
//! These structures replace the C FFI types and provide a clean, safe API
//! for working with RPM package metadata.

/// Checksum type used for package verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChecksumType {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    #[default]
    Unknown,
}

/// Compression type used for metadata files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionType {
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    #[default]
    None,
}

/// Represents an RPM dependency.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dependency {
    /// Name of the dependency.
    pub name: String,
    /// Epoch of the dependency version.
    pub epoch: Option<i32>,
    /// Version of the dependency.
    pub version: Option<String>,
    /// Release of the dependency.
    pub release: Option<String>,
    /// Comparison flags (e.g., "EQ", "LT", "GT", "LE", "GE").
    pub flags: String,
    /// Whether this is a pre-dependency.
    pub pre: bool,
}

/// Represents a changelog entry for a package.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangelogEntry {
    /// Author of the changelog entry.
    pub author: String,
    /// Date of the changelog entry (Unix timestamp).
    pub date: i64,
    /// Content of the changelog entry.
    pub content: String,
}

impl From<crate::createrepo_rs::rpm::DependencyInfo> for Dependency {
    fn from(d: crate::createrepo_rs::rpm::DependencyInfo) -> Self {
        Self {
            name: d.name,
            epoch: d.epoch,
            version: d.version,
            release: d.release,
            flags: d.flags,
            pre: d.pre,
        }
    }
}

impl From<crate::createrepo_rs::rpm::ChangelogInfo> for ChangelogEntry {
    fn from(c: crate::createrepo_rs::rpm::ChangelogInfo) -> Self {
        Self {
            author: c.author,
            date: c.date,
            content: c.content,
        }
    }
}

/// Represents a file within a package.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageFile {
    /// Path to the file.
    pub path: String,
    /// Type of the file.
    pub file_type: String,
    /// Digest/checksum of the file.
    pub digest: Option<String>,
    /// Size of the file.
    pub size: i64,
}

/// Represents a complete RPM package metadata.
///
/// This structure replaces the C `cr_Package` FFI type with
/// a safe, idiomatic Rust representation.
#[derive(Debug, Clone, Default)]
pub struct Package {
    // Identification
    /// Package identifier (hex MD5).
    pub pkgid: String,
    /// Name of the package.
    pub name: String,
    /// Architecture.
    pub arch: String,
    /// Version string.
    pub version: String,
    /// Epoch.
    pub epoch: Option<i32>,
    /// Release string.
    pub release: String,

    // Filename and location
    /// Filename of the package.
    pub filename: String,
    /// Relative path to the package.
    pub location: String,

    // Checksums
    /// Type of primary checksum.
    pub checksum_type: ChecksumType,
    /// Primary checksum value.
    pub checksum: String,
    /// Source package reference.
    pub source_pkg: Option<String>,

    // Sizes
    /// Size of the archive (compressed package size).
    pub size_archive: i64,
    /// Size when installed.
    pub size_installed: i64,
    /// Size of the package file.
    pub size_package: i64,

    // Time
    /// File modification time.
    pub time_file: i64,
    /// Build time.
    pub time_build: i64,

    // Metadata
    /// Package summary (short description).
    pub summary: Option<String>,
    /// Full package description.
    pub description: Option<String>,
    /// Packager.
    pub packager: Option<String>,
    /// Package URL.
    pub url: Option<String>,
    /// Package license.
    pub license: Option<String>,
    /// Vendor.
    pub vendor: Option<String>,
    /// Package group.
    pub group: Option<String>,
    /// Build host.
    pub buildhost: Option<String>,
    /// Source RPM name.
    pub sourcerpm: Option<String>,

    // Dependencies
    /// Packages required by this package.
    pub requires: Vec<Dependency>,
    /// Packages provided by this package.
    pub provides: Vec<Dependency>,
    /// Packages that conflict with this package.
    pub conflicts: Vec<Dependency>,
    /// Packages that this package obsoletes.
    pub obsoletes: Vec<Dependency>,
    /// Suggested packages.
    pub suggests: Vec<Dependency>,
    /// Packages that enhance this package.
    pub enhances: Vec<Dependency>,
    /// Recommended packages.
    pub recommends: Vec<Dependency>,
    /// Supplementary packages.
    pub supplements: Vec<Dependency>,

    // Files
    /// Files contained in the package.
    pub files: Vec<PackageFile>,

    // Changelog
    /// Changelog entries.
    pub changelogs: Vec<ChangelogEntry>,

    // Flags and locations
    /// Location href (relative path to package).
    pub location_href: Option<String>,
    /// Header start offset.
    pub header_start: Option<i64>,
    /// Header end offset.
    pub header_end: Option<i64>,
}

/// Represents a record within a repomd repository.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepomdRecord {
    pub record_type: String,
    pub location: String,
    pub checksum: Option<String>,
    pub timestamp: Option<i64>,
    pub size: Option<i64>,
    pub open_size: Option<i64>,
    pub open_checksum: Option<String>,
    #[doc(hidden)]
    pub checksum_type: Option<String>,
}

/// A distribution tag for the repository.
#[derive(Debug, Clone)]
pub struct DistroTag {
    /// The distribution tag value (e.g., "Fedora 39").
    pub tag: String,
    /// Optional CPE identifier.
    pub cpeid: Option<String>,
}

/// A content tag for the repository.
#[derive(Debug, Clone)]
pub struct ContentTag {
    /// The content tag value.
    pub tag: String,
}

/// A repository tag for the repository.
#[derive(Debug, Clone)]
pub struct RepoTag {
    /// The repository tag value.
    pub tag: String,
}

/// Represents the repomd.xml metadata for a repository.
///
/// Contains revision information and a collection of records
/// pointing to various metadata files.
#[derive(Debug, Clone, Default)]
pub struct Repomd {
    pub revision: Option<String>,
    pub records: Vec<RepomdRecord>,
    pub distro_tags: Vec<DistroTag>,
    pub content_tags: Vec<ContentTag>,
    pub repo_tags: Vec<RepoTag>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_type_default() {
        assert_eq!(ChecksumType::default(), ChecksumType::Unknown);
    }

    #[test]
    fn test_compression_type_default() {
        assert_eq!(CompressionType::default(), CompressionType::None);
    }

    #[test]
    fn test_dependency_default() {
        let dep = Dependency::default();
        assert!(dep.name.is_empty());
        assert!(dep.epoch.is_none());
        assert!(dep.version.is_none());
        assert!(dep.release.is_none());
        assert!(dep.flags.is_empty());
        assert!(!dep.pre);
    }

    #[test]
    fn test_package_default() {
        let pkg = Package::default();
        assert!(pkg.pkgid.is_empty());
        assert!(pkg.name.is_empty());
        assert!(pkg.requires.is_empty());
        assert!(pkg.provides.is_empty());
        assert!(pkg.files.is_empty());
        assert!(pkg.changelogs.is_empty());
    }

    #[test]
    fn test_repomd_record_default() {
        let record = RepomdRecord::default();
        assert!(record.record_type.is_empty());
        assert!(record.location.is_empty());
        assert!(record.checksum.is_none());
    }

    #[test]
    fn test_repomd_default() {
        let repomd = Repomd::default();
        assert!(repomd.revision.is_none());
        assert!(repomd.records.is_empty());
    }

    #[test]
    fn test_dependency_equality() {
        let dep1 = Dependency {
            name: "bash".to_string(),
            epoch: Some(0),
            version: Some("4.0".to_string()),
            release: Some("1".to_string()),
            flags: "EQ".to_string(),
            pre: false,
        };
        let dep2 = Dependency {
            name: "bash".to_string(),
            epoch: Some(0),
            version: Some("4.0".to_string()),
            release: Some("1".to_string()),
            flags: "EQ".to_string(),
            pre: false,
        };
        assert_eq!(dep1, dep2);
    }

    #[test]
    fn test_package_clone() {
        let pkg = Package {
            pkgid: "abc123".to_string(),
            name: "test".to_string(),
            arch: "x86_64".to_string(),
            version: "1.0".to_string(),
            epoch: Some(0),
            release: "1".to_string(),
            filename: "test-1.0-1.x86_64.rpm".to_string(),
            location: String::new(),
            checksum_type: ChecksumType::Sha256,
            checksum: "def456".to_string(),
            source_pkg: None,
            size_archive: 1024,
            size_installed: 2048,
            size_package: 4096,
            time_file: 1234567890,
            time_build: 1234567800,
            summary: Some("Test package".to_string()),
            description: Some("A test package".to_string()),
            packager: Some("Test Packager".to_string()),
            url: Some("https://example.com".to_string()),
            license: Some("MIT".to_string()),
            vendor: Some("Test".to_string()),
            group: Some("Development/Tools".to_string()),
            buildhost: Some("build.example.com".to_string()),
            sourcerpm: Some("test-1.0-1.src.rpm".to_string()),
            requires: vec![Dependency::default()],
            provides: vec![],
            conflicts: vec![],
            obsoletes: vec![],
            suggests: vec![],
            enhances: vec![],
            recommends: vec![],
            supplements: vec![],
            files: vec![PackageFile::default()],
            changelogs: vec![ChangelogEntry::default()],
            location_href: None,
            header_start: Some(100),
            header_end: Some(200),
        };
        let cloned = pkg;
        assert_eq!(cloned.pkgid, "abc123");
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.requires.len(), 1);
        assert_eq!(cloned.files.len(), 1);
    }
}
