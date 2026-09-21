use ash::vk;

pub use super::gpu_write::NoVertex;

pub trait VertexDescription: super::GPUWrite {
    fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription>;
    fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription>;
}

/// The vertex type of a pipeline fed with already-packed vertex bytes. The
/// shader entry supplies the binding and attribute descriptions, so this type
/// has none of its own.
pub struct VertexBytes;

impl super::render_graph::GPUWrite for VertexBytes {}

impl VertexDescription for VertexBytes {
    fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
        vec![]
    }

    fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
        vec![]
    }
}

impl VertexDescription for NoVertex {
    fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
        vec![]
    }

    fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
        vec![]
    }
}
