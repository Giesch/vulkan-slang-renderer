// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: wc_capillary_flow.compute.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;
use crate::renderer::*;
use crate::shaders::atlas::{ComputeShaderAtlasEntry, PrecompiledShader};
use crate::shaders::json::{ComputeReflectionJson, ReflectedPipelineLayout};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct Params {
    pub grid_size: glam::Vec2,
    pub diffuse_rate: f32,
    pub capacity: f32,
    pub sigma: f32,
    pub dry_threshold: f32,
    pub _padding_0: [u8; 8],
}

impl GPUWrite for Params {}
const _: () = assert!(std::mem::size_of::<Params>() == 32);
const _: () = assert!(std::mem::offset_of!(Params, grid_size) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, diffuse_rate) == 8);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, capacity) == 12);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, sigma) == 16);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, dry_threshold) == 20);
const _: () = assert!(std::mem::size_of::<f32>() == 4);

pub struct Resources<'a> {
    pub saturation_in: &'a TextureHandle,
    pub wet_mask_in: &'a TextureHandle,
    pub paper_height: &'a TextureHandle,
    pub saturation_out: &'a StorageTextureHandle,
    pub wet_mask_out: &'a StorageTextureHandle,
    pub params_buffer: &'a UniformBufferHandle<Params>,
}

pub const WORKGROUP_SIZE: [u32; 3] = [16, 16, 1];

pub struct Shader {
    pub reflection_json: ComputeReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/wc_capillary_flow.comp.json"
        ));

        let reflection_json: ComputeReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config(self, resources: Resources<'_>) -> ComputePipelineConfig<'_> {
        // NOTE each of these must be in descriptor set layout order in the reflection json

        #[rustfmt::skip]
        let texture_handles = vec![
            resources.saturation_in,
            resources.wet_mask_in,
            resources.paper_height,
        ];

        #[rustfmt::skip]
        let uniform_buffer_handles = vec![
            RawUniformBufferHandle::from_typed(resources.params_buffer),
        ];

        #[rustfmt::skip]
        let storage_texture_handles = vec![
            resources.saturation_out,
            resources.wet_mask_out,
        ];

        ComputePipelineConfig {
            shader: Box::new(self),
            texture_handles,
            uniform_buffer_handles,
            storage_texture_handles,
        }
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
            "/shaders/compiled/wc_capillary_flow.comp.spv"
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
}
