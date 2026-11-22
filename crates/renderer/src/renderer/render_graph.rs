//! Compatibility facade for the backend-neutral render graph.
pub use super::indirect::DrawIndexedIndirectCommand;
pub use mltrs_render_graph::*;
pub type PreparedRenderGraph<N> = mltrs_render_graph::PreparedRenderGraph<N, super::Renderer>;

use super::pipeline::PipelineIndex;
use super::pipeline::{
    Compute, DrawIndexed, DrawIndexedIndirect, DrawVertexCount, GraphicsPipelineHandle,
    NoPush as RendererNoPush, PickingPipelineHandle, PipelineHandle,
    PushBlock as RendererPushBlock,
};
use super::storage_buffer::{
    CpuUpdatedBufferHandle, GpuUpdatedBufferHandle, ImmutableBufferHandle, SingletonBufferHandle,
};
use super::uniform_buffer::UniformBufferHandle;
use super::vertex_description::{IndexedVertexInterface, NoVertex};

impl<T> From<&UniformBufferHandle<T>> for UniformSlot<T> {
    fn from(handle: &UniformBufferHandle<T>) -> Self {
        Self::from_backend(handle.index())
    }
}
impl<T> From<&CpuUpdatedBufferHandle<T>> for CpuUpdatedSlot<T> {
    fn from(handle: &CpuUpdatedBufferHandle<T>) -> Self {
        Self::from_backend(handle.index(), handle.len())
    }
}
impl<T> From<&GpuUpdatedBufferHandle<T>> for GpuUpdatedSlot<T> {
    fn from(handle: &GpuUpdatedBufferHandle<T>) -> Self {
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

impl From<&PipelineHandle<Compute, RendererNoPush>> for ComputePipelineKey<NoPush> {
    fn from(handle: &PipelineHandle<Compute, RendererNoPush>) -> Self {
        Self::new(handle.index().raw())
    }
}

impl<B: GraphShaderParams + PushConstantBlock> From<&PipelineHandle<Compute, RendererPushBlock<B>>>
    for ComputePipelineKey<PushBlock<B>>
{
    fn from(handle: &PipelineHandle<Compute, RendererPushBlock<B>>) -> Self {
        Self::new(handle.index().raw())
    }
}

// Graphics handles lower to the graph's untyped keys once the vertex
// interface matches the draw family: vertex-count draws need `NoVertex`, and
// indexed draws need an indexed interface.
macro_rules! renderer_graphics_pipeline_adapters {
    ($(($kind:ty, $key:ident, [$($bounds:tt)*], $interface:ty)),+ $(,)?) => {$(
        impl<$($bounds)*> From<&GraphicsPipelineHandle<$interface, $kind, RendererNoPush>>
            for $key<NoPush>
        {
            fn from(handle: &GraphicsPipelineHandle<$interface, $kind, RendererNoPush>) -> Self {
                Self::new(handle.raw().index().raw())
            }
        }

        impl<$($bounds)* B: GraphShaderParams + PushConstantBlock>
            From<&GraphicsPipelineHandle<$interface, $kind, RendererPushBlock<B>>>
            for $key<PushBlock<B>>
        {
            fn from(handle: &GraphicsPipelineHandle<$interface, $kind, RendererPushBlock<B>>) -> Self {
                Self::new(handle.raw().index().raw())
            }
        }
    )+};
}
renderer_graphics_pipeline_adapters! {
    (DrawVertexCount, DrawVertexCountKey, [], NoVertex),
    (DrawIndexed, DrawIndexedKey, [I: IndexedVertexInterface,], I),
    (DrawIndexedIndirect, DrawIndexedIndirectKey, [I: IndexedVertexInterface,], I),
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
