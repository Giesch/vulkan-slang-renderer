// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: skinning_oracle.compute.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

pub use super::skinning::{SkinJoint, VertexSkinning};
#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
use mltrs::renderer::render_graph::GPUWrite;
use mltrs::renderer::*;
use mltrs::shaders::atlas::{ComputeShaderAtlasEntry, PrecompiledShader};
use mltrs::shaders::json::{ComputeReflectionJson, ReflectedPipelineLayout};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct SkinningOraclePush {
    pub output: Addr<OracleOutput>,
}

impl GPUWrite for SkinningOraclePush {}
const _: () = assert!(std::mem::size_of::<SkinningOraclePush>() == 8);
const _: () = assert!(std::mem::offset_of!(SkinningOraclePush, output) == 0);
const _: () = assert!(std::mem::size_of::<Addr<OracleOutput>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct OracleOutput {
    pub position: glam::Vec4,
    pub normal: glam::Vec4,
    pub safe_normalized: glam::Vec4,
    pub flags: glam::UVec4,
}

impl GPUWrite for OracleOutput {}
const _: () = assert!(std::mem::size_of::<OracleOutput>() == 64);
const _: () = assert!(std::mem::offset_of!(OracleOutput, position) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(OracleOutput, normal) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(OracleOutput, safe_normalized) == 32);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(OracleOutput, flags) == 48);
const _: () = assert!(std::mem::size_of::<glam::UVec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct SkinningOracleParams {
    pub palette: ImmutableAddr<SkinJoint>,
    pub skinning: ImmutableAddr<VertexSkinning>,
    pub cases: ImmutableAddr<OracleCase>,
    pub case_count: u32,
    pub _padding_0: [u8; 4],
}

impl GPUWrite for SkinningOracleParams {}
const _: () = assert!(std::mem::size_of::<SkinningOracleParams>() == 32);
const _: () = assert!(std::mem::offset_of!(SkinningOracleParams, palette) == 0);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<SkinJoint>>() == 8);
const _: () = assert!(std::mem::offset_of!(SkinningOracleParams, skinning) == 8);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<VertexSkinning>>() == 8);
const _: () = assert!(std::mem::offset_of!(SkinningOracleParams, cases) == 16);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<OracleCase>>() == 8);
const _: () = assert!(std::mem::offset_of!(SkinningOracleParams, case_count) == 24);
const _: () = assert!(std::mem::size_of::<u32>() == 4);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct OracleCase {
    pub position: glam::Vec4,
    pub normal: glam::Vec4,
    pub raw_vector: glam::Vec4,
}

impl GPUWrite for OracleCase {}
const _: () = assert!(std::mem::size_of::<OracleCase>() == 48);
const _: () = assert!(std::mem::offset_of!(OracleCase, position) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(OracleCase, normal) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(OracleCase, raw_vector) == 32);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<SkinningOracleParams>,
}

impl GPURead for OracleOutput {
    const GPU_SIZE: usize = 64;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for OracleOutput"
        );

        Ok(Self {
            position: GPURead::read_gpu(&bytes[0..16])?,
            normal: GPURead::read_gpu(&bytes[16..32])?,
            safe_normalized: GPURead::read_gpu(&bytes[32..48])?,
            flags: GPURead::read_gpu(&bytes[48..64])?,
        })
    }
}

impl GPURead for OracleCase {
    const GPU_SIZE: usize = 48;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for OracleCase"
        );

        Ok(Self {
            position: GPURead::read_gpu(&bytes[0..16])?,
            normal: GPURead::read_gpu(&bytes[16..32])?,
            raw_vector: GPURead::read_gpu(&bytes[32..48])?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SkinningOraclePushBindings {
    pub output: BufferBinding<OracleOutput>,
}

impl GraphParamBindingSet for SkinningOraclePushBindings {
    type Pending = PendingParamBindings<Self>;

    fn pending() -> Self::Pending {
        Self::Pending::new()
    }
}

impl GraphBindingSet for SkinningOraclePushBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::Buffer(self.output.erased()));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct SkinningOraclePushInput {
    pub output: BufferBinding<OracleOutput>,
}

impl GraphShaderParams for SkinningOraclePush {
    type Data = ();
    type Bindings = SkinningOraclePushBindings;
    type Input = SkinningOraclePushInput;

    fn input(_data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            output: bindings.output,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            output: resolver.buf(input.output),
        }
    }
}

impl GraphBindingSet for SkinningOraclePushInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::Buffer(self.output.erased()));
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SkinningOracleParamsData {
    pub case_count: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct SkinningOracleParamsBindings {
    pub palette: ImmutableBufferBinding<SkinJoint>,
    pub skinning: ImmutableBufferBinding<VertexSkinning>,
    pub cases: ImmutableBufferBinding<OracleCase>,
}

impl GraphParamBindingSet for SkinningOracleParamsBindings {
    type Pending = PendingParamBindings<Self>;

    fn pending() -> Self::Pending {
        Self::Pending::new()
    }
}

impl GraphBindingSet for SkinningOracleParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::Buffer(self.palette.erased()));
        f(GraphBinding::Buffer(self.skinning.erased()));
        f(GraphBinding::Buffer(self.cases.erased()));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct SkinningOracleParamsInput {
    pub palette: ImmutableBufferBinding<SkinJoint>,
    pub skinning: ImmutableBufferBinding<VertexSkinning>,
    pub cases: ImmutableBufferBinding<OracleCase>,
    pub case_count: u32,
}

impl GraphShaderParams for SkinningOracleParams {
    type Data = SkinningOracleParamsData;
    type Bindings = SkinningOracleParamsBindings;
    type Input = SkinningOracleParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            case_count: data.case_count,
            palette: bindings.palette,
            skinning: bindings.skinning,
            cases: bindings.cases,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            palette: resolver.immutable_buf(input.palette),
            skinning: resolver.immutable_buf(input.skinning),
            cases: resolver.immutable_buf(input.cases),
            case_count: input.case_count,
            _padding_0: Default::default(),
        }
    }
}

impl GraphBindingSet for SkinningOracleParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::Buffer(self.palette.erased()));
        f(GraphBinding::Buffer(self.skinning.erased()));
        f(GraphBinding::Buffer(self.cases.erased()));
    }
}

impl mltrs::renderer::render_graph::PushConstantBlock for SkinningOraclePush {}
// 128 bytes is the vulkan-guaranteed maxPushConstantsSize
const _: () = assert!(std::mem::size_of::<SkinningOraclePush>() <= 128);

pub const WORKGROUP_SIZE: [u32; 3] = [1, 1, 1];

#[derive(Clone)]
pub struct Shader {
    pub reflection_json: ComputeReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/skinning_oracle.comp.json"
        ));

        let reflection_json: ComputeReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> ComputePipelineConfig<'a, PushBlock<SkinningOraclePush>> {
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
            "/shaders/compiled/skinning_oracle.comp.spv"
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
