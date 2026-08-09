use std::time::{Duration, Instant};

mod generated;

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};

use mltrs::game::Game;
use mltrs::ktx::load_ktx2_texture;
use mltrs::manifest_path;
use mltrs::renderer::{
    DrawError, DrawIndexed, FrameRenderer, PipelineHandle, Renderer, TextureFilter, TextureHandle,
    UniformBufferHandle,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::depth_texture::*;

fn main() -> Result<(), anyhow::Error> {
    DepthTextureGame::run()
}

pub struct DepthTextureGame {
    start_time: Instant,
    pipeline: PipelineHandle<DrawIndexed>,
    texture: TextureHandle,
    params_buffer: UniformBufferHandle<DepthTextureParams>,
}

// two squares at different z values,
// each in clockwise order
const VERTICES: [Vertex; 8] = [
    Vertex {
        position: Vec3::new(-0.5, -0.5, 0.0),
        color: Vec3::new(1.0, 0.0, 0.0),
        tex_coord: Vec2::new(1.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, -0.5, 0.0),
        color: Vec3::new(0.0, 1.0, 0.0),
        tex_coord: Vec2::new(0.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, 0.5, 0.0),
        color: Vec3::new(0.0, 0.0, 1.0),
        tex_coord: Vec2::new(0.0, 1.0),
    },
    Vertex {
        position: Vec3::new(-0.5, 0.5, 0.0),
        color: Vec3::new(1.0, 1.0, 1.0),
        tex_coord: Vec2::new(1.0, 1.0),
    },
    Vertex {
        position: Vec3::new(-0.5, -0.5, -0.5),
        color: Vec3::new(1.0, 0.0, 0.0),
        tex_coord: Vec2::new(1.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, -0.5, -0.5),
        color: Vec3::new(0.0, 1.0, 0.0),
        tex_coord: Vec2::new(0.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, 0.5, -0.5),
        color: Vec3::new(0.0, 0.0, 1.0),
        tex_coord: Vec2::new(0.0, 1.0),
    },
    Vertex {
        position: Vec3::new(-0.5, 0.5, -0.5),
        color: Vec3::new(1.0, 1.0, 1.0),
        tex_coord: Vec2::new(1.0, 1.0),
    },
];

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
        let pipeline_config = shaders
            .depth_texture
            .pipeline_config(resources)
            .with_vertices(VERTICES.to_vec(), INDICES.to_vec());
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let start_time = Instant::now();

        Ok(Self {
            start_time,
            pipeline,
            texture,
            params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        let elapsed = Instant::now() - self.start_time;
        let mvp = make_mvp_matrices(elapsed, aspect_ratio);
        let params = DepthTextureParams {
            mvp,
            texture: self.texture.bindless_handle(),
            _padding_0: Default::default(),
        };

        renderer.draw_indexed(&self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
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
