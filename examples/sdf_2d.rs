use std::time::Instant;

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::sdf_2d::*;

fn main() -> Result<(), anyhow::Error> {
    SDF2D::run()
}

struct SDF2D {
    start_time: Instant,
    pipeline: PipelineHandle<DrawVertexCount>,
    params_buffer: UniformBufferHandle<SDF2DParams>,
}

impl Game for SDF2D {
    fn window_title() -> &'static str {
        "SDF 2D"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();

        let params_buffer = renderer.create_uniform_buffer::<SDF2DParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().sdf_2d;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            start_time,
            pipeline,
            params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let time = (Instant::now() - self.start_time).as_secs_f32();
        let resolution = renderer.window_resolution();
        let params = SDF2DParams { time, resolution };

        // TODO allow multiple draw calls, split debug boxes to a separate one
        renderer.draw_vertex_count(&mut self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}
