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
    pub deposit_0_3: BindlessHandle<Sampler2D>,
    pub deposit_4_7: BindlessHandle<Sampler2D>,
    pub deposit_8_11: BindlessHandle<Sampler2D>,
    pub paper_height: BindlessHandle<Sampler2D>,
    pub wet_mask: BindlessHandle<Sampler2D>,
    pub texel_size: glam::Vec2,
    pub debug_view: DebugView,
    pub canvas_aspect: f32,
    pub window_aspect: f32,
    pub _padding_0: [u8; 4],
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
const _: () = assert!(std::mem::size_of::<DisplayParams>() == 448);
const _: () = assert!(std::mem::offset_of!(DisplayParams, deposit_0_3) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(DisplayParams, deposit_4_7) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(DisplayParams, deposit_8_11) == 16);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(DisplayParams, paper_height) == 24);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(DisplayParams, wet_mask) == 32);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(DisplayParams, texel_size) == 40);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(DisplayParams, debug_view) == 48);
const _: () = assert!(std::mem::size_of::<DebugView>() == 4);
const _: () = assert!(std::mem::offset_of!(DisplayParams, canvas_aspect) == 52);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(DisplayParams, window_aspect) == 56);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment0) == 64);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment1) == 96);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment2) == 128);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment3) == 160);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment4) == 192);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment5) == 224);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment6) == 256);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment7) == 288);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment8) == 320);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment9) == 352);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment10) == 384);
const _: () = assert!(std::mem::size_of::<PigmentKM>() == 32);
const _: () = assert!(std::mem::offset_of!(DisplayParams, pigment11) == 416);
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
    pub display_params_buffer: &'a UniformBufferHandle<DisplayParams>,
}

#[derive(Debug, Clone, Copy)]
pub struct DisplayParamsData {
    pub texel_size: glam::Vec2,
    pub debug_view: DebugView,
    pub canvas_aspect: f32,
    pub window_aspect: f32,
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

#[derive(Debug, Clone, Copy)]
pub struct DisplayParamsBindings {
    pub deposit_0_3: SampledTexBinding,
    pub deposit_4_7: SampledTexBinding,
    pub deposit_8_11: SampledTexBinding,
    pub paper_height: SampledTexBinding,
    pub wet_mask: SampledTexBinding,
}

impl GraphBindingSet for DisplayParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.deposit_0_3));
        f(GraphBinding::SampledTex(self.deposit_4_7));
        f(GraphBinding::SampledTex(self.deposit_8_11));
        f(GraphBinding::SampledTex(self.paper_height));
        f(GraphBinding::SampledTex(self.wet_mask));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct DisplayParamsInput {
    pub deposit_0_3: SampledTexBinding,
    pub deposit_4_7: SampledTexBinding,
    pub deposit_8_11: SampledTexBinding,
    pub paper_height: SampledTexBinding,
    pub wet_mask: SampledTexBinding,
    pub texel_size: glam::Vec2,
    pub debug_view: DebugView,
    pub canvas_aspect: f32,
    pub window_aspect: f32,
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

impl GraphShaderParams for DisplayParams {
    type Data = DisplayParamsData;
    type Bindings = DisplayParamsBindings;
    type Input = DisplayParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            texel_size: data.texel_size,
            debug_view: data.debug_view,
            canvas_aspect: data.canvas_aspect,
            window_aspect: data.window_aspect,
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
            deposit_0_3: bindings.deposit_0_3,
            deposit_4_7: bindings.deposit_4_7,
            deposit_8_11: bindings.deposit_8_11,
            paper_height: bindings.paper_height,
            wet_mask: bindings.wet_mask,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            deposit_0_3: resolver.sampled_tex(input.deposit_0_3),
            deposit_4_7: resolver.sampled_tex(input.deposit_4_7),
            deposit_8_11: resolver.sampled_tex(input.deposit_8_11),
            paper_height: resolver.sampled_tex(input.paper_height),
            wet_mask: resolver.sampled_tex(input.wet_mask),
            texel_size: input.texel_size,
            debug_view: input.debug_view,
            canvas_aspect: input.canvas_aspect,
            window_aspect: input.window_aspect,
            _padding_0: Default::default(),
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

impl GraphBindingSet for DisplayParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.deposit_0_3));
        f(GraphBinding::SampledTex(self.deposit_4_7));
        f(GraphBinding::SampledTex(self.deposit_8_11));
        f(GraphBinding::SampledTex(self.paper_height));
        f(GraphBinding::SampledTex(self.wet_mask));
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
