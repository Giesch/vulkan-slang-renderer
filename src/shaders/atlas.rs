use std::ffi::CString;

use ash::vk;

use crate::renderer::LayoutDescription;

use super::json::ReflectedPipelineLayout;

pub trait ShaderAtlasEntry {
    // dev only

    // used in hot reload
    fn source_file_name(&self) -> &str;

    // dev and release

    fn vertex_binding_descriptions(&self) -> Vec<vk::VertexInputBindingDescription>;
    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription>;

    // one set of descriptions per descriptor set
    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>>;

    // release only

    fn precompiled_shaders(&self) -> PrecompiledShaders;

    fn pipeline_layout(&self) -> &ReflectedPipelineLayout;
}

pub struct PrecompiledShaders {
    pub vert: PrecompiledShader,
    pub frag: PrecompiledShader,
}

pub struct PrecompiledShader {
    pub entry_point_name: CString,
    pub spv_bytes: Vec<u32>,
}

pub trait ComputeShaderAtlasEntry {
    fn source_file_name(&self) -> &str;
    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>>;
    fn precompiled_compute_shader(&self) -> PrecompiledShader;
    fn pipeline_layout(&self) -> &ReflectedPipelineLayout;
    fn workgroup_size(&self) -> [u32; 3];
}
