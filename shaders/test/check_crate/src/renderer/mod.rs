pub mod addr;
pub mod gpu_write;
pub mod vertex_description;

pub use addr::*;
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

pub struct PipelineConfig<'a, V, D>(PhantomData<(&'a (), V, D)>);

pub trait DrawCall {}
pub struct DrawIndexed;
impl DrawCall for DrawIndexed {}
pub struct DrawVertexCount;
impl DrawCall for DrawVertexCount {}

pub struct LayoutDescription;

pub struct IndexedPipelineConfig<'a, V>(PhantomData<(&'a (), V)>);

pub struct PipelineConfigBuilder<'a> {
    pub shader: Box<dyn crate::shaders::atlas::ShaderAtlasEntry>,
    pub texture_handles: Vec<&'a TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'a StorageTextureHandle>,
    pub disable_depth_test: bool,
}

pub struct ComputePipelineConfig<'a> {
    pub shader: Box<dyn crate::shaders::atlas::ComputeShaderAtlasEntry>,
    pub texture_handles: Vec<&'a TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'a StorageTextureHandle>,
}

impl<'a> PipelineConfigBuilder<'a> {
    pub fn build_indexed<V>(self) -> IndexedPipelineConfig<'a, V> {
        IndexedPipelineConfig(PhantomData)
    }

    pub fn build_vertex_count(self) -> PipelineConfig<'a, NoVertex, DrawVertexCount> {
        PipelineConfig(PhantomData)
    }
}
