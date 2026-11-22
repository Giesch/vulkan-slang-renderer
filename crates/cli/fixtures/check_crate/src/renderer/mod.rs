// Mirror the renderer's private backend bounds on public APIs.
#![allow(private_bounds)]

pub mod addr;
pub mod bindless;
// Exercise production byte decoding without linking the graphics backend.
#[path = "../../../../../renderer/src/renderer/gpu_read.rs"]
pub mod gpu_read;
pub mod gpu_write;
pub mod render_graph;
pub mod vertex_description;

pub use addr::*;
pub use bindless::*;
pub use render_graph::{
    BindingResolver, BufferBinding, GraphBinding, GraphBindingSet, GraphParamBindingSet,
    GraphShaderParams, ImmutableBufferBinding, PendingParamBindings, RawBufferBinding,
    ReadBufferBinding, SampledTexBinding, StorageTexBinding,
};
pub use vertex_description::*;

use std::marker::PhantomData;

pub struct UniformBufferHandle<T>(PhantomData<T>);
pub struct TextureHandle;
pub struct StorageTextureHandle;

pub struct RawUniformBufferHandle;
impl RawUniformBufferHandle {
    pub fn from_typed<T>(_: &UniformBufferHandle<T>) -> Self {
        Self
    }
}

pub struct NoPush;
pub struct PushBlock<P: self::gpu_write::PushConstantBlock>(PhantomData<P>);

/// `I` is the vertex interface: `TypedVertex<V>`, `RuntimeVertex`, or `NoVertex`.
pub struct PipelineConfig<'a, I, D, P = NoPush>(PhantomData<(&'a (), fn() -> I, D, P)>);

pub struct GraphicsPipelineHandle<I, D, P = NoPush>(PhantomData<(fn() -> I, D, P)>);

pub trait DrawCall {}
pub struct DrawIndexed;
impl DrawCall for DrawIndexed {}
pub struct DrawVertexCount;
impl DrawCall for DrawVertexCount {}

pub struct LayoutDescription;

pub struct IndexedPipelineConfig<'a, V, P = NoPush>(PhantomData<(&'a (), V, P)>);

pub struct PipelineConfigBuilder<'a> {
    pub shader: Box<dyn crate::shaders::atlas::ShaderAtlasEntry>,
    pub texture_handles: Vec<&'a TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'a StorageTextureHandle>,
}

pub struct ComputePipelineConfig<'a, P = NoPush>(PhantomData<(&'a (), P)>);

pub struct ComputePipelineConfigBuilder<'a> {
    pub shader: Box<dyn crate::shaders::atlas::ComputeShaderAtlasEntry>,
    pub texture_handles: Vec<&'a TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'a StorageTextureHandle>,
}

impl<'a> PipelineConfigBuilder<'a> {
    pub fn build_indexed<V: VertexDescription, P>(self) -> IndexedPipelineConfig<'a, V, P> {
        IndexedPipelineConfig(PhantomData)
    }

    pub fn build_indexed_bytes<P>(
        self,
        _vertex_bytes: Vec<u8>,
        _indices: Vec<u32>,
    ) -> PipelineConfig<'a, RuntimeVertex, DrawIndexed, P> {
        PipelineConfig(PhantomData)
    }

    pub fn build_vertex_count<P>(self) -> PipelineConfig<'a, NoVertex, DrawVertexCount, P> {
        PipelineConfig(PhantomData)
    }
}

impl<'a, V: VertexDescription, P> IndexedPipelineConfig<'a, V, P> {
    pub fn with_vertices(
        self,
        _vertices: Vec<V>,
        _indices: Vec<u32>,
    ) -> PipelineConfig<'a, TypedVertex<V>, DrawIndexed, P> {
        PipelineConfig(PhantomData)
    }
}

impl<'a> ComputePipelineConfigBuilder<'a> {
    pub fn build<P>(self) -> ComputePipelineConfig<'a, P> {
        ComputePipelineConfig(PhantomData)
    }
}
