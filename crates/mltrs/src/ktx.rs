//! Loading of KTX2 texture files (<https://registry.khronos.org/KTX/specs/2.0/ktxspec.v2.html>)
//!
//! Only 2D, non-array, non-cubemap textures with pre-baked mip levels
//! and no supercompression are supported.

use std::path::Path;

use anyhow::Context;
use ash::vk;

use crate::renderer::format_block_info;

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

pub fn load_ktx2(file_path: &Path) -> anyhow::Result<KtxTexture> {
    let bytes = std::fs::read(file_path)
        .with_context(|| format!("failed to read ktx2 file: {file_path:?}"))?;

    let reader = ktx2::Reader::new(&bytes)
        .map_err(|e| anyhow::anyhow!("failed to parse ktx2 file {file_path:?}: {e}"))?;

    let header = reader.header();

    // NOTE zstd supercompression could be supported here by decompressing
    // each level to its uncompressed_byte_length
    if let Some(scheme) = header.supercompression_scheme {
        anyhow::bail!("unsupported ktx2 supercompression: {scheme:?} in {file_path:?}");
    }

    // None means VK_FORMAT_UNDEFINED, ie Basis Universal data needing transcode
    let Some(ktx_format) = header.format else {
        anyhow::bail!("ktx2 file has no vulkan format (needs transcoding): {file_path:?}");
    };

    anyhow::ensure!(
        header.pixel_height > 0 && header.pixel_depth == 0,
        "expected a 2d texture: {file_path:?}"
    );
    anyhow::ensure!(
        header.layer_count == 0,
        "array textures are unsupported: {file_path:?}"
    );
    anyhow::ensure!(
        header.face_count == 1,
        "cubemap textures are unsupported: {file_path:?}"
    );
    // 0 legally means 'generate the mip chain at runtime';
    // that case is covered by Renderer::create_texture
    anyhow::ensure!(
        header.level_count >= 1,
        "expected pre-baked mip levels: {file_path:?}"
    );

    // KTX2 vkFormat values are Vulkan's own enum values by spec
    let format = vk::Format::from_raw(ktx_format.value() as i32);
    let block = format_block_info(format)
        .with_context(|| format!("unsupported texture format {format:?} in {file_path:?}"))?;

    let extent = vk::Extent2D {
        width: header.pixel_width,
        height: header.pixel_height,
    };

    let mut mip_data = Vec::with_capacity(header.level_count as usize);
    for (i, level) in reader.levels().enumerate() {
        let mip_width = (extent.width >> i).max(1);
        let mip_height = (extent.height >> i).max(1);
        let expected_size = mip_width.div_ceil(block.block_width) as usize
            * mip_height.div_ceil(block.block_height) as usize
            * block.block_bytes as usize;
        anyhow::ensure!(
            level.data.len() == expected_size,
            "mip level {i} has {} bytes, expected {expected_size}: {file_path:?}",
            level.data.len(),
        );

        mip_data.push(level.data.to_vec());
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
