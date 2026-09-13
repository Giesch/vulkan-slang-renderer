// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: wc_capillary_flow.compute.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

use mltrs::renderer::render_graph::GPUWrite;
use mltrs::renderer::*;
use mltrs::shaders::atlas::{ComputeShaderAtlasEntry, PrecompiledShader};
use mltrs::shaders::json::{ComputeReflectionJson, ReflectedPipelineLayout};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct Params {
    pub saturation_in: BindlessHandle<Sampler2D>,
    pub wet_mask_in: BindlessHandle<Sampler2D>,
    pub saturation_out: BindlessHandle<RwTexture2D>,
    pub wet_mask_out: BindlessHandle<RwTexture2D>,
    pub grid_size: glam::Vec2,
    pub diffuse_rate: f32,
    pub capacity: f32,
    pub sigma: f32,
    pub dry_threshold: f32,
    pub _padding_0: [u8; 8],
}

impl GPUWrite for Params {}
const _: () = assert!(std::mem::size_of::<Params>() == 64);
const _: () = assert!(std::mem::offset_of!(Params, saturation_in) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, wet_mask_in) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, saturation_out) == 16);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, wet_mask_out) == 24);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, grid_size) == 32);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(Params, diffuse_rate) == 40);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, capacity) == 44);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, sigma) == 48);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(Params, dry_threshold) == 52);
const _: () = assert!(std::mem::size_of::<f32>() == 4);

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<Params>,
}

#[derive(Debug, Clone, Copy)]
pub struct ParamsData {
    pub grid_size: glam::Vec2,
    pub diffuse_rate: f32,
    pub capacity: f32,
    pub sigma: f32,
    pub dry_threshold: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ParamsBindings {
    pub saturation_in: SampledTexBinding,
    pub wet_mask_in: SampledTexBinding,
    pub saturation_out: StorageTexBinding,
    pub wet_mask_out: StorageTexBinding,
}

impl GraphParamBindingSet for ParamsBindings {
    type Pending = PendingParamBindings<Self>;

    fn pending() -> Self::Pending {
        Self::Pending::new()
    }
}

impl GraphBindingSet for ParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.saturation_in));
        f(GraphBinding::SampledTex(self.wet_mask_in));
        f(GraphBinding::StorageTex(self.saturation_out));
        f(GraphBinding::StorageTex(self.wet_mask_out));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct ParamsInput {
    pub saturation_in: SampledTexBinding,
    pub wet_mask_in: SampledTexBinding,
    pub saturation_out: StorageTexBinding,
    pub wet_mask_out: StorageTexBinding,
    pub grid_size: glam::Vec2,
    pub diffuse_rate: f32,
    pub capacity: f32,
    pub sigma: f32,
    pub dry_threshold: f32,
}

impl GraphShaderParams for Params {
    type Data = ParamsData;
    type Bindings = ParamsBindings;
    type Input = ParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            grid_size: data.grid_size,
            diffuse_rate: data.diffuse_rate,
            capacity: data.capacity,
            sigma: data.sigma,
            dry_threshold: data.dry_threshold,
            saturation_in: bindings.saturation_in,
            wet_mask_in: bindings.wet_mask_in,
            saturation_out: bindings.saturation_out,
            wet_mask_out: bindings.wet_mask_out,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            saturation_in: resolver.sampled_tex(input.saturation_in),
            wet_mask_in: resolver.sampled_tex(input.wet_mask_in),
            saturation_out: resolver.storage_tex(input.saturation_out),
            wet_mask_out: resolver.storage_tex(input.wet_mask_out),
            grid_size: input.grid_size,
            diffuse_rate: input.diffuse_rate,
            capacity: input.capacity,
            sigma: input.sigma,
            dry_threshold: input.dry_threshold,
            _padding_0: Default::default(),
        }
    }
}

impl GraphBindingSet for ParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.saturation_in));
        f(GraphBinding::SampledTex(self.wet_mask_in));
        f(GraphBinding::StorageTex(self.saturation_out));
        f(GraphBinding::StorageTex(self.wet_mask_out));
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
            "/shaders/compiled/wc_capillary_flow.comp.json"
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

    fn reflection_json(&self) -> &ComputeReflectionJson {
        &self.reflection_json
    }
}
