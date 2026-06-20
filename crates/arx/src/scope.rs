//! Validation for user-facing repository scope names.
//!
//! Apt components and yum repo names are logical names, not filesystem paths.
//! Keep them to a single safe path segment before using them in `Path::join`.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// A user supplied repository scope name was not a single safe path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidScopeName {
    field: String,
    value: String,
}

impl InvalidScopeName {
    fn new(field: &str, value: &str) -> Self {
        Self {
            field: field.to_string(),
            value: value.to_string(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(field: &str, value: &str) -> Self {
        Self::new(field, value)
    }
}

impl fmt::Display for InvalidScopeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid {} {:?}: expected a single repository scope name, not a path",
            self.field, self.value
        )
    }
}

impl std::error::Error for InvalidScopeName {}

/// Validate an apt component, yum repo, or promotion scope name.
pub fn validate_scope_name<'a>(name: &'a str, field: &str) -> Result<&'a str, InvalidScopeName> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || is_windows_reserved_name(name)
        || name.chars().any(char::is_control)
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
    {
        return Err(InvalidScopeName::new(field, name));
    }

    Ok(name)
}

/// Validate a repo-relative path made only of safe path segments.
///
/// Unlike repository scope names, signing key paths may contain more than one
/// segment (for example `keys/private.asc`), but they must never be absolute,
/// contain `.`/`..`, carry Windows prefixes, or include unsafe segment names.
pub fn validate_repo_relative_path(path: &str, field: &str) -> Result<PathBuf, InvalidScopeName> {
    if path.is_empty() {
        return Err(InvalidScopeName::new(field, path));
    }

    let mut out = PathBuf::new();
    let mut saw_segment = false;
    for component in Path::new(path).components() {
        match component {
            Component::Normal(segment) => {
                let segment = segment
                    .to_str()
                    .ok_or_else(|| InvalidScopeName::new(field, path))?;
                validate_scope_name(segment, field)
                    .map_err(|_| InvalidScopeName::new(field, path))?;
                out.push(segment);
                saw_segment = true;
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => return Err(InvalidScopeName::new(field, path)),
        }
    }

    if !saw_segment {
        return Err(InvalidScopeName::new(field, path));
    }
    Ok(out)
}

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(test)]
mod tests {
    use super::{validate_repo_relative_path, validate_scope_name};

    #[test]
    fn accepts_common_repository_scope_names() {
        for name in ["main", "stable", "non-free-firmware", "myrepo_1.2"] {
            assert_eq!(validate_scope_name(name, "scope").unwrap(), name);
        }
    }

    #[test]
    fn accepts_safe_repo_relative_paths() {
        assert_eq!(
            validate_repo_relative_path("keys/private.asc", "signing private key").unwrap(),
            std::path::PathBuf::from("keys/private.asc")
        );
        assert_eq!(
            validate_repo_relative_path("keys", "signing keys dir").unwrap(),
            std::path::PathBuf::from("keys")
        );
    }

    #[test]
    fn rejects_repo_relative_path_escape_attempts() {
        for path in [
            "",
            ".",
            "..",
            "../escape",
            "keys/../escape",
            "/tmp/key.asc",
            r"C:\escape\key.asc",
            "keys/private.asc.old.",
            "keys/CON",
            "keys/bad name.asc",
            "keys/bad\name.asc",
        ] {
            assert!(
                validate_repo_relative_path(path, "signing key path").is_err(),
                "{path:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_paths_and_ambiguous_scope_names() {
        for name in [
            "",
            ".",
            "..",
            "../x",
            "x/../y",
            "/tmp/x",
            "x/y",
            r"x\y",
            "C:escape",
            "repo:arch",
            "main.",
            "main ",
            "CON",
            "con.txt",
            "LPT1",
            "bad\0name",
            "bad\nname",
        ] {
            assert!(
                validate_scope_name(name, "scope").is_err(),
                "{name:?} should be rejected"
            );
        }
    }
}
