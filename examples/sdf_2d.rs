use glam::Mat4;

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, FrameRenderer, PipelineHandle, Renderer, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::sdf_2d::*;

fn main() -> Result<(), anyhow::Error> {
    SDF2D::run()
}

struct SDF2D {
    pipeline: PipelineHandle,
    uniform_buffer: UniformBufferHandle<SDF2DParams>,
}

impl Game for SDF2D {
    fn window_title() -> &'static str {
        "SDF 2D"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let uniform_buffer = renderer.create_uniform_buffer::<SDF2DParams>()?;

        let resources = Resources {
            vertex_count: 3,
            params_buffer: &uniform_buffer,
        };

        let shader = ShaderAtlas::init().sdf_2d;
        let mut pipeline_config = shader.pipeline_config(resources);
        pipeline_config.disable_depth_test = true;
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            pipeline,
            uniform_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let (width, height) = renderer.window_size();
        let projection_matrix = Mat4::orthographic_lh(0.0, width, height, 0.0, 0.0, -1.0);
        let uniform_data = SDF2DParams { projection_matrix };

        renderer.draw_frame(&mut self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.uniform_buffer, uniform_data);
        })
    }
}
