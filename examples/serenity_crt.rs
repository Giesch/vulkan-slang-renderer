use std::time::Instant;

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, TextureFilter,
    UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::serenity_crt::*;
use vulkan_slang_renderer::util::load_image;

fn main() -> Result<(), anyhow::Error> {
    SerenityCRT::run()
}

struct SerenityCRT {
    start_time: Instant,
    pipeline: PipelineHandle<DrawVertexCount>,
    params_buffer: UniformBufferHandle<SerenityCRTParams>,
}

impl Game for SerenityCRT {
    fn window_title() -> &'static str {
        "Serenity CRT"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let image_name = "serenity_crt/castlevania_pixel_art.png";
        let pixel_art_image = load_image(image_name)?;
        let texture =
            renderer.create_texture(image_name, &pixel_art_image, TextureFilter::Nearest)?;

        let params_buffer = renderer.create_uniform_buffer::<SerenityCRTParams>()?;
        let resources = Resources {
            tex: &texture,
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().serenity_crt;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            start_time: Instant::now(),
            pipeline,
            params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();

        let params = SerenityCRTParams {
            resolution: renderer.window_resolution(),

            scanline_intensity: 0.95,
            scanline_count: 256.0 * 4.0,
            time: elapsed,
            y_offset: 0.0,
            brightness: 0.9,
            contrast: 1.05,
            saturation: 1.75,
            bloom_intensity: 0.95,
            bloom_threshold: 0.5,
            rgb_shift: 1.0,
            adaptive_intensity: 0.3,
            vignette_strength: 0.3,
            curvature: 0.1,
            flicker_strength: 0.01,
        };

        renderer.draw_vertex_count(&mut self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}
