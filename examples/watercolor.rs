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
        renderer.clear_storage_texture(&canvas, [0.0, 0.0, 0.0, 0.0])?;
        let canvas_sampled = renderer.storage_texture_as_sampled(&canvas)?;

        // Generate paper height map
        let paper_height =
            renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;

        let height_data = noise::generate_paper_height_map(CANVAS_WIDTH, CANVAS_HEIGHT);
        renderer.write_storage_texture(&paper_height, &height_data)?;
        let paper_height_sampled = renderer.storage_texture_as_sampled(&paper_height)?;

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
            paper_height: &paper_height_sampled,
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

/// Noise functions for paper height map
mod noise {
    fn hash(n: f32) -> f32 {
        let s = (n * 127.1).sin() * 43758.5453;
        s - s.floor()
    }

    fn hash2(x: f32, y: f32) -> (f32, f32) {
        let a = hash(x * 127.1 + y * 311.7);
        let b = hash(x * 269.5 + y * 183.3);
        (a, b)
    }

    fn smoothstep(t: f32) -> f32 {
        t * t * (3.0 - 2.0 * t)
    }

    /// Gradient vectors at integer grid points, dot product with offset
    fn grad(ix: i32, iy: i32, fx: f32, fy: f32) -> f32 {
        let (gx, gy) = hash2(ix as f32, iy as f32);
        // Remap [0,1] to [-1,1]
        let gx = gx * 2.0 - 1.0;
        let gy = gy * 2.0 - 1.0;
        gx * fx + gy * fy
    }

    /// Single octave Perlin noise, returns roughly [-1, 1]
    fn perlin(x: f32, y: f32) -> f32 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - x.floor();
        let fy = y - y.floor();

        let ux = smoothstep(fx);
        let uy = smoothstep(fy);

        let n00 = grad(ix, iy, fx, fy);
        let n10 = grad(ix + 1, iy, fx - 1.0, fy);
        let n01 = grad(ix, iy + 1, fx, fy - 1.0);
        let n11 = grad(ix + 1, iy + 1, fx - 1.0, fy - 1.0);

        let nx0 = n00 + ux * (n10 - n00);
        let nx1 = n01 + ux * (n11 - n01);
        nx0 + uy * (nx1 - nx0)
    }

    /// Fractal Brownian motion with 3 octaves of Perlin noise
    fn perlin_fbm(x: f32, y: f32) -> f32 {
        let mut value = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        for _ in 0..3 {
            value += amplitude * perlin(x * frequency, y * frequency);
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        value
    }

    /// Worley (cellular) noise — distance to nearest random feature point
    fn worley(x: f32, y: f32) -> f32 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;

        let mut min_dist = f32::MAX;

        for dy in -1..=1 {
            for dx in -1..=1 {
                let nx = ix + dx;
                let ny = iy + dy;
                let (px, py) = hash2(nx as f32 + 100.0, ny as f32 + 100.0);
                let point_x = nx as f32 + px;
                let point_y = ny as f32 + py;
                let dist_x = x - point_x;
                let dist_y = y - point_y;
                let dist = (dist_x * dist_x + dist_y * dist_y).sqrt();
                if dist < min_dist {
                    min_dist = dist;
                }
            }
        }

        min_dist
    }

    /// Generate paper height map data as R32F floats, normalized to [0, 1]
    pub fn generate_paper_height_map(width: u32, height: u32) -> Vec<f32> {
        let perlin_scale = 8.0;
        let worley_scale = 16.0;

        let mut data = Vec::with_capacity((width * height) as usize);

        for y in 0..height {
            for x in 0..width {
                let nx = x as f32 / width as f32 * perlin_scale;
                let ny = y as f32 / height as f32 * perlin_scale;

                let p = perlin_fbm(nx, ny);

                let wx = x as f32 / width as f32 * worley_scale;
                let wy = y as f32 / height as f32 * worley_scale;
                let w = worley(wx, wy);

                let h = 0.6 * p + 0.4 * w;
                data.push(h);
            }
        }

        // Normalize to [0, 1]
        let min = data.iter().copied().fold(f32::MAX, f32::min);
        let max = data.iter().copied().fold(f32::MIN, f32::max);
        let range = max - min;
        if range > 0.0 {
            for v in &mut data {
                *v = (*v - min) / range;
            }
        }

        data
    }
}
