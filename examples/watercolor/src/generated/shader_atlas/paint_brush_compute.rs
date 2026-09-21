// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: paint_brush.compute.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
use mltrs::renderer::render_graph::GPUWrite;
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

impl GPURead for StrokePoint {
    const GPU_SIZE: usize = 8;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for StrokePoint"
        );

        Ok(Self {
            position: GPURead::read_gpu(&bytes[0..8])?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BrushParamsData {
    pub point_count: u32,
    pub brush_radius: f32,
    pub brush_opacity: f32,
    pub brush_pressure: f32,
    pub pigment_color_0_3: glam::Vec4,
    pub pigment_color_4_7: glam::Vec4,
    pub pigment_color_8_11: glam::Vec4,
    pub canvas_size: glam::Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct BrushParamsBindings {
    pub wet_mask: StorageTexBinding,
    pub pressure: StorageTexBinding,
    pub pigment_0_3: StorageTexBinding,
    pub pigment_4_7: StorageTexBinding,
    pub pigment_8_11: StorageTexBinding,
    pub saturation: StorageTexBinding,
    pub stroke_points: ReadBufferBinding<StrokePoint>,
}

impl GraphParamBindingSet for BrushParamsBindings {
    type Pending = PendingParamBindings<Self>;

    fn pending() -> Self::Pending {
        Self::Pending::new()
    }
}

impl GraphBindingSet for BrushParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::StorageTex(self.wet_mask));
        f(GraphBinding::StorageTex(self.pressure));
        f(GraphBinding::StorageTex(self.pigment_0_3));
        f(GraphBinding::StorageTex(self.pigment_4_7));
        f(GraphBinding::StorageTex(self.pigment_8_11));
        f(GraphBinding::StorageTex(self.saturation));
        f(GraphBinding::Buffer(self.stroke_points.erased()));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct BrushParamsInput {
    pub wet_mask: StorageTexBinding,
    pub pressure: StorageTexBinding,
    pub pigment_0_3: StorageTexBinding,
    pub pigment_4_7: StorageTexBinding,
    pub pigment_8_11: StorageTexBinding,
    pub saturation: StorageTexBinding,
    pub point_count: u32,
    pub brush_radius: f32,
    pub brush_opacity: f32,
    pub brush_pressure: f32,
    pub pigment_color_0_3: glam::Vec4,
    pub pigment_color_4_7: glam::Vec4,
    pub pigment_color_8_11: glam::Vec4,
    pub canvas_size: glam::Vec2,
    pub stroke_points: ReadBufferBinding<StrokePoint>,
}

impl GraphShaderParams for BrushParams {
    type Data = BrushParamsData;
    type Bindings = BrushParamsBindings;
    type Input = BrushParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            point_count: data.point_count,
            brush_radius: data.brush_radius,
            brush_opacity: data.brush_opacity,
            brush_pressure: data.brush_pressure,
            pigment_color_0_3: data.pigment_color_0_3,
            pigment_color_4_7: data.pigment_color_4_7,
            pigment_color_8_11: data.pigment_color_8_11,
            canvas_size: data.canvas_size,
            wet_mask: bindings.wet_mask,
            pressure: bindings.pressure,
            pigment_0_3: bindings.pigment_0_3,
            pigment_4_7: bindings.pigment_4_7,
            pigment_8_11: bindings.pigment_8_11,
            saturation: bindings.saturation,
            stroke_points: bindings.stroke_points,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            wet_mask: resolver.storage_tex(input.wet_mask),
            pressure: resolver.storage_tex(input.pressure),
            pigment_0_3: resolver.storage_tex(input.pigment_0_3),
            pigment_4_7: resolver.storage_tex(input.pigment_4_7),
            pigment_8_11: resolver.storage_tex(input.pigment_8_11),
            saturation: resolver.storage_tex(input.saturation),
            point_count: input.point_count,
            brush_radius: input.brush_radius,
            brush_opacity: input.brush_opacity,
            brush_pressure: input.brush_pressure,
            pigment_color_0_3: input.pigment_color_0_3,
            pigment_color_4_7: input.pigment_color_4_7,
            pigment_color_8_11: input.pigment_color_8_11,
            canvas_size: input.canvas_size,
            stroke_points: resolver.read_buf(input.stroke_points),
        }
    }
}

impl GraphBindingSet for BrushParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::StorageTex(self.wet_mask));
        f(GraphBinding::StorageTex(self.pressure));
        f(GraphBinding::StorageTex(self.pigment_0_3));
        f(GraphBinding::StorageTex(self.pigment_4_7));
        f(GraphBinding::StorageTex(self.pigment_8_11));
        f(GraphBinding::StorageTex(self.saturation));
        f(GraphBinding::Buffer(self.stroke_points.erased()));
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
