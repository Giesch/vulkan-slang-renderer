//! Error types shared by the Rust and Roc code generators.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use eros::{InnerError, SendSyncError};

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

/// Compiling or reflecting `shader` with slang failed.
///
/// Both code generators classify every slang-reflection failure (the
/// `PrepareShaderErrors` of a shader, or the `SlangError` of the shared
/// modules) into this one error, so neither entry point lists them all. The
/// concrete slang-reflection error is the [`source`](std::error::Error::source);
/// [`downcast_ref`](Self::downcast_ref) reaches it directly.
#[derive(Debug)]
pub struct ShaderReflectionFailed {
    /// the shader file name, or "shared slang modules"
    pub shader: String,
    pub source: Box<dyn SendSyncError>,
}

impl ShaderReflectionFailed {
    /// A `classify` callback:
    /// `prepare(..).classify(ShaderReflectionFailed::of(&file_name))`.
    pub fn of(shader: &str) -> impl FnOnce(InnerError) -> Self + '_ {
        move |inner| Self {
            shader: shader.to_string(),
            source: inner.into_boxed(),
        }
    }

    /// The slang-reflection error, if it has type `E`.
    pub fn downcast_ref<E: std::error::Error + 'static>(&self) -> Option<&E> {
        let source: &(dyn std::error::Error + 'static) = self.source.as_ref();
        source.downcast_ref()
    }
}

impl fmt::Display for ShaderReflectionFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "compile and reflect {}", self.shader)
    }
}

// Not derived: thiserror's `#[source]` would expose the `Box` itself, whose own
// `source()` is the boxed error again, so the chain would print it twice and a
// `source().downcast_ref::<SlangError>()` would see the box, not the error.
impl std::error::Error for ShaderReflectionFailed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
