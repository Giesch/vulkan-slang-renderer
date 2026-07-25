use std::path::{Path, PathBuf};

use anyhow::Context;
use image::{DynamicImage, ImageReader};

/// Builds a path rooted at the *calling* crate's directory.
///
/// This must be a macro, not a function: `env!("CARGO_MANIFEST_DIR")` expands
/// where it is written, so a helper fn in this crate would bake in this
/// crate's directory and hand every consumer the wrong root.
///
/// ```ignore
/// let texture = manifest_path!["textures", "viking_room.png"];
/// ```
#[macro_export]
macro_rules! manifest_path {
    ($($segment:expr),* $(,)?) => {{
        let path: ::std::path::PathBuf =
            [env!("CARGO_MANIFEST_DIR"), $($segment),*].into_iter().collect();
        path
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

/// Loads an image from a full path; pair it with [`manifest_path!`] to read
/// assets out of the calling crate.
pub fn load_image(path: impl AsRef<Path>) -> anyhow::Result<DynamicImage> {
    let path = path.as_ref();
    let image = ImageReader::open(path)
        .with_context(|| format!("failed to open image: {path:?}"))?
        .decode()
        .with_context(|| format!("failed to decode image: {path:?}"))?;

    Ok(image)
}
