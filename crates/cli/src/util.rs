use std::path::PathBuf;

/// Joins path segments without anchoring them to any root.
pub fn relative_path<'a>(segments: impl IntoIterator<Item = &'a str>) -> PathBuf {
    segments.into_iter().collect()
}

/// Builds a path rooted at this crate's directory.
///
/// Only correct for paths that genuinely belong to `mltrs-cli` itself, which
/// today means test fixtures. Consumers locate their own files via the paths
/// passed to [`crate::build_tasks::Config`].
#[cfg(test)]
pub fn manifest_path<'a>(segments: impl IntoIterator<Item = &'a str>) -> PathBuf {
    let segments = segments.into_iter();
    let full_path = [env!("CARGO_MANIFEST_DIR")].into_iter().chain(segments);
    full_path.collect()
}
