// Mirror the renderer's private backend bounds on public APIs.
#![allow(private_bounds)]

pub mod addr;
pub mod bindless;
// Exercise production byte decoding without linking the graphics backend.
#[path = "../../../../../renderer/src/renderer/gpu_read.rs"]
pub mod gpu_read;
pub mod gpu_write;
pub mod render_graph;

pub use addr::*;
pub use bindless::*;
pub use render_graph::{
    BindingResolver, BufferBinding, GraphBinding, GraphBindingSet, GraphParamBindingSet,
    GraphShaderParams, ImmutableBufferBinding, PendingParamBindings, RawBufferBinding,
    ReadBufferBinding, SampledTexBinding, StorageTexBinding,
};

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

pub struct PipelineConfig<'a, K, P = NoPush>(PhantomData<(&'a (), K, P)>);

pub trait DrawCall {}
pub struct Graphics;
impl DrawCall for Graphics {}
pub struct GraphicsIndirect;
impl DrawCall for GraphicsIndirect {}

pub struct LayoutDescription;

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
    pub fn build_graphics<P>(self) -> PipelineConfig<'a, Graphics, P> {
        PipelineConfig(PhantomData)
    }

    pub fn build_graphics_indirect<P>(self) -> PipelineConfig<'a, GraphicsIndirect, P> {
        PipelineConfig(PhantomData)
    }
}

impl<'a> ComputePipelineConfigBuilder<'a> {
    pub fn build<P>(self) -> ComputePipelineConfig<'a, P> {
        ComputePipelineConfig(PhantomData)
    }
}
