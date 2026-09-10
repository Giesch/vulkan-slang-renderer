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

#[derive(Debug, Clone, Copy)]
pub struct ParamsData {
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

#[derive(Debug, Clone, Copy)]
pub struct ParamsBindings {
    pub pigment_in_0_3: SampledTexBinding,
    pub pigment_in_4_7: SampledTexBinding,
    pub pigment_in_8_11: SampledTexBinding,
    pub u_in: SampledTexBinding,
    pub v_in: SampledTexBinding,
    pub wet_mask: SampledTexBinding,
    pub pigment_out_0_3: StorageTexBinding,
    pub pigment_out_4_7: StorageTexBinding,
    pub pigment_out_8_11: StorageTexBinding,
    pub deposit_in_0_3: SampledTexBinding,
    pub deposit_in_4_7: SampledTexBinding,
    pub deposit_in_8_11: SampledTexBinding,
    pub deposit_out_0_3: StorageTexBinding,
    pub deposit_out_4_7: StorageTexBinding,
    pub deposit_out_8_11: StorageTexBinding,
    pub paper_height: SampledTexBinding,
}

impl GraphBindingSet for ParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.pigment_in_0_3));
        f(GraphBinding::SampledTex(self.pigment_in_4_7));
        f(GraphBinding::SampledTex(self.pigment_in_8_11));
        f(GraphBinding::SampledTex(self.u_in));
        f(GraphBinding::SampledTex(self.v_in));
        f(GraphBinding::SampledTex(self.wet_mask));
        f(GraphBinding::StorageTex(self.pigment_out_0_3));
        f(GraphBinding::StorageTex(self.pigment_out_4_7));
        f(GraphBinding::StorageTex(self.pigment_out_8_11));
        f(GraphBinding::SampledTex(self.deposit_in_0_3));
        f(GraphBinding::SampledTex(self.deposit_in_4_7));
        f(GraphBinding::SampledTex(self.deposit_in_8_11));
        f(GraphBinding::StorageTex(self.deposit_out_0_3));
        f(GraphBinding::StorageTex(self.deposit_out_4_7));
        f(GraphBinding::StorageTex(self.deposit_out_8_11));
        f(GraphBinding::SampledTex(self.paper_height));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct ParamsInput {
    pub pigment_in_0_3: SampledTexBinding,
    pub pigment_in_4_7: SampledTexBinding,
    pub pigment_in_8_11: SampledTexBinding,
    pub u_in: SampledTexBinding,
    pub v_in: SampledTexBinding,
    pub wet_mask: SampledTexBinding,
    pub pigment_out_0_3: StorageTexBinding,
    pub pigment_out_4_7: StorageTexBinding,
    pub pigment_out_8_11: StorageTexBinding,
    pub deposit_in_0_3: SampledTexBinding,
    pub deposit_in_4_7: SampledTexBinding,
    pub deposit_in_8_11: SampledTexBinding,
    pub deposit_out_0_3: StorageTexBinding,
    pub deposit_out_4_7: StorageTexBinding,
    pub deposit_out_8_11: StorageTexBinding,
    pub paper_height: SampledTexBinding,
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

impl GraphShaderParams for Params {
    type Data = ParamsData;
    type Bindings = ParamsBindings;
    type Input = ParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            grid_size: data.grid_size,
            dt: data.dt,
            transfer_rate: data.transfer_rate,
            pigment0: data.pigment0,
            pigment1: data.pigment1,
            pigment2: data.pigment2,
            pigment3: data.pigment3,
            pigment4: data.pigment4,
            pigment5: data.pigment5,
            pigment6: data.pigment6,
            pigment7: data.pigment7,
            pigment8: data.pigment8,
            pigment9: data.pigment9,
            pigment10: data.pigment10,
            pigment11: data.pigment11,
            pigment_in_0_3: bindings.pigment_in_0_3,
            pigment_in_4_7: bindings.pigment_in_4_7,
            pigment_in_8_11: bindings.pigment_in_8_11,
            u_in: bindings.u_in,
            v_in: bindings.v_in,
            wet_mask: bindings.wet_mask,
            pigment_out_0_3: bindings.pigment_out_0_3,
            pigment_out_4_7: bindings.pigment_out_4_7,
            pigment_out_8_11: bindings.pigment_out_8_11,
            deposit_in_0_3: bindings.deposit_in_0_3,
            deposit_in_4_7: bindings.deposit_in_4_7,
            deposit_in_8_11: bindings.deposit_in_8_11,
            deposit_out_0_3: bindings.deposit_out_0_3,
            deposit_out_4_7: bindings.deposit_out_4_7,
            deposit_out_8_11: bindings.deposit_out_8_11,
            paper_height: bindings.paper_height,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            pigment_in_0_3: resolver.sampled_tex(input.pigment_in_0_3),
            pigment_in_4_7: resolver.sampled_tex(input.pigment_in_4_7),
            pigment_in_8_11: resolver.sampled_tex(input.pigment_in_8_11),
            u_in: resolver.sampled_tex(input.u_in),
            v_in: resolver.sampled_tex(input.v_in),
            wet_mask: resolver.sampled_tex(input.wet_mask),
            pigment_out_0_3: resolver.storage_tex(input.pigment_out_0_3),
            pigment_out_4_7: resolver.storage_tex(input.pigment_out_4_7),
            pigment_out_8_11: resolver.storage_tex(input.pigment_out_8_11),
            deposit_in_0_3: resolver.sampled_tex(input.deposit_in_0_3),
            deposit_in_4_7: resolver.sampled_tex(input.deposit_in_4_7),
            deposit_in_8_11: resolver.sampled_tex(input.deposit_in_8_11),
            deposit_out_0_3: resolver.storage_tex(input.deposit_out_0_3),
            deposit_out_4_7: resolver.storage_tex(input.deposit_out_4_7),
            deposit_out_8_11: resolver.storage_tex(input.deposit_out_8_11),
            paper_height: resolver.sampled_tex(input.paper_height),
            grid_size: input.grid_size,
            dt: input.dt,
            transfer_rate: input.transfer_rate,
            pigment0: input.pigment0,
            pigment1: input.pigment1,
            pigment2: input.pigment2,
            pigment3: input.pigment3,
            pigment4: input.pigment4,
            pigment5: input.pigment5,
            pigment6: input.pigment6,
            pigment7: input.pigment7,
            pigment8: input.pigment8,
            pigment9: input.pigment9,
            pigment10: input.pigment10,
            pigment11: input.pigment11,
        }
    }
}

impl GraphBindingSet for ParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.pigment_in_0_3));
        f(GraphBinding::SampledTex(self.pigment_in_4_7));
        f(GraphBinding::SampledTex(self.pigment_in_8_11));
        f(GraphBinding::SampledTex(self.u_in));
        f(GraphBinding::SampledTex(self.v_in));
        f(GraphBinding::SampledTex(self.wet_mask));
        f(GraphBinding::StorageTex(self.pigment_out_0_3));
        f(GraphBinding::StorageTex(self.pigment_out_4_7));
        f(GraphBinding::StorageTex(self.pigment_out_8_11));
        f(GraphBinding::SampledTex(self.deposit_in_0_3));
        f(GraphBinding::SampledTex(self.deposit_in_4_7));
        f(GraphBinding::SampledTex(self.deposit_in_8_11));
        f(GraphBinding::StorageTex(self.deposit_out_0_3));
        f(GraphBinding::StorageTex(self.deposit_out_4_7));
        f(GraphBinding::StorageTex(self.deposit_out_8_11));
        f(GraphBinding::SampledTex(self.paper_height));
    }
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
