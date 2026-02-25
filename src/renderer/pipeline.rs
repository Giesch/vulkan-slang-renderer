use std::marker::PhantomData;

use ash::vk;

use crate::shaders::atlas::{ComputeShaderAtlasEntry, ShaderAtlasEntry};

use super::vertex_description::VertexDescription;
use super::{
    ComputeShaderPipelineLayout, RawStorageBufferHandle, RawUniformBufferHandle,
    ShaderPipelineLayout, TextureHandle,
};

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

/// A marker for compute pipelines
pub struct Compute;
impl DrawCall for Compute {}

#[derive(Debug)]
pub struct PipelineHandle<T> {
    index: usize,
    _phantom_data: PhantomData<T>,
}

impl<T> PipelineHandle<T> {
    pub(crate) fn index(&self) -> usize {
        self.index
    }
}

/// Distinct from PipelineHandle<T> — compile-time prevents misuse with main draw calls
#[derive(Debug)]
pub struct PickingPipelineHandle {
    pub(super) index: usize,
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

    pub fn add_picking(&mut self, pipeline: RendererPipeline) -> PickingPipelineHandle {
        let handle = PickingPipelineHandle {
            index: self.0.len(),
        };

        self.0.push(Some(pipeline));

        handle
    }

    pub fn get<T>(&self, handle: &PipelineHandle<T>) -> &RendererPipeline {
        self.0[handle.index].as_ref().unwrap()
    }

    pub fn get_picking(&self, handle: &PickingPipelineHandle) -> &RendererPipeline {
        self.0[handle.index].as_ref().unwrap()
    }

    #[cfg(debug_assertions)] // used only during hot reload
    pub fn get_mut<T>(&mut self, handle: &PipelineHandle<T>) -> &mut RendererPipeline {
        self.0[handle.index].as_mut().unwrap()
    }

    #[expect(unused)]
    pub fn take<T>(&mut self, handle: PipelineHandle<T>) -> RendererPipeline {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<RendererPipeline> {
        self.0.iter_mut().filter_map(|o| o.take()).collect()
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

#[derive(Clone, Copy, Debug, Default)]
pub enum StorageBufferFrameStrategy {
    #[default]
    Standard,
    PingPong,
}

/// the generic arguments for creating a pipeline
pub struct PipelineConfig<'t, V: VertexDescription, D: DrawCall> {
    pub(super) shader: Box<dyn ShaderAtlasEntry>,
    pub(super) vertex_config: VertexConfig<V>,
    _draw_call: PhantomData<D>,
    pub(super) texture_handles: Vec<&'t TextureHandle>,
    pub(super) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(super) storage_buffer_handles: Vec<RawStorageBufferHandle>,
    pub(super) storage_buffer_frame_strategy: StorageBufferFrameStrategy,

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
    pub storage_buffer_frame_strategy: StorageBufferFrameStrategy,

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
            storage_buffer_frame_strategy: self.storage_buffer_frame_strategy,
            disable_depth_test: self.disable_depth_test,
        }
    }
}

// --- Compute pipeline types ---

pub(super) struct ComputeRendererPipeline {
    pub layout: ComputeShaderPipelineLayout,
    pub pipeline: vk::Pipeline,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,
    #[cfg_attr(not(debug_assertions), expect(unused))]
    pub shader: Box<dyn ComputeShaderAtlasEntry>,
}

pub(super) struct ComputePipelineStorage(Vec<Option<ComputeRendererPipeline>>);

impl ComputePipelineStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add(&mut self, pipeline: ComputeRendererPipeline) -> PipelineHandle<Compute> {
        let handle = PipelineHandle {
            index: self.0.len(),
            _phantom_data: PhantomData,
        };

        self.0.push(Some(pipeline));

        handle
    }

    pub fn get(&self, handle: &PipelineHandle<Compute>) -> &ComputeRendererPipeline {
        self.0[handle.index].as_ref().unwrap()
    }

    #[cfg(debug_assertions)]
    #[expect(unused)]
    pub fn get_mut(&mut self, handle: &PipelineHandle<Compute>) -> &mut ComputeRendererPipeline {
        self.0[handle.index].as_mut().unwrap()
    }

    pub fn get_by_index(&self, index: usize) -> &ComputeRendererPipeline {
        self.0[index].as_ref().unwrap()
    }

    #[cfg(debug_assertions)]
    pub fn get_mut_by_index(&mut self, index: usize) -> &mut ComputeRendererPipeline {
        self.0[index].as_mut().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<ComputeRendererPipeline> {
        self.0.iter_mut().filter_map(|o| o.take()).collect()
    }
}

pub struct ComputePipelineConfig<'t> {
    pub(crate) shader: Box<dyn ComputeShaderAtlasEntry>,
    pub(crate) texture_handles: Vec<&'t TextureHandle>,
    pub(crate) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(crate) storage_buffer_handles: Vec<RawStorageBufferHandle>,
    pub storage_buffer_frame_strategy: StorageBufferFrameStrategy,
}
