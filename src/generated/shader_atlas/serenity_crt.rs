// GENERATED FILE (do not edit directly)

//! generated from slang shader: serenity_crt.shader.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use ash::vk;
use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;
#[allow(unused)]
use crate::renderer::vertex_description::VertexDescription;
use crate::renderer::*;
use crate::shaders::atlas::{PrecompiledShader, PrecompiledShaders, ShaderAtlasEntry};
use crate::shaders::json::{ReflectedPipelineLayout, ReflectionJson};

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct SerenityCRTParams {
    pub resolution: glam::Vec2,
    pub scanline_intensity: f32,
    pub scanline_count: f32,
    pub time: f32,
    pub y_offset: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub rgb_shift: f32,
    pub adaptive_intensity: f32,
    pub vignette_strength: f32,
    pub curvature: f32,
    pub flicker_strength: f32,
}

impl GPUWrite for SerenityCRTParams {}
const _: () = assert!(std::mem::size_of::<SerenityCRTParams>() == 64);

pub struct Resources<'a> {
    pub tex: &'a TextureHandle,
    pub params_buffer: &'a UniformBufferHandle<SerenityCRTParams>,
}

pub struct Shader {
    pub reflection_json: ReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/serenity_crt.json"
        ));

        let reflection_json: ReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config(
        self,
        resources: Resources<'_>,
    ) -> PipelineConfig<'_, !, DrawVertexCount> {
        // NOTE each of these must be in descriptor set layout order in the reflection json

        #[rustfmt::skip]
        let texture_handles = vec![
            resources.tex,
        ];

        #[rustfmt::skip]
        let uniform_buffer_handles = vec![
            RawUniformBufferHandle::from_typed(resources.params_buffer),
        ];

        #[rustfmt::skip]
        let storage_buffer_handles = vec![
        ];

        let vertex_config = VertexConfig::VertexCount;

        PipelineConfigBuilder {
            shader: Box::new(self),
            vertex_config,
            texture_handles,
            uniform_buffer_handles,
            storage_buffer_handles,
            disable_depth_test: false,
        }
        .build()
    }

    fn vert_entry_point_name(&self) -> CString {
        let entry_point = self
            .reflection_json
            .vertex_entry_point
            .entry_point_name
            .clone();

        CString::new(entry_point).unwrap()
    }

    fn frag_entry_point_name(&self) -> CString {
        let entry_point = self
            .reflection_json
            .fragment_entry_point
            .entry_point_name
            .clone();

        CString::new(entry_point).unwrap()
    }

    fn vert_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/serenity_crt.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/serenity_crt.frag.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }
}

impl ShaderAtlasEntry for Shader {
    fn source_file_name(&self) -> &str {
        &self.reflection_json.source_file_name
    }

    fn vertex_binding_descriptions(&self) -> Vec<vk::VertexInputBindingDescription> {
        vec![]
    }

    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription> {
        vec![]
    }

    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>> {
        self.reflection_json.layout_bindings()
    }

    fn precompiled_shaders(&self) -> PrecompiledShaders {
        let vert = PrecompiledShader {
            entry_point_name: self.vert_entry_point_name(),
            spv_bytes: self.vert_spv(),
        };

        let frag = PrecompiledShader {
            entry_point_name: self.frag_entry_point_name(),
            spv_bytes: self.frag_spv(),
        };

        PrecompiledShaders { vert, frag }
    }

    fn pipeline_layout(&self) -> &ReflectedPipelineLayout {
        &self.reflection_json.pipeline_layout
    }
}
