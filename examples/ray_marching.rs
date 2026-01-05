use glam::Vec3;
use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, StorageBufferHandle,
    UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::ray_marching::*;

fn main() -> Result<(), anyhow::Error> {
    SpheresDemo::run()
}

struct SpheresDemo {
    params_buffer: UniformBufferHandle<RayMarchingParams>,
    spheres_buffer: StorageBufferHandle<Sphere>,
    spheres: Vec<Sphere>,
    pipeline: PipelineHandle<DrawVertexCount>,
}

const MAX_SPHERES: u32 = 100;

impl Game for SpheresDemo {
    fn window_title() -> &'static str {
        "Spheres Demo"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
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
            params_buffer,
            spheres_buffer,
            spheres,
            pipeline,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let params = RayMarchingParams {
            resolution: renderer.window_resolution(),
            sphere_count: self.spheres.len() as u32,
        };

        renderer.draw_vertex_count(&mut self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
            gpu.write_storage(&mut self.spheres_buffer, &self.spheres);
        })
    }
}
