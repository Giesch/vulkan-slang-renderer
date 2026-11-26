use std::time::{Duration, Instant};

mod generated;

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};

use mltrs::game::Game;
use mltrs::ktx::load_ktx2_texture;
use mltrs::manifest_path;
use mltrs::renderer::{
    DrawError, DrawNode, FrameRenderer, IndexBufferHandle, PreparedRenderGraph, RenderGraph,
    Renderer, ResourcePlanner, SingletonBufferHandle, SingletonSlot, TextureFilter, TextureHandle,
    UniformBufferHandle, draw_indexed,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::depth_texture::*;

fn main() -> Result<(), anyhow::Error> {
    DepthTextureGame::run()
}

type DepthTextureGraph = PreparedRenderGraph<DrawNode<DepthTextureParams>>;

pub struct DepthTextureGame {
    start_time: Instant,
    graph: DepthTextureGraph,
    /// Bound into the graph at build time; kept here for ownership.
    _texture: TextureHandle,
    // The graph captured these buffers' slots at build time; the handles stay
    // here to keep the buffers alive.
    _params_buffer: UniformBufferHandle<DepthTextureParams>,
    _vertex_buffer: SingletonBufferHandle<Vertex>,
    _index_buffer: IndexBufferHandle,
}

// two squares at different z values,
// each in clockwise order
const VERTICES: [Vertex; 8] = [
    vertex(
        Vec3::new(-0.5, -0.5, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec2::new(1.0, 0.0),
    ),
    vertex(
        Vec3::new(0.5, -0.5, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(0.0, 0.0),
    ),
    vertex(
        Vec3::new(0.5, 0.5, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec2::new(0.0, 1.0),
    ),
    vertex(
        Vec3::new(-0.5, 0.5, 0.0),
        Vec3::new(1.0, 1.0, 1.0),
        Vec2::new(1.0, 1.0),
    ),
    vertex(
        Vec3::new(-0.5, -0.5, -0.5),
        Vec3::new(1.0, 0.0, 0.0),
        Vec2::new(1.0, 0.0),
    ),
    vertex(
        Vec3::new(0.5, -0.5, -0.5),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(0.0, 0.0),
    ),
    vertex(
        Vec3::new(0.5, 0.5, -0.5),
        Vec3::new(0.0, 0.0, 1.0),
        Vec2::new(0.0, 1.0),
    ),
    vertex(
        Vec3::new(-0.5, 0.5, -0.5),
        Vec3::new(1.0, 1.0, 1.0),
        Vec2::new(1.0, 1.0),
    ),
];

/// a std430 `Vertex`, whose float3 fields each pad to 16 bytes
const fn vertex(position: Vec3, color: Vec3, tex_coord: Vec2) -> Vertex {
    Vertex {
        position,
        _padding_0: [0; 4],
        color,
        _padding_1: [0; 4],
        tex_coord,
        _padding_2: [0; 8],
    }
}

// 2 quads of clockwise triangles,
// using the vertices above
#[rustfmt::skip]
const INDICES: [u32; 12] = [
    0, 1, 2, 2, 3, 0,
    4, 5, 6, 6, 7, 4,
];

impl Game for DepthTextureGame {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Depth Texture"
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        const IMAGE_FILE_NAME: &str = "texture.ktx2";
        let file_path = manifest_path!["textures", IMAGE_FILE_NAME];

        let texture = load_ktx2_texture(renderer, &file_path, TextureFilter::Linear)?;
        let params_buffer = renderer.create_uniform_buffer::<DepthTextureParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };
        let pipeline_config = shaders.depth_texture.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let vertex_buffer = renderer.create_singleton_buffer(&VERTICES)?;
        let index_buffer = renderer.create_index_buffer(&INDICES, vertex_buffer.len())?;

        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_indexed(
                &pipeline,
                &index_buffer,
                &params_buffer,
                DepthTextureParamsBindings {
                    texture: texture.bindless_handle().into(),
                    vertices: SingletonSlot::from(&vertex_buffer).addr(),
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
            _vertex_buffer: vertex_buffer,
            _index_buffer: index_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        let elapsed = Instant::now() - self.start_time;
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
