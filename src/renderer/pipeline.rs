use std::marker::PhantomData;

use ash::vk;

use crate::shaders::atlas::ShaderAtlasEntry;

use super::vertex_description::VertexDescription;
use super::{RawStorageBufferHandle, RawUniformBufferHandle, ShaderPipelineLayout, TextureHandle};

/// A marker trait for different draw call types
pub trait DrawCall {}

/// A marker that the pipeline uses basic cmd_draw draw calls,
/// passing a vertex count with no other vertex data
pub struct DrawVertexCount;
impl DrawCall for DrawVertexCount {}

/// A marker that the pipeline uses cmd_draw_indexed draw calls,
/// using pre-allocated vertex and index buffers
pub struct DrawIndexed;
impl DrawCall for DrawIndexed {}

#[derive(Debug)]
pub struct PipelineHandle<T> {
    index: usize,
    _phantom_data: PhantomData<T>,
}

pub(super) struct PipelineStorage(Vec<Option<RendererPipeline>>);

impl PipelineStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<T: DrawCall>(&mut self, pipeline: RendererPipeline) -> PipelineHandle<T> {
        let handle = PipelineHandle {
            index: self.0.len(),
            _phantom_data: PhantomData,
        };

        self.0.push(Some(pipeline));

        handle
    }

    pub fn get<T>(&self, handle: &PipelineHandle<T>) -> &RendererPipeline {
        self.0[handle.index].as_ref().unwrap()
    }

    #[cfg(debug_assertions)] // used only during hot reload
    pub fn get_mut<T>(&mut self, handle: &PipelineHandle<T>) -> &mut RendererPipeline {
        self.0[handle.index].as_mut().unwrap()
    }

    pub fn take<T>(&mut self, handle: PipelineHandle<T>) -> RendererPipeline {
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
    VertexCount, // this count is now passed in every time
}

pub struct VertexAndIndexBuffersHandle;

pub(super) struct VertexAndIndexBuffers {
    pub(super) vertex_buffer: vk::Buffer,
    pub(super) vertex_buffer_memory: vk::DeviceMemory,

    pub(super) index_buffer: vk::Buffer,
    pub(super) index_buffer_memory: vk::DeviceMemory,

    pub(super) index_count: u32,
}

/// the generic arguments for creating a pipeline
pub struct PipelineConfig<'t, V: VertexDescription, D: DrawCall> {
    pub(super) shader: Box<dyn ShaderAtlasEntry>,
    pub(super) vertex_config: VertexConfig<V>,
    _draw_call: PhantomData<D>,
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
    VertexCount,
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
    // NOTE this inferred generic relies on the correctness of generated code
    pub fn build<D: DrawCall>(self) -> PipelineConfig<'t, V, D> {
        PipelineConfig {
            shader: self.shader,
            vertex_config: self.vertex_config,
            _draw_call: PhantomData,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_buffer_handles: self.storage_buffer_handles,
            disable_depth_test: self.disable_depth_test,
        }
    }
}
