// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: wc_advect_and_transfer_pigment.compute.slang

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
#[repr(C, align(16))]
pub struct Params {
    pub pigment_in_0_3: BindlessHandle<Sampler2D>,
    pub pigment_in_4_7: BindlessHandle<Sampler2D>,
    pub pigment_in_8_11: BindlessHandle<Sampler2D>,
    pub u_in: BindlessHandle<Sampler2D>,
    pub v_in: BindlessHandle<Sampler2D>,
    pub wet_mask: BindlessHandle<Sampler2D>,
    pub pigment_out_0_3: BindlessHandle<RwTexture2D>,
    pub pigment_out_4_7: BindlessHandle<RwTexture2D>,
    pub pigment_out_8_11: BindlessHandle<RwTexture2D>,
    pub deposit_in_0_3: BindlessHandle<Sampler2D>,
    pub deposit_in_4_7: BindlessHandle<Sampler2D>,
    pub deposit_in_8_11: BindlessHandle<Sampler2D>,
    pub deposit_out_0_3: BindlessHandle<RwTexture2D>,
    pub deposit_out_4_7: BindlessHandle<RwTexture2D>,
    pub deposit_out_8_11: BindlessHandle<RwTexture2D>,
    pub paper_height: BindlessHandle<Sampler2D>,
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
const _: () = assert!(std::mem::size_of::<Params>() == 336);
const _: () = assert!(std::mem::offset_of!(Params, pigment_in_0_3) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, pigment_in_4_7) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, pigment_in_8_11) == 16);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, u_in) == 24);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, v_in) == 32);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, wet_mask) == 40);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, pigment_out_0_3) == 48);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, pigment_out_4_7) == 56);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, pigment_out_8_11) == 64);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, deposit_in_0_3) == 72);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, deposit_in_4_7) == 80);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, deposit_in_8_11) == 88);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, deposit_out_0_3) == 96);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, deposit_out_4_7) == 104);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, deposit_out_8_11) == 112);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, paper_height) == 120);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, grid_size) == 128);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, dt) == 136);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, transfer_rate) == 140);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, pigment0) == 144);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment1) == 160);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment2) == 176);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment3) == 192);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment4) == 208);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment5) == 224);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment6) == 240);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment7) == 256);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment8) == 272);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment9) == 288);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment10) == 304);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, pigment11) == 320);
const _: () = assert!(std::mem::size_of::<PigmentProperties>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
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
    pub params_buffer: &'a UniformBufferHandle<Params>,
}

pub const WORKGROUP_SIZE: [u32; 3] = [16, 16, 1];

#[derive(Clone)]
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

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> ComputePipelineConfig<'a, NoPush> {
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

    fn reflection_json(&self) -> &ComputeReflectionJson {
        &self.reflection_json
    }
}
