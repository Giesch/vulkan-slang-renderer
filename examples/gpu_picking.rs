use glam::{Mat4, Vec3};
use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PickingPipelineHandle, PipelineHandle, Renderer,
    StorageBufferHandle, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::gpu_picking::*;
use vulkan_slang_renderer::generated::shader_atlas::gpu_picking_id;

fn main() -> Result<(), anyhow::Error> {
    GpuPicking::run()
}

const MAX_CUBES: u32 = 256;

struct GpuPicking {
    params_buffer: UniformBufferHandle<GpuPickingParams>,
    picking_params_buffer: UniformBufferHandle<gpu_picking_id::GpuPickingIdParams>,
    cubes_buffer: StorageBufferHandle<Cube>,
    picking_cubes_buffer: StorageBufferHandle<gpu_picking_id::Cube>,
    pipeline: PipelineHandle<DrawVertexCount>,
    picking_pipeline: PickingPipelineHandle,
    cubes: Vec<Cube>,
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
        let cubes_buffer = renderer.create_storage_buffer::<Cube>(MAX_CUBES)?;
        let visual_resources = Resources {
            cubes: &cubes_buffer,
            params_buffer: &params_buffer,
        };
        let visual_config = atlas.gpu_picking.pipeline_config(visual_resources);
        let pipeline = renderer.create_pipeline(visual_config)?;

        // Picking ID shader pipeline
        let picking_params_buffer =
            renderer.create_uniform_buffer::<gpu_picking_id::GpuPickingIdParams>()?;
        let picking_cubes_buffer =
            renderer.create_storage_buffer::<gpu_picking_id::Cube>(MAX_CUBES)?;
        let picking_resources = gpu_picking_id::Resources {
            cubes: &picking_cubes_buffer,
            params_buffer: &picking_params_buffer,
        };
        let picking_config = atlas.gpu_picking_id.pipeline_config(picking_resources);
        let picking_pipeline = renderer.create_picking_pipeline(picking_config)?;

        let mut cubes = Vec::new();
        let radii = 0.3_f32;
        let spacing: f32 = 1.0;
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    let position = spacing * Vec3::new(x as f32, y as f32, z as f32);

                    cubes.push(Cube {
                        position,
                        _padding_0: Default::default(),
                        radii: Vec3::splat(radii),
                        _padding_1: Default::default(),
                    });
                }
            }
        }

        Ok(Self {
            params_buffer,
            picking_params_buffer,
            cubes_buffer,
            picking_cubes_buffer,
            pipeline,
            picking_pipeline,
            cubes,
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

        let mouse_position = [self.mouse_x, self.mouse_y];

        renderer.draw_vertex_count_with_picking(
            &self.pipeline,
            3,
            &self.picking_pipeline,
            mouse_position,
            |gpu| {
                let picking_params = GpuPickingParams {
                    camera: camera.clone(),
                    picked_object_id: picked_id,
                    cube_count: self.cubes.len() as u32,
                    _padding_0: Default::default(),
                };
                gpu.write_uniform(&mut self.params_buffer, picking_params);
                gpu.write_storage(&mut self.cubes_buffer, &self.cubes);

                let picking_id_params = gpu_picking_id::GpuPickingIdParams {
                    camera: to_picking_camera(&camera),
                    cube_count: self.cubes.len() as u32,
                    _padding_0: Default::default(),
                };
                gpu.write_uniform(&mut self.picking_params_buffer, picking_id_params);

                let picking_cubes: Vec<gpu_picking_id::Cube> =
                    self.cubes.iter().map(to_picking_id_cube).collect();
                gpu.write_storage(&mut self.picking_cubes_buffer, &picking_cubes);
            },
        )
    }
}

fn to_picking_id_cube(c: &Cube) -> gpu_picking_id::Cube {
    gpu_picking_id::Cube {
        position: c.position,
        _padding_0: Default::default(),
        radii: c.radii,
        _padding_1: Default::default(),
    }
}

fn to_picking_camera(c: &RayMarchCamera) -> gpu_picking_id::RayMarchCamera {
    gpu_picking_id::RayMarchCamera {
        inverse_view_proj: gpu_picking_id::Projection {
            matrix: c.inverse_view_proj.matrix,
        },
        position: c.position,
    }
}

fn build_camera(aspect_ratio: f32) -> RayMarchCamera {
    let position = Vec3::new(5.0, 5.0, -5.0);
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
