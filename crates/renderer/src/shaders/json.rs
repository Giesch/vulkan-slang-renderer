pub use mltrs_slang_reflection::json::*;

use crate::renderer::LayoutDescription;

pub use crate::renderer::ReflectionLayoutBindings;

pub fn layout_bindings_from_pipeline_layout(
    pipeline_layout: &ReflectedPipelineLayout,
) -> Vec<Vec<LayoutDescription>> {
    pipeline_layout
        .descriptor_set_layouts
        .iter()
        .map(|dsl| {
            use ash::vk;

            use crate::renderer::{
                StorageImageDescription, TextureDescription, UniformBufferDescription,
            };
            use crate::shaders::json::ReflectedBindingType;

            // NOTE this depends on the order from 'pipeline_config'
            // exactly matching the order of layout descriptions
            dsl.binding_ranges
                .iter()
                .map(|b| match b.descriptor_type {
                    ReflectedBindingType::ConstantBuffer => {
                        LayoutDescription::Uniform(UniformBufferDescription {
                            size: b.size as u64,
                            binding: b.binding,
                            descriptor_count: 1,
                        })
                    }

                    ReflectedBindingType::CombinedTextureSampler => {
                        LayoutDescription::Texture(TextureDescription {
                            binding: b.binding,
                            descriptor_count: 1,
                            sampled_image_only: false,
                        })
                    }

                    ReflectedBindingType::StorageImage => {
                        LayoutDescription::StorageImage(StorageImageDescription {
                            layout: vk::ImageLayout::GENERAL,
                            binding: b.binding,
                            descriptor_count: 1,
                        })
                    }

                    ReflectedBindingType::Texture => {
                        LayoutDescription::Texture(TextureDescription {
                            binding: b.binding,
                            descriptor_count: 1,
                            sampled_image_only: true,
                        })
                    }

                    b => todo!("unhandled binding type: {b:?}"),
                })
                .collect()
        })
        .collect()
}
