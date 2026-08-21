pub mod addr;
pub mod bindless;
pub mod gpu_write;
pub mod vertex_description;

pub use addr::*;
pub use bindless::*;
pub use gpu_write::*;
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
pub struct PushBlock<P: PushConstantBlock>(PhantomData<P>);

pub struct PipelineConfig<'a, V, D, P = NoPush>(PhantomData<(&'a (), V, D, P)>);

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
    pub fn build_indexed<V, P>(self) -> IndexedPipelineConfig<'a, V, P> {
        IndexedPipelineConfig(PhantomData)
    }

    pub fn build_vertex_count<P>(self) -> PipelineConfig<'a, NoVertex, DrawVertexCount, P> {
        PipelineConfig(PhantomData)
    }
}

impl<'a> ComputePipelineConfigBuilder<'a> {
    pub fn build<P>(self) -> ComputePipelineConfig<'a, P> {
        ComputePipelineConfig(PhantomData)
    }
}
