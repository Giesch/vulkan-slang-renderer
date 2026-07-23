use std::marker::PhantomData;

use ash::vk;

use crate::shaders::atlas::{ComputeShaderAtlasEntry, ShaderAtlasEntry};

use super::vertex_description::VertexDescription;
use super::{
    ComputeShaderPipelineLayout, RawUniformBufferHandle, ShaderPipelineLayout,
    StorageTextureHandle, TextureHandle,
};

/// A newtype-wrapped index into one of the renderer's pipeline/mesh storages.
/// Distinct types per storage make cross-storage index mixups a compile error.
/// The indexes are opaque to callers; only the renderer mints and resolves them.
pub trait PipelineIndex: Copy {
    #[doc(hidden)]
    fn from_raw(index: usize) -> Self;
    #[doc(hidden)]
    fn raw(self) -> usize;
}

/// Index into `PipelineStorage` (graphics pipelines, shared by DrawIndexed,
/// DrawVertexCount, and picking pipelines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphicsPipelineIndex(usize);

/// Index into `ComputePipelineStorage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComputePipelineIndex(usize);

/// Index into `Renderer::meshes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshIndex(usize);

impl PipelineIndex for GraphicsPipelineIndex {
    fn from_raw(index: usize) -> Self {
        Self(index)
    }
    fn raw(self) -> usize {
        self.0
    }
}

impl PipelineIndex for ComputePipelineIndex {
    fn from_raw(index: usize) -> Self {
        Self(index)
    }
    fn raw(self) -> usize {
        self.0
    }
}

impl MeshIndex {
    pub(super) fn from_raw(index: usize) -> Self {
        Self(index)
    }
    pub(super) fn raw(self) -> usize {
        self.0
    }
}

/// A marker trait for different draw call types
pub trait DrawCall {
    /// The typed index into the storage this draw-call kind lives in.
    type Index: PipelineIndex;
}

/// A marker that the pipeline uses basic cmd_draw draw calls,
/// passing a vertex count with no other vertex data
#[derive(Debug)]
pub struct DrawVertexCount;
impl DrawCall for DrawVertexCount {
    type Index = GraphicsPipelineIndex;
}

/// A marker that the pipeline uses cmd_draw_indexed draw calls,
/// using pre-allocated vertex and index buffers
#[derive(Debug)]
pub struct DrawIndexed;
impl DrawCall for DrawIndexed {
    type Index = GraphicsPipelineIndex;
}

/// A marker for compute pipelines
#[derive(Debug)]
pub struct Compute;
impl DrawCall for Compute {
    type Index = ComputePipelineIndex;
}

#[derive(Debug)]
pub struct PipelineHandle<T> {
    index: usize,
    _phantom_data: PhantomData<T>,
}

impl<T: DrawCall> PipelineHandle<T> {
    pub(crate) fn index(&self) -> T::Index {
        T::Index::from_raw(self.index)
    }
}

/// Distinct from PipelineHandle<T> — compile-time prevents misuse with main draw calls
#[derive(Debug)]
pub struct PickingPipelineHandle {
    pub(super) index: GraphicsPipelineIndex,
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
            index: GraphicsPipelineIndex::from_raw(self.0.len()),
        };

        self.0.push(Some(pipeline));

        handle
    }

    pub fn get<T>(&self, handle: &PipelineHandle<T>) -> &RendererPipeline {
        self.0[handle.index].as_ref().unwrap()
    }

    pub fn get_picking(&self, handle: &PickingPipelineHandle) -> &RendererPipeline {
        self.0[handle.index.raw()].as_ref().unwrap()
    }

    pub fn get_by_index(&self, index: GraphicsPipelineIndex) -> &RendererPipeline {
        self.0[index.raw()].as_ref().unwrap()
    }

    #[cfg(debug_assertions)] // used only during hot reload
    pub fn get_mut_by_index(&mut self, index: GraphicsPipelineIndex) -> &mut RendererPipeline {
        self.0[index.raw()].as_mut().unwrap()
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

    pub shader: Box<dyn ShaderAtlasEntry>,

    #[cfg_attr(not(debug_assertions), expect(unused))] // used only during hot reload
    pub disable_depth_test: bool,
}

