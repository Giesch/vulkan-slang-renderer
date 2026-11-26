use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec3};

mod generated;

use mltrs::game::Game;
use mltrs::renderer::{
    DrawError, DrawNode, FrameRenderer, IndexBufferHandle, PreparedRenderGraph, RenderGraph,
    Renderer, ResourcePlanner, SingletonBufferHandle, SingletonSlot, UniformBufferHandle,
    draw_indexed,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::basic_triangle::*;

fn main() -> Result<(), anyhow::Error> {
    BasicTriangle::run()
}

type TriangleGraph = PreparedRenderGraph<DrawNode<TriangleParams>>;

pub struct BasicTriangle {
    graph: TriangleGraph,
    // The graph captured these buffers' slots at build time; the handles stay
    // here to keep the buffers alive.
    _uniform_buffer: UniformBufferHandle<TriangleParams>,
    _vertex_buffer: SingletonBufferHandle<Vertex>,
    _index_buffer: IndexBufferHandle,
}

impl Game for BasicTriangle {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Basic Triangle"
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

        let bindings = TriangleParamsBindings {
            vertices: SingletonSlot::from(&vertex_buffer).addr(),
        };
        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_indexed(&pipeline, &index_buffer, &uniform_buffer, bindings),
        )?
        .prepare(renderer)?;

        Ok(Self {
            graph,
            _uniform_buffer: uniform_buffer,
            _vertex_buffer: vertex_buffer,
            _index_buffer: index_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        let mvp = make_basic_mvp_matrices(aspect_ratio);

        self.graph.execute(renderer, &TriangleParamsData { mvp })
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
