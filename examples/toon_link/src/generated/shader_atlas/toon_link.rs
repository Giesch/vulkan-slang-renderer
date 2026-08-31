// GENERATED FILE (do not edit directly)

//! generated from slang shader: toon_link.shader.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use ash::vk;
use facet::Facet;
use serde::Serialize;

pub use super::mltrs::MVPMatrices;
pub use super::tev::{GXAlphaCompare, GXLights, GXTevColorOverride, TevParams};
use mltrs::renderer::gpu_write::GPUWrite;
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
pub enum DebugMode {
    #[default]
    FinalTev = 0,
    WorldNormals = 1,
    Uv0 = 2,
    TevAlpha = 3,
    RasterColor0 = 4,
    Texgen1Coord = 5,
    RawTex0 = 6,
    RawTex1 = 7,
    ChannelPerPixel = 8,
    IdentityTexMtx = 9,
}

const _: () = assert!(std::mem::size_of::<DebugMode>() == 4);

impl From<DebugMode> for u32 {
    fn from(value: DebugMode) -> u32 {
        value as u32
    }
}

// A repr(int) enum holding a value outside its declared variants is undefined
// behavior. Data flows CPU -> GPU here, so the CPU never materializes a value it
// did not construct; any future readback must come back through this TryFrom,
// never a transmute or an `as` cast into the enum.
impl TryFrom<u32> for DebugMode {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            0 => Ok(Self::FinalTev),
            1 => Ok(Self::WorldNormals),
            2 => Ok(Self::Uv0),
            3 => Ok(Self::TevAlpha),
            4 => Ok(Self::RasterColor0),
            5 => Ok(Self::Texgen1Coord),
            6 => Ok(Self::RawTex0),
            7 => Ok(Self::RawTex1),
            8 => Ok(Self::ChannelPerPixel),
            9 => Ok(Self::IdentityTexMtx),
            other => Err(other),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct MultiDraw {
    pub individual_draws: ImmutableAddr<IndividualDraw>,
}

impl GPUWrite for MultiDraw {}
const _: () = assert!(std::mem::size_of::<MultiDraw>() == 8);
const _: () = assert!(std::mem::offset_of!(MultiDraw, individual_draws) == 0);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<IndividualDraw>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct IndividualDraw {
    pub material: ImmutableAddr<Material>,
}

impl GPUWrite for IndividualDraw {}
const _: () = assert!(std::mem::size_of::<IndividualDraw>() == 8);
const _: () = assert!(std::mem::offset_of!(IndividualDraw, material) == 0);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<Material>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct Material {
    pub tex0: BindlessHandle<Sampler2D>,
    pub tex1: BindlessHandle<Sampler2D>,
    pub tev: TevParams,
    pub alpha_compare: GXAlphaCompare,
    pub _padding_0: [u8; 12],
}

impl GPUWrite for Material {}
const _: () = assert!(std::mem::size_of::<Material>() == 1312);
const _: () = assert!(std::mem::offset_of!(Material, tex0) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Material, tex1) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(Material, tev) == 16);
const _: () = assert!(std::mem::size_of::<TevParams>() == 1264);
const _: () = assert!(std::mem::offset_of!(Material, alpha_compare) == 1280);
const _: () = assert!(std::mem::size_of::<GXAlphaCompare>() == 20);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct ToonLinkParams {
    pub mvp: MVPMatrices,
    pub lights: GXLights,
    pub env: GXTevColorOverride,
    pub debug_mode: DebugMode,
    pub _padding_0: [u8; 12],
}

impl GPUWrite for ToonLinkParams {}
const _: () = assert!(std::mem::size_of::<ToonLinkParams>() == 336);
const _: () = assert!(std::mem::offset_of!(ToonLinkParams, mvp) == 0);
const _: () = assert!(std::mem::size_of::<MVPMatrices>() == 192);
const _: () = assert!(std::mem::offset_of!(ToonLinkParams, lights) == 192);
const _: () = assert!(std::mem::size_of::<GXLights>() == 64);
const _: () = assert!(std::mem::offset_of!(ToonLinkParams, env) == 256);
const _: () = assert!(std::mem::size_of::<GXTevColorOverride>() == 64);
const _: () = assert!(std::mem::offset_of!(ToonLinkParams, debug_mode) == 320);
const _: () = assert!(std::mem::size_of::<DebugMode>() == 4);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct Vertex {
    pub position: glam::Vec3,
    pub normal: glam::Vec3,
    pub uv0: glam::Vec2,
}

impl GPUWrite for Vertex {}

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<ToonLinkParams>,
}

impl mltrs::renderer::gpu_write::PushConstantBlock for MultiDraw {}
// 128 bytes is the vulkan-guaranteed maxPushConstantsSize
const _: () = assert!(std::mem::size_of::<MultiDraw>() <= 128);

impl VertexDescription for Vertex {
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
                .offset(std::mem::offset_of!(Vertex, position) as u32)
                .format(ash::vk::Format::R32G32B32_SFLOAT)
                .binding(0)
                .location(0),
            ash::vk::VertexInputAttributeDescription::default()
                .offset(std::mem::offset_of!(Vertex, normal) as u32)
                .format(ash::vk::Format::R32G32B32_SFLOAT)
                .binding(0)
                .location(1),
            ash::vk::VertexInputAttributeDescription::default()
                .offset(std::mem::offset_of!(Vertex, uv0) as u32)
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
            "/shaders/compiled/toon_link.json"
        ));

        let reflection_json: ReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> IndexedPipelineConfig<'a, Vertex, PushBlock<MultiDraw>> {
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
            "/shaders/compiled/toon_link.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/toon_link.frag.spv"
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
        Vertex::binding_descriptions()
    }

    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription> {
        Vertex::attribute_descriptions()
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
