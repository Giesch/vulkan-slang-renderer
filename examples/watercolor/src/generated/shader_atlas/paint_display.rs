// GENERATED FILE (do not edit directly)

//! generated from slang shader: paint_display.shader.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use ash::vk;
use facet::Facet;
use serde::Serialize;

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
pub enum DebugView {
    #[default]
    Pigments = 0,
    WetAreaMask = 1,
}

const _: () = assert!(std::mem::size_of::<DebugView>() == 4);

impl From<DebugView> for u32 {
    fn from(value: DebugView) -> u32 {
        value as u32
    }
}

// A repr(int) enum holding a value outside its declared variants is undefined
// behavior. Data flows CPU -> GPU here, so the CPU never materializes a value it
// did not construct; any future readback must come back through this TryFrom,
// never a transmute or an `as` cast into the enum.
impl TryFrom<u32> for DebugView {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            0 => Ok(Self::Pigments),
            1 => Ok(Self::WetAreaMask),
            other => Err(other),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct DisplayParams {
    pub texel_size: glam::Vec2,
    pub debug_view: DebugView,
    pub canvas_aspect: f32,
    pub window_aspect: f32,
    pub _padding_0: [u8; 12],
    pub pigment0: PigmentKM,
    pub pigment1: PigmentKM,
    pub pigment2: PigmentKM,
    pub pigment3: PigmentKM,
    pub pigment4: PigmentKM,
    pub pigment5: PigmentKM,
    pub pigment6: PigmentKM,
    pub pigment7: PigmentKM,
    pub pigment8: PigmentKM,
    pub pigment9: PigmentKM,
    pub pigment10: PigmentKM,
    pub pigment11: PigmentKM,
}

impl GPUWrite for DisplayParams {}
const _: () = assert!(std::mem::size_of::<DisplayParams>() == 416);
const _: () = assert!(std::mem::offset_of!(DisplayParams, texel_size) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(DisplayParams, debug_view) == 8);
const _: () = assert!(std::mem::size_of::<DebugView>() == 4);
const _: () = assert!(std::mem::offset_of!(DisplayParams, canvas_aspect) == 12);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(DisplayParams, window_aspect) == 16);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment0) == 32);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment1) == 64);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment2) == 96);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment3) == 128);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment4) == 160);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment5) == 192);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment6) == 224);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment7) == 256);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment8) == 288);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment9) == 320);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment10) == 352);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment11) == 384);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct PigmentKM {
    pub absorption: glam::Vec3,
    pub _padding_0: [u8; 4],
    pub scattering: glam::Vec3,
    pub _padding_1: [u8; 4],
}

impl GPUWrite for PigmentKM {}
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(PigmentKM, absorption) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
const _: () = assert!(std::mem::offset_of!(PigmentKM, scattering) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);

pub struct Resources<'a> {
    pub deposit_0_3: &'a TextureHandle,
    pub deposit_4_7: &'a TextureHandle,
    pub deposit_8_11: &'a TextureHandle,
    pub paper_height: &'a TextureHandle,
    pub wet_mask: &'a TextureHandle,
    pub display_params_buffer: &'a UniformBufferHandle<DisplayParams>,
}

#[derive(Clone)]
pub struct Shader {
    pub reflection_json: ReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/paint_display.json"
        ));

        let reflection_json: ReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> PipelineConfig<'a, NoVertex, DrawVertexCount, NoPush> {
        // NOTE each of these must be in descriptor set layout order in the reflection json

        #[rustfmt::skip]
        let texture_handles = vec![
            resources.deposit_0_3,
            resources.deposit_4_7,
            resources.deposit_8_11,
            resources.paper_height,
            resources.wet_mask,
        ];

        #[rustfmt::skip]
        let uniform_buffer_handles = vec![
            RawUniformBufferHandle::from_typed(resources.display_params_buffer),
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
        .build_vertex_count()
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
            "/shaders/compiled/paint_display.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/paint_display.frag.spv"
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
        vec![]
    }

    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription> {
        vec![]
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
