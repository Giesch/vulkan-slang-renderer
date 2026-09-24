// GENERATED FILE (do not edit directly)

//! generated from slang shader: toon_link_modern.shader.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use ash::vk;
use facet::Facet;
use serde::Serialize;

pub use super::mltrs::MVPMatrices;
pub use super::skinning::{SkinJoint, VertexSkinning};
#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
use mltrs::renderer::render_graph::GPUWrite;
#[allow(unused)]
use mltrs::renderer::vertex_description::{NoVertex, VertexDescription};
use mltrs::renderer::*;
use mltrs::shaders::atlas::{PrecompiledShader, PrecompiledShaders, ShaderAtlasEntry};
use mltrs::shaders::json::{ReflectedPipelineLayout, ReflectionJson};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default, Facet)]
#[repr(u32)]
// variant names come from the shader author, so clippy's shared-prefix lint
// would otherwise force renaming away from the slang spelling
#[allow(clippy::enum_variant_names)]
pub enum ModernRamp {
    #[default]
    Analytic = 0,
    Texture = 1,
}

const _: () = assert!(std::mem::size_of::<ModernRamp>() == 4);

impl From<ModernRamp> for u32 {
    fn from(value: ModernRamp) -> u32 {
        value as u32
    }
}

// A repr(int) enum holding a value outside its declared variants is undefined
// behavior. Data flows CPU -> GPU here, so the CPU never materializes a value it
// did not construct; any future readback must come back through this TryFrom,
// never a transmute or an `as` cast into the enum.
impl TryFrom<u32> for ModernRamp {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            0 => Ok(Self::Analytic),
            1 => Ok(Self::Texture),
            other => Err(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default, Facet)]
#[repr(u32)]
// variant names come from the shader author, so clippy's shared-prefix lint
// would otherwise force renaming away from the slang spelling
#[allow(clippy::enum_variant_names)]
pub enum ModernDiagnostic {
    #[default]
    Final = 0,
    WorldNormals = 1,
    Uv0 = 2,
    NDotL = 3,
    BandOnly = 4,
    AlbedoOnly = 5,
}

const _: () = assert!(std::mem::size_of::<ModernDiagnostic>() == 4);

impl From<ModernDiagnostic> for u32 {
    fn from(value: ModernDiagnostic) -> u32 {
        value as u32
    }
}

// A repr(int) enum holding a value outside its declared variants is undefined
// behavior. Data flows CPU -> GPU here, so the CPU never materializes a value it
// did not construct; any future readback must come back through this TryFrom,
// never a transmute or an `as` cast into the enum.
impl TryFrom<u32> for ModernDiagnostic {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            0 => Ok(Self::Final),
            1 => Ok(Self::WorldNormals),
            2 => Ok(Self::Uv0),
            3 => Ok(Self::NDotL),
            4 => Ok(Self::BandOnly),
            5 => Ok(Self::AlbedoOnly),
            other => Err(other),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct ModernMultiDraw {
    pub individual_draws: ImmutableAddr<ModernIndividualDraw>,
}

impl GPUWrite for ModernMultiDraw {}
const _: () = assert!(std::mem::size_of::<ModernMultiDraw>() == 8);
const _: () = assert!(std::mem::offset_of!(ModernMultiDraw, individual_draws) == 0);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<ModernIndividualDraw>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct ModernIndividualDraw {
    pub material: ImmutableAddr<ModernMaterial>,
}

impl GPUWrite for ModernIndividualDraw {}
const _: () = assert!(std::mem::size_of::<ModernIndividualDraw>() == 8);
const _: () = assert!(std::mem::offset_of!(ModernIndividualDraw, material) == 0);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<ModernMaterial>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct ModernMaterial {
    pub albedo: BindlessHandle<Sampler2D>,
    pub pupil: BindlessHandle<Sampler2D>,
    pub pupil_offset: glam::Vec2,
    pub has_pupil: u32,
    pub lighting_mix: f32,
}

impl GPUWrite for ModernMaterial {}
const _: () = assert!(std::mem::size_of::<ModernMaterial>() == 32);
const _: () = assert!(std::mem::offset_of!(ModernMaterial, albedo) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(ModernMaterial, pupil) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(ModernMaterial, pupil_offset) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(ModernMaterial, has_pupil) == 24);
const _: () = assert!(std::mem::size_of::<u32>() == 4);
const _: () = assert!(std::mem::offset_of!(ModernMaterial, lighting_mix) == 28);
const _: () = assert!(std::mem::size_of::<f32>() == 4);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct ModernParams {
    pub mvp: MVPMatrices,
    pub main_direction: glam::Vec4,
    pub secondary_direction: glam::Vec4,
    pub shadow_color: glam::Vec4,
    pub lit_color: glam::Vec4,
    pub secondary_color: glam::Vec4,
    pub controls: glam::Vec4,
    pub ramp_texture: BindlessHandle<Sampler2D>,
    pub ramp: ModernRamp,
    pub diagnostic: ModernDiagnostic,
    pub palette: ImmutableAddr<SkinJoint>,
    pub skinning: ImmutableAddr<VertexSkinning>,
}

impl GPUWrite for ModernParams {}
const _: () = assert!(std::mem::size_of::<ModernParams>() == 320);
const _: () = assert!(std::mem::offset_of!(ModernParams, mvp) == 0);
const _: () = assert!(std::mem::size_of::<MVPMatrices>() == 192);
const _: () = assert!(std::mem::offset_of!(ModernParams, main_direction) == 192);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(ModernParams, secondary_direction) == 208);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(ModernParams, shadow_color) == 224);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(ModernParams, lit_color) == 240);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(ModernParams, secondary_color) == 256);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(ModernParams, controls) == 272);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(ModernParams, ramp_texture) == 288);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(ModernParams, ramp) == 296);
const _: () = assert!(std::mem::size_of::<ModernRamp>() == 4);
const _: () = assert!(std::mem::offset_of!(ModernParams, diagnostic) == 300);
const _: () = assert!(std::mem::size_of::<ModernDiagnostic>() == 4);
const _: () = assert!(std::mem::offset_of!(ModernParams, palette) == 304);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<SkinJoint>>() == 8);
const _: () = assert!(std::mem::offset_of!(ModernParams, skinning) == 312);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<VertexSkinning>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct ModernVertex {
    pub position: glam::Vec3,
    pub normal: glam::Vec3,
    pub uv0: glam::Vec2,
}

