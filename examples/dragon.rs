use std::time::Instant;

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::dragon::*;

fn main() -> Result<(), anyhow::Error> {
    Dragon::run()
}

struct Dragon {
    start_time: Instant,
    params_buffer: UniformBufferHandle<DragonParams>,
    pipeline: PipelineHandle<DrawVertexCount>,
}

impl Game for Dragon {
    type EditState = ();

    fn window_title() -> &'static str {
        "Dragon Curve"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();

        let params_buffer = renderer.create_uniform_buffer::<DragonParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().dragon;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            start_time,
            params_buffer,
            pipeline,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let time = (Instant::now() - self.start_time).as_secs_f32();

        let params = DragonParams {
            time,
            _padding_0: Default::default(),
        };

        renderer.draw_vertex_count(&self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}
