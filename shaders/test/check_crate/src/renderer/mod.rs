pub mod gpu_write;
pub mod vertex_description;

pub use gpu_write::*;
pub use vertex_description::*;

use std::marker::PhantomData;

pub struct UniformBufferHandle<T>(PhantomData<T>);
pub struct StorageBufferHandle<T>(PhantomData<T>);
pub struct TextureHandle;

pub struct RawUniformBufferHandle;
impl RawUniformBufferHandle {
    pub fn from_typed<T>(_: &UniformBufferHandle<T>) -> Self {
        Self
    }
}

pub struct RawStorageBufferHandle;
impl RawStorageBufferHandle {
    pub fn from_typed<T>(_: &StorageBufferHandle<T>) -> Self {
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

pub enum VertexConfig<V> {
    VertexAndIndexBuffers(Vec<V>, Vec<u32>),
    VertexCount,
}

pub struct PipelineConfigBuilder<'a, V> {
    pub shader: Box<dyn crate::shaders::atlas::ShaderAtlasEntry>,
    pub vertex_config: VertexConfig<V>,
    pub texture_handles: Vec<&'a TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_buffer_handles: Vec<RawStorageBufferHandle>,
    pub disable_depth_test: bool,
}

impl<'a, V> PipelineConfigBuilder<'a, V> {
    pub fn build<D: DrawCall>(self) -> PipelineConfig<'a, V, D> {
        PipelineConfig(PhantomData)
    }
}
