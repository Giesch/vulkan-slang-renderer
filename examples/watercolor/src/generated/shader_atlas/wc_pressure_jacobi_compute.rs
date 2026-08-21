// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: wc_pressure_jacobi.compute.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

use mltrs::renderer::gpu_write::GPUWrite;
use mltrs::renderer::*;
use mltrs::shaders::atlas::{ComputeShaderAtlasEntry, PrecompiledShader};
use mltrs::shaders::json::{ComputeReflectionJson, ReflectedPipelineLayout};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct JacobiDispatch {
    pub pressure_in: BindlessHandle<Sampler2D>,
    pub pressure_out: BindlessHandle<RwTexture2D>,
}

impl GPUWrite for JacobiDispatch {}
const _: () = assert!(std::mem::size_of::<JacobiDispatch>() == 16);
const _: () = assert!(std::mem::offset_of!(JacobiDispatch, pressure_in) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(JacobiDispatch, pressure_out) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct Params {
    pub divergence: BindlessHandle<Sampler2D>,
    pub grid_size: glam::Vec2,
}

impl GPUWrite for Params {}
const _: () = assert!(std::mem::size_of::<Params>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, divergence) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, grid_size) == 8);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<Params>,
}

impl mltrs::renderer::gpu_write::PushConstantBlock for JacobiDispatch {}
// 128 bytes is the vulkan-guaranteed maxPushConstantsSize
const _: () = assert!(std::mem::size_of::<JacobiDispatch>() <= 128);

pub const WORKGROUP_SIZE: [u32; 3] = [16, 16, 1];

#[derive(Clone)]
pub struct Shader {
    pub reflection_json: ComputeReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/wc_pressure_jacobi.comp.json"
        ));

        let reflection_json: ComputeReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> ComputePipelineConfig<'a, PushBlock<JacobiDispatch>> {
        // NOTE each of these must be in descriptor set layout order in the reflection json

        #[rustfmt::skip]
        let texture_handles = vec![
        ];

        #[rustfmt::skip]
        let uniform_buffer_handles = vec![
            RawUniformBufferHandle::from_typed(resources.params_buffer),
        ];

        #[rustfmt::skip]
        let storage_texture_handles = vec![
        ];

        ComputePipelineConfigBuilder {
            shader: Box::new(self.clone()),
            texture_handles,
            uniform_buffer_handles,
            storage_texture_handles,
        }
        .build()
    }

    fn comp_entry_point_name(&self) -> CString {
        let entry_point = self
            .reflection_json
            .compute_entry_point
            .entry_point_name
            .clone();

        CString::new(entry_point).unwrap()
    }

    fn comp_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/wc_pressure_jacobi.comp.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }
}

impl ComputeShaderAtlasEntry for Shader {
    fn source_file_name(&self) -> &str {
        &self.reflection_json.source_file_name
    }

    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>> {
        self.reflection_json.layout_bindings()
    }

    fn precompiled_compute_shader(&self) -> PrecompiledShader {
        PrecompiledShader {
            entry_point_name: self.comp_entry_point_name(),
            spv_bytes: self.comp_spv(),
        }
    }

    fn pipeline_layout(&self) -> &ReflectedPipelineLayout {
        &self.reflection_json.pipeline_layout
    }

    fn workgroup_size(&self) -> [u32; 3] {
        self.reflection_json.workgroup_size
    }

    fn reflection_json(&self) -> &ComputeReflectionJson {
        &self.reflection_json
    }
}
