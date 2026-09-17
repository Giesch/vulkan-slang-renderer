use std::f32::consts::TAU;

mod generated;

use std::time::Instant;

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Quat, Vec2, Vec3};
use mltrs::game::*;
use mltrs::renderer::{
    DrawError, DrawVertexCountNode, FrameRenderer, PreparedRenderGraph, RenderGraph, Renderer,
    ResourcePlanner, SingletonBufferHandle, SingletonSlot, StorageBufferHandle, StorageSlot,
    UniformBufferHandle, UploadNode, draw_vertex_count, upload,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::ray_marching::*;

fn main() -> Result<(), anyhow::Error> {
    RayMarching::run()
}

const SHAPE_BUFFER_SIZE: u32 = 32;

const MOON_START: Vec3 = Vec3::new(1.0, 0.0, 1.0);
const SUN_START: Vec3 = Vec3::new(4.0, 5.0, 2.0);

type RayMarchingGraph =
    PreparedRenderGraph<(UploadNode<BoxRect>, DrawVertexCountNode<RayMarchingParams>)>;

struct RayMarching {
    start_time: Instant,
    graph: RayMarchingGraph,
    sun_position: Vec3,
    /// The graph captured this buffer's slot at build time; the handle stays
    /// here to keep the buffer alive.
    _params_buffer: UniformBufferHandle<RayMarchingParams>,
    /// The graph reads this buffer every frame; the handle stays here to keep
    /// the buffer alive.
    _spheres_buffer: SingletonBufferHandle<Sphere>,
    /// The graph uploads into this buffer every frame; the handle stays here
    /// to keep the buffer alive.
    _boxes_buffer: StorageBufferHandle<BoxRect>,
    spheres: Vec<Sphere>,
    boxes: Vec<BoxRect>,
    intent: Intent,
    camera_controller: RaymarchCameraController,
}

impl Game for RayMarching {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Ray Marching"
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();

        let spheres = vec![Sphere {
            center: Vec3::ZERO,
            radius: 1.0,
            color: Vec3::new(0.2, 0.2, 0.6),
            _padding_0: Default::default(),
        }];

        let params_buffer = renderer.create_uniform_buffer::<RayMarchingParams>()?;
        let spheres_buffer = renderer.create_singleton_buffer(&spheres)?;
        let boxes_buffer = renderer.create_storage_buffer::<BoxRect>(SHAPE_BUFFER_SIZE)?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };

        let pipeline_config = shaders.ray_marching.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let boxes = vec![BoxRect {
            radii: Vec3::splat(0.2),
            color: Vec3::new(0.2, 0.6, 0.2),
            transform: Projection {
                matrix: Mat4::from_translation(-MOON_START),
            },
            _padding_0: Default::default(),
            _padding_1: Default::default(),
        }];

        let camera_controller = RaymarchCameraController {
            position: Vec3::new(0.0, 0.0, -5.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.2,
        };

        let bindings = RayMarchingParamsBindings {
            spheres: SingletonSlot::from(&spheres_buffer).addr().into(),
            boxes: StorageSlot::from(&boxes_buffer).read_addr(),
        };

        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            (
                upload(&boxes_buffer),
                draw_vertex_count(&pipeline, &params_buffer, 3, bindings),
            ),
        )?
        .prepare(renderer)?;

        Ok(Self {
            start_time,
            graph,
            sun_position: SUN_START,
            _params_buffer: params_buffer,
            _spheres_buffer: spheres_buffer,
            _boxes_buffer: boxes_buffer,
            boxes,

            spheres,
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
                _ => {}
            },

            Input::KeyUp(key) => match key {
                Key::W => self.intent.forward = false,
                Key::S => self.intent.backward = false,
                Key::A => self.intent.left = false,
                Key::D => self.intent.right = false,
                Key::Q => self.intent.roll_left = false,
                Key::E => self.intent.roll_right = false,
                Key::Space => {}
                _ => {}
            },

            _ => {}
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

            let matrix = local_rotation * translation * orbit_rotation;
            Projection { matrix }
        };

        self.boxes[0].transform = cube_moon_transform;
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let camera = self.camera_controller.camera(renderer.aspect_ratio());
        let resolution = renderer.window_resolution();

        let frame = frame_inputs(
            &self.boxes,
            &self.spheres,
            self.sun_position,
            camera,
            resolution,
        );

        self.graph.execute(renderer, &frame)
    }
}

