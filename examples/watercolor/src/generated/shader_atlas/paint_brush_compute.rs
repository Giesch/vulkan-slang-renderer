// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: paint_brush.compute.slang

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
pub struct BrushParams {
    pub wet_mask: BindlessHandle<RwTexture2D>,
    pub pressure: BindlessHandle<RwTexture2D>,
    pub pigment_0_3: BindlessHandle<RwTexture2D>,
    pub pigment_4_7: BindlessHandle<RwTexture2D>,
    pub pigment_8_11: BindlessHandle<RwTexture2D>,
    pub saturation: BindlessHandle<RwTexture2D>,
    pub point_count: u32,
    pub brush_radius: f32,
    pub brush_opacity: f32,
    pub brush_pressure: f32,
    pub pigment_color_0_3: glam::Vec4,
    pub pigment_color_4_7: glam::Vec4,
    pub pigment_color_8_11: glam::Vec4,
    pub canvas_size: glam::Vec2,
    pub stroke_points: ReadAddr<StrokePoint>,
}

impl GPUWrite for BrushParams {}
const _: () = assert!(std::mem::size_of::<BrushParams>() == 128);
const _: () = assert!(std::mem::offset_of!(BrushParams, wet_mask) == 0);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BrushParams, pressure) == 8);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BrushParams, pigment_0_3) == 16);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BrushParams, pigment_4_7) == 24);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BrushParams, pigment_8_11) == 32);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BrushParams, saturation) == 40);
const _: () = assert!(std::mem::size_of::<BindlessHandle<RwTexture2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(BrushParams, point_count) == 48);
const _: () = assert!(std::mem::size_of::<u32>() == 4);
const _: () = assert!(std::mem::offset_of!(BrushParams, brush_radius) == 52);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(BrushParams, brush_opacity) == 56);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(BrushParams, brush_pressure) == 60);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(BrushParams, pigment_color_0_3) == 64);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(BrushParams, pigment_color_4_7) == 80);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(BrushParams, pigment_color_8_11) == 96);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(BrushParams, canvas_size) == 112);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(BrushParams, stroke_points) == 120);
const _: () = assert!(std::mem::size_of::<ReadAddr<StrokePoint>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(8))]
pub struct StrokePoint {
    pub position: glam::Vec2,
}

impl GPUWrite for StrokePoint {}
const _: () = assert!(std::mem::size_of::<StrokePoint>() == 8);
const _: () = assert!(std::mem::offset_of!(StrokePoint, position) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);

pub struct Resources<'a> {
    pub brush_params_buffer: &'a UniformBufferHandle<BrushParams>,
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
            "/shaders/compiled/paint_brush.comp.json"
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
            RawUniformBufferHandle::from_typed(resources.brush_params_buffer),
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
            "/shaders/compiled/paint_brush.comp.spv"
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
