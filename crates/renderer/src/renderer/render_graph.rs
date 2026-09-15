//! Compatibility facade for the backend-neutral render graph.
pub use super::indirect::DrawIndexedIndirectCommand;
pub use mltrs_render_graph::*;
pub type PreparedRenderGraph<N> = mltrs_render_graph::PreparedRenderGraph<N, super::Renderer>;

use super::pipeline::PipelineIndex;
use super::pipeline::{
    Compute, DrawIndexed, DrawIndexedIndirect, DrawVertexCount, NoPush as RendererNoPush,
    PickingPipelineHandle, PipelineHandle, PushBlock as RendererPushBlock,
};
use super::storage_buffer::{
    GpuOnlyBufferHandle, ImmutableBufferHandle, SingletonBufferHandle, StorageBufferHandle,
};
use super::uniform_buffer::UniformBufferHandle;

impl<T> From<&UniformBufferHandle<T>> for UniformSlot<T> {
    fn from(handle: &UniformBufferHandle<T>) -> Self {
        Self::from_backend(handle.index())
    }
}
impl<T> From<&StorageBufferHandle<T>> for StorageSlot<T> {
    fn from(handle: &StorageBufferHandle<T>) -> Self {
        Self::from_backend(handle.index(), handle.len())
    }
}
impl<T> From<&GpuOnlyBufferHandle<T>> for GpuOnlySlot<T> {
    fn from(handle: &GpuOnlyBufferHandle<T>) -> Self {
        Self::from_backend(handle.index())
    }
}
impl<T> From<&ImmutableBufferHandle<T>> for ImmutableSlot<T> {
    fn from(handle: &ImmutableBufferHandle<T>) -> Self {
        Self::from_backend(handle.index(), handle.len())
    }
}
impl<T> From<&SingletonBufferHandle<T>> for SingletonSlot<T> {
    fn from(handle: &SingletonBufferHandle<T>) -> Self {
        Self::from_backend(handle.index(), handle.len())
    }
}
macro_rules! renderer_pipeline_adapters {
    ($(($kind:ty, $key:ident)),+ $(,)?) => {$(
        impl From<&PipelineHandle<$kind, RendererNoPush>> for $key<NoPush> {
            fn from(handle: &PipelineHandle<$kind, RendererNoPush>) -> Self { Self::new(handle.index().raw()) }
        }
        impl<B: GraphShaderParams + PushConstantBlock> From<&PipelineHandle<$kind, RendererPushBlock<B>>> for $key<PushBlock<B>> {
            fn from(handle: &PipelineHandle<$kind, RendererPushBlock<B>>) -> Self { Self::new(handle.index().raw()) }
        }
    )+};
}
renderer_pipeline_adapters! {
    (Compute, ComputePipelineKey),
    (DrawVertexCount, DrawVertexCountKey),
    (DrawIndexed, DrawIndexedKey),
    (DrawIndexedIndirect, DrawIndexedIndirectKey),
}
impl From<&PickingPipelineHandle> for PickingPipelineKey {
    fn from(handle: &PickingPipelineHandle) -> Self {
        Self::new(handle.index.raw())
    }
}
impl super::ToVk for GraphFormat {
    type Vk = ash::vk::Format;
    fn to_vk(&self) -> Self::Vk {
        match self {
            Self::R32Float => ash::vk::Format::R32_SFLOAT,
            Self::Rgba32Float => ash::vk::Format::R32G32B32A32_SFLOAT,
        }
    }
}
