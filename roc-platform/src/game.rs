use std::sync::OnceLock;

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec3};

use mltrs::game::Game;
use mltrs::renderer::{
    DrawError, FrameRenderer, Graphics, IndexBufferHandle, PipelineHandle, Renderer,
    SingletonBufferHandle, UniformBufferHandle,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::basic_triangle::*;

/// `Game::window_title` is an associated fn returning `&'static str`, so the
/// title Roc supplies cannot be threaded through an argument. A `OnceLock` in a
/// static outlives the process, so borrowing from it yields a real `&'static str`.
static WINDOW_TITLE: OnceLock<String> = OnceLock::new();

const FALLBACK_WINDOW_TITLE: &str = "mltrs";

/// Returns `Err` with the unused title if a title was already set.
pub fn set_window_title(title: String) -> Result<(), String> {
    WINDOW_TITLE.set(title)
}

pub struct BasicTriangle {
    pipeline: PipelineHandle<Graphics>,
    uniform_buffer: UniformBufferHandle<TriangleParams>,
    vertex_buffer: SingletonBufferHandle<Vertex>,
    index_buffer: IndexBufferHandle,
}

impl Game for BasicTriangle {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        WINDOW_TITLE
            .get()
            .map(String::as_str)
            .unwrap_or(FALLBACK_WINDOW_TITLE)
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let uniform_buffer = renderer.create_uniform_buffer::<TriangleParams>()?;
        let vertex_buffer = renderer.create_singleton_buffer(&VERTICES)?;
        let index_buffer = renderer.create_index_buffer(&INDICES, vertex_buffer.len())?;

        let resources = Resources {
            params_buffer: &uniform_buffer,
        };

        let pipeline_config = shaders.basic_triangle.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            pipeline,
            uniform_buffer,
            vertex_buffer,
            index_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        let params = TriangleParams {
            mvp: make_basic_mvp_matrices(aspect_ratio),
            vertices: renderer.singleton_addr(&self.vertex_buffer),
            _padding_0: Default::default(),
        };

        renderer.draw_indexed(&self.pipeline, &self.index_buffer, |gpu| {
            gpu.write_uniform(&mut self.uniform_buffer, params);
        })
    }
}

const VERTICES: [Vertex; 3] = [
    vertex(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
    vertex(Vec3::new(1.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
    vertex(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
];

/// a std430 `Vertex`, whose float3 fields each pad to 16 bytes
const fn vertex(position: Vec3, color: Vec3) -> Vertex {
    Vertex {
        position,
        _padding_0: [0; 4],
        color,
        _padding_1: [0; 4],
    }
}

const INDICES: [u32; 3] = [0, 1, 2];

fn make_basic_mvp_matrices(aspect_ratio: f32) -> MVPMatrices {
    let model = Mat4::IDENTITY;

    let eye = Vec3::new(0.0, 0.0, 6.0);
    let view = look_at_mat4(eye, Vec3::ZERO, Vec3::Y);

    let fov_degrees: f32 = 45.0;
    let proj = directx::perspective(fov_degrees.to_radians(), aspect_ratio, 0.1, 10.0);

    MVPMatrices { model, view, proj }
}
