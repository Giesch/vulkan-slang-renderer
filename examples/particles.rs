use std::time::Instant;

use ash::vk;
use glam::{Vec2, Vec4};

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    Compute, DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer,
    StorageBufferFrameStrategy, StorageBufferHandle, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::particle_render;
use vulkan_slang_renderer::generated::shader_atlas::particles_compute;

fn main() -> Result<(), anyhow::Error> {
    Particles::run()
}

const NUM_PARTICLES: u32 = 4096;

struct Particles {
    last_frame: Instant,
    compute_pipeline: PipelineHandle<Compute>,
    render_pipeline: PipelineHandle<DrawVertexCount>,
    #[expect(unused)] // used only on the GPU after startup
    particle_buffer: StorageBufferHandle<particles_compute::Particle>,
    sim_params_buffer: UniformBufferHandle<particles_compute::SimParams>,
    render_params_buffer: UniformBufferHandle<particle_render::RenderParams>,
}

impl Game for Particles {
    type EditState = ();

    fn window_title() -> &'static str {
        "Particles"
    }

    fn initial_window_size() -> (u32, u32) {
        (800, 800)
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let initial_particles = create_initial_particles();

        let mut particle_buffer =
            renderer.create_storage_buffer::<particles_compute::Particle>(NUM_PARTICLES)?;

        let sim_params_buffer = renderer.create_uniform_buffer::<particles_compute::SimParams>()?;

        let render_params_buffer =
            renderer.create_uniform_buffer::<particle_render::RenderParams>()?;

        renderer.write_storage_all_frames(&mut particle_buffer, &initial_particles);

        // Create compute pipeline — both inputs point to the same buffer,
        // frame offsets handle the ping-pong via descriptor set wiring
        let shaders = ShaderAtlas::init();

        let compute_resources = particles_compute::Resources {
            particles_in: &particle_buffer,
            particles_out: &particle_buffer,
            sim_params_buffer: &sim_params_buffer,
        };
        let mut compute_config = shaders.particles_compute.pipeline_config(compute_resources);
        // particles_in reads previous frame (offset -1), particles_out writes current frame (offset 0)
        compute_config.storage_buffer_frame_strategy = StorageBufferFrameStrategy::PingPong;
        let compute_pipeline = renderer.create_compute_pipeline(compute_config)?;

        // Create render pipeline — reads current frame's data (default offset 0)
        let render_particles = particle_buffer.cast::<particle_render::Particle>();
        let render_resources = particle_render::Resources {
            particles: &render_particles,
            render_params_buffer: &render_params_buffer,
        };
        let render_config = shaders.particle_render.pipeline_config(render_resources);
        let render_pipeline = renderer.create_pipeline(render_config)?;

        let last_frame = Instant::now();

        Ok(Self {
            last_frame,
            compute_pipeline,
            render_pipeline,
            particle_buffer,
            sim_params_buffer,
            render_params_buffer,
        })
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let now = Instant::now();
        let delta_time = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;

        let workgroup_size = particles_compute::WORKGROUP_SIZE[0];
        let workgroup_count = (NUM_PARTICLES + workgroup_size - 1) / workgroup_size;

        // Dispatch compute shader
        renderer.dispatch(&self.compute_pipeline, workgroup_count, 1, 1);

        // Barrier: compute writes must complete before vertex shader reads
        renderer.memory_barrier(
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::VERTEX_SHADER,
            vk::AccessFlags::SHADER_WRITE,
            vk::AccessFlags::SHADER_READ,
        );

        let vertex_count = NUM_PARTICLES * 6; // 6 vertices per particle quad
        renderer.draw_vertex_count(&self.render_pipeline, vertex_count, |gpu| {
            gpu.write_uniform(
                &mut self.sim_params_buffer,
                particles_compute::SimParams {
                    delta_time,
                    _padding_0: Default::default(),
                },
            );
            gpu.write_uniform(
                &mut self.render_params_buffer,
                particle_render::RenderParams {
                    particle_count: NUM_PARTICLES,
                    _padding_0: Default::default(),
                },
            );
        })?;

        Ok(())
    }
}

fn create_initial_particles() -> Vec<particles_compute::Particle> {
    let mut particles = Vec::with_capacity(NUM_PARTICLES as usize);

    for i in 0..NUM_PARTICLES {
        let t = i as f32 / NUM_PARTICLES as f32;
        let angle = t * std::f32::consts::TAU * 4.0;
        let radius = t * 0.5;

        let position = Vec2::new(angle.cos() * radius, angle.sin() * radius);

        // Tangential velocity for spiral motion
        let speed = 0.1 + t * 0.3;
        let velocity = Vec2::new(-angle.sin() * speed, angle.cos() * speed);

        // Color: cycle through hues
        let hue = t * 6.0;
        let color = hue_to_rgb(hue);

        particles.push(particles_compute::Particle {
            position,
            velocity,
            color,
        });
    }

    particles
}

fn hue_to_rgb(hue: f32) -> Vec4 {
    let h = hue % 6.0;
    let f = h - h.floor();

    let (r, g, b) = match h.floor() as u32 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };

    Vec4::new(r, g, b, 1.0)
}
