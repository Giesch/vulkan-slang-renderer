// GENERATED FILE (do not edit directly)

//! generated from slang shader: ray_marching.shader.slang

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
pub struct RayMarchingParams {
    pub camera: RayMarchCamera,
    pub light_position: glam::Vec3,
    pub sphere_count: u32,
    pub box_count: u32,
    pub _padding_0: [u8; 12],
}

impl GPUWrite for RayMarchingParams {}
const _: () = assert!(std::mem::size_of::<RayMarchingParams>() == 112);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct BoxRect {
    pub transform: glam::Mat4,
    pub radii: glam::Vec3,
    pub _padding_0: [u8; 4],
    pub color: glam::Vec3,
    pub _padding_1: [u8; 4],
}

impl GPUWrite for BoxRect {}
const _: () = assert!(std::mem::size_of::<BoxRect>() == 96);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct Sphere {
    pub center: glam::Vec3,
    pub radius: f32,
    pub color: glam::Vec3,
    pub _padding_0: [u8; 4],
}

impl GPUWrite for Sphere {}
const _: () = assert!(std::mem::size_of::<Sphere>() == 32);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct RayMarchCamera {
    pub inverse_view_proj: glam::Mat4,
    pub position: glam::Vec3,
}

impl GPUWrite for RayMarchCamera {}

pub struct Resources<'a> {
    pub spheres: &'a StorageBufferHandle<Sphere>,
    pub boxes: &'a StorageBufferHandle<BoxRect>,
    pub params_buffer: &'a UniformBufferHandle<RayMarchingParams>,
}

pub struct Shader {
    pub reflection_json: ReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/ray_marching.json"
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
        ];

        #[rustfmt::skip]
        let uniform_buffer_handles = vec![
            RawUniformBufferHandle::from_typed(resources.params_buffer),
        ];

        #[rustfmt::skip]
        let storage_buffer_handles = vec![
            RawStorageBufferHandle::from_typed(resources.spheres),
            RawStorageBufferHandle::from_typed(resources.boxes),
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
            "/shaders/compiled/ray_marching.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/ray_marching.frag.spv"
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
