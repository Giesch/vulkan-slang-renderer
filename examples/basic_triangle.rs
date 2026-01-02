use glam::{Mat4, Vec3};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawIndexed, FrameRenderer, PipelineHandle, Renderer, UniformBufferHandle,
};
use vulkan_slang_renderer::shaders::COLUMN_MAJOR;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::basic_triangle::*;

fn main() -> Result<(), anyhow::Error> {
    BasicTriangle::run()
}

pub struct BasicTriangle {
    pipeline: PipelineHandle<DrawIndexed>,
    uniform_buffer: UniformBufferHandle<MVPMatrices>,
}

impl Game for BasicTriangle {
    fn window_title() -> &'static str {
        "Basic Triangle"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let uniform_buffer = renderer.create_uniform_buffer::<MVPMatrices>()?;

        let resources = Resources {
            vertices: VERTICES.to_vec(),
            indices: INDICES.to_vec(),
            matrices_buffer: &uniform_buffer,
        };

        let shader = ShaderAtlas::init().basic_triangle;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            pipeline,
            uniform_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        let mvp = make_basic_mvp_matrices(aspect_ratio);

        renderer.draw_indexed(&self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.uniform_buffer, mvp);
        })
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
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);

    let fov_degrees: f32 = 45.0;
    let proj = Mat4::perspective_rh(fov_degrees.to_radians(), aspect_ratio, 0.1, 10.0);

    normalize_mvp(MVPMatrices { model, view, proj })
}

fn normalize_mvp(mut mvp: MVPMatrices) -> MVPMatrices {
    // GLM & glam use column-major matrices, but D3D12 and Slang use row-major by default
    // it's also possible to avoid the transpose by reversing the mul() calls in shaders
    // https://discord.com/channels/1303735196696445038/1395879559827816458/1396913440584634499
    if !COLUMN_MAJOR {
        mvp.model = mvp.model.transpose();
        mvp.view = mvp.view.transpose();
        mvp.proj = mvp.proj.transpose();
    }

    mvp
}
