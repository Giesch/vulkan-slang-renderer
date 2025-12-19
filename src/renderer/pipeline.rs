use ash::vk;

use crate::shaders::atlas::ShaderAtlasEntry;

use super::ShaderPipelineLayout;
use super::vertex_description::VertexDescription;
use super::{RawUniformBufferHandle, TextureHandle};

#[derive(Debug)]
pub struct PipelineHandle {
    index: usize,
}

pub(super) struct PipelineStorage(Vec<Option<RendererPipeline>>);

impl PipelineStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add(&mut self, pipeline: RendererPipeline) -> PipelineHandle {
        let index = self.0.len();
        let handle = PipelineHandle { index };
        self.0.push(Some(pipeline));

        handle
    }

    pub fn get(&self, handle: &PipelineHandle) -> &RendererPipeline {
        self.0[handle.index].as_ref().unwrap()
    }

    // used only for hot reload
    #[cfg(debug_assertions)]
    pub fn get_mut(&mut self, handle: &PipelineHandle) -> &mut RendererPipeline {
        self.0[handle.index].as_mut().unwrap()
    }

    pub fn take(&mut self, handle: PipelineHandle) -> RendererPipeline {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<RendererPipeline> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}

pub(super) struct RendererPipeline {
    pub layout: ShaderPipelineLayout,
    pub pipeline: vk::Pipeline,

    pub vertex_pipeline_config: VertexPipelineConfig,

    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,

    #[cfg_attr(not(debug_assertions), expect(unused))]
    pub shader: Box<dyn ShaderAtlasEntry>,
}

pub(super) enum VertexPipelineConfig {
    VertexAndIndexBuffers(VertexAndIndexBuffers),
    VertexCount(u32),
}

impl RendererPipeline {
    pub(super) fn draw(&self, device: &ash::Device, command_buffer: vk::CommandBuffer) {
        match &self.vertex_pipeline_config {
            VertexPipelineConfig::VertexAndIndexBuffers(vi_bufs) => unsafe {
                device.cmd_draw_indexed(command_buffer, vi_bufs.index_count, 1, 0, 0, 0);
            },

            VertexPipelineConfig::VertexCount(vertex_count) => unsafe {
                device.cmd_draw(command_buffer, *vertex_count, 1, 0, 0);
            },
        }
    }
}

pub(super) struct VertexAndIndexBuffers {
    pub vertex_buffer: vk::Buffer,
    pub vertex_buffer_memory: vk::DeviceMemory,

    pub index_buffer: vk::Buffer,
    pub index_buffer_memory: vk::DeviceMemory,

    pub index_count: u32,
}

/// the generic arguments for creating a pipeline
pub struct PipelineConfig<'t, V: VertexDescription> {
    pub shader: Box<dyn ShaderAtlasEntry>,
    pub vertex_config: VertexConfig<V>,
    pub texture_handles: Vec<&'t TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
}

pub enum VertexConfig<V> {
    VertexAndIndexBuffers(Vec<V>, Vec<u32>),
    VertexCount(u32),
}
