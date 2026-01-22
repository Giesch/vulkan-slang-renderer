use std::time::Instant;

use glam::Vec2;

use vulkan_slang_renderer::game::{Game, Input, MouseButton};
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, TextureFilter,
    UniformBufferHandle,
};
use vulkan_slang_renderer::util::load_image;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::koch_curve::*;

fn main() -> Result<(), anyhow::Error> {
    KochCurve::run()
}

pub struct KochCurve {
    start_time: Instant,
    pipeline: PipelineHandle<DrawVertexCount>,
    params_buffer: UniformBufferHandle<KochCurveParams>,
    mouse_down: bool,
    mouse_position: Vec2,
}

impl Game for KochCurve {
    type EditState = ();

    fn window_title() -> &'static str {
        "Koch Curve 3D"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        const IMAGE_FILE_NAME: &str = "istockphoto-uffizi-blurred-612x612.jpg";
        let image = load_image(IMAGE_FILE_NAME)?;
        let cube_map = renderer.create_texture(IMAGE_FILE_NAME, &image, TextureFilter::Linear)?;

        let params_buffer = renderer.create_uniform_buffer::<KochCurveParams>()?;

        let resources = Resources {
            params_buffer: &params_buffer,
            cube_map: &cube_map,
        };

        let shader = ShaderAtlas::init().koch_curve;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            start_time: Instant::now(),
            pipeline,
            params_buffer,
            mouse_down: false,
            mouse_position: Vec2::ZERO,
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::MouseDown { button, x, y } => {
                if button == MouseButton::Left {
                    self.mouse_down = true;
                    self.mouse_position = Vec2::new(x, y);
                }
            }

            Input::MouseUp { button, .. } => {
                if button == MouseButton::Left {
                    self.mouse_down = false;
                }
            }

            Input::MouseMotion { x, y } => {
                if self.mouse_down {
                    self.mouse_position = Vec2::new(x, y);
                }
            }

            _ => {}
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let time = (Instant::now() - self.start_time).as_secs_f32();

        let resolution = renderer.window_resolution();
        let mut mouse = self.mouse_position.clone();
        mouse.y = resolution.y - mouse.y;

        let params = KochCurveParams {
            resolution,
            mouse,
            time,
            _padding_0: Default::default(),
        };

        renderer.draw_vertex_count(&self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}
