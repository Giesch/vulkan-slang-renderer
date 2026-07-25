use std::ffi::CString;

use ash::vk;

use crate::renderer::LayoutDescription;
use crate::shaders::json::{ComputeReflectionJson, ReflectedPipelineLayout, ReflectionJson};

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
    fn reflection_json(&self) -> &ReflectionJson;

    // dev only: the slang source dir this entry was generated from.
    // Emitted by codegen as the generating crate's CARGO_MANIFEST_DIR, so hot
    // reload finds sources even when the shader lives in a consumer crate.
    fn shaders_source_dir(&self) -> &'static std::path::Path;
}

pub trait ComputeShaderAtlasEntry {
    fn source_file_name(&self) -> &str;
    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>>;
    fn precompiled_compute_shader(&self) -> PrecompiledShader;
    fn pipeline_layout(&self) -> &ReflectedPipelineLayout;
    fn workgroup_size(&self) -> [u32; 3];
    fn reflection_json(&self) -> &ComputeReflectionJson;

    // dev only: the slang source dir this entry was generated from.
    // Emitted by codegen as the generating crate's CARGO_MANIFEST_DIR, so hot
    // reload finds sources even when the shader lives in a consumer crate.
    fn shaders_source_dir(&self) -> &'static std::path::Path;
}