impl GPUWrite for ModernVertex {}

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<ModernParams>,
}

impl GPURead for ModernRamp {
    const GPU_SIZE: usize = 4;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        let tag = <u32 as GPURead>::read_gpu(bytes)?;

        Self::try_from(tag).map_err(|tag| anyhow::anyhow!("invalid ModernRamp tag: {tag}"))
    }
}

impl GPURead for ModernDiagnostic {
    const GPU_SIZE: usize = 4;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        let tag = <u32 as GPURead>::read_gpu(bytes)?;

        Self::try_from(tag).map_err(|tag| anyhow::anyhow!("invalid ModernDiagnostic tag: {tag}"))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModernMultiDrawBindings {
    pub individual_draws: ImmutableBufferBinding<ModernIndividualDraw>,
}

impl GraphParamBindingSet for ModernMultiDrawBindings {
    type Pending = PendingParamBindings<Self>;

    fn pending() -> Self::Pending {
        Self::Pending::new()
    }
}

impl GraphBindingSet for ModernMultiDrawBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::Buffer(self.individual_draws.erased()));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct ModernMultiDrawInput {
    pub individual_draws: ImmutableBufferBinding<ModernIndividualDraw>,
}

impl GraphShaderParams for ModernMultiDraw {
    type Data = ();
    type Bindings = ModernMultiDrawBindings;
    type Input = ModernMultiDrawInput;

    fn input(_data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            individual_draws: bindings.individual_draws,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            individual_draws: resolver.immutable_buf(input.individual_draws),
        }
    }
}

impl GraphBindingSet for ModernMultiDrawInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::Buffer(self.individual_draws.erased()));
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModernParamsData {
    pub mvp: MVPMatrices,
    pub main_direction: glam::Vec4,
    pub secondary_direction: glam::Vec4,
    pub shadow_color: glam::Vec4,
    pub lit_color: glam::Vec4,
    pub secondary_color: glam::Vec4,
    pub controls: glam::Vec4,
    pub ramp: ModernRamp,
    pub diagnostic: ModernDiagnostic,
}

#[derive(Debug, Clone, Copy)]
pub struct ModernParamsBindings {
    pub ramp_texture: SampledTexBinding,
    pub palette: ImmutableBufferBinding<SkinJoint>,
    pub skinning: ImmutableBufferBinding<VertexSkinning>,
}

impl GraphParamBindingSet for ModernParamsBindings {
    type Pending = PendingParamBindings<Self>;

    fn pending() -> Self::Pending {
        Self::Pending::new()
    }
}

