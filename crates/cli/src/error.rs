//! Error types shared by the Rust and Roc code generators.

use std::io;
use std::path::{Path, PathBuf};

/// A filesystem operation on `path` failed.
#[derive(Debug, thiserror::Error)]
#[error("{operation} {}", path.display())]
pub struct PathIoError {
    /// what was being done, eg. "read shader source directory"
    pub operation: &'static str,
    pub path: PathBuf,
    #[source]
    pub source: io::Error,
}

impl PathIoError {
    /// A `map_err` adapter: `fs::read(path).map_err(PathIoError::at("read", path))`.
    pub fn at<'a>(operation: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> Self + 'a {
        move |source| Self {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}
