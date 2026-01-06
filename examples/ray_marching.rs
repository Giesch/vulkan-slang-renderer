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

struct RaymarchCameraController {
    position: Vec3,
    yaw: f32,
    pitch: f32,
    move_speed: f32,

    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
}

impl RaymarchCameraController {
    fn new(position: Vec3, move_speed: f32) -> Self {
        Self {
            position,
            move_speed,
            yaw: 0.0,
            pitch: 0.0,
            forward: false,
            backward: false,
            left: false,
            right: false,
        }
    }

    fn forward_direction(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }

    fn update(&mut self) {
        let forward_dir = self.forward_direction();
        let right_dir = -Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());

        if self.forward {
            self.position += forward_dir * self.move_speed;
        }
        if self.backward {
            self.position -= forward_dir * self.move_speed;
        }
        if self.left {
            self.position -= right_dir * self.move_speed;
        }
        if self.right {
            self.position += right_dir * self.move_speed;
        }
    }

    fn camera(&self, aspect_ratio: f32) -> RayMarchCamera {
        let fov_y_radians = 45.0_f32.to_radians();
        let view = {
            let target = self.position + self.forward_direction();
            Mat4::look_at_rh(self.position, target, Vec3::Y)
        };
        let proj = Mat4::perspective_rh(fov_y_radians, aspect_ratio, 0.1, 1000.0);

        let view_proj = proj * view;
        let inverse_view_proj = view_proj.inverse();

        RayMarchCamera {
            position: self.position,
            padding: 0.0,
            inverse_view_proj,
        }
    }
}

struct RayMarching {
    start_time: Instant,
    params_buffer: UniformBufferHandle<RayMarchingParams>,
    spheres_buffer: StorageBufferHandle<Sphere>,
    spheres: Vec<Sphere>,
    pipeline: PipelineHandle<DrawVertexCount>,
    camera_controller: RaymarchCameraController,
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

        let spheres = vec![
            Sphere {
                center: Vec3::ZERO,
                radius: 1.0,
                color: Vec3::new(0.2, 0.2, 0.6),
                _padding_0: [0; 4],
            },
            Sphere {
                center: Vec3::splat(1.0),
                radius: 0.2,
                color: Vec3::new(0.2, 0.2, 0.6),
                _padding_0: [0; 4],
            },
        ];

        let camera_controller = RaymarchCameraController::new(Vec3::new(0.0, 0.0, -5.0), 0.1);

        Ok(Self {
            start_time,
            params_buffer,
            spheres_buffer,
            spheres,
            pipeline,
            camera_controller,
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::KeyDown(key) => match key {
                Key::W => self.camera_controller.forward = true,
                Key::S => self.camera_controller.backward = true,
                Key::A => self.camera_controller.left = true,
                Key::D => self.camera_controller.right = true,
                Key::Space => {}
            },

            Input::KeyUp(key) => match key {
                Key::W => self.camera_controller.forward = false,
                Key::S => self.camera_controller.backward = false,
                Key::A => self.camera_controller.left = false,
                Key::D => self.camera_controller.right = false,
                Key::Space => {}
            },
        }
    }

    fn update(&mut self) {
        self.camera_controller.update();
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let elapsed = elapsed * 0.1;

        let light_position = Vec3::new(4.0, 5.0, 2.0);
        let rotation = Mat4::from_rotation_y(TAU * elapsed.fract());
        let light_position = rotation.transform_point3(light_position);

        let camera = self.camera_controller.camera(renderer.aspect_ratio());

        let params = RayMarchingParams {
            camera,
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
