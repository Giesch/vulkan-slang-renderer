// GENERATED FILE (do not edit directly)

//! generated from slang shader: suzanne.shader.slang

use std::ffi::CString;
use std::io::Cursor;

use ash::util::read_spv;
use serde::Serialize;

pub use super::mltrs::MVPMatrices;
#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
use mltrs::renderer::render_graph::GPUWrite;
use mltrs::renderer::*;
use mltrs::shaders::atlas::{PrecompiledShader, PrecompiledShaders, ShaderAtlasEntry};
use mltrs::shaders::json::{ReflectedPipelineLayout, ReflectionJson};

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct SuzanneParams {
    pub mvp: MVPMatrices,
    pub time: f32,
    pub _padding_0: [u8; 4],
    pub texture0: BindlessHandle<Sampler2D>,
    pub texture1: BindlessHandle<Sampler2D>,
    pub texture2: BindlessHandle<Sampler2D>,
    pub vertices: ImmutableAddr<Vertex>,
    pub _padding_1: [u8; 8],
}

impl GPUWrite for SuzanneParams {}
const _: () = assert!(std::mem::size_of::<SuzanneParams>() == 240);
const _: () = assert!(std::mem::offset_of!(SuzanneParams, mvp) == 0);
const _: () = assert!(std::mem::size_of::<MVPMatrices>() == 192);
const _: () = assert!(std::mem::offset_of!(SuzanneParams, time) == 192);
const _: () = assert!(std::mem::size_of::<f32>() == 4);
const _: () = assert!(std::mem::offset_of!(SuzanneParams, texture0) == 200);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(SuzanneParams, texture1) == 208);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(SuzanneParams, texture2) == 216);
const _: () = assert!(std::mem::size_of::<BindlessHandle<Sampler2D>>() == 8);
const _: () = assert!(std::mem::offset_of!(SuzanneParams, vertices) == 224);
const _: () = assert!(std::mem::size_of::<ImmutableAddr<Vertex>>() == 8);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct Vertex {
    pub position: glam::Vec3,
    pub _padding_0: [u8; 4],
    pub color: glam::Vec3,
    pub _padding_1: [u8; 4],
    pub tex_coord: glam::Vec2,
    pub _padding_2: [u8; 8],
}

impl GPUWrite for Vertex {}
const _: () = assert!(std::mem::size_of::<Vertex>() == 48);
const _: () = assert!(std::mem::offset_of!(Vertex, position) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
const _: () = assert!(std::mem::offset_of!(Vertex, color) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
const _: () = assert!(std::mem::offset_of!(Vertex, tex_coord) == 32);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);

pub struct Resources<'a> {
    pub params_buffer: &'a UniformBufferHandle<SuzanneParams>,
}

impl GPURead for Vertex {
    const GPU_SIZE: usize = 48;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for Vertex"
        );

        Ok(Self {
            position: glam::Vec3::from_array(<[f32; 3] as GPURead>::read_gpu(&bytes[0..12])?),
            _padding_0: [0; 4],
            color: glam::Vec3::from_array(<[f32; 3] as GPURead>::read_gpu(&bytes[16..28])?),
            _padding_1: [0; 4],
            tex_coord: GPURead::read_gpu(&bytes[32..40])?,
            _padding_2: [0; 8],
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SuzanneParamsData {
    pub mvp: MVPMatrices,
    pub time: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SuzanneParamsBindings {
    pub texture0: SampledTexBinding,
    pub texture1: SampledTexBinding,
    pub texture2: SampledTexBinding,
    pub vertices: ImmutableBufferBinding<Vertex>,
}

impl GraphParamBindingSet for SuzanneParamsBindings {
    type Pending = PendingParamBindings<Self>;

    fn pending() -> Self::Pending {
        Self::Pending::new()
    }
}

impl GraphBindingSet for SuzanneParamsBindings {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.texture0));
        f(GraphBinding::SampledTex(self.texture1));
        f(GraphBinding::SampledTex(self.texture2));
        f(GraphBinding::Buffer(self.vertices.erased()));
    }
}
/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct SuzanneParamsInput {
    pub mvp: MVPMatrices,
    pub time: f32,
    pub texture0: SampledTexBinding,
    pub texture1: SampledTexBinding,
    pub texture2: SampledTexBinding,
    pub vertices: ImmutableBufferBinding<Vertex>,
}

impl GraphShaderParams for SuzanneParams {
    type Data = SuzanneParamsData;
    type Bindings = SuzanneParamsBindings;
    type Input = SuzanneParamsInput;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            mvp: data.mvp,
            time: data.time,
            texture0: bindings.texture0,
            texture1: bindings.texture1,
            texture2: bindings.texture2,
            vertices: bindings.vertices,
        }
    }

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
        Self {
            mvp: input.mvp,
            time: input.time,
            _padding_0: Default::default(),
            texture0: resolver.sampled_tex(input.texture0),
            texture1: resolver.sampled_tex(input.texture1),
            texture2: resolver.sampled_tex(input.texture2),
            vertices: resolver.immutable_buf(input.vertices),
            _padding_1: Default::default(),
        }
    }
}

impl GraphBindingSet for SuzanneParamsInput {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding)) {
        f(GraphBinding::SampledTex(self.texture0));
        f(GraphBinding::SampledTex(self.texture1));
        f(GraphBinding::SampledTex(self.texture2));
        f(GraphBinding::Buffer(self.vertices.erased()));
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
            "/shaders/compiled/suzanne.json"
        ));

        let reflection_json: ReflectionJson = serde_json::from_str(json_str).unwrap();

        Self { reflection_json }
    }

    pub fn pipeline_config<'a>(
        &self,
        resources: Resources<'a>,
    ) -> PipelineConfig<'a, Graphics, NoPush> {
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
        .build_graphics()
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
            "/shaders/compiled/suzanne.vert.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }

    fn frag_spv(&self) -> Vec<u32> {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/compiled/suzanne.frag.spv"
        ));
        let byte_reader = &mut Cursor::new(bytes);
        read_spv(byte_reader).expect("failed to convert spv byte layout")
    }
}

impl ShaderAtlasEntry for Shader {
    fn source_file_name(&self) -> &str {
        &self.reflection_json.source_file_name
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