pub(super) enum VertexPipelineConfig {
    VertexAndIndexBuffers(VertexAndIndexBuffers),
    /// index into Renderer::meshes; the buffers outlive this pipeline
    SharedMesh(MeshIndex),
    VertexCount, // this count is now passed in every time
}

/// A handle to a mesh created with Renderer::create_mesh, whose vertex and
/// index buffers can be shared by multiple pipelines via
/// PipelineConfig::with_shared_mesh. The vertex type parameter ties the mesh
/// to pipelines with a matching vertex layout at compile time.
#[derive(Debug)]
pub struct MeshHandle<V: VertexDescription> {
    pub(super) index: MeshIndex,
    pub(super) _phantom_data: PhantomData<V>,
}

pub(super) struct VertexAndIndexBuffers {
    pub(super) vertex_buffer: vk::Buffer,
    pub(super) vertex_buffer_memory: vk_mem::Allocation,

    pub(super) index_buffer: vk::Buffer,
    pub(super) index_buffer_memory: vk_mem::Allocation,

    pub(super) index_count: u32,
}

/// the generic arguments for creating a pipeline
pub struct PipelineConfig<'t, V: VertexDescription, D: DrawCall> {
    pub(super) shader: Box<dyn ShaderAtlasEntry>,
    pub(super) vertex_config: VertexConfig<V>,
    _draw_call: PhantomData<D>,
    pub(super) texture_handles: Vec<&'t TextureHandle>,
    pub(super) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(super) storage_texture_handles: Vec<&'t StorageTextureHandle>,

    pub disable_depth_test: bool,
}

/// which type of draw call to use, and the necessary data for it
pub enum VertexConfig<V> {
    // use a cmd_draw_indexed call, with prepared vertex and index buffers,
    // and an associated Vertex type
    VertexAndIndexBuffers(Vec<V>, Vec<u32>),
    // use cmd_draw_indexed calls against a shared mesh created with
    // Renderer::create_mesh (the index is into Renderer::meshes)
    SharedMesh(MeshIndex),
    // use a basic cmd_draw call passing a vertex count, with no vertex or index buffers,
    // and so no Vertex type
    VertexCount,
}

impl<'t, V: VertexDescription> PipelineConfig<'t, V, DrawIndexed> {
    /// Draw from a shared mesh instead of per-pipeline vertex/index buffers.
    /// Replaces any vertex/index data already in the config (the generated
    /// `pipeline_config(resources)` can be given empty vecs).
    pub fn with_shared_mesh(mut self, mesh: &MeshHandle<V>) -> Self {
        self.vertex_config = VertexConfig::SharedMesh(mesh.index);
        self
    }
}

pub struct PipelineConfigBuilder<'t, V: VertexDescription> {
    pub shader: Box<dyn ShaderAtlasEntry>,
    pub vertex_config: VertexConfig<V>,
    pub texture_handles: Vec<&'t TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'t StorageTextureHandle>,

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
            storage_texture_handles: self.storage_texture_handles,
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

    pub fn get_by_index(&self, index: ComputePipelineIndex) -> &ComputeRendererPipeline {
        self.0[index.raw()].as_ref().unwrap()
    }

    #[cfg(debug_assertions)]
    pub fn get_mut_by_index(
        &mut self,
        index: ComputePipelineIndex,
    ) -> &mut ComputeRendererPipeline {
        self.0[index.raw()].as_mut().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<ComputeRendererPipeline> {
        self.0.iter_mut().filter_map(|o| o.take()).collect()
    }
}

pub struct ComputePipelineConfig<'t> {
    pub(crate) shader: Box<dyn ComputeShaderAtlasEntry>,
    pub(crate) texture_handles: Vec<&'t TextureHandle>,
    pub(crate) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(crate) storage_texture_handles: Vec<&'t StorageTextureHandle>,
}
