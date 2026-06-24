use glob::Pattern;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Error, Debug)]
pub enum WalkError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid glob pattern: {0}")]
    Glob(#[from] glob::PatternError),
}

pub struct DirectoryWalker {
    #[allow(dead_code)]
    path: PathBuf,
    exclude_patterns: Vec<Pattern>,
    skip_symlinks: bool,
    inner: Option<walkdir::IntoIter>,
}

impl DirectoryWalker {
    pub fn new(path: &Path) -> Result<Self, WalkError> {
        let walker = WalkDir::new(path).follow_links(false).into_iter();
        Ok(Self {
            path: path.to_path_buf(),
            exclude_patterns: Vec::new(),
            skip_symlinks: false,
            inner: Some(walker),
        })
    }

    pub fn exclude_patterns(mut self, patterns: Vec<String>) -> Result<Self, WalkError> {
        self.exclude_patterns = patterns
            .iter()
            .map(|p| Pattern::new(p))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
    }

    #[must_use]
    pub const fn skip_symlinks(mut self, skip: bool) -> Self {
        self.skip_symlinks = skip;
        self
    }

    pub const fn init(self) -> Result<Self, WalkError> {
        Ok(self)
    }
}

impl Iterator for DirectoryWalker {
    type Item = PathBuf;

    fn next(&mut self) -> Option<Self::Item> {
        let skip_symlinks = self.skip_symlinks;
        let exclude_patterns = &self.exclude_patterns;

        loop {
            let entry = self.inner.as_mut()?.next()?;

            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if skip_symlinks && entry.path_is_symlink() {
                continue;
            }

            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if !name.ends_with(".rpm") {
                continue;
            }

            for pattern in exclude_patterns {
                if pattern.matches(name) {
                    continue;
                }
            }

            return Some(path.to_path_buf());
        }
    }
}

impl DirectoryWalker {
    #[must_use]
    pub fn collect(self) -> Vec<PathBuf> {
        let mut results = Vec::new();
        for path in self {
            results.push(path);
        }
        results
    }
}
