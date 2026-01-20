use std::time::Instant;

use glam::{Mat4, Quat, Vec3};
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
    intent: Intent,
    camera_controller: RaymarchCameraController,
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

        let camera_controller = RaymarchCameraController {
            position: Vec3::new(0.0, 0.0, -5.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        };

        Ok(Self {
            start_time,
            params_buffer,
            pipeline,
            intent: Default::default(),
            camera_controller,
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::KeyDown(key) => match key {
                Key::W => self.intent.forward = true,
                Key::S => self.intent.backward = true,
                Key::A => self.intent.left = true,
                Key::D => self.intent.right = true,
                Key::Q => self.intent.roll_left = true,
                Key::E => self.intent.roll_right = true,
                Key::Space => {}
            },

            Input::KeyUp(key) => match key {
                Key::W => self.intent.forward = false,
                Key::S => self.intent.backward = false,
                Key::A => self.intent.left = false,
                Key::D => self.intent.right = false,
                Key::Q => self.intent.roll_left = false,
                Key::E => self.intent.roll_right = false,
                Key::Space => {}
            },
        }
    }

    fn update(&mut self) {
        self.camera_controller.update(&self.intent);
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let time = (Instant::now() - self.start_time).as_secs_f32();
        let camera = self.camera_controller.camera(renderer.aspect_ratio());

        let params = DragonParams {
            camera,
            time,
            _padding_0: Default::default(),
        };

        renderer.draw_vertex_count(&self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}

// TODO share with raymarch example
struct RaymarchCameraController {
    position: Vec3,
    // aka left/right facing angle
    yaw: f32,
    // aka up/down facing angle
    pitch: f32,
    // aka left/right lean angle
    roll: f32,
}

// Translated player camera controls
#[derive(Default)]
struct Intent {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    roll_left: bool,
    roll_right: bool,
}

impl RaymarchCameraController {
    fn forward_direction(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }

    fn right_direction(&self) -> Vec3 {
        let forward = self.forward_direction();
        let base_right = forward.cross(Vec3::Y).normalize_or_zero();
        Quat::from_axis_angle(forward, self.roll) * base_right
    }

    fn update(&mut self, intent: &Intent) {
        const MOVE_SPEED: f32 = 0.01;
        const ROLL_SPEED: f32 = 0.03;

        let forward_dir = self.forward_direction();
        let right_dir = self.right_direction();

        let mut movement = Vec3::ZERO;
        if intent.forward {
            movement += forward_dir;
        }
        if intent.backward {
            movement -= forward_dir;
        }
        if intent.left {
            movement -= right_dir;
        }
        if intent.right {
            movement += right_dir;
        }

        if intent.roll_left {
            self.roll += ROLL_SPEED;
        }
        if intent.roll_right {
            self.roll -= ROLL_SPEED;
        }

        self.position += movement.normalize_or_zero() * MOVE_SPEED;
    }

    fn camera(&self, aspect_ratio: f32) -> RayMarchCamera {
        let fov_y_radians = 45.0_f32.to_radians();

        let forward = self.forward_direction();
        let up = Quat::from_axis_angle(forward, self.roll) * Vec3::Y;

        let target = self.position + forward;
        let view = Mat4::look_at_rh(self.position, target, up);
        let proj = Mat4::perspective_rh(fov_y_radians, aspect_ratio, 0.1, 1000.0);
        let inverse_view_proj = (proj * view).inverse();

        RayMarchCamera {
            position: self.position,
            inverse_view_proj: Projection {
                matrix: inverse_view_proj,
            },
        }
    }
}
