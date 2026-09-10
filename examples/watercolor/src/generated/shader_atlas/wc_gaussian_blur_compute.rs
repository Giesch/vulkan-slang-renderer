// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: wc_gaussian_blur.compute.slang

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
pub struct BlurDispatch {
    pub input_tex: BindlessHandle<Sampler2D>,
    pub output_tex: BindlessHandle<RwTexture2D>,
    pub direction: glam::Vec2,
}

impl GPUWrite for BlurDispatch {}
const _: () = assert!(std::mem::size_of::<BlurDispatch>() == 24);
const _: () = assert!(std::mem::offset_of!(BlurDispatch, input_tex) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BlurDispatch, output_tex) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BlurDispatch, direction) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct Params {
    pub grid_size: glam::Vec2,
    pub _padding_0: [u8; 8],
}

impl GPUWrite for Params {}
const _: () = assert!(std::mem::size_of::<Params>() == 16);
const _: () = assert!(std::mem::offset_of!(Params, grid_size) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<Params>,
}

#[derive(Debug, Clone, Copy)]
pub struct BlurDispatchData {
    pub direction: glam::Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct BlurDispatchBindings {
    pub input_tex: SampledTexBinding,
    pub output_tex: StorageTexBinding,
}

impl GraphBindingSet for BlurDispatchBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.input_tex));
        f(GraphBinding::StorageTex(self.output_tex));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct BlurDispatchInput {
    pub input_tex: SampledTexBinding,
    pub output_tex: StorageTexBinding,
    pub direction: glam::Vec2,
}

impl GraphShaderParams for BlurDispatch {
    type Data = BlurDispatchData;
    type Bindings = BlurDispatchBindings;
    type Input = BlurDispatchInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            direction: data.direction,
            input_tex: bindings.input_tex,
            output_tex: bindings.output_tex,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            input_tex: resolver.sampled_tex(input.input_tex),
            output_tex: resolver.storage_tex(input.output_tex),
            direction: input.direction,
        }
    }
}

impl GraphBindingSet for BlurDispatchInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.input_tex));
        f(GraphBinding::StorageTex(self.output_tex));
    }
}

/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct ParamsInput {
    pub grid_size: glam::Vec2,
}

impl GraphShaderParams for Params {
    type Data = Self;
    type Bindings = ();
    type Input = ParamsInput;

    fn input(data: &Self::Data, _bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            grid_size: data.grid_size,
        }
    }

    fn assemble_input(input: &Self::Input, _resolver: &BindingResolver<'_>) -> Self {
        Self {
            grid_size: input.grid_size,
            _padding_0: Default::default(),
        }
    }
}

impl GraphBindingSet for ParamsInput {
    fn visit(&self, _f: &mut dyn FnMut(GraphBinding)) {}
}

impl mltrs::renderer::gpu_write::PushConstantBlock for BlurDispatch {}
// 128 bytes is the vulkan-guaranteed maxPushConstantsSize
const _: () = assert!(std::mem::size_of::<BlurDispatch>() <= 128);

pub const WORKGROUP_SIZE: [u32; 3] = [16, 16, 1];

#[derive(Clone)]
pub struct Shader {
    pub reflection_json: ComputeReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/wc_gaussian_blur.comp.json"
        ));

        let reflection_json: ComputeReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> ComputePipelineConfig<'a, PushBlock<BlurDispatch>> {
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
            "/shaders/compiled/wc_gaussian_blur.comp.spv"
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
