use std::ffi::CString;

use ash::vk;

use crate::renderer::LayoutDescription;
use crate::shaders::json::ReflectedPipelineLayout;

pub struct PrecompiledShader {
    pub entry_point_name: CString,
    pub spv_bytes: Vec<u32>,
}

pub struct PrecompiledShaders {
    pub vert: PrecompiledShader,
    pub frag: PrecompiledShader,
}

pub trait ShaderAtlasEntry {
    fn source_file_name(&self) -> &str;
    fn vertex_binding_descriptions(&self) -> Vec<vk::VertexInputBindingDescription>;
    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription>;
    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>>;
    fn precompiled_shaders(&self) -> PrecompiledShaders;
    fn pipeline_layout(&self) -> &ReflectedPipelineLayout;
}
