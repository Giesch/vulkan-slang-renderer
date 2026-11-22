use std::marker::PhantomData;

pub use super::gpu_write::NoVertex;

pub trait VertexDescription {
    fn binding_descriptions() -> Vec<ash::vk::VertexInputBindingDescription>;
    fn attribute_descriptions() -> Vec<ash::vk::VertexInputAttributeDescription>;
}

/// Stub of the renderer's typed vertex interface marker.
pub struct TypedVertex<V>(PhantomData<fn() -> V>);

/// Stub of the renderer's packed-bytes vertex interface marker.
pub struct RuntimeVertex;
