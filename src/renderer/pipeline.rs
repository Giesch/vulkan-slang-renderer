use ash::vk;

use crate::shaders::atlas::ShaderAtlasEntry;

use super::vertex_description::VertexDescription;
use super::{RawStorageBufferHandle, RawUniformBufferHandle, ShaderPipelineLayout, TextureHandle};

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

    #[cfg(debug_assertions)] // used only during hot reload
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

    #[cfg_attr(not(debug_assertions), expect(unused))] // used only during hot reload
    pub shader: Box<dyn ShaderAtlasEntry>,

    #[cfg_attr(not(debug_assertions), expect(unused))] // used only during hot reload
    pub disable_depth_test: bool,
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
    pub(super) shader: Box<dyn ShaderAtlasEntry>,
    pub(super) vertex_config: VertexConfig<V>,
    pub(super) texture_handles: Vec<&'t TextureHandle>,
    pub(super) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(super) storage_buffer_handles: Vec<RawStorageBufferHandle>,

    pub disable_depth_test: bool,
}

/// which type of draw call to use, and the necessary data for it
pub enum VertexConfig<V> {
    // use a cmd_draw_indexed call, with prepared vertex and index buffers,
    // and an associated Vertex type
    VertexAndIndexBuffers(Vec<V>, Vec<u32>),
    // use a basic cmd_draw call passing a vertex count, with no vertex or index buffers,
    // and so no Vertex type
    VertexCount(u32),
}

pub struct PipelineConfigBuilder<'t, V: VertexDescription> {
    pub shader: Box<dyn ShaderAtlasEntry>,
    pub vertex_config: VertexConfig<V>,
    pub texture_handles: Vec<&'t TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_buffer_handles: Vec<RawStorageBufferHandle>,

    pub disable_depth_test: bool,
}

impl<'t, V: VertexDescription> PipelineConfigBuilder<'t, V> {
    pub fn build(self) -> PipelineConfig<'t, V> {
        PipelineConfig {
            shader: self.shader,
            vertex_config: self.vertex_config,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_buffer_handles: self.storage_buffer_handles,
            disable_depth_test: self.disable_depth_test,
        }
    }
}
