use ash::vk;
use glam::{Vec2, Vec4};

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    Compute, DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer,
    StorageBufferHandle, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::paint_brush_compute;
use vulkan_slang_renderer::generated::shader_atlas::paint_display;

fn main() -> Result<(), anyhow::Error> {
    Watercolor::run()
}

const CANVAS_WIDTH: u32 = 1024;
const CANVAS_HEIGHT: u32 = 768;
const MAX_STROKE_POINTS_PER_FRAME: u32 = 256;

struct Watercolor {
    // Pipelines
    brush_pipeline: PipelineHandle<Compute>,
    display_pipeline: PipelineHandle<DrawVertexCount>,

    // Buffers
    stroke_points_buffer: StorageBufferHandle<paint_brush_compute::StrokePoint>,
    brush_params_buffer: UniformBufferHandle<paint_brush_compute::BrushParams>,

    // Input state
    painting: bool,
    stroke_points: Vec<Vec2>,
    prev_mouse_pos: Option<Vec2>,

    // Brush settings
    brush_color: Vec4,
    brush_radius: f32,
    brush_opacity: f32,
}

impl Game for Watercolor {
    type EditState = ();

    fn window_title() -> &'static str {
        "Watercolor"
    }

    fn initial_window_size() -> (u32, u32) {
        (CANVAS_WIDTH, CANVAS_HEIGHT)
    }

    fn render_scale() -> Option<f32> {
        Some(1.0)
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self> {
        let canvas = renderer.create_storage_texture(
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            vk::Format::R32G32B32A32_SFLOAT,
        )?;
        renderer.clear_storage_texture(
            &canvas,
            vk::ClearColorValue {
                float32: [1.0, 1.0, 1.0, 1.0],
            },
        )?;
        let canvas_sampled = renderer.storage_texture_as_sampled(&canvas)?;

        let stroke_points_buffer = renderer
            .create_storage_buffer::<paint_brush_compute::StrokePoint>(
                MAX_STROKE_POINTS_PER_FRAME,
            )?;
        let brush_params_buffer =
            renderer.create_uniform_buffer::<paint_brush_compute::BrushParams>()?;
        let shaders = ShaderAtlas::init();

        let brush_resources = paint_brush_compute::Resources {
            canvas: &canvas,
            stroke_points: &stroke_points_buffer,
            brush_params_buffer: &brush_params_buffer,
        };
        let brush_config = shaders.paint_brush_compute.pipeline_config(brush_resources);
        let brush_pipeline = renderer.create_compute_pipeline(brush_config)?;

        let display_resources = paint_display::Resources {
            canvas: &canvas_sampled,
        };
        let display_config = shaders.paint_display.pipeline_config(display_resources);
        let display_pipeline = renderer.create_pipeline(display_config)?;

        Ok(Self {
            brush_pipeline,
            display_pipeline,
            stroke_points_buffer,
            brush_params_buffer,
            painting: false,
            stroke_points: Vec::new(),
            prev_mouse_pos: None,
            brush_color: Vec4::new(0.1, 0.2, 0.6, 1.0),
            brush_radius: 20.0,
            brush_opacity: 0.3,
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::MouseDown {
                button: MouseButton::Left,
                x,
                y,
            } => {
                self.painting = true;
                let pos = Vec2::new(x, y);
                self.stroke_points.push(pos);
                self.prev_mouse_pos = Some(pos);
            }

            Input::MouseMotion { x, y } if self.painting => {
                let pos = Vec2::new(x, y);
                if let Some(prev) = self.prev_mouse_pos {
                    let spacing = self.brush_radius * 0.3;
                    let dist = prev.distance(pos);

                    if dist > spacing {
                        let steps = (dist / spacing).ceil() as u32;
                        for i in 1..=steps {
                            let t = i as f32 / steps as f32;
                            self.stroke_points.push(prev.lerp(pos, t));
                        }
                    } else if dist > 1.0 {
                        self.stroke_points.push(pos);
                    }
                }
                self.prev_mouse_pos = Some(pos);
            }

            Input::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.painting = false;
                self.prev_mouse_pos = None;
            }

            _other_input_event => {}
        }
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let stroke_points = std::mem::take(&mut self.stroke_points);
        let point_count = stroke_points
            .len()
            .min(MAX_STROKE_POINTS_PER_FRAME as usize) as u32;

        if point_count > 0 {
            let workgroup_x = (CANVAS_WIDTH + 15) / 16;
            let workgroup_y = (CANVAS_HEIGHT + 15) / 16;
            renderer.dispatch(&self.brush_pipeline, workgroup_x, workgroup_y, 1);

            renderer.memory_barrier(
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::SHADER_READ,
            );
        }

        let brush_color = self.brush_color;
        let brush_radius = self.brush_radius;
        let brush_opacity = self.brush_opacity;
        let brush_params_buffer = &mut self.brush_params_buffer;
        let stroke_points_buffer = &mut self.stroke_points_buffer;

        let window_size = renderer.window_resolution();
        let canvas_size = Vec2::new(CANVAS_WIDTH as f32, CANVAS_HEIGHT as f32);

        renderer.draw_vertex_count(&self.display_pipeline, 3, |gpu| {
            if point_count > 0 {
                let gpu_points: Vec<paint_brush_compute::StrokePoint> = stroke_points
                    [..point_count as usize]
                    .iter()
                    .map(|&position| {
                        let canvas_pos = position * canvas_size / window_size;
                        paint_brush_compute::StrokePoint {
                            position: canvas_pos,
                            pad: Vec2::ZERO,
                        }
                    })
                    .collect();

                gpu.write_storage(stroke_points_buffer, &gpu_points);
            }

            gpu.write_uniform(
                brush_params_buffer,
                paint_brush_compute::BrushParams {
                    point_count,
                    brush_radius,
                    brush_opacity,
                    _padding_0: Default::default(),
                    brush_color,
                    canvas_size: Vec2::new(CANVAS_WIDTH as f32, CANVAS_HEIGHT as f32),
                    _padding_1: Default::default(),
                },
            );
        })?;

        Ok(())
    }
}
