use ash::vk;

pub use super::gpu_write::NoVertex;

pub trait VertexDescription: super::GPUWrite {
    fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription>;
    fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription>;
}

impl VertexDescription for NoVertex {
    fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
        vec![]
    }

    fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
        vec![]
    }
}
