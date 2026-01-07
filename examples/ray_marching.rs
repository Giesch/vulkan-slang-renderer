use std::f32::consts::TAU;
use std::time::Instant;

use glam::{Mat4, Quat, Vec3};
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

const SHAPE_BUFFER_SIZE: u32 = 32;

const MOON_START: Vec3 = Vec3::new(1.0, 0.0, 1.0);
const SUN_START: Vec3 = Vec3::new(4.0, 5.0, 2.0);

struct RayMarching {
    start_time: Instant,
    params_buffer: UniformBufferHandle<RayMarchingParams>,
    sun_position: Vec3,
    spheres_buffer: StorageBufferHandle<Sphere>,
    boxes_buffer: StorageBufferHandle<BoxRect>,
    spheres: Vec<Sphere>,
    boxes: Vec<BoxRect>,
    pipeline: PipelineHandle<DrawVertexCount>,
    intent: Intent,
    camera_controller: RaymarchCameraController,
}

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
        let spheres_buffer = renderer.create_storage_buffer::<Sphere>(SHAPE_BUFFER_SIZE)?;
        let boxes_buffer = renderer.create_storage_buffer::<BoxRect>(SHAPE_BUFFER_SIZE)?;
        let resources = Resources {
            spheres: &spheres_buffer,
            boxes: &boxes_buffer,
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().ray_marching;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let spheres = vec![Sphere {
            center: Vec3::ZERO,
            radius: 1.0,
            color: Vec3::new(0.2, 0.2, 0.6),
            _padding_0: Default::default(),
        }];

        let boxes = vec![BoxRect {
            radii: Vec3::splat(0.2),
            color: Vec3::new(0.2, 0.6, 0.2),
            transform: Mat4::from_translation(-MOON_START),
            _padding_0: Default::default(),
            _padding_1: Default::default(),
        }];

        let camera_controller = RaymarchCameraController {
            position: Vec3::new(0.0, 0.0, -5.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.2,
        };

        Ok(Self {
            start_time,
            params_buffer,
            sun_position: SUN_START,
            spheres_buffer,
            boxes_buffer,
            boxes,

            spheres,
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

        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let elapsed = elapsed * 0.1;

        let sun_rotation = Mat4::from_rotation_y(TAU * (elapsed * 0.25).fract());
        self.sun_position = sun_rotation.transform_point3(SUN_START);

        let cube_moon_transform = {
            let local_rotation = Mat4::from_rotation_z(TAU * (2.0 * elapsed).fract());
            let translation = Mat4::from_translation(MOON_START);
            let orbit_rotation =
                Mat4::from_quat(Quat::from_rotation_y(TAU * (1.0 * elapsed).fract()));

            local_rotation * translation * orbit_rotation
        };

        self.boxes[0].transform = cube_moon_transform;
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let camera = self.camera_controller.camera(renderer.aspect_ratio());

        let params = RayMarchingParams {
            camera,
            light_position: self.sun_position,
            resolution: renderer.window_resolution(),
            sphere_count: self.spheres.len() as u32,
            box_count: self.boxes.len() as u32,
        };

        renderer.draw_vertex_count(&mut self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
            gpu.write_storage(&mut self.spheres_buffer, &self.spheres);
            gpu.write_storage(&mut self.boxes_buffer, &self.boxes);
        })
    }
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

struct RaymarchCameraController {
    position: Vec3,
    // aka left/right facing angle
    yaw: f32,
    // aka up/down facing angle
    pitch: f32,
    // aka left/right lean angle
    roll: f32,
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
            inverse_view_proj,
        }
    }
}
