use std::path::{Path, PathBuf};

use anyhow::Context;
use image::{DynamicImage, ImageReader};

/// Builds a path from the *calling* crate's manifest dir. A macro because
/// `env!("CARGO_MANIFEST_DIR")` in a library fn would bake in the library's
/// own dir; expanding at the call site yields the consumer's crate dir.
#[macro_export]
macro_rules! manifest_path {
    ($($seg:expr),* $(,)?) => {{
        let p: ::std::path::PathBuf =
            [env!("CARGO_MANIFEST_DIR"), $($seg),*].into_iter().collect();
        p
    }};
}

pub fn manifest_path<'a>(segments: impl IntoIterator<Item = &'a str>) -> PathBuf {
    let segments = segments.into_iter();
    let full_path = [env!("CARGO_MANIFEST_DIR")].into_iter().chain(segments);
    full_path.collect()
}

pub fn relative_path<'a>(segments: impl IntoIterator<Item = &'a str>) -> PathBuf {
    segments.into_iter().collect()
}

pub fn load_image(path: impl AsRef<Path>) -> anyhow::Result<DynamicImage> {
    let file_path = path.as_ref();
    let image = ImageReader::open(file_path)
        .with_context(|| format!("failed to open image: {file_path:?}"))?
        .decode()
        .with_context(|| format!("failed to decode image: {file_path:?}"))?;

    Ok(image)
}
