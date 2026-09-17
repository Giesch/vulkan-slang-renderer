use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec3};

mod generated;

use mltrs::game::Game;
use mltrs::renderer::{
    DrawError, DrawNode, FrameRenderer, PreparedRenderGraph, RenderGraph, Renderer,
    ResourcePlanner, UniformBufferHandle, draw_indexed,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::basic_triangle::*;

fn main() -> Result<(), anyhow::Error> {
    BasicTriangle::run()
}

type TriangleGraph = PreparedRenderGraph<DrawNode<MVPMatrices>>;

pub struct BasicTriangle {
    graph: TriangleGraph,
    /// The graph captured this buffer's slot at build time; the handle stays
    /// here to keep the buffer alive.
    _uniform_buffer: UniformBufferHandle<MVPMatrices>,
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
        let uniform_buffer = renderer.create_uniform_buffer::<MVPMatrices>()?;

        let resources = Resources {
            matrices_buffer: &uniform_buffer,
        };

        let pipeline_config = shaders
            .basic_triangle
            .pipeline_config(resources)
            .with_vertices(VERTICES.to_vec(), INDICES.to_vec());
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_indexed(&pipeline, &uniform_buffer, ()),
        )?
        .prepare(renderer)?;

        Ok(Self {
            graph,
            _uniform_buffer: uniform_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        let mvp = make_basic_mvp_matrices(aspect_ratio);

        self.graph.execute(renderer, &mvp)
    }
}

const VERTICES: [Vertex; 3] = [
    Vertex {
        position: Vec3::new(-1.0, -1.0, 0.0),
        color: Vec3::new(1.0, 0.0, 0.0),
    },
    Vertex {
        position: Vec3::new(1.0, -1.0, 0.0),
        color: Vec3::new(0.0, 1.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.0, 1.0, 0.0),
        color: Vec3::new(0.0, 0.0, 1.0),
    },
];

const INDICES: [u32; 3] = [0, 1, 2];

fn make_basic_mvp_matrices(aspect_ratio: f32) -> MVPMatrices {
    let model = Mat4::IDENTITY;

    let eye = Vec3::new(0.0, 0.0, 6.0);
    let view = look_at_mat4(eye, Vec3::ZERO, Vec3::Y);

    let fov_degrees: f32 = 45.0;
    let proj = directx::perspective(fov_degrees.to_radians(), aspect_ratio, 0.1, 10.0);

    MVPMatrices { model, view, proj }
}
