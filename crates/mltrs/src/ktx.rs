//! Loading of KTX2 texture files (<https://registry.khronos.org/KTX/specs/2.0/ktxspec.v2.html>)
//!
//! Only 2D, non-array, non-cubemap textures with pre-baked mip levels are
//! supported. Of the supercompression schemes, only zstd is; BasisLZ needs
//! transcoding rather than decompression.

use std::io;
use std::path::{Path, PathBuf};

use ash::vk;
use eros::{IntoUnion, ReshapeUnion};

use crate::renderer::{
    Renderer, SamplerOptions, TextureFilter, TextureHandle, format_block_info, level_byte_len,
    mip_extent,
};

/// The KTX2 file could not be read from disk.
#[derive(Debug, thiserror::Error)]
#[error("failed to read ktx2 file: {path:?}")]
pub struct Ktx2ReadFailed {
    pub path: PathBuf,
    #[source]
    pub source: io::Error,
}

/// The KTX2 header or level index is malformed.
#[derive(Debug, thiserror::Error)]
#[error("failed to parse ktx2 file {path:?}: {source}")]
pub struct InvalidKtx2Header {
    pub path: PathBuf,
    pub source: ktx2::ParseError,
}

/// The file uses a supercompression scheme other than zstd (eg. BasisLZ).
#[derive(Debug, thiserror::Error)]
#[error("unsupported ktx2 supercompression: {scheme:?} in {path:?}")]
pub struct UnsupportedKtx2Supercompression {
    pub path: PathBuf,
    pub scheme: ktx2::SupercompressionScheme,
}

/// The file has `VK_FORMAT_UNDEFINED`, ie. Basis Universal data that needs
/// transcoding rather than decompression.
#[derive(Debug, thiserror::Error)]
#[error("ktx2 file has no vulkan format (needs transcoding): {path:?}")]
pub struct Ktx2NeedsTranscoding {
    pub path: PathBuf,
}

/// Which texture shape a KTX2 file has that the loader does not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ktx2Shape {
    /// Not a 2D texture (1D: `pixel_height == 0`, or 3D: `pixel_depth > 0`).
    Not2d {
        pixel_height: u32,
        pixel_depth: u32,
    },
    Array {
        layer_count: u32,
    },
    Cubemap {
        face_count: u32,
    },
    /// `level_count == 0` legally means "generate the mip chain at runtime",
    /// which the renderer never does.
    NoPrebakedMips,
}

impl std::fmt::Display for Ktx2Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Not2d { .. } => f.write_str("expected a 2d texture"),
            Self::Array { .. } => f.write_str("array textures are unsupported"),
            Self::Cubemap { .. } => f.write_str("cubemap textures are unsupported"),
            Self::NoPrebakedMips => f.write_str("expected pre-baked mip levels"),
        }
    }
}

/// The file describes a texture shape the loader does not support.
#[derive(Debug, thiserror::Error)]
#[error("{shape}: {path:?}")]
pub struct UnsupportedKtx2Shape {
    pub path: PathBuf,
    pub shape: Ktx2Shape,
}

/// The file's Vulkan format has no known block layout in the renderer.
#[derive(Debug, thiserror::Error)]
#[error("unsupported texture format {format:?} in {path:?}")]
pub struct UnsupportedKtx2Format {
    pub path: PathBuf,
    pub format: vk::Format,
}

/// A zstd-supercompressed mip level failed to decompress (corrupt data, or
/// larger than the size implied by the image extent).
#[derive(Debug, thiserror::Error)]
#[error("failed to decompress zstd mip level {level}: {path:?}")]
pub struct Ktx2ZstdFailed {
    pub path: PathBuf,
    pub level: usize,
    #[source]
    pub source: io::Error,
}

/// A mip level's (decompressed) byte length does not match its extent and format.
#[derive(Debug, thiserror::Error)]
#[error("mip level {level} has {actual} bytes, expected {expected}: {path:?}")]
pub struct Ktx2MipLevelSizeMismatch {
    pub path: PathBuf,
    pub level: usize,
    pub actual: usize,
    pub expected: usize,
}

/// Everything [`load_ktx2`] can fail with.
pub type Ktx2Errors = (
    Ktx2ReadFailed,
    InvalidKtx2Header,
    UnsupportedKtx2Supercompression,
    Ktx2NeedsTranscoding,
    UnsupportedKtx2Shape,
    UnsupportedKtx2Format,
    Ktx2ZstdFailed,
    Ktx2MipLevelSizeMismatch,
);

/// Everything [`load_ktx2_texture`] can fail with: [`Ktx2Errors`] plus the
/// renderer's [`TextureErrors`](crate::renderer::TextureErrors).
pub type Ktx2TextureErrors = (
    Ktx2ReadFailed,
    InvalidKtx2Header,
    UnsupportedKtx2Supercompression,
    Ktx2NeedsTranscoding,
    UnsupportedKtx2Shape,
    UnsupportedKtx2Format,
    Ktx2ZstdFailed,
    Ktx2MipLevelSizeMismatch,
    crate::renderer::ZeroTextureExtent,
    crate::renderer::InvalidMipLevels,
    crate::renderer::UnsupportedTextureFormat,
    crate::renderer::BindlessHeapFull,
    vk::Result,
);

