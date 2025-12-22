// GENERATED FILE (do not edit directly)

//! generated from slang shader: basic_triangle.shader.slang

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
pub struct MVPMatrices {
    pub model: glam::Mat4,
    pub view: glam::Mat4,
    pub proj: glam::Mat4,
}

impl GPUWrite for MVPMatrices {}

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct Vertex {
    pub position: glam::Vec3,
    pub color: glam::Vec3,
}

impl GPUWrite for Vertex {}

pub struct Resources<'a> {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub mvp_buffer: &'a UniformBufferHandle<MVPMatrices>,
}

impl VertexDescription for Vertex {
    fn binding_descriptions() -> Vec<ash::vk::VertexInputBindingDescription> {
        let binding_description = ash::vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<Self>() as u32)
            .input_rate(ash::vk::VertexInputRate::VERTEX);

        vec![binding_description]
    }

    fn attribute_descriptions() -> Vec<ash::vk::VertexInputAttributeDescription> {
        vec![
            ash::vk::VertexInputAttributeDescription::default()
                .offset(std::mem::offset_of!(Vertex, position) as u32)
                .format(ash::vk::Format::R32G32B32_SFLOAT)
                .binding(0)
                .location(0),
            ash::vk::VertexInputAttributeDescription::default()
                .offset(std::mem::offset_of!(Vertex, color) as u32)
                .format(ash::vk::Format::R32G32B32_SFLOAT)
                .binding(0)
                .location(1),
        ]
    }
}

pub struct Shader {
    pub reflection_json: ReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/basic_triangle.json"
        ));

        let reflection_json: ReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config(self, resources: Resources<'_>) -> PipelineConfig<'_, Vertex> {
        // NOTE each of these must be in descriptor set layout order in the reflection json

        #[rustfmt::skip]
        let texture_handles = vec![
        ];

        #[rustfmt::skip]
        let uniform_buffer_handles = vec![
            RawUniformBufferHandle::from_typed(resources.mvp_buffer),
        ];

        #[rustfmt::skip]
        let storage_buffer_handles = vec![
        ];

        let vertex_config =
            VertexConfig::VertexAndIndexBuffers(resources.vertices, resources.indices);

        PipelineConfig {
            shader: Box::new(self),
            vertex_config,
            texture_handles,
            uniform_buffer_handles,
            storage_buffer_handles,
            disable_depth_test: false,
        }
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
            "/shaders/compiled/basic_triangle.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/basic_triangle.frag.spv"
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
        Vertex::binding_descriptions()
    }

    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription> {
        Vertex::attribute_descriptions()
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
