// GENERATED FILE (do not edit directly)

//! generated from slang shader: serenity_crt.shader.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use ash::vk;
use serde::Serialize;

use mltrs::renderer::gpu_write::GPUWrite;
#[allow(unused)]
use mltrs::renderer::vertex_description::{NoVertex, VertexDescription};
use mltrs::renderer::*;
use mltrs::shaders::atlas::{PrecompiledShader, PrecompiledShaders, ShaderAtlasEntry};
use mltrs::shaders::json::{ReflectedPipelineLayout, ReflectionJson};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct SerenityCRTParams {
    pub tex: BindlessHandle<Sampler2D>,
    pub resolution: glam::Vec2,
    pub scanline_intensity: f32,
    pub scanline_count: f32,
    pub time: f32,
    pub y_offset: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub rgb_shift: f32,
    pub adaptive_intensity: f32,
    pub vignette_strength: f32,
    pub curvature: f32,
    pub flicker_strength: f32,
    pub _padding_0: [u8; 8],
}

impl GPUWrite for SerenityCRTParams {}
const _: () = assert!(std::mem::size_of::<SerenityCRTParams>() == 80);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, tex) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, resolution) == 8);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, scanline_intensity) == 16);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, scanline_count) == 20);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, time) == 24);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, y_offset) == 28);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, brightness) == 32);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, contrast) == 36);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, saturation) == 40);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, bloom_intensity) == 44);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, bloom_threshold) == 48);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, rgb_shift) == 52);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, adaptive_intensity) == 56);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, vignette_strength) == 60);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, curvature) == 64);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SerenityCRTParams, flicker_strength) == 68);
const _: () = assert!(std::mem::size_of::<f32>() == 4);

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<SerenityCRTParams>,
}

#[derive(Debug, Clone, Copy)]
pub struct SerenityCRTParamsData {
    pub resolution: glam::Vec2,
    pub scanline_intensity: f32,
    pub scanline_count: f32,
    pub time: f32,
    pub y_offset: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub rgb_shift: f32,
    pub adaptive_intensity: f32,
    pub vignette_strength: f32,
    pub curvature: f32,
    pub flicker_strength: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SerenityCRTParamsBindings {
    pub tex: SampledTexBinding,
}

impl GraphBindingSet for SerenityCRTParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.tex));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct SerenityCRTParamsInput {
    pub tex: SampledTexBinding,
    pub resolution: glam::Vec2,
    pub scanline_intensity: f32,
    pub scanline_count: f32,
    pub time: f32,
    pub y_offset: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub rgb_shift: f32,
    pub adaptive_intensity: f32,
    pub vignette_strength: f32,
    pub curvature: f32,
    pub flicker_strength: f32,
}

impl GraphShaderParams for SerenityCRTParams {
    type Data = SerenityCRTParamsData;
    type Bindings = SerenityCRTParamsBindings;
    type Input = SerenityCRTParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            resolution: data.resolution,
            scanline_intensity: data.scanline_intensity,
            scanline_count: data.scanline_count,
            time: data.time,
            y_offset: data.y_offset,
            brightness: data.brightness,
            contrast: data.contrast,
            saturation: data.saturation,
            bloom_intensity: data.bloom_intensity,
            bloom_threshold: data.bloom_threshold,
            rgb_shift: data.rgb_shift,
            adaptive_intensity: data.adaptive_intensity,
            vignette_strength: data.vignette_strength,
            curvature: data.curvature,
            flicker_strength: data.flicker_strength,
            tex: bindings.tex,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            tex: resolver.sampled_tex(input.tex),
            resolution: input.resolution,
            scanline_intensity: input.scanline_intensity,
            scanline_count: input.scanline_count,
            time: input.time,
            y_offset: input.y_offset,
            brightness: input.brightness,
            contrast: input.contrast,
            saturation: input.saturation,
            bloom_intensity: input.bloom_intensity,
            bloom_threshold: input.bloom_threshold,
            rgb_shift: input.rgb_shift,
            adaptive_intensity: input.adaptive_intensity,
            vignette_strength: input.vignette_strength,
            curvature: input.curvature,
            flicker_strength: input.flicker_strength,
            _padding_0: Default::default(),
        }
    }
}

impl GraphBindingSet for SerenityCRTParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.tex));
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
            "/shaders/compiled/serenity_crt.json"
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
            "/shaders/compiled/serenity_crt.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/serenity_crt.frag.spv"
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
