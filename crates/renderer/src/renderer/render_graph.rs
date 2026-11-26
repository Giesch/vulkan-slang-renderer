//! Compatibility facade for the backend-neutral render graph.
pub use super::indirect::DrawIndexedIndirectCommand;
pub use mltrs_render_graph::*;
pub type PreparedRenderGraph<N> = mltrs_render_graph::PreparedRenderGraph<N, super::Renderer>;

use super::index_buffer::IndexBufferHandle;
use super::pipeline::PipelineIndex;
use super::pipeline::{
    Compute, Graphics, GraphicsIndirect, NoPush as RendererNoPush, PickingPipelineHandle,
    PipelineHandle, PushBlock as RendererPushBlock,
};
use super::storage_buffer::{
    CpuUpdatedBufferHandle, GpuUpdatedBufferHandle, ImmutableBufferHandle, SingletonBufferHandle,
};
use super::uniform_buffer::UniformBufferHandle;

impl<T> From<&UniformBufferHandle<T>> for UniformSlot<T> {
    fn from(handle: &UniformBufferHandle<T>) -> Self {
        Self::from_backend(handle.index())
    }
}

// slots take the handle's stride, not `size_of::<T>()`: a `BufferBytes`
// buffer's element size is only known at runtime
impl<T> From<&CpuUpdatedBufferHandle<T>> for CpuUpdatedSlot<T> {
    fn from(handle: &CpuUpdatedBufferHandle<T>) -> Self {
        Self::from_backend_with_element_size(
            handle.index(),
            handle.len(),
            handle.element_stride() as usize,
        )
    }
}

impl<T> From<&GpuUpdatedBufferHandle<T>> for GpuUpdatedSlot<T> {
    fn from(handle: &GpuUpdatedBufferHandle<T>) -> Self {
        Self::from_backend_with_element_size(
            handle.index(),
            handle.len(),
            handle.element_stride() as usize,
        )
    }
}

impl<T> From<&ImmutableBufferHandle<T>> for ImmutableSlot<T> {
    fn from(handle: &ImmutableBufferHandle<T>) -> Self {
        Self::from_backend_with_element_size(
            handle.index(),
            handle.len(),
            handle.element_stride() as usize,
        )
    }
}

impl<T> From<&SingletonBufferHandle<T>> for SingletonSlot<T> {
    fn from(handle: &SingletonBufferHandle<T>) -> Self {
        Self::from_backend_with_element_size(
            handle.index(),
            handle.len(),
            handle.element_stride() as usize,
        )
    }
}

impl From<&IndexBufferHandle> for IndexBufferSlot {
    fn from(handle: &IndexBufferHandle) -> Self {
        Self::from_backend(handle.index().raw(), handle.index_count())
    }
}

// Each renderer pipeline kind lowers to the graph key of the same family, so
// a draw node cannot name a pipeline of another kind.
macro_rules! renderer_pipeline_adapters {
    ($(($kind:ty, $key:ident)),+ $(,)?) => {$(
        impl From<&PipelineHandle<$kind, RendererNoPush>> for $key<NoPush> {
            fn from(handle: &PipelineHandle<$kind, RendererNoPush>) -> Self {
                Self::new(handle.index().raw())
            }
        }

        impl<B: GraphShaderParams + PushConstantBlock>
            From<&PipelineHandle<$kind, RendererPushBlock<B>>> for $key<PushBlock<B>>
        {
            fn from(handle: &PipelineHandle<$kind, RendererPushBlock<B>>) -> Self {
                Self::new(handle.index().raw())
            }
        }
    )+};
}
renderer_pipeline_adapters! {
    (Compute, ComputePipelineKey),
    (Graphics, GraphicsPipelineKey),
    (GraphicsIndirect, GraphicsIndirectPipelineKey),
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