/// A decoded KTX2 texture, ready for upload via
/// [`Renderer::create_texture_with_mips`](crate::renderer::Renderer::create_texture_with_mips).
pub struct KtxTexture {
    pub source_file_name: String,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    /// One entry per mip level, level 0 (largest) first.
    pub mip_data: Vec<Vec<u8>>,
}

impl KtxTexture {
    pub fn mip_slices(&self) -> Vec<&[u8]> {
        self.mip_data.iter().map(|level| level.as_slice()).collect()
    }
}

/// Read a KTX2 file and upload its pre-baked mip levels as a texture.
pub fn load_ktx2_texture(
    renderer: &mut Renderer,
    file_path: &Path,
    filter: TextureFilter,
) -> eros::Result<TextureHandle, Ktx2TextureErrors> {
    let ktx = load_ktx2(file_path).widen()?;

    renderer
        .create_texture_with_mips(
            &ktx.source_file_name,
            ktx.format,
            ktx.extent,
            &ktx.mip_slices(),
            SamplerOptions {
                filter,
                ..Default::default()
            },
        )
        .widen()
}

pub fn load_ktx2(file_path: &Path) -> eros::Result<KtxTexture, Ktx2Errors> {
    let path = || file_path.to_path_buf();

    let bytes = std::fs::read(file_path)
        .map_err(|source| Ktx2ReadFailed {
            path: path(),
            source,
        })
        .union()?;

    let reader = ktx2::Reader::new(&bytes)
        .map_err(|source| InvalidKtx2Header {
            path: path(),
            source,
        })
        .union()?;

    let header = reader.header();

    let zstd_compressed = match header.supercompression_scheme {
        None => false,
        Some(ktx2::SupercompressionScheme::Zstandard) => true,
        Some(scheme) => {
            return Err(UnsupportedKtx2Supercompression {
                path: path(),
                scheme,
            })
            .union();
        }
    };

    // None means VK_FORMAT_UNDEFINED, ie Basis Universal data needing transcode
    let Some(ktx_format) = header.format else {
        return Err(Ktx2NeedsTranscoding { path: path() }).union();
    };

    let unsupported_shape = if header.pixel_height == 0 || header.pixel_depth != 0 {
        Some(Ktx2Shape::Not2d {
            pixel_height: header.pixel_height,
            pixel_depth: header.pixel_depth,
        })
    } else if header.layer_count != 0 {
        Some(Ktx2Shape::Array {
            layer_count: header.layer_count,
        })
    } else if header.face_count != 1 {
        Some(Ktx2Shape::Cubemap {
            face_count: header.face_count,
        })
    } else if header.level_count == 0 {
        // 0 legally means 'generate the mip chain at runtime', which the
        // renderer never does — every level it uploads has to be in the file
        Some(Ktx2Shape::NoPrebakedMips)
    } else {
        None
    };
    if let Some(shape) = unsupported_shape {
        return Err(UnsupportedKtx2Shape {
            path: path(),
            shape,
        })
        .union();
    }

    // KTX2 vkFormat values are Vulkan's own enum values by spec
    let format = vk::Format::from_raw(ktx_format.value() as i32);
    let Some(block) = format_block_info(format) else {
        return Err(UnsupportedKtx2Format {
            path: path(),
            format,
        })
        .union();
    };

    let extent = vk::Extent2D {
        width: header.pixel_width,
        height: header.pixel_height,
    };

    let mut mip_data = Vec::with_capacity(header.level_count as usize);
    for (i, level) in reader.levels().enumerate() {
        let expected_size = level_byte_len(block, mip_extent(extent, i));

        // expected_size comes from the image dimensions, so using it as the
        // output bound keeps a bogus level.uncompressed_byte_length from
        // driving a huge allocation; the check below then sees the
        // decompressed size, which is the invariant worth checking
        let data = if zstd_compressed {
            zstd::bulk::decompress(level.data, expected_size)
                .map_err(|source| Ktx2ZstdFailed {
                    path: path(),
                    level: i,
                    source,
                })
                .union()?
        } else {
            level.data.to_vec()
        };

        if data.len() != expected_size {
            return Err(Ktx2MipLevelSizeMismatch {
                path: path(),
                level: i,
                actual: data.len(),
                expected: expected_size,
            })
            .union();
        }

        mip_data.push(data);
    }
    debug_assert!(mip_data.len() == header.level_count as usize);

    let source_file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    Ok(KtxTexture {
        source_file_name,
        format,
        extent,
        mip_data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_a_read_error_with_its_path() {
        let path = Path::new("definitely/not/here.ktx2");
        let err = load_ktx2(path).err().expect("missing file must fail");
        let read = err
            .narrow::<Ktx2ReadFailed, _>()
            .expect("expected Ktx2ReadFailed");
        assert_eq!(read.path, path);
        assert_eq!(read.source.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn bad_magic_is_an_invalid_header() -> eros::Result<()> {
        let dir = std::env::temp_dir().join(format!("mltrs-ktx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("bad_magic.ktx2");
        std::fs::write(&path, [0u8; 128])?;

        let err = load_ktx2(&path).err().expect("bad magic must fail");
        std::fs::remove_dir_all(&dir)?;
        let header = err
            .narrow::<InvalidKtx2Header, _>()
            .expect("expected InvalidKtx2Header");
        assert!(matches!(header.source, ktx2::ParseError::BadMagic));
        Ok(())
    }
}
