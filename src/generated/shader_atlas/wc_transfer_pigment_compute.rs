// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: wc_transfer_pigment.compute.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;
use crate::renderer::*;
use crate::shaders::atlas::{ComputeShaderAtlasEntry, PrecompiledShader};
use crate::shaders::json::{ComputeReflectionJson, ReflectedPipelineLayout};

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct Params {
    pub grid_size: glam::Vec2,
    pub transfer_rate: f32,
    pub pad: f32,
    pub pigment0: PigmentProperties,
    pub pigment1: PigmentProperties,
    pub pigment2: PigmentProperties,
    pub pigment3: PigmentProperties,
}

impl GPUWrite for Params {}
const _: () = assert!(std::mem::size_of::<Params>() == 80);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct PigmentProperties {
    pub density: f32,
    pub staining_power: f32,
    pub granulation: f32,
    pub pad0: f32,
}

impl GPUWrite for PigmentProperties {}

pub struct Resources<'a> {
    pub pigment: &'a StorageTextureHandle,
    pub deposit: &'a StorageTextureHandle,
    pub paper_height: &'a TextureHandle,
    pub wet_mask: &'a TextureHandle,
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
            "/shaders/compiled/wc_transfer_pigment.comp.json"
        ));

        let reflection_json: ComputeReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config(self, resources: Resources<'_>) -> ComputePipelineConfig<'_> {
        // NOTE each of these must be in descriptor set layout order in the reflection json

        #[rustfmt::skip]
        let texture_handles = vec![
            resources.paper_height,
            resources.wet_mask,
        ];

        #[rustfmt::skip]
        let uniform_buffer_handles = vec![
            RawUniformBufferHandle::from_typed(resources.params_buffer),
        ];

        #[rustfmt::skip]
        let storage_buffer_handles = vec![
        ];

        #[rustfmt::skip]
        let storage_texture_handles = vec![
            resources.pigment,
            resources.deposit,
        ];

        ComputePipelineConfig {
            shader: Box::new(self),
            texture_handles,
            uniform_buffer_handles,
            storage_buffer_handles,
            storage_texture_handles,
            storage_buffer_frame_strategy: StorageBufferFrameStrategy::default(),
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
            "/shaders/compiled/wc_transfer_pigment.comp.spv"
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
