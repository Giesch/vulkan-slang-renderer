use ash::vk;
use eros::IntoUnion as _;

use super::bindless::{BindlessHandle, Sampler2D};
use super::descriptor_heap::BindlessIndex;
use super::error::{
    InvalidMipLevels, TextureDataErrors, UnsupportedTextureFormat, ZeroTextureExtent,
};
use super::{FormatBlockInfo, format_block_info, level_byte_len, mip_extent};

#[derive(Debug)]
pub struct TextureHandle {
    #[expect(unused)] // for debugging
    #[cfg(debug_assertions)]
    source_file_name: String,
    index: usize,
    /// This texture's slot in the bindless heap, distinct from `index`.
    bindless_slot: BindlessIndex,
}

impl TextureHandle {
    /// This texture as a shader-visible handle, to write into a param struct
    /// declaring a `Sampler2D.Handle` field.
    pub fn bindless_handle(&self) -> BindlessHandle<Sampler2D> {
        super::bindless::from_slot(self.bindless_slot)
    }
}

pub(super) struct TextureStorage(Vec<Option<Texture>>);

impl TextureStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add(&mut self, texture: Texture, bindless_slot: BindlessIndex) -> TextureHandle {
        let handle = TextureHandle {
            #[cfg(debug_assertions)]
            source_file_name: texture.source_file_name.clone(),
            index: self.0.len(),
            bindless_slot,
        };
        self.0.push(Some(texture));

        handle
    }

    pub fn get(&self, handle: &TextureHandle) -> &Texture {
        self.0[handle.index].as_ref().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<Texture> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}

/// Describes whether a texture owns its underlying image and device memory,
/// or is a non-owning alias of an image owned by another resource (e.g. a StorageTexture).
pub(super) enum ImageOwnership {
    /// This texture owns the image and memory, and must free them on destroy.
    Owned(vk_mem::Allocation),
    /// This texture aliases an image owned by another resource. Only the view
    /// and sampler should be destroyed.
    Aliased,
}

pub(super) struct Texture {
    #[cfg_attr(not(debug_assertions), expect(unused))]
    pub(super) source_file_name: String,
    pub(super) image: vk::Image,
    pub(super) image_ownership: ImageOwnership,
    pub(super) image_view: vk::ImageView,
    pub(super) sampler: vk::Sampler,
    #[expect(unused)] // currently unused after init
    pub(super) mip_levels: u32,
    pub(super) image_layout: vk::ImageLayout,
}

/// Rejects a zero width or height. Vulkan forbids zero-extent images, so
/// every user-facing texture constructor calls this before any Vulkan call or
/// allocation. `texture` names the texture and is only called on failure.
pub(super) fn validate_texture_extent(
    texture: impl FnOnce() -> String,
    extent: vk::Extent2D,
) -> Result<(), ZeroTextureExtent> {
    if extent.width == 0 || extent.height == 0 {
        return Err(ZeroTextureExtent {
            texture: texture(),
            width: extent.width,
            height: extent.height,
        });
    }

    Ok(())
}

/// Every check a sampled texture's extent, format and mip data must pass
/// before upload, in order: non-zero extent, at least one level, no more
/// levels than the extent allows, a known format, and an exact byte length
/// per level. Returns the format's block layout.
pub(super) fn validate_texture_levels(
    texture: &str,
    format: vk::Format,
    extent: vk::Extent2D,
    levels: &[&[u8]],
) -> eros::Result<FormatBlockInfo, TextureDataErrors> {
    validate_texture_extent(|| texture.to_owned(), extent).union()?;

    if levels.is_empty() {
        return Err(InvalidMipLevels::Empty).union();
    }
    // the zero-extent check above keeps ilog2 from panicking
    let max_levels = extent.width.max(extent.height).ilog2() + 1;
    if levels.len() > max_levels as usize {
        return Err(InvalidMipLevels::TooMany {
            levels: levels.len(),
            max: max_levels,
            width: extent.width,
            height: extent.height,
        })
        .union();
    }

    let block = format_block_info(format)
        .ok_or(UnsupportedTextureFormat { format })
        .union()?;

    for (i, level) in levels.iter().enumerate() {
        let expected = level_byte_len(block, mip_extent(extent, i));
        let bytes = level.len();
        if bytes != expected {
            return Err(InvalidMipLevels::ByteLength {
                level: i,
                bytes,
                expected,
                format,
            })
            .union();
        }
    }

    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extent(width: u32, height: u32) -> vk::Extent2D {
        vk::Extent2D { width, height }
    }

    #[test]
    fn zero_extent_is_rejected_in_either_dimension() {
        for (width, height) in [(0, 0), (0, 4), (4, 0)] {
            let error =
                validate_texture_extent(|| "t.png".to_owned(), extent(width, height)).unwrap_err();
            assert_eq!(
                error,
                ZeroTextureExtent {
                    texture: "t.png".to_owned(),
                    width,
                    height,
                }
            );
        }
    }

    #[test]
    fn nonzero_extent_is_accepted_without_naming_the_texture() {
        validate_texture_extent(|| unreachable!("only named on failure"), extent(1, 1)).unwrap();
    }

    #[test]
    fn zero_extent_levels_are_rejected_before_mip_math() {
        // the old upload path panicked here in `ilog2`
        let error = validate_texture_levels(
            "empty.ktx2",
            vk::Format::R8G8B8A8_SRGB,
            extent(0, 0),
            &[&[]],
        )
        .unwrap_err();

        assert_eq!(
            error.downcast_inner_ref::<ZeroTextureExtent>(),
            Some(&ZeroTextureExtent {
                texture: "empty.ktx2".to_owned(),
                width: 0,
                height: 0,
            })
        );
    }

    #[test]
    fn zero_width_with_nonzero_height_is_rejected() {
        let error =
            validate_texture_levels("strip", vk::Format::R8G8B8A8_SRGB, extent(0, 8), &[&[]])
                .unwrap_err();

        assert!(error.downcast_inner_ref::<ZeroTextureExtent>().is_some());
    }

    #[test]
    fn valid_levels_pass() {
        let level0 = [0u8; 4 * 4 * 4];
        let level1 = [0u8; 2 * 2 * 4];
        let level2 = [0u8; 4];
        let block = validate_texture_levels(
            "ok",
            vk::Format::R8G8B8A8_UNORM,
            extent(4, 4),
            &[&level0, &level1, &level2],
        )
        .unwrap();

        assert_eq!(block.block_bytes, 4);
    }

    #[test]
    fn too_many_levels_are_rejected() {
        let level = [0u8; 4];
        let error = validate_texture_levels(
            "one_texel",
            vk::Format::R8G8B8A8_UNORM,
            extent(1, 1),
            &[&level, &level],
        )
        .unwrap_err();

        assert!(matches!(
            error.downcast_inner_ref::<InvalidMipLevels>(),
            Some(InvalidMipLevels::TooMany { max: 1, .. })
        ));
    }
}
