pub trait VertexDescription {
    fn binding_descriptions() -> Vec<ash::vk::VertexInputBindingDescription>;
    fn attribute_descriptions() -> Vec<ash::vk::VertexInputAttributeDescription>;
}
