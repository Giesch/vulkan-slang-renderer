// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: wc_advect_and_transfer_pigment.compute.slang

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
    pub dt: f32,
    pub transfer_rate: f32,
    pub pigment0: PigmentProperties,
    pub pigment1: PigmentProperties,
    pub pigment2: PigmentProperties,
    pub pigment3: PigmentProperties,
    pub pigment4: PigmentProperties,
    pub pigment5: PigmentProperties,
    pub pigment6: PigmentProperties,
    pub pigment7: PigmentProperties,
    pub pigment8: PigmentProperties,
    pub pigment9: PigmentProperties,
    pub pigment10: PigmentProperties,
    pub pigment11: PigmentProperties,
}

impl GPUWrite for Params {}
const _: () = assert!(std::mem::size_of::<Params>() == 208);
const _: () = assert!(std::mem::offset_of!(Params, grid_size) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, dt) == 8);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, transfer_rate) == 12);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, pigment0) == 16);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment1) == 32);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment2) == 48);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment3) == 64);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment4) == 80);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment5) == 96);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment6) == 112);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment7) == 128);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment8) == 144);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment9) == 160);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment10) == 176);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment11) == 192);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct PigmentProperties {
    pub density: f32,
    pub staining_power: f32,
    pub granulation: f32,
    pub _padding_0: [u8; 4],
}

impl GPUWrite for PigmentProperties {}
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(PigmentProperties, density) == 0);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(PigmentProperties, staining_power) == 4);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(PigmentProperties, granulation) == 8);
const _: () = assert!(std::mem::size_of::<f32>() == 4);

pub struct Resources<'a> {
    pub pigment_in_0_3: &'a TextureHandle,
    pub pigment_in_4_7: &'a TextureHandle,
    pub pigment_in_8_11: &'a TextureHandle,
    pub u_in: &'a TextureHandle,
    pub v_in: &'a TextureHandle,
    pub wet_mask: &'a TextureHandle,
    pub paper_height: &'a TextureHandle,
    pub pigment_out_0_3: &'a StorageTextureHandle,
    pub pigment_out_4_7: &'a StorageTextureHandle,
    pub pigment_out_8_11: &'a StorageTextureHandle,
    pub deposit_in_0_3: &'a TextureHandle,
    pub deposit_in_4_7: &'a TextureHandle,
    pub deposit_in_8_11: &'a TextureHandle,
    pub deposit_out_0_3: &'a StorageTextureHandle,
    pub deposit_out_4_7: &'a StorageTextureHandle,
    pub deposit_out_8_11: &'a StorageTextureHandle,
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
            "/shaders/compiled/wc_advect_and_transfer_pigment.comp.json"
        ));

        let reflection_json: ComputeReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config(self, resources: Resources<'_>) -> ComputePipelineConfig<'_> {
        // NOTE each of these must be in descriptor set layout order in the reflection json

        #[rustfmt::skip]
        let texture_handles = vec![
            resources.pigment_in_0_3,
            resources.pigment_in_4_7,
            resources.pigment_in_8_11,
            resources.u_in,
            resources.v_in,
            resources.wet_mask,
            resources.paper_height,
            resources.deposit_in_0_3,
            resources.deposit_in_4_7,
            resources.deposit_in_8_11,
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
            resources.pigment_out_0_3,
            resources.pigment_out_4_7,
            resources.pigment_out_8_11,
            resources.deposit_out_0_3,
            resources.deposit_out_4_7,
            resources.deposit_out_8_11,
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
            "/shaders/compiled/wc_advect_and_transfer_pigment.comp.spv"
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
