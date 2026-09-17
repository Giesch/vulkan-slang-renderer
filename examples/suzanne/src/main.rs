use std::time::{Duration, Instant};

mod generated;

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
use crate::generated::shader_atlas::suzanne::*;

fn main() -> Result<(), anyhow::Error> {
    Suzanne::run()
}

type SuzanneGraph = PreparedRenderGraph<DrawNode<SuzanneParams>>;

pub struct Suzanne {
    start_time: Instant,
    graph: SuzanneGraph,
    /// Bound into the graph at build time; kept here for ownership.
    _textures: Vec<TextureHandle>,
    /// The graph captured this buffer's slot at build time; the handle stays
    /// here to keep the buffer alive.
    _params_buffer: UniformBufferHandle<SuzanneParams>,
}

impl Suzanne {
    fn load_vertices() -> anyhow::Result<(Vec<Vertex>, Vec<u32>)> {
        let file_path = manifest_path!["models", "suzanne", "suzanne.obj"];

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

impl Game for Suzanne {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Suzanne"
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let (vertices, indices) = Self::load_vertices()?;

        let mut textures = Vec::new();
        for i in 0..3 {
            let file_name = format!("suzanne{i}.ktx2");
            let file_path = manifest_path!["models", "suzanne", file_name.as_str()];
            let texture = load_ktx2_texture(renderer, &file_path, TextureFilter::Linear)?;
            textures.push(texture);
        }

        let params_buffer = renderer.create_uniform_buffer::<SuzanneParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };
        let pipeline_config = shaders
            .suzanne
            .pipeline_config(resources)
            .with_vertices(vertices, indices);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_indexed(
                &pipeline,
                &params_buffer,
                SuzanneParamsBindings {
                    texture0: textures[0].bindless_handle().into(),
                    texture1: textures[1].bindless_handle().into(),
                    texture2: textures[2].bindless_handle().into(),
                },
            ),
        )?
        .prepare(renderer)?;

        let start_time = Instant::now();

        Ok(Self {
            start_time,
            graph,
            _textures: textures,
            _params_buffer: params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = Instant::now() - self.start_time;
        let aspect_ratio = renderer.aspect_ratio();

        let mvp = make_mvp_matrices(elapsed, aspect_ratio);
        let time = elapsed.as_secs_f32();
        let params = SuzanneParamsData { mvp, time };

        self.graph.execute(renderer, &params)
    }
}

fn make_mvp_matrices(elapsed: Duration, aspect_ratio: f32) -> MVPMatrices {
    const TURN_DEGREES_PER_SECOND: f32 = 20.0;
    const FOV_DEGREES: f32 = 45.0;

    let turn_radians = elapsed.as_secs_f32() * TURN_DEGREES_PER_SECOND.to_radians();

    // Blender's monkey faces +Z in obj coordinates; spin it around +Y
    let model = Mat4::from_rotation_y(turn_radians);
    let eye = Vec3::new(0.0, 0.5, 3.0);
    let view = look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let fov_y_radians = FOV_DEGREES.to_radians();
    let proj = directx::perspective(fov_y_radians, aspect_ratio, 0.1, 10.0);

    MVPMatrices { model, view, proj }
}
