use std::f32::consts::TAU;
use std::time::Instant;

use glam::{Mat4, Vec3};
use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, StorageBufferHandle,
    UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::ray_marching::*;

fn main() -> Result<(), anyhow::Error> {
    RayMarching::run()
}

struct RayMarching {
    start_time: Instant,
    params_buffer: UniformBufferHandle<RayMarchingParams>,
    spheres_buffer: StorageBufferHandle<Sphere>,
    spheres: Vec<Sphere>,
    pipeline: PipelineHandle<DrawVertexCount>,
}

const MAX_SPHERES: u32 = 100;

impl Game for RayMarching {
    fn window_title() -> &'static str {
        "Ray Marching"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();

        let params_buffer = renderer.create_uniform_buffer::<RayMarchingParams>()?;
        let spheres_buffer = renderer.create_storage_buffer::<Sphere>(MAX_SPHERES)?;
        let resources = Resources {
            spheres: &spheres_buffer,
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().ray_marching;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let spheres = vec![Sphere {
            center: Vec3::ZERO,
            radius: 1.0,
        }];

        Ok(Self {
            start_time,
            params_buffer,
            spheres_buffer,
            spheres,
            pipeline,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let elapsed = elapsed * 0.1;

        let light_position = Vec3::new(4.0, 5.0, 2.0);
        let rotation = Mat4::from_rotation_y(TAU * elapsed.fract());
        let light_position = rotation.transform_point3(light_position);

        // this assumes a 2x2 x-y image plane at float3(0.0)
        // NOTE the shader can't handle this changing yet
        let camera_position = Vec3::new(0.0, 0.0, -5.0);

        let params = RayMarchingParams {
            camera_position,
            light_position,
            resolution: renderer.window_resolution(),
            sphere_count: self.spheres.len() as u32,
        };

        renderer.draw_vertex_count(&mut self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
            gpu.write_storage(&mut self.spheres_buffer, &self.spheres);
        })
    }
}
