use std::ffi::CString;

use ash::vk;

use crate::renderer::LayoutDescription;

use super::json::{ComputeReflectionJson, ReflectedPipelineLayout, ReflectionJson};

/// The generated `ShaderAtlas` struct: every shader entry in a crate, plus the
/// one slang source dir they were all generated from.
///
/// A project has exactly one of these. `Game::run` reads `SHADERS_SOURCE_DIR`
/// off the type before the atlas exists, so the renderer can start watching
/// before any pipeline is created.
pub trait ShaderAtlasRoot {
    /// dev only: absolute path to the slang source dir this atlas was generated
    /// from (baked in with the consuming crate's manifest dir).
    ///
    /// A `&str` rather than a `&Path` because `Path::new` is not a `const fn`.
    const SHADERS_SOURCE_DIR: &'static str;

    fn init() -> Self;
}

pub trait ShaderAtlasEntry {
    // dev only

    // used in hot reload
    fn source_file_name(&self) -> &str;

    // used in hot reload to detect interface changes that require a rebuild
    fn reflection_json(&self) -> &ReflectionJson;

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

    fn reflection_json(&self) -> &ComputeReflectionJson;
    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>>;
    fn precompiled_compute_shader(&self) -> PrecompiledShader;
    fn pipeline_layout(&self) -> &ReflectedPipelineLayout;
    fn workgroup_size(&self) -> [u32; 3];
}
