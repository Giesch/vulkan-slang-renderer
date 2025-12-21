use serde::{Deserialize, Serialize};

use crate::renderer::LayoutDescription;

mod parameters;
pub use parameters::*;

mod pipeline_builders;
pub use pipeline_builders::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionJson {
    pub source_file_name: String,
    pub global_parameters: Vec<GlobalParameter>,
    pub vertex_entry_point: EntryPoint,
    pub fragment_entry_point: EntryPoint,
    pub pipeline_layout: ReflectedPipelineLayout,
}

impl ReflectionJson {
    // TODO replace this with directly inlining the data in generated code?
    pub fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>> {
        self.pipeline_layout
            .descriptor_set_layouts
            .iter()
            .map(|dsl| {
                use ash::vk;

                use crate::renderer::{
                    StorageBufferDescription, TextureDescription, UniformBufferDescription,
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

                        ReflectedBindingType::StorageBuffer => {
                            LayoutDescription::Storage(StorageBufferDescription {
                                size: b.size as u64,
                                binding: b.binding,
                                descriptor_count: 1,
                            })
                        }

                        ReflectedBindingType::CombinedTextureSampler => {
                            LayoutDescription::Texture(TextureDescription {
                                layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                binding: b.binding,
                                descriptor_count: 1,
                            })
                        }

                        b => todo!("unhandled binding type: {b:?}"),
                    })
                    .collect()
            })
            .collect()
    }
}