impl GraphBindingSet for ModernParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.ramp_texture));
        f(GraphBinding::Buffer(self.palette.erased()));
        f(GraphBinding::Buffer(self.skinning.erased()));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct ModernParamsInput {
    pub mvp: MVPMatrices,
    pub main_direction: glam::Vec4,
    pub secondary_direction: glam::Vec4,
    pub shadow_color: glam::Vec4,
    pub lit_color: glam::Vec4,
    pub secondary_color: glam::Vec4,
    pub controls: glam::Vec4,
    pub ramp_texture: SampledTexBinding,
    pub ramp: ModernRamp,
    pub diagnostic: ModernDiagnostic,
    pub palette: ImmutableBufferBinding<SkinJoint>,
    pub skinning: ImmutableBufferBinding<VertexSkinning>,
}

impl GraphShaderParams for ModernParams {
    type Data = ModernParamsData;
    type Bindings = ModernParamsBindings;
    type Input = ModernParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            mvp: data.mvp,
            main_direction: data.main_direction,
            secondary_direction: data.secondary_direction,
            shadow_color: data.shadow_color,
            lit_color: data.lit_color,
            secondary_color: data.secondary_color,
            controls: data.controls,
            ramp: data.ramp,
            diagnostic: data.diagnostic,
            ramp_texture: bindings.ramp_texture,
            palette: bindings.palette,
            skinning: bindings.skinning,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            mvp: input.mvp,
            main_direction: input.main_direction,
            secondary_direction: input.secondary_direction,
            shadow_color: input.shadow_color,
            lit_color: input.lit_color,
            secondary_color: input.secondary_color,
            controls: input.controls,
            ramp_texture: resolver.sampled_tex(input.ramp_texture),
            ramp: input.ramp,
            diagnostic: input.diagnostic,
            palette: resolver.immutable_buf(input.palette),
            skinning: resolver.immutable_buf(input.skinning),
        }
    }
}

impl GraphBindingSet for ModernParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.ramp_texture));
        f(GraphBinding::Buffer(self.palette.erased()));
        f(GraphBinding::Buffer(self.skinning.erased()));
    }
}

impl mltrs::renderer::render_graph::PushConstantBlock for ModernMultiDraw {}
// 128 bytes is the vulkan-guaranteed maxPushConstantsSize
const _: () = assert!(std::mem::size_of::<ModernMultiDraw>() <= 128);

// the shared vertex layout rule (mltrs_slang_reflection::json::vertex_layout)
// must agree with this struct's Rust layout
const _: () = assert!(std::mem::size_of::<ModernVertex>() == 32);
const _: () = assert!(std::mem::offset_of!(ModernVertex, position) == 0);
const _: () = assert!(std::mem::offset_of!(ModernVertex, normal) == 12);
const _: () = assert!(std::mem::offset_of!(ModernVertex, uv0) == 24);

impl VertexDescription for ModernVertex {
    fn binding_descriptions() -> Vec<ash::vk::VertexInputBindingDescription> {
        let binding_description = ash::vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<Self>() as u32)
            .input_rate(ash::vk::VertexInputRate::VERTEX);

        vec![binding_description]
    }

    fn attribute_descriptions() -> Vec<ash::vk::VertexInputAttributeDescription> {
        vec![
            ash::vk::VertexInputAttributeDescription::default()
                .offset(std::mem::offset_of!(ModernVertex, position) as u32)
                .format(ash::vk::Format::R32G32B32_SFLOAT)
                .binding(0)
                .location(0),
            ash::vk::VertexInputAttributeDescription::default()
                .offset(std::mem::offset_of!(ModernVertex, normal) as u32)
                .format(ash::vk::Format::R32G32B32_SFLOAT)
                .binding(0)
                .location(1),
            ash::vk::VertexInputAttributeDescription::default()
                .offset(std::mem::offset_of!(ModernVertex, uv0) as u32)
                .format(ash::vk::Format::R32G32_SFLOAT)
                .binding(0)
                .location(2),
        ]
    }
}

#[derive(Clone)]
pub struct Shader {
    pub reflection_json: ReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/toon_link_modern.json"
        ));

        let reflection_json: ReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> IndexedPipelineConfig<'a, ModernVertex, PushBlock<ModernMultiDraw>> {
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

        PipelineConfigBuilder {
            shader: Box::new(self.clone()),
            texture_handles,
            uniform_buffer_handles,
            storage_texture_handles,
        }
        .build_indexed()
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
            "/shaders/compiled/toon_link_modern.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/toon_link_modern.frag.spv"
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
        ModernVertex::binding_descriptions()
    }

    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription> {
        ModernVertex::attribute_descriptions()
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

    fn reflection_json(&self) -> &ReflectionJson {
        &self.reflection_json
    }
}
