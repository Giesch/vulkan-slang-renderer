// GENERATED FILE (do not edit directly)

//! generated from slang compute shader: recipes.compute.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

pub use super::shared::Solution;
use mltrs::renderer::gpu_write::GPUWrite;
use mltrs::renderer::*;
use mltrs::shaders::atlas::{ComputeShaderAtlasEntry, PrecompiledShader};
use mltrs::shaders::json::{ComputeReflectionJson, ReflectedPipelineLayout};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct IngredientParams {
    pub weights: [glam::IVec4; 4],
    pub calories: glam::IVec4,
    pub solution: Addr<Solution>,
    pub _padding_0: [u8; 8],
}

impl GPUWrite for IngredientParams {}
const _: () = assert!(std::mem::size_of::<IngredientParams>() == 96);
const _: () = assert!(std::mem::offset_of!(IngredientParams, weights) == 0);
const _: () = assert!(std::mem::size_of::<[glam::IVec4; 4]>() == 64);
const _: () = assert!(std::mem::offset_of!(IngredientParams, calories) == 64);
const _: () = assert!(std::mem::size_of::<glam::IVec4>() == 16);
const _: () = assert!(std::mem::offset_of!(IngredientParams, solution) == 80);
const _: () = assert!(std::mem::size_of::<Addr<Solution>>() == 8);

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<IngredientParams>,
}

pub const WORKGROUP_SIZE: [u32; 3] = [10, 10, 10];

#[derive(Clone)]
pub struct Shader {
    pub reflection_json: ComputeReflectionJson,
}

impl Shader {
    pub fn init() -> Self {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/recipes.comp.json"
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
            "/shaders/compiled/recipes.comp.spv"
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
