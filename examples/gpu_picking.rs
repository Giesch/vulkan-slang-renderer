use glam::{Mat4, Vec3};
use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PickingPipelineHandle, PipelineHandle, Renderer,
    UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::gpu_picking::*;
use vulkan_slang_renderer::generated::shader_atlas::gpu_picking_id;

fn main() -> Result<(), anyhow::Error> {
    GpuPicking::run()
}

struct GpuPicking {
    params_buffer: UniformBufferHandle<GpuPickingParams>,
    picking_params_buffer: UniformBufferHandle<gpu_picking_id::GpuPickingIdParams>,
    pipeline: PipelineHandle<DrawVertexCount>,
    picking_pipeline: PickingPipelineHandle,
    mouse_x: f32,
    mouse_y: f32,
}

impl Game for GpuPicking {
    type EditState = ();

    fn window_title() -> &'static str {
        "GPU Picking"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self> {
        let atlas = ShaderAtlas::init();

        // Visual shader pipeline
        let params_buffer = renderer.create_uniform_buffer::<GpuPickingParams>()?;
        let visual_resources = Resources {
            params_buffer: &params_buffer,
        };
        let visual_config = atlas.gpu_picking.pipeline_config(visual_resources);
        let pipeline = renderer.create_pipeline(visual_config)?;

        // Picking ID shader pipeline
        let picking_params_buffer =
            renderer.create_uniform_buffer::<gpu_picking_id::GpuPickingIdParams>()?;
        let picking_resources = gpu_picking_id::Resources {
            params_buffer: &picking_params_buffer,
        };
        let picking_config = atlas.gpu_picking_id.pipeline_config(picking_resources);
        let picking_pipeline = renderer.create_picking_pipeline(picking_config)?;

        Ok(Self {
            params_buffer,
            picking_params_buffer,
            pipeline,
            picking_pipeline,
            mouse_x: 0.0,
            mouse_y: 0.0,
        })
    }

    fn input(&mut self, input: Input) {
        if let Input::MouseMotion { x, y } = input {
            self.mouse_x = x;
            self.mouse_y = y;
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let picked_id = renderer.picked_object_id();
        let aspect_ratio = renderer.aspect_ratio();

        let camera = build_camera(aspect_ratio);
        let cube_position = Vec3::new(0.0, 0.0, 0.0);
        let cube_radii = Vec3::splat(0.8);

        let mouse_position = [self.mouse_x, self.mouse_y];

        renderer.draw_vertex_count_with_picking(
            &self.pipeline,
            3,
            &self.picking_pipeline,
            mouse_position,
            |gpu| {
                gpu.write_uniform(
                    &mut self.params_buffer,
                    GpuPickingParams {
                        camera: camera.clone(),
                        cube_position,
                        _padding_0: Default::default(),
                        cube_radii,
                        picked_object_id: picked_id,
                    },
                );
                gpu.write_uniform(
                    &mut self.picking_params_buffer,
                    gpu_picking_id::GpuPickingIdParams {
                        camera: gpu_picking_id::RayMarchCamera {
                            inverse_view_proj: gpu_picking_id::Projection {
                                matrix: camera.inverse_view_proj.matrix,
                            },
                            position: camera.position,
                        },
                        cube_position,
                        _padding_0: Default::default(),
                        cube_radii,
                        _padding_1: Default::default(),
                    },
                );
            },
        )
    }
}

fn build_camera(aspect_ratio: f32) -> RayMarchCamera {
    let position = Vec3::new(3.0, 3.0, -3.0);
    let target = Vec3::ZERO;
    let up = Vec3::Y;

    let fov_y = 45.0_f32.to_radians();
    let view = Mat4::look_at_rh(position, target, up);
    let proj = Mat4::perspective_rh(fov_y, aspect_ratio, 0.1, 100.0);
    let inverse_view_proj = (proj * view).inverse();

    RayMarchCamera {
        inverse_view_proj: Projection {
            matrix: inverse_view_proj,
        },
        position,
    }
}
