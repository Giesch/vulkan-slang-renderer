pub use super::gpu_write::NoVertex;

pub trait VertexDescription {
    fn binding_descriptions() -> Vec<ash::vk::VertexInputBindingDescription>;
    fn attribute_descriptions() -> Vec<ash::vk::VertexInputAttributeDescription>;
}

impl VertexDescription for NoVertex {
    fn binding_descriptions() -> Vec<ash::vk::VertexInputBindingDescription> {
        vec![]
    }

    fn attribute_descriptions() -> Vec<ash::vk::VertexInputAttributeDescription> {
        vec![]
    }
}