/// The per-frame graph inputs, in node order
fn frame_inputs(
    boxes: &[BoxRect],
    spheres: &[Sphere],
    sun_position: Vec3,
    camera: RayMarchCamera,
    resolution: Vec2,
) -> (Vec<BoxRect>, RayMarchingParamsData) {
    (
        boxes.to_vec(),
        RayMarchingParamsData {
            camera,
            light_position: sun_position,
            sphere_count: spheres.len() as u32,
            box_count: boxes.len() as u32,
            resolution,
        },
    )
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
        let view = look_at_mat4(self.position, target, up);
        let proj = directx::perspective(fov_y_radians, aspect_ratio, 0.1, 1000.0);
        let inverse_view_proj = (proj * view).inverse();

        RayMarchCamera {
            position: self.position,
            inverse_view_proj: Projection {
                matrix: inverse_view_proj,
            },
            _padding_0: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spheres(n: usize) -> Vec<Sphere> {
        vec![
            Sphere {
                center: Vec3::ZERO,
                radius: 1.0,
                color: Vec3::ONE,
                _padding_0: Default::default(),
            };
            n
        ]
    }

    fn box_rect(radii: f32) -> BoxRect {
        BoxRect {
            radii: Vec3::splat(radii),
            color: Vec3::ONE,
            transform: Projection {
                matrix: Mat4::IDENTITY,
            },
            _padding_0: Default::default(),
            _padding_1: Default::default(),
        }
    }

    /// AC2/AC4: the frame inputs must track the current animated boxes and
    /// the window resolution, and snapshot the boxes rather than alias them.
    #[test]
    fn ray_marching_frame_input_tracks_boxes_and_resolution() {
        let controller = RaymarchCameraController {
            position: Vec3::new(0.0, 0.0, -5.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.2,
        };
        let camera = controller.camera(1.5);

        let mut boxes = vec![box_rect(0.2)];
        let frame = frame_inputs(
            &boxes,
            &spheres(3),
            SUN_START,
            camera,
            Vec2::new(1024.0, 768.0),
        );

        assert_eq!(frame.0.len(), 1, "one box uploaded this frame");
        assert_eq!(frame.0[0].radii, Vec3::splat(0.2));
        assert_eq!(frame.0[0].color, Vec3::ONE);
        assert_eq!(frame.0[0].transform.matrix, Mat4::IDENTITY);
        assert_eq!(frame.1.box_count, 1);
        assert_eq!(frame.1.sphere_count, 3);
        assert_eq!(frame.1.light_position, SUN_START);
        assert_eq!(frame.1.resolution, Vec2::new(1024.0, 768.0));
        // the supplied camera reaches the draw params unchanged
        assert_eq!(frame.1.camera.position, camera.position);
        assert_eq!(
            frame.1.camera.inverse_view_proj.matrix,
            camera.inverse_view_proj.matrix
        );

        // changed boxes and resolution reach the next frame's inputs, and the
        // captured upload is a snapshot, not an alias of the game state
        boxes[0].radii = Vec3::splat(0.9);
        boxes[0].color = Vec3::new(0.8, 0.1, 0.3);
        boxes[0].transform.matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let next = frame_inputs(
            &boxes,
            &spheres(3),
            SUN_START,
            camera,
            Vec2::new(640.0, 480.0),
        );
        assert_eq!(next.0[0].radii, Vec3::splat(0.9));
        assert_eq!(next.0[0].color, Vec3::new(0.8, 0.1, 0.3));
        assert_eq!(
            next.0[0].transform.matrix,
            Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
        );
        assert_eq!(next.1.resolution, Vec2::new(640.0, 480.0));
        assert_eq!(frame.0[0].radii, Vec3::splat(0.2), "earlier snapshot kept");
        assert_eq!(frame.0[0].color, Vec3::ONE, "earlier snapshot kept");

        // boundary: an empty scene uploads nothing and reports zero counts
        let empty = frame_inputs(&[], &spheres(0), SUN_START, camera, Vec2::ONE);
        assert!(empty.0.is_empty());
        assert_eq!(empty.1.box_count, 0);
        assert_eq!(empty.1.sphere_count, 0);
    }
}
