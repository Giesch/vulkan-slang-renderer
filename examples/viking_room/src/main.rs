use std::path::PathBuf;

mod generated;

use std::time::{Duration, Instant};

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};

use mltrs::game::Game;
use mltrs::ktx::load_ktx2_texture;
use mltrs::manifest_path;
use mltrs::renderer::{
    DrawError, DrawNode, FrameRenderer, PreparedRenderGraph, RenderGraph, Renderer,
    ResourcePlanner, TextureFilter, TextureHandle, UniformBufferHandle, draw_indexed,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::depth_texture::*;

fn main() -> Result<(), anyhow::Error> {
    VikingRoom::run()
}

type VikingRoomGraph = PreparedRenderGraph<DrawNode<DepthTextureParams>>;

pub struct VikingRoom {
    start_time: Instant,
    graph: VikingRoomGraph,
    /// Bound into the graph at build time; kept here for ownership.
    _texture: TextureHandle,
    /// The graph captured this buffer's slot at build time; the handle stays
    /// here to keep the buffer alive.
    _params_buffer: UniformBufferHandle<DepthTextureParams>,
}

impl VikingRoom {
    // From unknownue's rust version of the vulkan tutorial
    // https://github.com/unknownue/vulkan-tutorial-rust/blob/master/src/tutorials/27_model_loading.rs
    fn load_vertices() -> anyhow::Result<(Vec<Vertex>, Vec<u32>)> {
        let file_path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "models", "viking_room.obj"]
            .iter()
            .collect();

        let (mut models, _materials) = tobj::load_obj(file_path, &tobj::GPU_LOAD_OPTIONS)?;

        debug_assert!(models.len() == 1);
        let model = models.remove(0);

        let mut vertices = vec![];
        let mesh = model.mesh;
        let vertices_count = mesh.positions.len() / 3;
        for i in 0..vertices_count {
            let position = {
                let offset = i * 3;
                Vec3::new(
                    mesh.positions[offset],
                    mesh.positions[offset + 1],
                    mesh.positions[offset + 2],
                )
            };

            let tex_coord = {
                let offset = i * 2;
                let u = mesh.texcoords[offset];
                // in obj, 0 is the bottom, in vulkan, 0 is the top
                // (for texture coordinates)
                let v = 1.0 - mesh.texcoords[offset + 1];
                Vec2::new(u, v)
            };

            let vertex = Vertex {
                position,
                color: Vec3::splat(1.0),
                tex_coord,
            };

            vertices.push(vertex);
        }

        Ok((vertices, mesh.indices))
    }
}

impl Game for VikingRoom {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Viking Room"
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let (vertices, indices) = Self::load_vertices()?;

        const IMAGE_FILE_NAME: &str = "viking_room.ktx2";
        let file_path = manifest_path!["textures", IMAGE_FILE_NAME];

        let texture = load_ktx2_texture(renderer, &file_path, TextureFilter::Linear)?;
        let params_buffer = renderer.create_uniform_buffer::<DepthTextureParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };
        let pipeline_config = shaders
            .depth_texture
            .pipeline_config(resources)
            .with_vertices(vertices, indices);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_indexed(
                &pipeline,
                &params_buffer,
                DepthTextureParamsBindings {
                    texture: texture.bindless_handle().into(),
                },
            ),
        )?
        .prepare(renderer)?;

        let start_time = Instant::now();

        Ok(Self {
            start_time,
            graph,
            _texture: texture,
            _params_buffer: params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = Instant::now() - self.start_time;
        let aspect_ratio = renderer.aspect_ratio();

        let mvp = make_mvp_matrices(elapsed, aspect_ratio);
        let params = DepthTextureParamsData { mvp };

        self.graph.execute(renderer, &params)
    }
}

fn make_mvp_matrices(elapsed: Duration, aspect_ratio: f32) -> MVPMatrices {
    const TURN_DEGREES_PER_SECOND: f32 = 5.0;
    const STARTING_ANGLE_DEGREES: f32 = 45.0;

    let turn_radians = elapsed.as_secs_f32() * TURN_DEGREES_PER_SECOND.to_radians();

    let model = Mat4::from_rotation_z(turn_radians);
    let eye = Vec3::splat(2.0);
    let view = look_at_mat4(eye, Vec3::ZERO, Vec3::Z);
    let fov_y_radians = STARTING_ANGLE_DEGREES.to_radians();
    let proj = directx::perspective(fov_y_radians, aspect_ratio, 0.1, 10.0);

    MVPMatrices { model, view, proj }
}
